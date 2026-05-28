//! Updater-related Tauri commands
//! 更新器相关的 Tauri 命令
//!
//! Implements a four-state machine over the update lifecycle so the
//! frontend can support background download + click-to-install:
//!
//!   None ── check ──▶ Available ── download ──▶ Downloading ─┬─▶ Ready ── install ──▶ (restart)
//!                       ▲                                    │
//!                       │                                    └─▶ (cancel/fail) ── back to Available
//!                       └──────── (newer version found, bytes discarded) ────────────────────────
//!
//! `install_update` accepts both `Ready` (uses cached bytes) and `Available`
//! (legacy `download_and_install` fallback so the dialog still works without
//! a prior `download_update`).

use crate::bootstrap::TauriAppRuntime;
use crate::commands::record_trace_fields;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt as _;
use tokio::sync::Notify;
use tracing::{error, info, info_span, warn, Instrument};
use uc_core::settings::channel::detect_channel;
use uc_core::settings::model::UpdateChannel;
use uc_observability::analytics::{
    Event, InstallKind as AnalyticsInstallKind, UpdateAction, UpdateActionOutcome,
    UpdateCheckOutcome, UpdateCheckSource, UpdateFailureKind,
};
use uc_platform::ports::observability::TraceMetadata;

/// Tauri event channel name for broadcast download progress.
/// Subscribed by the frontend `UpdateContext` so any window/component can
/// reflect background download state, unlike the per-invocation
/// `tauri::ipc::Channel` which only delivers to its single creator.
pub const UPDATE_PROGRESS_EVENT: &str = "update-download-progress";

/// Broadcast Tauri event name carrying the result of a check_for_update call.
///
/// Payload: `Option<UpdateMetadata>` —— `Some(meta)` when the backend just
/// transitioned `PendingUpdate` to `Available` (or preserved a same-version
/// `Ready` snapshot), `None` when the check returned UpToDate and the backend
/// transitioned to `None`.
///
/// Subscribed by the frontend `UpdateContext` so the Sidebar/AboutSection
/// indicator can light up the moment a scheduler-driven (or manual) check
/// resolves —— Phase 6A removed the frontend's own startup check, so without
/// this broadcast the UI would never learn about scheduler-detected updates
/// until the next mount.
pub const UPDATE_AVAILABLE_EVENT: &str = "update-available";

/// Events emitted during update download.
///
/// Carried both via the broadcast `UPDATE_PROGRESS_EVENT` (background
/// download) and via the per-invocation `Channel<DownloadEvent>` passed to
/// `install_update` (legacy combined download+install path).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "event", content = "data")]
pub enum DownloadEvent {
    #[serde(rename_all = "camelCase")]
    Started {
        // 字节计数，远小于 2^53；显式 `Number<u64>` 让 TS 拿到 `number` 而非
        // `bigint`（specta 默认对 u64 直接 panic，强制开发者声明精度策略）。
        #[specta(type = Option<specta_typescript::Number<u64>>)]
        content_length: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        #[specta(type = specta_typescript::Number<usize>)]
        chunk_length: usize,
    },
    Finished,
    #[serde(rename_all = "camelCase")]
    Failed {
        error: String,
    },
}

/// Coarse-grained phase used by `get_download_progress` so a frontend that
/// mounts mid-download can render the right icon variant.
#[derive(Debug, Clone, Copy, Serialize, Default, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum DownloadPhase {
    #[default]
    Idle,
    Available,
    Downloading,
    Ready,
}

/// Snapshot of current download state, queryable from the frontend so a
/// just-mounted listener can sync up before attaching to the event stream.
#[derive(Debug, Clone, Serialize, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgressSnapshot {
    pub phase: DownloadPhase,
    // u64 字节计数 → TS `number`，理由同 `DownloadEvent::Started::content_length`。
    #[specta(type = specta_typescript::Number<u64>)]
    pub downloaded: u64,
    #[specta(type = Option<specta_typescript::Number<u64>>)]
    pub total: Option<u64>,
    /// Newly-available (latest) version. `None` when phase is `Idle`.
    pub version: Option<String>,
    /// Currently-installed app version, lifted directly from
    /// `app.package_info().version`. Always populated even when phase is
    /// `Idle`, so a mid-mount frontend can render "current vs. latest"
    /// without waiting for a fresh `check_for_update` round-trip.
    pub current_version: String,
    /// Release notes for the available version, if any. `None` when phase
    /// is `Idle` or the release ships no notes.
    pub body: Option<String>,
    /// Release date for the available version, if any. `None` when phase
    /// is `Idle`.
    pub date: Option<String>,
}

/// Metadata returned to the frontend when an update is available.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetadata {
    pub version: String,
    pub current_version: String,
    pub body: Option<String>,
    pub date: Option<String>,
}

/// Lifecycle of a pending update. Held inside the `PendingUpdate` mutex.
///
/// `Downloading` deliberately does **not** hold the `Update` —— the
/// `download_update` task owns it for the duration of `Update::download`,
/// and re-installs it into the state machine when the future resolves.
/// While downloading, other commands can still read the `info` and
/// `progress` fields, and `cancel_download` can fire the `Notify`.
#[derive(Default)]
pub enum PendingUpdateState {
    #[default]
    None,
    Available(tauri_plugin_updater::Update),
    Downloading {
        info: UpdateMetadata,
        progress: DownloadProgressSnapshot,
        cancel: Arc<Notify>,
    },
    Ready {
        update: tauri_plugin_updater::Update,
        bytes: Vec<u8>,
        downloaded_at: SystemTime,
    },
}

/// Mutex-wrapped state managed by Tauri so commands can transition the
/// state machine atomically.
pub struct PendingUpdate(pub Mutex<PendingUpdateState>);

impl PendingUpdate {
    pub fn new() -> Self {
        Self(Mutex::new(PendingUpdateState::None))
    }
}

impl Default for PendingUpdate {
    fn default() -> Self {
        Self::new()
    }
}

fn metadata_of(update: &tauri_plugin_updater::Update) -> UpdateMetadata {
    UpdateMetadata {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        body: update.body.clone(),
        date: update.date.map(|d| d.to_string()),
    }
}

fn lock_state(
    pending: &Mutex<PendingUpdateState>,
) -> Result<std::sync::MutexGuard<'_, PendingUpdateState>, String> {
    pending
        .lock()
        .map_err(|e| format!("updater: failed to lock pending state: {}", e))
}

/// Convert an `UpdateChannel` to its URL path segment string.
fn channel_as_str(channel: &UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Alpha => "alpha",
        UpdateChannel::Beta => "beta",
        UpdateChannel::Rc => "rc",
    }
}

/// Parse a channel name string into an `UpdateChannel`.
fn parse_channel(s: &str) -> UpdateChannel {
    match s.to_ascii_lowercase().as_str() {
        "alpha" => UpdateChannel::Alpha,
        "beta" => UpdateChannel::Beta,
        "rc" => UpdateChannel::Rc,
        _ => UpdateChannel::Stable,
    }
}

/// Check for an available update on the specified (or auto-detected) channel.
///
/// Crate-internal entry shared by the `check_for_update` Tauri command and
/// the background update scheduler. **Does not** emit any telemetry —
/// callers decide the `update_check_performed.source` field (`manual` /
/// `startup` / `scheduled` / `window_show`) and emit the event themselves
/// (schema doc §7.8 红线：两类 source 绝不混用同一调用路径)。
///
/// Side-effects on `PendingUpdate`:
/// - `Some(metadata)` returned & version matches an existing `Ready`: keep
///   cached bytes (refresh `Update` handle, preserve `downloaded_at`).
/// - `Some(metadata)` returned but version differs from `Ready`: discard
///   bytes, transition to `Available(new_update)`.
/// - `None` returned: transition to `None` (clear any prior cached bytes).
/// - State is `Downloading`: refuse to re-check (v1 simplification — wait
///   for the in-flight download to finish or be cancelled first).
pub(crate) async fn do_check_for_update(
    app: &AppHandle,
    channel: Option<UpdateChannel>,
    pending: &PendingUpdate,
) -> Result<Option<UpdateMetadata>, String> {
    {
        let guard = lock_state(&pending.0)?;
        if matches!(*guard, PendingUpdateState::Downloading { .. }) {
            return Err("updater: download in progress, cannot re-check".to_string());
        }
    }

    let resolved_channel = channel.unwrap_or_else(|| {
        let version = app.package_info().version.to_string();
        detect_channel(&version)
    });
    let channel_str = channel_as_str(&resolved_channel);

    info!(channel = %channel_str, "checking for update");

    let primary_url: url::Url = format!("https://release.uniclipboard.app/{}.json", channel_str)
        .parse()
        .map_err(|e| format!("Invalid primary updater URL: {}", e))?;
    let fallback_url: url::Url = format!(
        "https://uniclipboard.github.io/UniClipboard/{}.json",
        channel_str
    )
    .parse()
    .map_err(|e| format!("Invalid fallback updater URL: {}", e))?;

    let updater = app
        .updater_builder()
        .endpoints(vec![primary_url, fallback_url])
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;

    let update = updater.check().await.map_err(|e| e.to_string())?;

    let broadcast: Option<UpdateMetadata>;
    {
        let mut guard = lock_state(&pending.0)?;
        if matches!(*guard, PendingUpdateState::Downloading { .. }) {
            // A concurrent download started after our initial pre-check but
            // before we reacquired the lock. Leave the in-flight state alone
            // and surface the snapshot we observed without mutating it.
            info!(
                channel = %channel_str,
                "download started concurrently; preserving Downloading state"
            );
            broadcast = update.as_ref().map(metadata_of);
        } else {
            broadcast = match update {
                Some(update) => {
                    let metadata = metadata_of(&update);
                    info!(
                        channel = %channel_str,
                        new_version = %metadata.version,
                        "update available"
                    );

                    let prev = std::mem::take(&mut *guard);
                    *guard = match prev {
                        PendingUpdateState::Ready {
                            update: prev_update,
                            bytes,
                            downloaded_at,
                        } if prev_update.version == update.version => {
                            info!(
                                version = %metadata.version,
                                "preserving cached download bytes for same version"
                            );
                            PendingUpdateState::Ready {
                                update,
                                bytes,
                                downloaded_at,
                            }
                        }
                        _ => PendingUpdateState::Available(update),
                    };
                    Some(metadata)
                }
                None => {
                    info!(channel = %channel_str, "no update available");
                    *guard = PendingUpdateState::None;
                    None
                }
            };
        }
    }

    // Broadcast the transition so frontend listeners (e.g. `UpdateContext`)
    // can refresh the indicator without a re-mount. Mirrors the public
    // return value: `Some(meta)` for Available / preserved-Ready, `None`
    // for UpToDate. Emit failures are non-fatal —— the next mount snapshot
    // will reconcile.
    if let Err(err) = app.emit(UPDATE_AVAILABLE_EVENT, &broadcast) {
        warn!(
            target: "updater",
            error = %err,
            "failed to broadcast update-available event"
        );
    }

    Ok(broadcast)
}

/// Convert the running binary's [`InstallKind`] (Tauri command wire form) to
/// the telemetry [`AnalyticsInstallKind`]. Both enums must stay wire-equivalent
/// (schema doc §7.9)；这里只是把同形态值在两个 crate 的类型之间搬运一次。
pub(crate) fn install_kind_for_telemetry(kind: InstallKind) -> AnalyticsInstallKind {
    match kind {
        InstallKind::Macos => AnalyticsInstallKind::Macos,
        InstallKind::Windows => AnalyticsInstallKind::Windows,
        InstallKind::AppImage => AnalyticsInstallKind::AppImage,
        InstallKind::Deb => AnalyticsInstallKind::Deb,
        InstallKind::Rpm => AnalyticsInstallKind::Rpm,
        InstallKind::Unknown => AnalyticsInstallKind::Unknown,
    }
}

/// Heuristic classifier for `updater.check()` failure strings. The Tauri
/// updater plugin folds transport / HTTP / signature failures into a single
/// `Display` string，所以这里按子串匹配把它归类。优先级：parse > http >
/// network > other —— 签名 / JSON 错误信号最强，先扣下；HTTP 状态码次之；
/// 兜底归 `Network`（含 DNS / TLS / connect 类失败）或 `Other`。
///
/// 重命名禁止（schema doc §8），值仅四个：`network` / `http_error` /
/// `parse_error` / `other`。
pub(crate) fn classify_check_failure(err: &str) -> UpdateFailureKind {
    let lower = err.to_ascii_lowercase();
    if lower.contains("signature")
        || lower.contains("minisign")
        || lower.contains("parse")
        || lower.contains("json")
        || lower.contains("decode")
    {
        UpdateFailureKind::ParseError
    } else if lower.contains("http")
        || lower.contains("status code")
        || lower.contains(" 4")
        || lower.contains(" 5")
    {
        UpdateFailureKind::HttpError
    } else if lower.contains("connect")
        || lower.contains("dns")
        || lower.contains("tls")
        || lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("network")
        || lower.contains("transport")
    {
        UpdateFailureKind::Network
    } else {
        UpdateFailureKind::Other
    }
}

/// Tauri command — thin shell over [`do_check_for_update`] that emits
/// `update_check_performed { source: "manual", ... }` after the inner
/// resolves. scheduler-driven checks emit the same event with a different
/// `source`（schema doc §7.8）；inner 函数本身不 emit。
#[tauri::command]
#[specta::specta]
pub async fn check_for_update(
    app: AppHandle,
    channel: Option<String>,
    pending: State<'_, PendingUpdate>,
    runtime: State<'_, Arc<TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<Option<UpdateMetadata>, String> {
    let span = info_span!(
        "command.updater.check_for_update",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    let analytics = runtime.analytics();
    let resolved_channel = channel.as_deref().map(parse_channel);

    async move {
        let result = do_check_for_update(&app, resolved_channel, pending.inner()).await;

        // Phase 5B: 任何 source 的 check 完成（成功或失败）都更新 LastCheckAt，
        // 让 `show_main_window` 顺手检查阈值不会在用户刚手动检查完后又触发。
        app.state::<crate::update_scheduler::LastCheckAt>()
            .record_now();

        let install_kind = install_kind_for_telemetry(detect_install_kind());
        let (outcome, failure_kind) = match &result {
            Ok(Some(_)) => (UpdateCheckOutcome::Available, None),
            Ok(None) => (UpdateCheckOutcome::UpToDate, None),
            Err(err) => (
                UpdateCheckOutcome::Failed,
                Some(classify_check_failure(err)),
            ),
        };
        analytics.capture(Event::UpdateCheckPerformed {
            source: UpdateCheckSource::Manual,
            outcome,
            failure_kind,
            install_kind,
        });

        result
    }
    .instrument(span)
    .await
}

/// Manual update check triggered from the system tray menu item.
///
/// Behaves like the `check_for_update` Tauri command (same `Manual` telemetry
/// source, same `LastCheckAt` refresh) and additionally routes
/// "found a new version" through [`NotifyContext::notify_if_new_version`] so
/// the user sees the Sparkle-style update window without having to open the
/// main window first.
///
/// Fire-and-forget; errors are logged but not propagated — the tray click
/// handler is synchronous and has no UI to report errors to. The
/// `UPDATE_AVAILABLE_EVENT` emitted by [`do_check_for_update`] still feeds
/// the main window's `UpdateContext` if it happens to be open.
pub(crate) async fn perform_manual_check_from_tray(app: &AppHandle) {
    let runtime = match app.try_state::<Arc<TauriAppRuntime>>() {
        Some(r) => r,
        None => {
            warn!(
                target: "updater",
                "TauriAppRuntime not mounted; aborting tray-initiated update check"
            );
            return;
        }
    };
    let pending = match app.try_state::<PendingUpdate>() {
        Some(p) => p,
        None => {
            warn!(
                target: "updater",
                "PendingUpdate not mounted; aborting tray-initiated update check"
            );
            return;
        }
    };

    // Channel resolution mirrors the scheduler / window_show_check path:
    // settings-pinned channel wins, otherwise detect from app version.
    let app_version = app.package_info().version.to_string();
    let resolved_channel = match runtime.settings_port().load().await {
        Ok(settings) => crate::update_scheduler::scheduler::resolve_channel(
            settings.general.update_channel.clone(),
            &app_version,
        ),
        Err(err) => {
            warn!(
                target: "updater",
                error = %err,
                "failed to load settings; falling back to version-detected channel"
            );
            detect_channel(&app_version)
        }
    };

    info!(target: "updater", "running tray-initiated update check");
    let result = do_check_for_update(app, Some(resolved_channel.clone()), pending.inner()).await;

    app.state::<crate::update_scheduler::LastCheckAt>()
        .record_now();

    let install_kind = install_kind_for_telemetry(detect_install_kind());

    if let Ok(Some(metadata)) = &result {
        match app.try_state::<Arc<crate::update_scheduler::NotifyContext>>() {
            Some(ctx) => {
                ctx.notify_if_new_version(&resolved_channel, &metadata.version, install_kind)
                    .await;
            }
            None => warn!(
                target: "updater",
                "NotifyContext not mounted; tray check found update but cannot dedup/notify"
            ),
        }
    }

    let (outcome, failure_kind) = match &result {
        Ok(Some(_)) => (UpdateCheckOutcome::Available, None),
        Ok(None) => (UpdateCheckOutcome::UpToDate, None),
        Err(err) => (
            UpdateCheckOutcome::Failed,
            Some(classify_check_failure(err)),
        ),
    };
    runtime.analytics().capture(Event::UpdateCheckPerformed {
        source: UpdateCheckSource::Manual,
        outcome,
        failure_kind,
        install_kind,
    });
}

/// Download the pending update in the background.
///
/// Crate-internal entry shared by the `download_update` Tauri command and
/// the background update scheduler (Phase 4B auto-download branch).
/// **Does not** emit any telemetry —— caller decides
/// `update_action_invoked` framing (typically a `Started` event at the
/// command entry and a terminal `Succeeded` / `Failed` / `Cancelled`
/// after this returns).
///
/// Broadcasts download progress via `UPDATE_PROGRESS_EVENT` so the
/// frontend's `UpdateContext` listener can render a progress bar
/// regardless of which caller invoked the download.
///
/// Returns [`DownloadError`] so callers can distinguish precondition
/// rejections (state machine misuse) from in-flight cancellation /
/// failure. The Tauri command flattens that back into the historical
/// `Result<(), String>` wire shape; the scheduler can map terminal
/// states directly to `UpdateActionOutcome` without string heuristics.
///
/// Pre-condition: state must be `Available`. `Downloading` /
/// `Ready` / `None` return `DownloadError::Precondition`.
pub(crate) async fn do_download_update(
    app: &AppHandle,
    pending: &PendingUpdate,
) -> Result<(), DownloadError> {
    let cancel = Arc::new(Notify::new());

    let (update, info) = {
        let mut guard = lock_state(&pending.0).map_err(DownloadError::Precondition)?;
        match std::mem::take(&mut *guard) {
            PendingUpdateState::None => {
                return Err(DownloadError::Precondition(
                    "updater: no pending update to download".to_string(),
                ));
            }
            PendingUpdateState::Available(update) => {
                let info = metadata_of(&update);
                let progress = DownloadProgressSnapshot {
                    phase: DownloadPhase::Downloading,
                    downloaded: 0,
                    total: None,
                    version: Some(info.version.clone()),
                    current_version: info.current_version.clone(),
                    body: info.body.clone(),
                    date: info.date.clone(),
                };
                *guard = PendingUpdateState::Downloading {
                    info: info.clone(),
                    progress,
                    cancel: cancel.clone(),
                };
                (update, info)
            }
            other @ PendingUpdateState::Downloading { .. } => {
                *guard = other;
                return Err(DownloadError::Precondition(
                    "updater: already downloading".to_string(),
                ));
            }
            other @ PendingUpdateState::Ready { .. } => {
                *guard = other;
                return Err(DownloadError::Precondition(
                    "updater: already downloaded, ready to install".to_string(),
                ));
            }
        }
    };

    info!(version = %info.version, "background download starting");

    let mut started_emitted = false;
    let app_for_chunk = app.clone();

    let on_chunk = |chunk_length: usize, content_length: Option<u64>| {
        if !started_emitted {
            started_emitted = true;
            let _ = app_for_chunk.emit(
                UPDATE_PROGRESS_EVENT,
                DownloadEvent::Started { content_length },
            );
        }
        if let Ok(mut guard) = pending.0.lock() {
            if let PendingUpdateState::Downloading { progress, .. } = &mut *guard {
                progress.downloaded = progress.downloaded.saturating_add(chunk_length as u64);
                if progress.total.is_none() {
                    progress.total = content_length;
                }
            }
        }
        let _ = app_for_chunk.emit(
            UPDATE_PROGRESS_EVENT,
            DownloadEvent::Progress { chunk_length },
        );
    };

    let app_for_finish = app.clone();
    let on_finish = move || {
        let _ = app_for_finish.emit(UPDATE_PROGRESS_EVENT, DownloadEvent::Finished);
    };

    let cancel_for_select = cancel.clone();
    let download_result: Result<Vec<u8>, DownloadOutcome> = tokio::select! {
        biased;
        _ = cancel_for_select.notified() => Err(DownloadOutcome::Cancelled),
        res = update.download(on_chunk, on_finish) => {
            res.map_err(|e| DownloadOutcome::Failed(e.to_string()))
        }
    };

    match download_result {
        Ok(bytes) => {
            info!(
                version = %info.version,
                size = bytes.len(),
                "background download complete"
            );
            let mut guard = lock_state(&pending.0).map_err(DownloadError::Failed)?;
            *guard = PendingUpdateState::Ready {
                update,
                bytes,
                downloaded_at: SystemTime::now(),
            };
            Ok(())
        }
        Err(DownloadOutcome::Cancelled) => {
            info!(version = %info.version, "background download cancelled");
            let _ = app.emit(
                UPDATE_PROGRESS_EVENT,
                DownloadEvent::Failed {
                    error: "cancelled".to_string(),
                },
            );
            let mut guard = lock_state(&pending.0).map_err(DownloadError::Cancelled)?;
            *guard = PendingUpdateState::Available(update);
            Err(DownloadError::Cancelled(
                "updater: download cancelled".to_string(),
            ))
        }
        Err(DownloadOutcome::Failed(err)) => {
            error!(version = %info.version, error = %err, "background download failed");
            let _ = app.emit(
                UPDATE_PROGRESS_EVENT,
                DownloadEvent::Failed { error: err.clone() },
            );
            let mut guard = lock_state(&pending.0).map_err(DownloadError::Failed)?;
            *guard = PendingUpdateState::Available(update);
            Err(DownloadError::Failed(err))
        }
    }
}

/// Failure modes of [`do_download_update`].
///
/// Distinguishes precondition rejections (state-machine misuse, e.g.
/// "already downloading") from in-flight cancellation / failure so the
/// Tauri command can map them onto `UpdateActionOutcome` without string
/// heuristics. The inner `String` is the legacy wire error message
/// returned to the frontend.
pub(crate) enum DownloadError {
    /// Wrong state for download (no pending update / already downloading /
    /// already ready). The action never began —— callers typically should
    /// not emit `Started` for this branch (the user did not actually start
    /// a download attempt).
    Precondition(String),
    /// `cancel_download` was signalled mid-stream. Maps to
    /// `UpdateActionOutcome::Cancelled`.
    Cancelled(String),
    /// `Update::download` returned an error or a downstream lock acquire
    /// failed. Maps to `UpdateActionOutcome::Failed`.
    Failed(String),
}

impl DownloadError {
    fn into_wire(self) -> String {
        match self {
            Self::Precondition(s) | Self::Cancelled(s) | Self::Failed(s) => s,
        }
    }

    /// Short telemetry identifier (< 32 chars，无 URL / 路径 / IP，schema
    /// doc §6.1). 仅 `Cancelled` 路径返回 `None`——cancel 有专属 outcome 槽位，
    /// 不需要 `error_kind` 再重述。
    ///
    /// `pub(crate)` 因为 `update_scheduler::scheduler` 的 auto-download 分支
    /// 也要把 `DownloadError` 映射到 `update_action_invoked.error_kind`。
    pub(crate) fn error_kind(&self) -> Option<&'static str> {
        match self {
            Self::Precondition(s) => Some(if s.contains("no pending update") {
                "no_pending_update"
            } else if s.contains("already downloading") {
                "already_downloading"
            } else if s.contains("already downloaded") {
                "already_ready"
            } else {
                "precondition"
            }),
            Self::Cancelled(_) => None,
            Self::Failed(_) => Some("download_failed"),
        }
    }
}

/// Download the pending update in the background, broadcasting progress
/// via `UPDATE_PROGRESS_EVENT`. Awaitable: the future resolves when the
/// download completes, fails, or is cancelled.
///
/// Thin shell over [`do_download_update`] that emits
/// `update_action_invoked { action = "download_bg", outcome = started }`
/// at entry (once the action actually transitions to `Downloading`) and
/// a terminal `Succeeded` / `Failed` / `Cancelled` after the inner
/// returns. Precondition rejections are returned to the frontend without
/// emitting any telemetry —— the user did not actually start a download,
/// so we keep the funnel分母干净.
///
/// Pre-condition: state must be `Available`. `Downloading` returns
/// "already downloading"; `Ready` returns "already downloaded"; `None`
/// returns "no pending update".
#[tauri::command]
#[specta::specta]
pub async fn download_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
    runtime: State<'_, Arc<TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.updater.download_update",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    let analytics = runtime.analytics();

    async move {
        let result = do_download_update(&app, pending.inner()).await;

        // Started + terminal pair: only emit when the action actually began
        // (i.e., not a precondition rejection). schema doc §7.8 落地备注: a
        // download lifecycle emits Started once + terminal once.
        let did_start = !matches!(result, Err(DownloadError::Precondition(_)));
        if did_start {
            analytics.capture(Event::UpdateActionInvoked {
                action: UpdateAction::DownloadBg,
                outcome: UpdateActionOutcome::Started,
                error_kind: None,
            });
        }

        let outcome = match &result {
            Ok(()) => Some(UpdateActionOutcome::Succeeded),
            Err(DownloadError::Cancelled(_)) => Some(UpdateActionOutcome::Cancelled),
            Err(DownloadError::Failed(_)) => Some(UpdateActionOutcome::Failed),
            Err(DownloadError::Precondition(_)) => None,
        };
        if let Some(outcome) = outcome {
            analytics.capture(Event::UpdateActionInvoked {
                action: UpdateAction::DownloadBg,
                outcome,
                error_kind: result
                    .as_ref()
                    .err()
                    .and_then(|e| e.error_kind())
                    .map(|s| s.to_string()),
            });
        }

        result.map_err(DownloadError::into_wire)
    }
    .instrument(span)
    .await
}

enum DownloadOutcome {
    Cancelled,
    Failed(String),
}

/// Signal an in-flight `download_update` to abort. No-op if the state is
/// not `Downloading`. The download future is dropped by the `tokio::select!`
/// in `download_update`, which terminates the underlying HTTP stream.
#[tauri::command]
#[specta::specta]
pub async fn cancel_download(
    pending: State<'_, PendingUpdate>,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.updater.cancel_download",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let guard = lock_state(&pending.0)?;
        match &*guard {
            PendingUpdateState::Downloading { cancel, .. } => {
                cancel.notify_one();
                info!("cancel_download signalled");
            }
            _ => {
                info!("cancel_download requested with no active download");
            }
        }
        Ok(())
    }
    .instrument(span)
    .await
}

/// Return the current download state so a freshly-mounted frontend listener
/// can sync up before attaching to `UPDATE_PROGRESS_EVENT`.
#[tauri::command]
#[specta::specta]
pub async fn get_download_progress(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
    _trace: Option<TraceMetadata>,
) -> Result<DownloadProgressSnapshot, String> {
    let _ = _trace;
    let current_version = app.package_info().version.to_string();
    let guard = lock_state(&pending.0)?;
    let snapshot = match &*guard {
        PendingUpdateState::None => DownloadProgressSnapshot {
            current_version: current_version.clone(),
            ..Default::default()
        },
        PendingUpdateState::Available(update) => DownloadProgressSnapshot {
            phase: DownloadPhase::Available,
            downloaded: 0,
            total: None,
            version: Some(update.version.clone()),
            current_version: update.current_version.clone(),
            body: update.body.clone(),
            date: update.date.map(|d| d.to_string()),
        },
        PendingUpdateState::Downloading { progress, .. } => progress.clone(),
        PendingUpdateState::Ready { update, bytes, .. } => DownloadProgressSnapshot {
            phase: DownloadPhase::Ready,
            downloaded: bytes.len() as u64,
            total: Some(bytes.len() as u64),
            version: Some(update.version.clone()),
            current_version: update.current_version.clone(),
            body: update.body.clone(),
            date: update.date.map(|d| d.to_string()),
        },
    };
    Ok(snapshot)
}

/// Install the pending update.
///
/// - `Ready`: install cached bytes via `Update::install`, then restart.
///   On failure, restore the `Ready` state so the user can retry.
/// - `Available`: legacy combined download+install path via
///   `Update::download_and_install`, with progress on the per-invocation
///   `Channel<DownloadEvent>` for compatibility with the existing dialog.
/// - `Downloading`: refuse (caller should wait or cancel first).
/// - `None`: refuse.
#[tauri::command]
#[specta::specta]
pub async fn install_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
    on_event: Channel<DownloadEvent>,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.updater.install_update",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        // Inspect state while holding the lock; only `mem::take` for the
        // installable variants. For the refusal variants we never touch the
        // state, so a concurrent `check_for_update` / `download_update`
        // cannot have its write clobbered by an unconditional restore.
        let state = {
            let mut guard = lock_state(&pending.0)?;
            match &*guard {
                PendingUpdateState::None => {
                    return Err("updater: no pending update".to_string());
                }
                PendingUpdateState::Downloading { .. } => {
                    return Err(
                        "updater: download in progress; wait or cancel first".to_string(),
                    );
                }
                PendingUpdateState::Ready { .. } | PendingUpdateState::Available(_) => {
                    std::mem::take(&mut *guard)
                }
            }
        };

        match state {
            PendingUpdateState::None | PendingUpdateState::Downloading { .. } => {
                unreachable!("filtered above while holding the lock")
            }
            PendingUpdateState::Ready { update, bytes, .. } => {
                info!(version = %update.version, size = bytes.len(), "installing pre-downloaded update");
                let total = bytes.len() as u64;
                let _ = on_event.send(DownloadEvent::Started {
                    content_length: Some(total),
                });
                let _ = on_event.send(DownloadEvent::Progress {
                    chunk_length: bytes.len(),
                });
                let _ = on_event.send(DownloadEvent::Finished);
                let _ = app.emit(UPDATE_PROGRESS_EVENT, DownloadEvent::Finished);

                match update.install(&bytes) {
                    Ok(()) => {
                        info!("update installed, restarting app");
                        app.restart();
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        error!(error = %err_str, "install from cached bytes failed");
                        // Only restore if the slot is still empty. A concurrent
                        // `check_for_update` may have observed `None` and
                        // written a newer `Available`/`Ready`; clobbering that
                        // would silently lose the newer version.
                        let mut guard = lock_state(&pending.0)?;
                        if matches!(&*guard, PendingUpdateState::None) {
                            *guard = PendingUpdateState::Ready {
                                update,
                                bytes,
                                downloaded_at: SystemTime::now(),
                            };
                        }
                        let _ = on_event.send(DownloadEvent::Failed {
                            error: err_str.clone(),
                        });
                        Err(err_str)
                    }
                }
            }
            PendingUpdateState::Available(update) => {
                info!(version = %update.version, "downloading+installing via fallback path");

                let mut first_chunk = true;
                let install_result = update
                    .download_and_install(
                        |chunk_length, content_length| {
                            if first_chunk {
                                first_chunk = false;
                                let _ = on_event
                                    .send(DownloadEvent::Started { content_length });
                            }
                            let _ = on_event.send(DownloadEvent::Progress { chunk_length });
                        },
                        || {
                            let _ = on_event.send(DownloadEvent::Finished);
                        },
                    )
                    .await;

                match install_result {
                    Ok(()) => {
                        info!("update installed, restarting app");
                        app.restart();
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        error!(error = %err_str, "download_and_install failed");
                        // See the `Ready` failure branch above for the
                        // conditional-restore rationale.
                        let mut guard = lock_state(&pending.0)?;
                        if matches!(&*guard, PendingUpdateState::None) {
                            *guard = PendingUpdateState::Available(update);
                        }
                        let _ = on_event.send(DownloadEvent::Failed {
                            error: err_str.clone(),
                        });
                        Err(err_str)
                    }
                }
            }
        }
    }
    .instrument(span)
    .await
}

/// Installation provenance of the running binary.
///
/// Used by the frontend to short-circuit in-app update when the user is on a
/// system-packaged Linux build: Tauri's Linux updater only supports
/// AppImage, so deb/rpm users must be routed to their package manager.
#[derive(Debug, Clone, Copy, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallKind {
    Macos,
    Windows,
    AppImage,
    Deb,
    Rpm,
    Unknown,
}

/// Dev-only: manually open the Sparkle-style updater window with mock data.
///
/// Wired to a debug-build button in `AboutSection.tsx` so we can iterate on
/// the window's UI without waiting for a real update to be detected. Release
/// builds short-circuit with an error so the command can't be misused —
/// `#[cfg]` is intentionally on the body, not the signature, to keep the
/// specta-generated TS surface stable across build profiles.
#[tauri::command]
#[specta::specta]
pub async fn dev_open_updater_window(
    app: AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let _ = _trace;
    #[cfg(debug_assertions)]
    {
        crate::update_scheduler::open_or_focus_updater_window(&app, true)
            .map_err(|err| format!("failed to open updater window: {err}"))
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = app;
        Err("dev_open_updater_window is only available in debug builds".to_string())
    }
}

/// Detect how the current binary was installed.
///
/// Cached after the first call. On Linux the detection asks dpkg/rpm whether
/// `current_exe()` is in their package DB — that way users on a mixed system
/// (e.g. apt + rpm side-by-side) get the right answer rather than a guess
/// based on `/etc/*-release`.
#[tauri::command]
#[specta::specta]
pub async fn get_install_kind(_trace: Option<TraceMetadata>) -> Result<InstallKind, String> {
    let _ = _trace;
    tokio::task::spawn_blocking(detect_install_kind)
        .await
        .map_err(|e| format!("install kind detection task panicked: {}", e))
}

#[cfg(target_os = "macos")]
pub(crate) fn detect_install_kind() -> InstallKind {
    InstallKind::Macos
}

#[cfg(target_os = "windows")]
pub(crate) fn detect_install_kind() -> InstallKind {
    InstallKind::Windows
}

#[cfg(target_os = "linux")]
pub(crate) fn detect_install_kind() -> InstallKind {
    use std::sync::OnceLock;
    static CACHE: OnceLock<InstallKind> = OnceLock::new();
    *CACHE.get_or_init(detect_install_kind_linux)
}

#[cfg(target_os = "linux")]
fn detect_install_kind_linux() -> InstallKind {
    // AppImage runtime exports the absolute AppImage path here. Trust it
    // over any path-based heuristic — the AppImage may be living anywhere.
    if std::env::var_os("APPIMAGE").is_some() {
        return InstallKind::AppImage;
    }

    let Ok(exe) = std::env::current_exe() else {
        return InstallKind::Unknown;
    };

    // Skip the shell-out for dev builds running out of `target/` — they
    // never belong to a package DB.
    let exe_str = exe.to_string_lossy();
    let is_system_prefix = exe_str.starts_with("/usr/")
        || exe_str.starts_with("/opt/")
        || exe_str.starts_with("/bin/")
        || exe_str.starts_with("/sbin/");
    if !is_system_prefix {
        return InstallKind::Unknown;
    }

    if std::process::Command::new("dpkg-query")
        .arg("-S")
        .arg(&exe)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return InstallKind::Deb;
    }

    if std::process::Command::new("rpm")
        .arg("-qf")
        .arg(&exe)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return InstallKind::Rpm;
    }

    InstallKind::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_channel_normalizes_case_and_unknown() {
        assert!(matches!(parse_channel("alpha"), UpdateChannel::Alpha));
        assert!(matches!(parse_channel("ALPHA"), UpdateChannel::Alpha));
        assert!(matches!(parse_channel("Beta"), UpdateChannel::Beta));
        assert!(matches!(parse_channel("rc"), UpdateChannel::Rc));
        assert!(matches!(parse_channel("stable"), UpdateChannel::Stable));
        assert!(matches!(parse_channel("unknown"), UpdateChannel::Stable));
        assert!(matches!(parse_channel(""), UpdateChannel::Stable));
    }

    #[test]
    fn channel_as_str_round_trips_through_parse_channel() {
        for ch in [
            UpdateChannel::Stable,
            UpdateChannel::Alpha,
            UpdateChannel::Beta,
            UpdateChannel::Rc,
        ] {
            let s = channel_as_str(&ch);
            let parsed = parse_channel(s);
            assert_eq!(
                std::mem::discriminant(&parsed),
                std::mem::discriminant(&ch),
                "channel round-trip mismatch for {:?}",
                s
            );
        }
    }

    #[test]
    fn default_state_is_none() {
        assert!(matches!(
            PendingUpdateState::default(),
            PendingUpdateState::None
        ));
        let p = PendingUpdate::new();
        assert!(matches!(*p.0.lock().unwrap(), PendingUpdateState::None));
    }

    #[test]
    fn download_event_wire_format_is_stable() {
        // The shape here matches the TS `DownloadEvent` union in `src/api/updater.ts`.
        // Changing it will silently break the existing dialog progress bar.
        let started = DownloadEvent::Started {
            content_length: Some(1_048_576),
        };
        assert_eq!(
            serde_json::to_string(&started).unwrap(),
            r#"{"event":"Started","data":{"contentLength":1048576}}"#
        );

        let progress = DownloadEvent::Progress {
            chunk_length: 16_384,
        };
        assert_eq!(
            serde_json::to_string(&progress).unwrap(),
            r#"{"event":"Progress","data":{"chunkLength":16384}}"#
        );

        assert_eq!(
            serde_json::to_string(&DownloadEvent::Finished).unwrap(),
            r#"{"event":"Finished"}"#
        );

        let failed = DownloadEvent::Failed {
            error: "boom".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&failed).unwrap(),
            r#"{"event":"Failed","data":{"error":"boom"}}"#
        );
    }

    #[test]
    fn progress_snapshot_default_is_idle() {
        let snap = DownloadProgressSnapshot::default();
        assert_eq!(snap.phase, DownloadPhase::Idle);
        assert_eq!(snap.downloaded, 0);
        assert!(snap.total.is_none());
        assert!(snap.version.is_none());
    }

    #[test]
    fn install_kind_wire_format_matches_frontend_union() {
        // The TS side has `type InstallKind = "macos" | "windows" | "appimage"
        // | "deb" | "rpm" | "unknown"`. Changing serde rename_all here will
        // silently desync the package-manager dialog routing.
        for (variant, expected) in [
            (InstallKind::Macos, r#""macos""#),
            (InstallKind::Windows, r#""windows""#),
            (InstallKind::AppImage, r#""appimage""#),
            (InstallKind::Deb, r#""deb""#),
            (InstallKind::Rpm, r#""rpm""#),
            (InstallKind::Unknown, r#""unknown""#),
        ] {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected);
        }
    }

    #[test]
    fn progress_snapshot_wire_format() {
        let snap = DownloadProgressSnapshot {
            phase: DownloadPhase::Downloading,
            downloaded: 4096,
            total: Some(1024 * 1024),
            version: Some("0.10.0".to_string()),
            current_version: "0.9.0".to_string(),
            body: Some("Bug fixes".to_string()),
            date: Some("2026-05-22T00:00:00Z".to_string()),
        };
        assert_eq!(
            serde_json::to_string(&snap).unwrap(),
            r#"{"phase":"downloading","downloaded":4096,"total":1048576,"version":"0.10.0","currentVersion":"0.9.0","body":"Bug fixes","date":"2026-05-22T00:00:00Z"}"#
        );
    }

    #[test]
    fn install_kind_for_telemetry_round_trips_wire_form() {
        // 两个 InstallKind（commands/updater.rs 与 uc-observability::analytics）必须
        // wire-equivalent（schema doc §7.9）。锁住映射后任何一侧加变体都会编译报错。
        for (src, expected_wire) in [
            (InstallKind::Macos, r#""macos""#),
            (InstallKind::Windows, r#""windows""#),
            (InstallKind::AppImage, r#""appimage""#),
            (InstallKind::Deb, r#""deb""#),
            (InstallKind::Rpm, r#""rpm""#),
            (InstallKind::Unknown, r#""unknown""#),
        ] {
            let mapped = install_kind_for_telemetry(src);
            assert_eq!(
                serde_json::to_string(&mapped).unwrap(),
                expected_wire,
                "install_kind_for_telemetry({:?}) wire form mismatch",
                src
            );
        }
    }

    #[test]
    fn download_error_kind_maps_to_short_identifiers() {
        // 短标识符 < 32 字符，无 URL / 路径 / IP（schema doc §6.1）。
        // Precondition 三个变体各自有专属 wire identifier，便于 dashboard
        // slicing；`Cancelled` 因为 outcome 已经表达终态，error_kind 返回 None
        // 避免双重信息。
        let cases = [
            (
                DownloadError::Precondition("updater: no pending update to download".into()),
                Some("no_pending_update"),
            ),
            (
                DownloadError::Precondition("updater: already downloading".into()),
                Some("already_downloading"),
            ),
            (
                DownloadError::Precondition("updater: already downloaded, ready to install".into()),
                Some("already_ready"),
            ),
            (
                DownloadError::Precondition("updater: unknown precondition".into()),
                Some("precondition"),
            ),
            (
                DownloadError::Cancelled("updater: download cancelled".into()),
                None,
            ),
            (
                DownloadError::Failed("network blew up".into()),
                Some("download_failed"),
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.error_kind(), expected);
            // 所有 identifier 都 < 32 字符，保证 PostHog property 值不爆字段长度。
            if let Some(kind) = expected {
                assert!(kind.len() < 32, "error_kind too long: {kind}");
            }
        }
    }

    #[test]
    fn classify_check_failure_buckets_common_strings() {
        // 优先级顺序：parse > http > network > other。覆盖 §7.9 四个枚举槽位。
        for (input, expected) in [
            (
                "signature verification failed",
                UpdateFailureKind::ParseError,
            ),
            ("minisign error", UpdateFailureKind::ParseError),
            ("failed to parse manifest", UpdateFailureKind::ParseError),
            ("invalid JSON in response", UpdateFailureKind::ParseError),
            ("base64 decode failed", UpdateFailureKind::ParseError),
            ("HTTP 404 Not Found", UpdateFailureKind::HttpError),
            (
                "server returned status code 500",
                UpdateFailureKind::HttpError,
            ),
            ("connection refused", UpdateFailureKind::Network),
            ("dns resolution failed", UpdateFailureKind::Network),
            ("tls handshake error", UpdateFailureKind::Network),
            ("operation timed out", UpdateFailureKind::Network),
            ("transport error", UpdateFailureKind::Network),
            ("something completely unexpected", UpdateFailureKind::Other),
            ("", UpdateFailureKind::Other),
        ] {
            assert_eq!(
                classify_check_failure(input),
                expected,
                "classify_check_failure({:?}) mismatch",
                input
            );
        }
    }
}

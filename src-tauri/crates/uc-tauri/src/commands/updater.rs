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

use crate::commands::record_trace_fields;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::UpdaterExt as _;
use tokio::sync::Notify;
use tracing::{error, info, info_span, Instrument};
use uc_core::settings::channel::detect_channel;
use uc_core::settings::model::UpdateChannel;
use uc_platform::ports::observability::TraceMetadata;

/// Tauri event channel name for broadcast download progress.
/// Subscribed by the frontend `UpdateContext` so any window/component can
/// reflect background download state, unlike the per-invocation
/// `tauri::ipc::Channel` which only delivers to its single creator.
pub const UPDATE_PROGRESS_EVENT: &str = "update-download-progress";

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
    pub version: Option<String>,
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
/// Side-effects on `PendingUpdate`:
/// - `Some(metadata)` returned & version matches an existing `Ready`: keep
///   cached bytes (refresh `Update` handle, preserve `downloaded_at`).
/// - `Some(metadata)` returned but version differs from `Ready`: discard
///   bytes, transition to `Available(new_update)`.
/// - `None` returned: transition to `None` (clear any prior cached bytes).
/// - State is `Downloading`: refuse to re-check (v1 simplification — wait
///   for the in-flight download to finish or be cancelled first).
#[tauri::command]
#[specta::specta]
pub async fn check_for_update(
    app: AppHandle,
    channel: Option<String>,
    pending: State<'_, PendingUpdate>,
    _trace: Option<TraceMetadata>,
) -> Result<Option<UpdateMetadata>, String> {
    let span = info_span!(
        "command.updater.check_for_update",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        {
            let guard = lock_state(&pending.0)?;
            if matches!(*guard, PendingUpdateState::Downloading { .. }) {
                return Err("updater: download in progress, cannot re-check".to_string());
            }
        }

        let resolved_channel = match channel {
            Some(ref s) => parse_channel(s),
            None => {
                let version = app.package_info().version.to_string();
                detect_channel(&version)
            }
        };
        let channel_str = channel_as_str(&resolved_channel);

        info!(channel = %channel_str, "checking for update");

        let primary_url: url::Url =
            format!("https://release.uniclipboard.app/{}.json", channel_str)
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

        let mut guard = lock_state(&pending.0)?;
        match update {
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
                        update: _prev_update,
                        bytes,
                        downloaded_at,
                    } if metadata.version == update.version => {
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
                Ok(Some(metadata))
            }
            None => {
                info!(channel = %channel_str, "no update available");
                *guard = PendingUpdateState::None;
                Ok(None)
            }
        }
    }
    .instrument(span)
    .await
}

/// Download the pending update in the background, broadcasting progress
/// via `UPDATE_PROGRESS_EVENT`. Awaitable: the future resolves when the
/// download completes, fails, or is cancelled.
///
/// Pre-condition: state must be `Available`. `Downloading` returns
/// "already downloading"; `Ready` returns "already downloaded"; `None`
/// returns "no pending update".
#[tauri::command]
#[specta::specta]
pub async fn download_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.updater.download_update",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let cancel = Arc::new(Notify::new());

        let (update, info) = {
            let mut guard = lock_state(&pending.0)?;
            match std::mem::take(&mut *guard) {
                PendingUpdateState::None => {
                    return Err("updater: no pending update to download".to_string());
                }
                PendingUpdateState::Available(update) => {
                    let info = metadata_of(&update);
                    let progress = DownloadProgressSnapshot {
                        phase: DownloadPhase::Downloading,
                        downloaded: 0,
                        total: None,
                        version: Some(info.version.clone()),
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
                    return Err("updater: already downloading".to_string());
                }
                other @ PendingUpdateState::Ready { .. } => {
                    *guard = other;
                    return Err("updater: already downloaded, ready to install".to_string());
                }
            }
        };

        info!(version = %info.version, "background download starting");

        let mut started_emitted = false;
        let app_for_chunk = app.clone();
        let pending_inner = pending.inner();

        let on_chunk = |chunk_length: usize, content_length: Option<u64>| {
            if !started_emitted {
                started_emitted = true;
                let _ = app_for_chunk.emit(
                    UPDATE_PROGRESS_EVENT,
                    DownloadEvent::Started { content_length },
                );
            }
            if let Ok(mut guard) = pending_inner.0.lock() {
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
                let mut guard = lock_state(&pending.0)?;
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
                let mut guard = lock_state(&pending.0)?;
                *guard = PendingUpdateState::Available(update);
                Err("updater: download cancelled".to_string())
            }
            Err(DownloadOutcome::Failed(err)) => {
                error!(version = %info.version, error = %err, "background download failed");
                let _ = app.emit(
                    UPDATE_PROGRESS_EVENT,
                    DownloadEvent::Failed { error: err.clone() },
                );
                let mut guard = lock_state(&pending.0)?;
                *guard = PendingUpdateState::Available(update);
                Err(err)
            }
        }
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
    pending: State<'_, PendingUpdate>,
    _trace: Option<TraceMetadata>,
) -> Result<DownloadProgressSnapshot, String> {
    let _ = _trace;
    let guard = lock_state(&pending.0)?;
    let snapshot = match &*guard {
        PendingUpdateState::None => DownloadProgressSnapshot::default(),
        PendingUpdateState::Available(update) => DownloadProgressSnapshot {
            phase: DownloadPhase::Available,
            downloaded: 0,
            total: None,
            version: Some(update.version.clone()),
        },
        PendingUpdateState::Downloading { progress, .. } => progress.clone(),
        PendingUpdateState::Ready { update, bytes, .. } => DownloadProgressSnapshot {
            phase: DownloadPhase::Ready,
            downloaded: bytes.len() as u64,
            total: Some(bytes.len() as u64),
            version: Some(update.version.clone()),
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
    fn progress_snapshot_wire_format() {
        let snap = DownloadProgressSnapshot {
            phase: DownloadPhase::Downloading,
            downloaded: 4096,
            total: Some(1024 * 1024),
            version: Some("0.10.0".to_string()),
        };
        assert_eq!(
            serde_json::to_string(&snap).unwrap(),
            r#"{"phase":"downloading","downloaded":4096,"total":1048576,"version":"0.10.0"}"#
        );
    }
}

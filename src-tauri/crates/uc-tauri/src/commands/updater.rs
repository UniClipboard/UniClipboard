//! Updater-related Tauri commands
//! 更新器相关的 Tauri 命令
//!
//! These commands provide channel-aware update checking and installation.
//! 这些命令提供频道感知的更新检查和安装功能。

use crate::commands::record_trace_fields;
use serde::Serialize;
use std::sync::Mutex;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_updater::UpdaterExt as _;
use tracing::{error, info, info_span, Instrument};
use uc_core::ports::observability::TraceMetadata;
use uc_core::settings::channel::detect_channel;
use uc_core::settings::model::UpdateChannel;

/// Events emitted during update download via `tauri::ipc::Channel`.
/// 更新下载期间通过 `tauri::ipc::Channel` 发出的事件。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum DownloadEvent {
    #[serde(rename_all = "camelCase")]
    Started {
        content_length: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Progress {
        chunk_length: usize,
    },
    Finished,
    #[serde(rename_all = "camelCase")]
    Failed {
        error: String,
    },
}

/// Holds a pending update ready for installation.
/// 保存等待安装的挂起更新。
pub struct PendingUpdate(pub Mutex<Option<tauri_plugin_updater::Update>>);

/// Metadata returned to the frontend when an update is available.
/// 当有可用更新时返回给前端的元数据。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetadata {
    pub version: String,
    pub current_version: String,
    pub body: Option<String>,
    pub date: Option<String>,
}

/// Convert an `UpdateChannel` to its URL path segment string.
/// 将 `UpdateChannel` 转换为其 URL 路径段字符串。
fn channel_as_str(channel: &UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Alpha => "alpha",
        UpdateChannel::Beta => "beta",
        UpdateChannel::Rc => "rc",
    }
}

/// Parse a channel name string into an `UpdateChannel`.
/// 将频道名称字符串解析为 `UpdateChannel`。
fn parse_channel(s: &str) -> UpdateChannel {
    match s.to_ascii_lowercase().as_str() {
        "alpha" => UpdateChannel::Alpha,
        "beta" => UpdateChannel::Beta,
        "rc" => UpdateChannel::Rc,
        _ => UpdateChannel::Stable,
    }
}

/// Check for an available update on the specified (or auto-detected) channel.
/// 检查指定（或自动检测）频道上的可用更新。
///
/// ## Arguments / 参数
///
/// - `channel`: Optional channel override (`"stable"`, `"alpha"`, `"beta"`, `"rc"`).
///   When absent, the channel is inferred from the running version string.
///
/// ## Returns / 返回值
///
/// - `Some(UpdateMetadata)` if a newer version is available on that channel.
/// - `None` if the app is already up to date.
#[tauri::command]
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
        // Resolve the update channel
        let resolved_channel = match channel {
            Some(ref s) => parse_channel(s),
            None => {
                let version = app.package_info().version.to_string();
                detect_channel(&version)
            }
        };
        let channel_str = channel_as_str(&resolved_channel);

        info!(channel = %channel_str, "checking for update");

        // Primary: Cloudflare Worker
        let primary_url: url::Url =
            format!("https://release.uniclipboard.app/{}.json", channel_str)
                .parse()
                .map_err(|e| format!("Invalid primary updater URL: {}", e))?;

        // Fallback: GitHub Pages
        let fallback_url: url::Url = format!(
            "https://uniclipboard.github.io/UniClipboard/{}.json",
            channel_str
        )
        .parse()
        .map_err(|e| format!("Invalid fallback updater URL: {}", e))?;

        // Build updater with dual endpoints (tries in order, first success wins)
        let updater = app
            .updater_builder()
            .endpoints(vec![primary_url, fallback_url])
            .map_err(|e| e.to_string())?
            .build()
            .map_err(|e| e.to_string())?;

        let update = updater.check().await.map_err(|e| e.to_string())?;

        match update {
            Some(update) => {
                info!(
                    channel = %channel_str,
                    new_version = %update.version,
                    "update available"
                );
                let metadata = UpdateMetadata {
                    version: update.version.clone(),
                    current_version: update.current_version.clone(),
                    body: update.body.clone(),
                    date: update.date.map(|d| d.to_string()),
                };
                // Persist the update so install_update can retrieve it
                let mut guard = pending
                    .0
                    .lock()
                    .map_err(|e| format!("Failed to lock pending update: {}", e))?;
                *guard = Some(update);
                Ok(Some(metadata))
            }
            None => {
                info!(channel = %channel_str, "no update available");
                Ok(None)
            }
        }
    }
    .instrument(span)
    .await
}

/// Download and install a previously checked pending update, then restart the app.
/// 下载并安装之前检查的挂起更新，然后重启应用程序。
///
/// Must be called after `check_for_update` has found an available update.
/// 必须在 `check_for_update` 找到可用更新后调用。
#[tauri::command]
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
        // Take the pending update out of the state
        let update = {
            let mut guard = pending
                .0
                .lock()
                .map_err(|e| format!("Failed to lock pending update: {}", e))?;
            guard.take()
        };

        let update = update.ok_or_else(|| "No pending update available".to_string())?;

        info!(new_version = %update.version, "installing update");

        let mut first_chunk = true;
        update
            .download_and_install(
                |chunk_length, content_length| {
                    if first_chunk {
                        first_chunk = false;
                        if let Err(e) = on_event.send(DownloadEvent::Started { content_length }) {
                            let send_err = e.to_string();
                            error!(
                                event = "Started",
                                content_length,
                                error = %send_err,
                                "failed to send download event"
                            );
                            if let Err(fallback_err) = on_event.send(DownloadEvent::Failed {
                                error: format!("Failed to send Started event: {}", send_err),
                            }) {
                                error!(
                                    event = "Failed",
                                    original_event = "Started",
                                    content_length,
                                    error = %fallback_err,
                                    "failed to send fallback failure event"
                                );
                            }
                        }
                    }
                    if let Err(e) = on_event.send(DownloadEvent::Progress { chunk_length }) {
                        let send_err = e.to_string();
                        error!(
                            event = "Progress",
                            chunk_length,
                            content_length,
                            error = %send_err,
                            "failed to send download event"
                        );
                        if let Err(fallback_err) = on_event.send(DownloadEvent::Failed {
                            error: format!("Failed to send Progress event: {}", send_err),
                        }) {
                            error!(
                                event = "Failed",
                                original_event = "Progress",
                                chunk_length,
                                content_length,
                                error = %fallback_err,
                                "failed to send fallback failure event"
                            );
                        }
                    }
                },
                || {
                    if let Err(e) = on_event.send(DownloadEvent::Finished) {
                        let send_err = e.to_string();
                        error!(
                            event = "Finished",
                            error = %send_err,
                            "failed to send download event"
                        );
                        if let Err(fallback_err) = on_event.send(DownloadEvent::Failed {
                            error: format!("Failed to send Finished event: {}", send_err),
                        }) {
                            error!(
                                event = "Failed",
                                original_event = "Finished",
                                error = %fallback_err,
                                "failed to send fallback failure event"
                            );
                        }
                    }
                },
            )
            .await
            .map_err(|e| e.to_string())?;

        info!("update installed, restarting app");
        app.restart();
    }
    .instrument(span)
    .await
}

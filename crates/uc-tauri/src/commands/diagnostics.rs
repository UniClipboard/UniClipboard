//! Native startup support that remains available when the daemon is offline.

use tauri_plugin_dialog::DialogExt;
use tracing::{info_span, Instrument};

use crate::commands::{record_trace_fields, CommandError, TraceMetadata};

async fn startup_status_for_diagnostics() -> Option<serde_json::Value> {
    let client = match uc_daemon_client::build_local_http_client_with_timeout(
        uc_desktop::daemon_probe::PROBE_TIMEOUT,
    ) {
        Ok(client) => client,
        Err(_) => {
            tracing::warn!(
                error_kind = "startup_status_client_failed",
                retryable = true,
                "startup status could not be included in diagnostics"
            );
            return None;
        }
    };
    let status = match uc_desktop::startup::read_startup_status(&client).await {
        Ok(Some(status)) => status,
        Ok(None) => return None,
        Err(_) => {
            tracing::warn!(
                error_kind = "startup_status_unavailable",
                retryable = true,
                "startup status could not be included in diagnostics"
            );
            return None;
        }
    };
    match serde_json::to_value(status) {
        Ok(status) => Some(status),
        Err(_) => {
            tracing::warn!(
                error_kind = "startup_status_serialize_failed",
                retryable = false,
                "startup status could not be included in diagnostics"
            );
            None
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn export_startup_logs(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<Option<String>, CommandError> {
    let span = info_span!(
        "command.diagnostics.export_startup_logs",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async move {
        let startup_status = startup_status_for_diagnostics().await;
        let logs_dir = runtime.desktop().storage_paths().logs_dir.clone();
        let worker_span = tracing::Span::current();
        let result = tauri::async_runtime::spawn_blocking(move || {
            worker_span.in_scope(|| -> anyhow::Result<Option<String>> {
                let name = format!(
                    "uniclipboard-logs-{}.zip",
                    chrono::Utc::now().format("%Y%m%d-%H%M%S")
                );
                let Some(selected) = app
                    .dialog()
                    .file()
                    .set_file_name(name)
                    .add_filter("ZIP archive", &["zip"])
                    .blocking_save_file()
                else {
                    tracing::info!("startup log export cancelled");
                    return Ok(None);
                };
                let destination = selected
                    .into_path()
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                uc_observability::startup_logs::export_startup_logs_with_status(
                    &logs_dir,
                    &destination,
                    startup_status,
                )?;
                tracing::info!("startup logs exported");
                Ok(Some(destination.to_string_lossy().into_owned()))
            })
        })
        .await;
        result
            .map_err(anyhow::Error::from)
            .and_then(|result| result)
            .map_err(|error| {
                tracing::error!(
                    error_kind = "startup_log_export_failed",
                    error = "Could not create startup log archive",
                    retryable = true,
                    "startup log export failed"
                );
                CommandError::InternalError(error.to_string())
            })
    }
    .instrument(span)
    .await
}

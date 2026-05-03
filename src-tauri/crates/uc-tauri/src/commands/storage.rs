//! Storage management Tauri commands
//! 存储管理相关的 Tauri 命令

use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;
use tauri_plugin_opener::OpenerExt;
use tracing::{info_span, Instrument};
use uc_platform::ports::observability::TraceMetadata;

/// Open the application data directory in the system file manager.
/// 在系统文件管理器中打开应用数据目录。
#[tauri::command]
pub async fn open_data_directory(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.storage.open_data_dir",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let dir = runtime.storage_paths().app_data_root_dir.clone();
        if !dir.exists() {
            return Err(CommandError::NotFound(format!(
                "Directory does not exist: {}",
                dir.display()
            )));
        }

        app.opener()
            .open_path(dir.to_string_lossy(), None::<&str>)
            .map_err(|e| CommandError::InternalError(e.to_string()))?;

        tracing::info!(dir = %dir.display(), "Opened data directory");
        Ok(())
    }
    .instrument(span)
    .await
}

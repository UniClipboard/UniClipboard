pub mod autostart;
pub mod error;
pub mod quick_panel;
pub mod restart;
pub mod startup;
pub mod storage;
pub mod tray;
pub mod updater;

use tracing::Span;
use uc_platform::ports::observability::TraceMetadata;

/// Get the OS process ID of the Tauri application.
///
/// 获取 Tauri 应用的操作系统进程 ID。
#[tauri::command]
pub fn get_tauri_pid() -> u32 {
    std::process::id()
}

/// Get the stable local device identifier used for telemetry correlation.
#[tauri::command]
pub async fn get_device_id(
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<String, CommandError> {
    Ok(runtime.device_id())
}

// Re-export commonly used types
pub use autostart::*;

pub use startup::*;
pub use storage::*;
pub use updater::*;

pub use error::CommandError;

pub(crate) fn record_trace_fields(span: &Span, trace: &Option<TraceMetadata>) {
    if let Some(metadata) = trace.as_ref() {
        span.record("trace_id", tracing::field::display(&metadata.trace_id));
        span.record("trace_ts", metadata.timestamp);
    }
}

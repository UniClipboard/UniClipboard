//! Lifecycle-related Tauri commands
//! 应用生命周期相关的 Tauri 命令

use crate::bootstrap::AppRuntime;
use crate::commands::record_trace_fields;
use std::sync::Arc;
use tauri::State;
use tracing::{info_span, Instrument};
use uc_core::ports::observability::TraceMetadata;

/// Retry lifecycle boot (watcher + network + session ready).
///
/// 重试生命周期启动（监视器 + 网络 + 会话就绪）。
#[tauri::command]
pub async fn retry_lifecycle(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.lifecycle.retry",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        runtime
            .usecases()
            .app_lifecycle_coordinator()
            .ensure_ready()
            .await
            .map_err(|e| e.to_string())
    }
    .instrument(span)
    .await
}

/// Get current lifecycle status as JSON.
///
/// 获取当前生命周期状态（JSON）。
#[tauri::command]
pub async fn get_lifecycle_status(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<String, String> {
    let span = info_span!(
        "command.lifecycle.get_status",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let status_port = runtime.usecases().get_lifecycle_status();
        let state = status_port.get_state().await;
        serde_json::to_string(&state).map_err(|e| e.to_string())
    }
    .instrument(span)
    .await
}

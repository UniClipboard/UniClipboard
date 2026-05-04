//! Restart-related Tauri commands.
//! 重启相关的 Tauri 命令。
//!
//! Phase 95: covers GUI mode only (D-B1). CLI daemon mode is out of scope.

use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;
use tracing::{info, info_span, Instrument};
use uc_platform::ports::observability::TraceMetadata;

/// Restarts the running Tauri application to apply settings changes.
///
/// This triggers a graceful application restart intended for GUI mode only. If `_trace` is
/// provided, its fields are attached to the command's tracing span for correlation.
///
/// # Parameters
///
/// - `app`: the application handle used to initiate the restart.
/// - `_trace`: optional trace metadata to record on the restart span.
///
/// # Returns
///
/// `Ok(())` when the restart command is issued (control may not return because the process exits).
///
/// # Examples
///
/// ```no_run
/// // Trigger a graceful restart (example; requires a valid `tauri::AppHandle` in an async context)
/// let _ = restart_app(app_handle, None).await;
/// ```
#[tauri::command]
pub async fn restart_app(
    app: tauri::AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.restart.restart_app",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        info!("restarting app for settings change (LAN-only Mode)");
        app.restart();
        // app.restart() 会调用 process exit；以下不可达，仅满足类型签名。
        #[allow(unreachable_code)]
        Ok(())
    }
    .instrument(span)
    .await
}

// ===== Phase 95 边界 fence =====
//
// 1. D-B1: 仅 cover GUI mode。本文件 NOT 暴露任何 daemon HTTP admin/restart 端点。
//    CLI daemon (`uniclip daemon`) 用户走 systemctl/launchd（PROJECT.md §Out of Scope）。
//
// 2. Pitfall 5 防御: 本文件 NOT 引用 telemetry / OTLP / pkarr / auto-update 任何字段；
//    `restart_app` 只是 `app.restart()` thin wrapper，没有副作用越界（不 disable 遥测、不 reset state）。
//
// 3. 历史注记：原 `get_restart_state` + `PROCESS_STARTED_AT` 用于 mtime-based 跨 session
//    pending 推导（D-D1），但 mtime 无法区分 settings.json 中具体改动的字段，会在用户改
//    其它设置后误报 LAN-only pending。已改为前端 in-memory pending（仅当前 session 内切
//    换后显示）—— 见 `NetworkSection.tsx` 顶部 jsdoc。

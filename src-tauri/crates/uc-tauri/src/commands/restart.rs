//! Restart-related Tauri commands.
//! 重启相关的 Tauri 命令。
//!
//! 所有"需要重启"设置 (LAN-only Mode / mobile_sync 端口等) 走进程级
//! `app.restart()`。决策背景见
//! `.planning/quick/260510-phase4-deps-share/findings.md` §0 (方案 C)。
//!
//! Phase 95 边界 fence:
//!
//! 1. D-B1: 仅 cover GUI mode。本文件 NOT 暴露任何 daemon HTTP admin/restart
//!    端点。CLI daemon (`uniclip daemon`) 用户走 systemctl/launchd
//!    (PROJECT.md §Out of Scope)。
//! 2. Pitfall 5 防御: 本文件 NOT 引用 telemetry / OTLP / pkarr / auto-update
//!    任何字段;`restart_app` 只是 `app.restart()` 的包装(加 graceful
//!    shutdown),没有副作用越界(不 disable 遥测、不 reset state)。

use std::time::Duration;

use tauri::Emitter;
use tracing::{info, info_span, warn, Instrument};
use uc_platform::ports::observability::TraceMetadata;

use crate::commands::record_trace_fields;
use crate::run::{FRONTEND_SHUTDOWN_EVENT, SHUTDOWN_FRONTEND_GRACE_MS};

/// Restarts the running Tauri application to apply settings changes.
///
/// 流程:
/// 1. emit `app://shutting-down` → 前端 disconnect WebSocket
/// 2. wait `SHUTDOWN_FRONTEND_GRACE_MS` 让 WS close frame 飞过 loopback
/// 3. `app.restart()` —— Tauri spawn 新进程 + exit 当前进程
///
/// ADR-008 P3-3 (B2'-3): GUI 是外部 daemon 的纯客户端,重启**只重启 GUI 进程**,
/// daemon 作为独立进程留守——新 GUI 起来后 probe→reconnect 即可,不存在旧的
/// in-process daemon 占着端口的问题(那是 in-process 模型的历史约束)。所以这里
/// 不再 graceful-shutdown daemon;只通知前端断 WS 让 daemon 端尽快释放旧连接。
///
/// 注意(ADR-008 P3-3 遗留,P4 处理):部分"需要重启"设置(LAN-only Mode /
/// mobile_sync 端口等)实际改的是 **daemon 侧** iroh/网络 bind,而本命令只重启
/// GUI 进程、不再重启 daemon——所以这些设置不会因 GUI 重启而在 daemon 侧重新
/// 生效。daemon 侧的 re-bind / 重启编排属于 ADR-008 D16(setup→operational =
/// 重启 daemon)与 P4 的范畴,本命令不承担。
#[tauri::command]
#[specta::specta]
pub async fn restart_app(
    app: tauri::AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), crate::commands::error::CommandError> {
    let span = info_span!(
        "command.restart.restart_app",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        perform_restart(&app).await;
        #[allow(unreachable_code)]
        Ok(())
    }
    .instrument(span)
    .await
}

/// Graceful shutdown + `app.restart()`.
///
/// Shared entry for the `restart_app` Tauri command and the tray "Restart"
/// menu item. Does not return — `app.restart()` internally calls
/// `std::process::exit`. Callers must therefore expect this future to never
/// complete on the happy path.
pub(crate) async fn perform_restart(app: &tauri::AppHandle) {
    info!("restarting app for settings change");

    // graceful shutdown daemon 前先通知前端断 WS,让 axum
    // `with_graceful_shutdown` 立即返回不等 30s heartbeat。
    if let Err(error) = app.emit(FRONTEND_SHUTDOWN_EVENT, ()) {
        warn!(
            error = %error,
            event = FRONTEND_SHUTDOWN_EVENT,
            "failed to emit shutdown hint to frontend before restart; daemon \
             graceful shutdown will fall back to heartbeat-driven WS disconnect"
        );
    }

    // 给前端 close frame 飞过 loopback 的时间。daemon 是独立进程,这里不再
    // 停它(ADR-008 P3-3 B2'-3:GUI 纯客户端,重启只重启 GUI 进程)。
    tokio::time::sleep(Duration::from_millis(SHUTDOWN_FRONTEND_GRACE_MS)).await;

    // 新进程 spawn + 当前进程 exit。app.restart() 内部调用
    // std::process::exit,后续代码不可达。
    app.restart();
}

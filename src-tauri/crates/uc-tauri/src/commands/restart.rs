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

use tauri::{Emitter, Manager};
use tracing::{error, info, info_span, warn, Instrument};
use uc_desktop::DaemonOwnership;
use uc_platform::ports::observability::TraceMetadata;

use crate::commands::record_trace_fields;
use crate::run::{DAEMON_SHUTDOWN_TIMEOUT, FRONTEND_SHUTDOWN_EVENT, SHUTDOWN_FRONTEND_GRACE_MS};

/// Restarts the running Tauri application to apply settings changes.
///
/// 流程:
/// 1. emit `app://shutting-down` → 前端 disconnect WebSocket
/// 2. wait `SHUTDOWN_FRONTEND_GRACE_MS` 让 WS close frame 飞过 loopback
/// 3. graceful shutdown owned daemon (释放 HTTP / LAN 端口),最长
///    [`DAEMON_SHUTDOWN_TIMEOUT`] 兜底
/// 4. `app.restart()` —— Tauri spawn 新进程 + exit 当前进程
///
/// 不走 Tauri 的 `RunEvent::ExitRequested` 路径 —— `app.restart()` 是 raw
/// exit,不触发 RunEvent。所以这里**必须**自己重做一次 graceful shutdown,
/// 否则新进程启动时旧 daemon 还在持端口 → bind 失败 → daemon 起不来。
///
/// 用户的"需要重启"设置(LAN-only Mode / mobile_sync 端口等)都走这条
/// 路径。iroh `IrohNodeBuilder::bind` 是进程级单次约束(Pitfall 3),
/// 任何涉及 iroh_config 变更的 settings 必须新进程重新 bind,所以
/// `app.restart()` 是合适的、唯一的入口。
#[tauri::command]
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

        // 给前端 close frame 飞过 loopback 的时间。
        tokio::time::sleep(Duration::from_millis(SHUTDOWN_FRONTEND_GRACE_MS)).await;

        // 主动 graceful shutdown owned daemon,等到 HTTP / LAN 端口完全
        // 释放再 spawn 新进程 —— 否则新进程 daemon bind 撞 WSAEADDRINUSE,
        // 即便 Phase 1 已修了连带 panic,daemon 仍然起不来。
        let ownership = app.state::<DaemonOwnership>().inner().clone();
        if let Some(handle) = ownership.take_owned() {
            match handle.shutdown(DAEMON_SHUTDOWN_TIMEOUT).await {
                Ok(()) => info!("daemon stopped before restart"),
                Err(err) => error!(
                    error = %err,
                    "daemon shutdown failed before restart; new process may fail to bind"
                ),
            }
        }

        // 新进程 spawn + 当前进程 exit。app.restart() 内部调用
        // std::process::exit,以下代码不可达。
        app.restart();
        #[allow(unreachable_code)]
        Ok(())
    }
    .instrument(span)
    .await
}

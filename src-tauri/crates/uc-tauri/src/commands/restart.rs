//! Restart-related Tauri commands.
//! 重启相关的 Tauri 命令。
//!
//! 两条路径:
//! - [`restart_app`] —— 进程级重启(`tauri::AppHandle::restart`)。在 in-process
//!   daemon 模型下会引发新旧 GUI 进程同时持端口的窗口,新进程 bind 失败
//!   触发 daemon panic(详见 `uc-desktop/src/daemon/app.rs` 关于
//!   `JoinHandle polled after completion` 的注释)。**仅作为兜底通道保留**,
//!   配置变更触发的"重启"应优先走 [`restart_daemon`]。
//! - [`restart_daemon`] —— in-process daemon-only 重启:关掉旧 daemon、起
//!   新 daemon、等健康、刷新 connection info、通知前端。GUI 进程不重启。
//!
//! Phase 95: covers GUI mode only (D-B1). CLI daemon mode is out of scope.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{Emitter, Manager};
use tracing::{error, info, info_span, warn, Instrument};
use uc_daemon_client::DaemonConnectionState;
use uc_desktop::daemon::ProcessRuntimeHandles;
use uc_desktop::daemon_probe::{
    reload_in_process_daemon, ReloadInProcessDaemonError, HEALTH_CHECK_TIMEOUT,
    HEALTH_POLL_INTERVAL,
};
use uc_desktop::DaemonOwnership;
use uc_platform::ports::observability::TraceMetadata;

use crate::commands::record_trace_fields;

/// 这个 GUI shell 期望 daemon 上报的 `packageVersion` —— 与 `run.rs::EXPECTED_PACKAGE_VERSION`
/// 同源（`uc-tauri` 自己的 cargo 版本）。两处独立读 `env!` 而不集中导出，是因为
/// 跨模块 `pub const` 仍然在编译期复制；统一来源对二进制大小无收益,反而增加耦合。
const EXPECTED_PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// daemon graceful shutdown 上限。同 `run.rs::DAEMON_SHUTDOWN_TIMEOUT`,覆盖
/// daemon 内部 5s service join + 5s http graceful + service.stop() 串行的最坏路径。
const DAEMON_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);

/// 给前端响应 `app://daemon-restarting` 事件、断开 WS 的时间。
/// 与 `run.rs::SHUTDOWN_FRONTEND_GRACE_MS` 同语义,值取齐。
const FRONTEND_DISCONNECT_GRACE: Duration = Duration::from_millis(100);

/// daemon 即将关闭。前端据此 `daemonWs.disconnect()`,让 daemon 端的
/// axum `with_graceful_shutdown` 立即返回(不等 30s heartbeat)。
const DAEMON_RESTARTING_EVENT: &str = "app://daemon-restarting";

/// 新 daemon 已健康。前端据此重新走 connectDaemonWs() 拉新 connection_info
/// + JWT session,而不是用旧 token 直接 reconnect(token 在 daemon 重启时已轮换)。
const DAEMON_READY_EVENT: &str = "app://daemon-ready";

/// 前端可 pattern-match 的 typed 错误。区分"外部 daemon 不能动"、"shutdown 失败"、
/// "新 daemon 起不来"三类:第一类 UI 提示用户自己重启外部 daemon;后两类提示内部
/// 错误并提供"完整重启进程"作为兜底。
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(tag = "code", content = "message")]
pub enum RestartDaemonError {
    /// daemon 是外部进程(`cli daemon` 拉起的独立 binary 等),GUI 不能动它。
    #[error("daemon is external; restart it from its own entry point (CLI / system service)")]
    ExternalDaemon,
    #[error("shutdown of previous daemon failed: {0}")]
    ShutdownFailed(String),
    #[error("failed to start new daemon: {0}")]
    BootstrapFailed(String),
}

impl From<ReloadInProcessDaemonError> for RestartDaemonError {
    fn from(err: ReloadInProcessDaemonError) -> Self {
        match err {
            ReloadInProcessDaemonError::NotOwned => Self::ExternalDaemon,
            ReloadInProcessDaemonError::Shutdown(e) => Self::ShutdownFailed(e.to_string()),
            ReloadInProcessDaemonError::Bootstrap(e) => Self::BootstrapFailed(e.to_string()),
        }
    }
}

/// Restarts the running Tauri application to apply settings changes.
///
/// Process-level restart via `tauri::AppHandle::restart()`. **Prefer
/// [`restart_daemon`]** — the daemon-only path avoids the new-process-vs-old-process
/// port-conflict window that this command exhibits when an in-process daemon
/// owns the HTTP listener.
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
        info!("restarting app for settings change (LAN-only Mode)");
        app.restart();
        // app.restart() 会调用 process exit；以下不可达，仅满足类型签名。
        #[allow(unreachable_code)]
        Ok(())
    }
    .instrument(span)
    .await
}

/// In-process daemon 重启 —— 配置变更触发"需要重启"时的推荐路径。
///
/// 流程:
/// 1. emit `app://daemon-restarting` → 前端 `daemonWs.disconnect()`
/// 2. 给前端 100ms 发 close frame
/// 3. [`reload_in_process_daemon`] 串行做:take_owned → shutdown → start_in_process
///    → set_owned → wait_for_daemon_health → load_daemon_connection_info
/// 4. 用新 connection_info 覆盖 [`DaemonConnectionState`]
/// 5. emit `app://daemon-ready` → 前端 `connectDaemonWs()` 重新 wait + connect
///
/// 失败模式:
/// - [`RestartDaemonError::ExternalDaemon`]:daemon 是外部进程,本命令不动它。
/// - [`RestartDaemonError::ShutdownFailed`]:旧 daemon graceful shutdown 失败,
///   新 daemon **未启动**;旧 daemon 处于不确定状态,前端应提示用户彻底
///   重启进程。
/// - [`RestartDaemonError::BootstrapFailed`]:新 daemon 启动 / 健康探测失败。
///
/// 任何失败路径都会先 emit `app://daemon-ready` 让前端解除"重启中"UI(否则
/// UI 卡死) —— 即便 daemon 实际不健康,frontend reconnect 会自己再失败提示。
#[tauri::command]
pub async fn restart_daemon(
    app: tauri::AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), RestartDaemonError> {
    let span = info_span!(
        "command.restart.restart_daemon",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        info!("restarting in-process daemon for settings change");

        // Tauri State 必须在 async block 内取——AppHandle 提供 state<T>() 接口
        // (run.rs 通过 .manage() 注册了 DaemonOwnership / DaemonConnectionState)。
        let ownership = app.state::<DaemonOwnership>().inner().clone();
        let daemon_conn = app.state::<DaemonConnectionState>().inner().clone();
        // 进程级 AppFacade(GUI shell 启动时装的唯一一份);daemon reload 在它上面
        // swap 5 个 daemon-lifecycle 子 facade。
        let app_facade = Arc::clone(
            app.state::<Arc<crate::bootstrap::TauriAppRuntime>>()
                .inner()
                .app_facade(),
        );
        // 进程级一次性资源 (sqlite pool / repos / settings / blob workers /
        // clipboard_write_coordinator / file_transfer_lifecycle 等);
        // 由 run.rs 在启动期 .manage() 注册。daemon reload 复用同一份,
        // sqlite pool 等不会因 daemon 重启而被销毁重建。
        let process_handles = app.state::<ProcessRuntimeHandles>().inner().clone();

        // 1. 通知前端断 WS。emit 失败仅 warn — 不阻断 reload。
        if let Err(error) = app.emit(DAEMON_RESTARTING_EVENT, ()) {
            warn!(
                error = %error,
                event = DAEMON_RESTARTING_EVENT,
                "failed to emit daemon-restarting hint to frontend; \
                 daemon graceful shutdown will fall back to heartbeat-driven WS disconnect"
            );
        }

        // 2. 给前端 close frame 飞过 loopback 的时间。
        tokio::time::sleep(FRONTEND_DISCONNECT_GRACE).await;

        // 3. take_owned → shutdown → start_in_process → wait_for_health → load_info
        let reload_result = reload_in_process_daemon(
            &ownership,
            EXPECTED_PACKAGE_VERSION,
            DAEMON_SHUTDOWN_TIMEOUT,
            HEALTH_CHECK_TIMEOUT,
            HEALTH_POLL_INTERVAL,
            app_facade,
            process_handles,
        )
        .await;

        // 4. 不论成功失败都先发 daemon-ready 让前端解锁 UI(失败的话前端 reconnect 会
        //    自己再报错)。失败后才 propagate 错误给 caller。
        match reload_result {
            Ok(connection_info) => {
                daemon_conn.set(connection_info);
                if let Err(error) = app.emit(DAEMON_READY_EVENT, ()) {
                    warn!(
                        error = %error,
                        event = DAEMON_READY_EVENT,
                        "failed to emit daemon-ready hint to frontend; \
                         frontend WS reconnect may stall waiting for the signal"
                    );
                }
                info!("in-process daemon reload completed");
                Ok(())
            }
            Err(reload_err) => {
                // 即便失败也先 emit ready,让前端解除"重启中"loading 态;
                // 实际 reconnect 会在 daemon 不健康时自己失败并提示。
                if let Err(emit_err) = app.emit(DAEMON_READY_EVENT, ()) {
                    warn!(
                        error = %emit_err,
                        event = DAEMON_READY_EVENT,
                        "failed to emit daemon-ready hint after reload failure"
                    );
                }
                error!(error = %reload_err, "in-process daemon reload failed");
                Err(reload_err.into())
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_daemon_error_external_daemon_serializes_with_code() {
        let err = RestartDaemonError::ExternalDaemon;
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "ExternalDaemon");
    }

    #[test]
    fn restart_daemon_error_shutdown_failed_carries_message() {
        let err = RestartDaemonError::ShutdownFailed("graceful shutdown timed out".to_string());
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "ShutdownFailed");
        assert!(json["message"].as_str().unwrap().contains("timed out"));
    }

    #[test]
    fn from_reload_error_not_owned_maps_to_external_daemon() {
        let mapped: RestartDaemonError = ReloadInProcessDaemonError::NotOwned.into();
        assert!(matches!(mapped, RestartDaemonError::ExternalDaemon));
    }

    #[test]
    fn from_reload_error_shutdown_maps_to_shutdown_failed_with_chained_message() {
        let mapped: RestartDaemonError =
            ReloadInProcessDaemonError::Shutdown(anyhow::anyhow!("boom shutdown")).into();
        match mapped {
            RestartDaemonError::ShutdownFailed(msg) => {
                assert!(msg.contains("boom shutdown"))
            }
            other => panic!("expected ShutdownFailed, got {other:?}"),
        }
    }
}

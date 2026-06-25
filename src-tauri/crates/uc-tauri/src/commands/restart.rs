//! Restart-related Tauri commands.
//! 重启相关的 Tauri 命令。
//!
//! 两个入口：
//!
//! - [`restart_app`]：重启 GUI 进程（历史命令，QuickPanel 资源清理等仍用）。
//! - [`restart_daemon`]：仅重启 daemon 进程（network 等 bind-time 设置变更后
//!   调用，GUI 保持不动）。
//!
//! D-B1: 仅 cover GUI mode。CLI daemon (`uniclip daemon`) 用户走
//! systemctl/launchd (PROJECT.md §Out of Scope)。

use std::time::Duration;

use tauri::Emitter;
use tracing::{info, info_span, warn, Instrument};
use uc_core::ports::observability::TraceMetadata;
use uc_daemon_client::DaemonConnectionState;

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
/// Shared entry for the `restart_app` Tauri command and the GUI half of
/// [`perform_full_restart`]. Does not return — `app.restart()` internally
/// calls `std::process::exit`. Callers must therefore expect this future to
/// never complete on the happy path.
pub(crate) async fn perform_restart(app: &tauri::AppHandle) {
    info!("restarting app for settings change");

    if let Err(error) = app.emit(FRONTEND_SHUTDOWN_EVENT, ()) {
        warn!(
            error = %error,
            event = FRONTEND_SHUTDOWN_EVENT,
            "failed to emit shutdown hint to frontend before restart; daemon \
             graceful shutdown will fall back to heartbeat-driven WS disconnect"
        );
    }

    tokio::time::sleep(Duration::from_millis(SHUTDOWN_FRONTEND_GRACE_MS)).await;

    app.restart();
}

/// Restart **both** the daemon and the GUI process.
///
/// Shared entry for the tray "Restart" menu item. Mirrors the frontend
/// full-restart sequence (`restart_daemon` → `restart_app`): the daemon is
/// restarted first via [`restart_local_daemon`] so a freshly-spawned, healthy
/// engine is already running by the time the replacement GUI bootstraps and
/// reconnects, then the GUI process is replaced via [`perform_restart`].
///
/// Unlike the `restart_daemon` Tauri command we do **not** refresh the live
/// `DaemonConnectionState` or clear the session-token cache here: the GUI is
/// about to be torn down by `app.restart()`, so the new process bootstraps a
/// fresh connection and mints a new JWT against the restarted daemon anyway.
///
/// A daemon-restart failure is logged but non-fatal — we still restart the
/// GUI, whose bootstrap will attempt its own spawn/recovery and surface any
/// error in the UI. Does not return on the happy path ([`perform_restart`]
/// ends in `app.restart()` → `process::exit`).
///
/// [`restart_local_daemon`]: uc_desktop::daemon_probe::restart_local_daemon
pub(crate) async fn perform_full_restart(app: &tauri::AppHandle) {
    info!("full restart: restarting daemon before GUI");
    match uc_desktop::daemon_probe::restart_local_daemon(env!("CARGO_PKG_VERSION")).await {
        Ok(_) => info!("full restart: daemon restarted, proceeding to GUI restart"),
        Err(error) => warn!(
            %error,
            "full restart: daemon restart failed; restarting GUI anyway — \
             the new GUI bootstrap will retry spawn/recovery"
        ),
    }

    perform_restart(app).await;
}

// ── restart_daemon ─────────────────────────────────────────────────────

/// Restart only the `uniclipd` daemon process without touching the GUI.
///
/// 用于 network 等 bind-time 设置变更后：daemon 侧的 iroh endpoint 在进程
/// 启动时绑定一次，运行时无法热更新，所以需要重启 daemon 进程让新配置生效。
/// GUI 保持在线，WS bridge 自动重连到新 daemon。
#[tauri::command]
#[specta::specta]
pub async fn restart_daemon(
    connection_state: tauri::State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<(), crate::commands::error::CommandError> {
    let span = info_span!(
        "command.restart.restart_daemon",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let new_info = uc_desktop::daemon_probe::restart_local_daemon(env!("CARGO_PKG_VERSION"))
            .await
            .map_err(|e| {
                crate::commands::CommandError::internal(
                    anyhow::Error::new(e).context("daemon restart failed"),
                )
            })?;

        connection_state.set(new_info);
        // The static JWT cache holds a token minted by the OLD daemon —
        // the new daemon has a fresh JWT secret and will reject it.
        uc_daemon_client::http::clear_session_token_cache().await;
        info!("daemon restarted, connection state refreshed, session cache cleared");

        let conn: DaemonConnectionState = (*connection_state).clone();
        tauri::async_runtime::spawn(async move {
            uc_desktop::daemon_recovery::recover_after_restart(conn).await;
        });

        Ok(())
    }
    .instrument(span)
    .await
}

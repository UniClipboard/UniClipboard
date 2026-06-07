//! `mobile_sync` 子命令共享的小工具:命令骨架、错误渲染、重启提示、JSON 包装。
//!
//! Non-debug commands use the daemon-client path (P5-2b ADR):
//! [`MobileSyncDaemonCtx`] + [`enter`] / [`finish_daemon_json`] / [`finish_daemon`].
//!
//! The hidden `debug` subcommand (debug builds only) still needs in-process
//! facade access — its legacy lifecycle types are `#[cfg(feature = "dev-tools")]`.

#[cfg(feature = "dev-tools")]
use std::sync::Arc;

use serde::Serialize;

#[cfg(feature = "dev-tools")]
use uc_application::facade::{
    ApplyIncomingMobileClipError, GetLatestMobileSyncDocError, GetMobileSyncFileError,
    MobileSyncFacade,
};

use crate::commands::app_session::connect_or_spawn_oneshot_daemon;
#[cfg(feature = "dev-tools")]
use crate::commands::app_session::{build_app_session, refuse_if_daemon_running, CliAppSession};
use crate::exit_codes;
use crate::ui;

// ── Legacy in-process lifecycle (debug subcommand only) ────────────────

#[cfg(feature = "dev-tools")]
/// Wired CLI session + a clone of the mobile-sync facade. Built by
/// [`enter_write`]; consumed by [`finish_json`] / [`finish`].
pub struct MobileSyncCmdCtx {
    pub cli: CliAppSession,
    pub facade: Arc<MobileSyncFacade>,
}

#[cfg(feature = "dev-tools")]
/// Boilerplate for **write commands** that need in-process facade access
/// (debug subcommand only). Refuses if daemon is running, then builds a
/// CLI app session and takes the mobile-sync facade.
pub async fn enter_write(header: &str, json: bool, verbose: bool) -> Result<MobileSyncCmdCtx, i32> {
    if !json {
        ui::header(header);
    }
    refuse_if_daemon_running().await?;
    enter_inner(verbose).await
}

#[cfg(feature = "dev-tools")]
async fn enter_inner(verbose: bool) -> Result<MobileSyncCmdCtx, i32> {
    let cli = build_app_session(verbose).await?;
    let Some(facade) = cli.app_facade().mobile_sync.get().cloned() else {
        ui::error("Mobile-sync facade is not wired in this build.");
        cli.shutdown().await;
        return Err(exit_codes::EXIT_ERROR);
    };
    Ok(MobileSyncCmdCtx { cli, facade })
}

#[cfg(feature = "dev-tools")]
/// Pretty-print `dto` as JSON to stdout, then shut the ctx down. Returns
/// SUCCESS on serialize ok, ERROR otherwise (shutdown still happens).
pub async fn finish_json<T: Serialize>(ctx: MobileSyncCmdCtx, dto: &T) -> i32 {
    let exit = match serde_json::to_string_pretty(dto) {
        Ok(s) => {
            println!("{s}");
            exit_codes::EXIT_SUCCESS
        }
        Err(err) => {
            ui::error(&format!("Failed to serialize: {err}"));
            exit_codes::EXIT_ERROR
        }
    };
    ctx.cli.shutdown().await;
    exit
}

#[cfg(feature = "dev-tools")]
/// Shut the ctx down, return the given exit code. Use for the
/// human-readable branch where rendering happened inline.
pub async fn finish(ctx: MobileSyncCmdCtx, exit: i32) -> i32 {
    ctx.cli.shutdown().await;
    exit
}

// ── Daemon-client lifecycle (P5-2b ADR) ────────────────────────────────

/// Daemon-client context for non-debug mobile-sync commands. Holds an HTTP
/// client for daemon API calls and a control-lease guard that keeps a
/// transient Oneshot daemon alive.
pub struct MobileSyncDaemonCtx {
    pub client: uc_daemon_client::http::DaemonMobileSyncClient,
    _lease: uc_daemon_client::service::ControlLeaseGuard,
}

/// Connect to (or spawn) a daemon, hold a control lease, build a
/// mobile-sync HTTP client. Used by all non-debug mobile-sync commands.
pub async fn enter(header: &str, json: bool, verbose: bool) -> Result<MobileSyncDaemonCtx, i32> {
    if !json && !header.is_empty() {
        ui::header(header);
    }
    let service = connect_or_spawn_oneshot_daemon(verbose).await?;
    let lease = service.hold_control_lease().await.map_err(|err| {
        ui::error(&format!("Failed to hold daemon session lease: {err}"));
        exit_codes::EXIT_ERROR
    })?;
    let ctx = uc_daemon_client::DaemonClientContext::from_env().map_err(|err| {
        ui::error(&format!("Failed to connect to daemon: {err}"));
        exit_codes::EXIT_ERROR
    })?;
    Ok(MobileSyncDaemonCtx {
        client: ctx.mobile_sync_client(),
        _lease: lease,
    })
}

/// Pretty-print `dto` as JSON to stdout, then drop the daemon ctx.
pub async fn finish_daemon_json<T: Serialize>(_ctx: MobileSyncDaemonCtx, dto: &T) -> i32 {
    match serde_json::to_string_pretty(dto) {
        Ok(s) => {
            println!("{s}");
            exit_codes::EXIT_SUCCESS
        }
        Err(err) => {
            ui::error(&format!("Failed to serialize: {err}"));
            exit_codes::EXIT_ERROR
        }
    }
}

/// Drop the daemon ctx, return the given exit code.
pub async fn finish_daemon(_ctx: MobileSyncDaemonCtx, exit: i32) -> i32 {
    exit
}

// ── Interactive input helpers (shared by setup / devices add) ───────────

/// Read one line from stdin (no prompt printed). Used when the password
/// arrives via pipe / heredoc — keeps it out of shell history.
pub fn read_password_stdin() -> Result<String, String> {
    use std::io::{self, BufRead};
    let mut buf = String::new();
    io::stdin()
        .lock()
        .read_line(&mut buf)
        .map_err(|e| e.to_string())?;
    Ok(buf.trim_end_matches(['\n', '\r']).to_string())
}

// ── Error renderers + restart hint ──────────────────────────────────────

/// 把 `restart_required=true` 转化为面向用户的提示字符串(英文,人类可读)。
pub fn restart_hint() -> &'static str {
    "Restart the daemon to apply: `uniclip stop && uniclip start`."
}

// ── P5a.9 debug subcommand error renderers ──────────────────────────────

#[cfg(feature = "dev-tools")]
pub fn render_apply_incoming_error(err: &ApplyIncomingMobileClipError) -> String {
    match err {
        ApplyIncomingMobileClipError::Inbound(inner) => {
            format!("Inbound clipboard apply failed: {inner}")
        }
        ApplyIncomingMobileClipError::EncodeFailed(msg) => {
            format!("V3 envelope encode failed: {msg}")
        }
        ApplyIncomingMobileClipError::Internal(msg) => {
            format!("Internal apply error: {msg}")
        }
    }
}

#[cfg(feature = "dev-tools")]
pub fn render_get_latest_doc_error(err: &GetLatestMobileSyncDocError) -> String {
    match err {
        GetLatestMobileSyncDocError::NotFound => {
            "No clipboard entry yet (404 — same response iPhone would see).".into()
        }
        GetLatestMobileSyncDocError::Port(inner) => {
            format!("Snapshot port failure: {inner}")
        }
    }
}

#[cfg(feature = "dev-tools")]
pub fn render_get_file_error(err: &GetMobileSyncFileError) -> String {
    match err {
        GetMobileSyncFileError::NotFound => "No matching file for this dataName (404).".into(),
        GetMobileSyncFileError::Port(inner) => {
            format!("Snapshot port failure: {inner}")
        }
        GetMobileSyncFileError::Staging(msg) => {
            // P5a.3.5: File 出站读 staging 文件 IO 失败(权限 / 中途盘错)。
            format!("File staging IO failure: {msg}")
        }
    }
}

//! CLI session helpers: daemon probe, in-process wiring, and dual-dispatch.
//!
//! Business commands either build a self-contained `CliAppSession` (no
//! daemon) or delegate to a running daemon via `DaemonService`.

use crate::exit_codes;
use crate::local_daemon::probe_running;
use crate::ui;

use uc_daemon_client::{DaemonClientContext, DaemonService, HttpWsDaemonService};
use uc_daemon_contract::probe::ProbeOutcome;

// ── In-process session (dev-tools only) ────────────────────────────────

/// [`build_app_session`] 返回的 CLI 会话。
#[cfg(feature = "dev-tools")]
pub struct CliAppSession {
    pub runtime: uc_bootstrap::CliAppRuntime,
}

#[cfg(feature = "dev-tools")]
impl CliAppSession {
    pub fn app_facade(&self) -> &std::sync::Arc<uc_application::facade::AppFacade> {
        self.runtime.app_facade()
    }

    pub async fn shutdown(self) {
        self.runtime.shutdown().await;
    }
}

/// 当同 profile 已有 daemon 运行时拒绝执行业务命令。
///
/// 在 IPC 转发落地前,同一个 profile 的两个进程会用同一个 Ed25519
/// secret 绑定两个 iroh endpoint,并且 daemon 自己的流程会和 CLI 竞争。
/// 因此独立 CLI 业务命令要求用户先 `stop` daemon。
#[cfg(feature = "dev-tools")]
pub async fn refuse_if_daemon_running() -> Result<(), i32> {
    match probe_running().await {
        Ok(ProbeOutcome::Compatible(_)) => {
            ui::error(
                "A daemon is already running for this profile. Stop it first with \
                 `uniclip stop`, or rerun under a different --profile.",
            );
            Err(exit_codes::EXIT_DAEMON_UNREACHABLE)
        }
        // ADR-008 P5-L L2: an incompatible-version daemon used to be invisible
        // here (it failed status!="ok" and was treated as "no daemon"), so the
        // CLI would silently spin up a competing in-process session against a
        // mismatched daemon. Surface a clear error naming the version gap.
        Ok(outcome @ ProbeOutcome::Incompatible { .. }) => {
            ui::error(&crate::local_daemon::incompatible_outcome_error(outcome).to_string());
            Err(exit_codes::EXIT_DAEMON_UNREACHABLE)
        }
        Ok(ProbeOutcome::Absent) => Ok(()),
        // 探测网络错误按"没有可冲突 daemon"处理。
        Err(err) => {
            tracing::debug!(error = %err, "daemon probe failed; assuming no daemon");
            Ok(())
        }
    }
}

/// 为 CLI 业务命令构造独立 application session。
///
/// 默认使用 `Cli` 日志 profile;`verbose` 打开时切到 `Dev`,方便调试
/// 单机双进程 pairing。
///
/// wiring 前设置 `UC_DISABLE_SYSTEM_CLIPBOARD=1`,避免独立 CLI 命令提前触碰
/// 系统剪贴板适配器。
#[cfg(feature = "dev-tools")]
pub async fn build_app_session(verbose: bool) -> Result<CliAppSession, i32> {
    // 必须在 bootstrap wiring 前设置,避免 CLI 进程触碰系统剪贴板适配器。
    std::env::set_var("UC_DISABLE_SYSTEM_CLIPBOARD", "1");

    let log_profile = if verbose {
        Some(uc_observability::LogProfile::Dev)
    } else {
        Some(uc_observability::LogProfile::Cli)
    };
    match uc_bootstrap::build_cli_app_runtime(log_profile).await {
        Ok(runtime) => Ok(CliAppSession { runtime }),
        Err(err) => {
            ui::error(&format!("Failed to wire dependencies: {err}"));
            Err(exit_codes::EXIT_ERROR)
        }
    }
}

// ── Daemon-client session (always available) ──────────────────────────

/// ADR-008 P5-1a: connect to a running compatible daemon, or spawn a transient
/// Oneshot daemon when none is present, and return a `DaemonService` client.
/// Business commands (send/watch) use this instead of `resolve_execution_mode`
/// — they NEVER fall back to an in-process session.
///
/// * Compatible(any residency) → reuse it.
/// * Incompatible              → clear error (no silent attach).
/// * Absent                    → setup gate, then spawn a Oneshot daemon.
pub async fn connect_or_spawn_oneshot_daemon(verbose: bool) -> Result<Box<dyn DaemonService>, i32> {
    let _ = verbose; // reserved; the daemon path builds no in-process session.
    match probe_running().await {
        Ok(ProbeOutcome::Compatible(_)) => build_daemon_client_service(),
        Ok(outcome @ ProbeOutcome::Incompatible { .. }) => {
            ui::error(&crate::local_daemon::incompatible_outcome_error(outcome).to_string());
            Err(exit_codes::EXIT_DAEMON_UNREACHABLE)
        }
        Ok(ProbeOutcome::Absent) => {
            // Don't spawn a useless Oneshot for an unprovisioned profile.
            // Mirror start.rs's lenient unwrap_or(true): if setup state is
            // unreadable, attempt the spawn and let the real error surface.
            if !crate::setup_check::is_setup_complete().unwrap_or(true) {
                ui::error("No space on this profile — run `uniclip init` or `uniclip join` first.");
                return Err(exit_codes::EXIT_ERROR);
            }
            match crate::local_daemon::spawn_oneshot_and_wait().await {
                Ok(_session) => build_daemon_client_service(),
                Err(err) => {
                    ui::error(&err.to_string());
                    Err(exit_codes::EXIT_ERROR)
                }
            }
        }
        // No in-process fallback in P5-1a. connect/timeout already map to
        // Absent upstream, so a probe Err is a genuine failure → hard error.
        Err(err) => {
            ui::error(&format!("Failed to probe local daemon: {err}"));
            Err(exit_codes::EXIT_DAEMON_UNREACHABLE)
        }
    }
}

fn build_daemon_client_service() -> Result<Box<dyn DaemonService>, i32> {
    match DaemonClientContext::from_env() {
        Ok(ctx) => Ok(Box::new(HttpWsDaemonService::new(ctx))),
        Err(err) => {
            ui::error(&format!("Daemon is running but failed to connect: {err}"));
            Err(exit_codes::EXIT_ERROR)
        }
    }
}

/// Like [`connect_or_spawn_oneshot_daemon`] but skips the `is_setup_complete` gate.
///
/// Used by `init` and `join` which ARE the commands that complete setup — they
/// need a running daemon to call `POST /v2/setup/initialize` or
/// `POST /v2/setup/redeem`, but the profile has no space yet so the setup gate
/// would reject them.
pub async fn ensure_daemon_for_setup(verbose: bool) -> Result<Box<dyn DaemonService>, i32> {
    let _ = verbose; // reserved; the daemon path builds no in-process session.
    match probe_running().await {
        Ok(ProbeOutcome::Compatible(_)) => build_daemon_client_service(),
        Ok(outcome @ ProbeOutcome::Incompatible { .. }) => {
            ui::error(&crate::local_daemon::incompatible_outcome_error(outcome).to_string());
            Err(exit_codes::EXIT_DAEMON_UNREACHABLE)
        }
        Ok(ProbeOutcome::Absent) => {
            // No setup gate — we ARE the setup command.
            match crate::local_daemon::spawn_oneshot_and_wait().await {
                Ok(_session) => build_daemon_client_service(),
                Err(err) => {
                    ui::error(&err.to_string());
                    Err(exit_codes::EXIT_ERROR)
                }
            }
        }
        Err(err) => {
            ui::error(&format!("Failed to probe local daemon: {err}"));
            Err(exit_codes::EXIT_DAEMON_UNREACHABLE)
        }
    }
}

/// ADR-008 P5-1c: wait for the daemon to come back after a controlled restart,
/// then build a fresh `DaemonService` client. Used by `watch`/`recv` to
/// reconnect after the WS drops during promotion.
pub async fn wait_and_reconnect_daemon(
    timeout: std::time::Duration,
) -> Result<Box<dyn DaemonService>, i32> {
    let deadline = tokio::time::Instant::now() + timeout;
    let poll_interval = std::time::Duration::from_millis(200);
    loop {
        match probe_running().await {
            Ok(ProbeOutcome::Compatible(_)) => return build_daemon_client_service(),
            Ok(ProbeOutcome::Incompatible { .. }) | Ok(ProbeOutcome::Absent) | Err(_) => {}
        }
        if tokio::time::Instant::now() >= deadline {
            ui::error("Timed out waiting for daemon to restart.");
            return Err(exit_codes::EXIT_DAEMON_UNREACHABLE);
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// 从系统 hostname 推导默认设备名。
///
/// 设置了 `UC_PROFILE` 时追加 profile 后缀,方便单机双实例时区分设备。
/// hostname 读取失败或不是 UTF-8 时返回 `None`。
pub fn default_device_name() -> Option<String> {
    let raw = hostname::get().ok()?.into_string().ok()?;
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        return None;
    }
    match std::env::var("UC_PROFILE") {
        Ok(p) if !p.is_empty() => Some(format!("{trimmed} ({p})")),
        _ => Some(trimmed),
    }
}

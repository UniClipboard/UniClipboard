//! Daemon-client connections and independent dev-tools Engine sessions.
//!
//! Retained compatibility commands delegate to the external daemon. Only
//! Engine-backed diagnostics build a `CliAppSession` via uc-bootstrap; they
//! refuse a detected daemon on the same profile and never provide a fallback
//! for daemon-client commands.

use crate::exit_codes;
use crate::local_daemon::{probe_running, probe_running_for_reuse, probe_running_for_reuse_within};
use crate::ui;

use uc_daemon_client::{
    ControlLeaseGuard, DaemonClientContext, DaemonService, HttpWsDaemonService,
};
use uc_daemon_contract::probe::{ProbeOutcome, DEGRADED_HEALTH_INCOMPATIBILITY_DETAILS};

// ── In-process session (dev-tools only) ────────────────────────────────

/// Independent Engine session returned by [`build_app_session`] for diagnostics.
#[cfg(feature = "dev-tools")]
pub struct CliAppSession {
    pub runtime: uc_bootstrap::CliEngineRuntime,
}

#[cfg(feature = "dev-tools")]
impl CliAppSession {
    pub fn engine(&self) -> &std::sync::Arc<uc_engine::Engine> {
        self.runtime.engine()
    }

    pub fn file_handles(&self) -> &uc_bootstrap::DesktopHostFileHandles {
        self.runtime.file_handles()
    }

    pub async fn recover_session(&self) -> Result<bool, uc_engine::EngineError> {
        match self
            .engine()
            .execute(uc_engine::Operation::RecoverSession(
                uc_engine::RecoverSessionInput {
                    allow_secure_storage_unlock: true,
                },
            ))
            .await?
        {
            uc_engine::OperationResult::SessionRecovered { unlocked, resumed } => {
                Ok(unlocked && resumed)
            }
            _ => Ok(false),
        }
    }

    pub async fn shutdown(self) {
        self.runtime.shutdown().await;
    }
}

/// Reject an independent diagnostic session when a daemon is detected on the
/// same profile. The two runtimes would otherwise compete for profile storage
/// and bind separate iroh endpoints with the same device identity.
/// Stop the test profile's daemon first or select a different profile.
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

/// Build the independent Engine runtime used by dev-tools diagnostics.
///
/// Use the Cli logging profile, or Dev when verbose output is enabled.
/// Disable the system clipboard before bootstrap wiring; clipboard probing
/// uses its own platform adapter instead of this runtime.
#[cfg(feature = "dev-tools")]
pub async fn build_app_session(verbose: bool) -> Result<CliAppSession, i32> {
    // Disable the system clipboard before constructing the diagnostic runtime.
    std::env::set_var("UC_DISABLE_SYSTEM_CLIPBOARD", "1");

    let log_profile = if verbose {
        Some(uc_observability::LogProfile::Dev)
    } else {
        Some(uc_observability::LogProfile::Cli)
    };
    match uc_bootstrap::build_cli_engine_runtime(log_profile).await {
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
/// Daemon-client commands (including send/watch) use this connection path and
/// never fall back to an independent Engine session.
///
/// * Compatible(any residency) → reuse it.
/// * Incompatible              → clear error (no silent attach).
/// * Absent                    → setup gate, then spawn a Oneshot daemon.
pub async fn connect_or_spawn_oneshot_daemon(verbose: bool) -> Result<Box<dyn DaemonService>, i32> {
    connect_or_spawn_oneshot_daemon_until(verbose, None).await
}

/// [`connect_or_spawn_oneshot_daemon`] bounded by one absolute `deadline`
/// shared by the incumbent-readiness wait and the spawned-daemon health wait,
/// so the total wait cannot exceed the caller's budget. A missed deadline
/// exits with [`exit_codes::EXIT_DAEMON_UNREACHABLE`] and names the stage.
/// `None` keeps the default startup budget and exit codes.
pub async fn connect_or_spawn_oneshot_daemon_until(
    verbose: bool,
    deadline: Option<tokio::time::Instant>,
) -> Result<Box<dyn DaemonService>, i32> {
    let _ = verbose; // reserved; the daemon path builds no in-process session.
    let remaining = |default: std::time::Duration| match deadline {
        Some(deadline) => deadline.saturating_duration_since(tokio::time::Instant::now()),
        None => default,
    };
    let report_timeout = |error: &crate::local_daemon::LocalDaemonError| -> i32 {
        ui::error(&error.to_string());
        if deadline.is_some()
            && matches!(
                error,
                crate::local_daemon::LocalDaemonError::StartupTimeout { .. }
            )
        {
            ui::warn(
                "The daemon may still be starting. Retry, raise --connect-timeout, \
                 or run `uniclip start` first.",
            );
            exit_codes::EXIT_DAEMON_UNREACHABLE
        } else {
            exit_codes::EXIT_ERROR
        }
    };
    let probe = probe_running_for_reuse_within(remaining(
        uc_daemon_process::timing::DAEMON_STARTUP_TIMEOUT,
    ))
    .await;
    match probe {
        Ok(ProbeOutcome::Compatible(_)) => build_daemon_client_service(true),
        Ok(outcome @ ProbeOutcome::Incompatible { .. }) => {
            ui::error(&crate::local_daemon::incompatible_outcome_error(outcome).to_string());
            Err(exit_codes::EXIT_DAEMON_UNREACHABLE)
        }
        Ok(ProbeOutcome::Absent) => match crate::local_daemon::spawn_oneshot_and_wait_within(
            remaining(uc_daemon_process::timing::DAEMON_STARTUP_TIMEOUT),
        )
        .await
        {
            Ok(_session) => {
                let context = DaemonClientContext::from_env().map_err(|error| {
                    ui::error(&format!("Failed to connect to daemon: {error}"));
                    exit_codes::EXIT_ERROR
                })?;
                match context.query_client().get_profile_recovery().await {
                    Ok(status) if !status.background_ready => {
                        Ok(Box::new(HttpWsDaemonService::new(context)))
                    }
                    Ok(_) => match crate::setup_check::is_setup_complete().await {
                        Ok(true) => Ok(Box::new(HttpWsDaemonService::new(context))),
                        Ok(false) => {
                            ui::error("No space on this profile; run `uniclip space init` or `uniclip space join` first.");
                            Err(exit_codes::EXIT_ERROR)
                        }
                        Err(error) => {
                            ui::error(&format!("Failed to read setup state: {error}"));
                            Err(exit_codes::EXIT_ERROR)
                        }
                    },
                    Err(error) => {
                        ui::error(&format!("Failed to read profile recovery state: {error}"));
                        Err(exit_codes::EXIT_ERROR)
                    }
                }
            }
            Err(err) => Err(report_timeout(&err)),
        },
        // No in-process fallback in P5-1a. connect/timeout already map to
        // Absent upstream, so a probe Err is a genuine failure → hard error.
        Err(err @ crate::local_daemon::LocalDaemonError::StartupTimeout { .. })
            if deadline.is_some() =>
        {
            Err(report_timeout(&err))
        }
        Err(err) => {
            ui::error(&format!("Failed to probe local daemon: {err}"));
            Err(exit_codes::EXIT_DAEMON_UNREACHABLE)
        }
    }
}

/// Connect to (or spawn) the daemon, hold a control lease for the command's
/// duration, and return a ready [`DaemonClientContext`].
///
/// Bundles the boilerplate shared by every daemon-client command — probe/spawn,
/// lease acquisition, and context construction — behind one consistent error
/// path. The returned [`ControlLeaseGuard`] must stay bound for the rest of the
/// command (e.g. `let (_lease, ctx) = connect_with_lease(verbose).await?;`):
/// dropping it releases the lease and lets a transient Oneshot daemon exit.
pub async fn connect_with_lease(
    verbose: bool,
) -> Result<(ControlLeaseGuard, DaemonClientContext), i32> {
    let service = connect_or_spawn_oneshot_daemon(verbose).await?;

    let lease = service.hold_control_lease().await.map_err(|err| {
        ui::error(&format!("Failed to hold daemon session lease: {err}"));
        exit_codes::EXIT_ERROR
    })?;

    let ctx = DaemonClientContext::from_env().map_err(|err| {
        ui::error(&format!("Failed to connect to daemon: {err}"));
        exit_codes::EXIT_ERROR
    })?;

    Ok((lease, ctx))
}

/// Connect to the daemon through the transport-agnostic application facade
/// while retaining a control lease for the command lifetime.
pub async fn connect_facade_with_lease(
    verbose: bool,
) -> Result<(ControlLeaseGuard, Box<dyn DaemonService>), i32> {
    let service = connect_or_spawn_oneshot_daemon(verbose).await?;
    let lease = service.hold_control_lease().await.map_err(|err| {
        ui::error(&format!("Failed to hold daemon session lease: {err}"));
        exit_codes::EXIT_ERROR
    })?;
    Ok((lease, service))
}

/// Connect to the daemon for setup and admission workflows without requiring
/// the profile to have completed setup.
pub async fn connect_setup_facade_with_lease(
    verbose: bool,
) -> Result<(ControlLeaseGuard, Box<dyn DaemonService>), i32> {
    connect_setup_facade_with_lease_inner(verbose, true).await
}

pub async fn reconnect_setup_facade_with_lease(
    verbose: bool,
) -> Result<(ControlLeaseGuard, Box<dyn DaemonService>), i32> {
    connect_setup_facade_with_lease_inner(verbose, false).await
}

async fn connect_setup_facade_with_lease_inner(
    verbose: bool,
    report_errors: bool,
) -> Result<(ControlLeaseGuard, Box<dyn DaemonService>), i32> {
    let service = ensure_daemon_for_setup_inner(verbose, report_errors).await?;
    let lease = service.hold_control_lease().await.map_err(|err| {
        report_daemon_connection_error(
            format!("Failed to hold daemon session lease: {err}"),
            report_errors,
        );
        exit_codes::EXIT_ERROR
    })?;
    Ok((lease, service))
}

fn build_daemon_client_service(report_errors: bool) -> Result<Box<dyn DaemonService>, i32> {
    match DaemonClientContext::from_env() {
        Ok(ctx) => Ok(Box::new(HttpWsDaemonService::new(ctx))),
        Err(err) => {
            report_daemon_connection_error(
                format!("Daemon is running but failed to connect: {err}"),
                report_errors,
            );
            Err(exit_codes::EXIT_ERROR)
        }
    }
}

fn report_daemon_connection_error(message: String, report_errors: bool) {
    if report_errors {
        ui::error(&message);
    } else {
        tracing::debug!(error = %message, "daemon reconnect attempt failed");
    }
}

/// Like [`connect_or_spawn_oneshot_daemon`] but skips the `is_setup_complete` gate.
///
/// Used by `init` and `join` which ARE the commands that complete setup — they
/// need a running daemon to call `POST /v2/setup/initialize` or
/// `POST /v2/setup/redeem`, but the profile has no space yet so the setup gate
/// would reject them.
pub async fn ensure_daemon_for_setup(verbose: bool) -> Result<Box<dyn DaemonService>, i32> {
    ensure_daemon_for_setup_inner(verbose, true).await
}

async fn ensure_daemon_for_setup_inner(
    verbose: bool,
    report_errors: bool,
) -> Result<Box<dyn DaemonService>, i32> {
    let _ = verbose; // reserved; the daemon path builds no in-process session.
    match probe_running_for_reuse().await {
        Ok(ProbeOutcome::Compatible(_)) => build_daemon_client_service(report_errors),
        Ok(outcome) if setup_control_contract_matches(&outcome) => {
            build_daemon_client_service(report_errors)
        }
        Ok(outcome @ ProbeOutcome::Incompatible { .. }) => {
            report_daemon_connection_error(
                crate::local_daemon::incompatible_outcome_error(outcome).to_string(),
                report_errors,
            );
            Err(exit_codes::EXIT_DAEMON_UNREACHABLE)
        }
        Ok(ProbeOutcome::Absent) => {
            // No setup gate — we ARE the setup command.
            match crate::local_daemon::spawn_oneshot_and_wait().await {
                Ok(_session) => build_daemon_client_service(report_errors),
                Err(err) => {
                    report_daemon_connection_error(err.to_string(), report_errors);
                    Err(exit_codes::EXIT_ERROR)
                }
            }
        }
        Err(err) => {
            report_daemon_connection_error(
                format!("Failed to probe local daemon: {err}"),
                report_errors,
            );
            Err(exit_codes::EXIT_DAEMON_UNREACHABLE)
        }
    }
}

fn setup_control_contract_matches(outcome: &ProbeOutcome) -> bool {
    matches!(
        outcome,
        ProbeOutcome::Incompatible {
            details,
            observed_package_version: Some(package_version),
            observed_api_revision: Some(api_revision),
            ..
        } if details == DEGRADED_HEALTH_INCOMPATIBILITY_DETAILS
            && package_version == env!("CARGO_PKG_VERSION")
            && api_revision == uc_daemon_contract::DAEMON_API_REVISION
    )
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
            Ok(ProbeOutcome::Compatible(_)) => return build_daemon_client_service(true),
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

#[cfg(test)]
mod tests {
    use super::setup_control_contract_matches;
    use uc_daemon_contract::probe::{ProbeOutcome, DEGRADED_HEALTH_INCOMPATIBILITY_DETAILS};

    fn incompatible(status: &str, package_version: &str, api_revision: &str) -> ProbeOutcome {
        ProbeOutcome::Incompatible {
            details: if status == "degraded" {
                DEGRADED_HEALTH_INCOMPATIBILITY_DETAILS.to_string()
            } else {
                format!("daemon reported unhealthy status {status}")
            },
            observed_package_version: Some(package_version.to_string()),
            observed_api_revision: Some(api_revision.to_string()),
        }
    }

    #[test]
    fn setup_control_can_attach_to_matching_degraded_daemon() {
        assert!(setup_control_contract_matches(&incompatible(
            "degraded",
            env!("CARGO_PKG_VERSION"),
            uc_daemon_contract::DAEMON_API_REVISION,
        )));
    }

    #[test]
    fn setup_control_rejects_mismatched_daemon_contracts() {
        assert!(!setup_control_contract_matches(&incompatible(
            "degraded",
            "0.0.0",
            uc_daemon_contract::DAEMON_API_REVISION,
        )));
        assert!(!setup_control_contract_matches(&incompatible(
            "degraded",
            env!("CARGO_PKG_VERSION"),
            "future-api",
        )));
        assert!(!setup_control_contract_matches(&incompatible(
            "failed",
            env!("CARGO_PKG_VERSION"),
            uc_daemon_contract::DAEMON_API_REVISION,
        )));
        assert!(!setup_control_contract_matches(&ProbeOutcome::Absent));
    }
}

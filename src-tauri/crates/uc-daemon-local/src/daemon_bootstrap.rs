use anyhow::Result;
use std::future::Future;
use std::time::Duration;

use tauri_plugin_shell::process::CommandChild;
use uc_daemon_contract::api::auth::DaemonConnectionInfo;

// Pure contract types live in `crate::contract` (no feature gate). Re-export
// them here so historical `uc_daemon_local::daemon_bootstrap::DaemonBootstrapError`
// imports keep working. New consumers should `use uc_daemon_local::contract::*;`.
pub use crate::contract::{DaemonBootstrapError, ProbeOutcome, SpawnReason};
pub use crate::health_wait::{wait_for_daemon_health, wait_for_endpoint_absent};

use crate::daemon_lifecycle::GuiOwnedDaemonState;

const MAX_INCOMPATIBLE_REPLACEMENT_ATTEMPTS: u8 = 1;

/// Ensures a compatible daemon is available (spawning or replacing as needed) and returns its connection info.
///
/// Probes the daemon state and:
/// - If compatible, clears any GUI-owned daemon state.
/// - If absent, spawns a daemon and waits until it becomes compatible.
/// - If incompatible, attempts to terminate and replace the incompatible daemon (subject to a maximum replacement attempt limit).
///
/// The provided hooks are used for side-effecting operations:
/// - `spawn` should try to start a daemon and return `Some((child, pid))` if a new process was spawned or `None` if no new process was started.
/// - `probe` must return the current `ProbeOutcome`.
/// - `terminate_incompatible` should stop the running incompatible daemon.
/// - `load_connection_info` should load and return `DaemonConnectionInfo` after a compatible daemon is confirmed.
///
/// # Returns
///
/// `DaemonConnectionInfo` for the compatible daemon, or a `DaemonBootstrapError` describing why bootstrapping failed.
///
/// # Examples
///
/// ```ignore
/// # use std::time::Duration;
/// # use tokio::runtime::Runtime;
/// # use uc_daemon_local::{ProbeOutcome, HealthResponse, DaemonConnectionInfo, DaemonBootstrapError};
/// // This example uses simplified stubs for the hooks.
/// let rt = Runtime::new().unwrap();
/// rt.block_on(async {
///     let gui_state = /* obtain GuiOwnedDaemonState */ unimplemented!();
///
///     let mut spawn = || -> Result<Option<(/* CommandChild */ (), u32)>, DaemonBootstrapError> {
///         Ok(None)
///     };
///
///     let mut probe = || async {
///         Ok(ProbeOutcome::Compatible(HealthResponse::default()))
///     };
///
///     let load_connection_info = || -> Result<DaemonConnectionInfo, DaemonBootstrapError> {
///         Ok(DaemonConnectionInfo::default())
///     };
///
///     let mut terminate_incompatible = || -> Result<(), DaemonBootstrapError> { Ok(()) };
///
///     let conn = super::bootstrap_daemon_connection_with_hooks(
///         &gui_state,
///         &mut spawn,
///         &mut probe,
///         load_connection_info,
///         &mut terminate_incompatible,
///         Duration::from_secs(5),
///         Duration::from_secs(10),
///         Duration::from_millis(200),
///     ).await;
///
///     // `conn` is `Ok(DaemonConnectionInfo)` when a compatible daemon is available.
/// });
/// ```
pub async fn bootstrap_daemon_connection_with_hooks<
    Spawn,
    Probe,
    ProbeFuture,
    LoadInfo,
    Terminate,
>(
    gui_owned_daemon_state: &GuiOwnedDaemonState,
    mut spawn: Spawn,
    mut probe: Probe,
    load_connection_info: LoadInfo,
    mut terminate_incompatible: Terminate,
    incompatible_exit_timeout: Duration,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<DaemonConnectionInfo, DaemonBootstrapError>
where
    Spawn: FnMut() -> Result<Option<(CommandChild, u32)>, DaemonBootstrapError>,
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
    LoadInfo: Fn() -> Result<DaemonConnectionInfo, DaemonBootstrapError>,
    Terminate: FnMut() -> Result<(), DaemonBootstrapError>,
{
    let mut replacement_attempt = 0_u8;

    match probe().await? {
        ProbeOutcome::Compatible(_) => {
            let _ = gui_owned_daemon_state.clear();
        }
        ProbeOutcome::Absent => {
            spawn_and_wait_for_compatible(
                gui_owned_daemon_state,
                &mut spawn,
                &mut probe,
                timeout,
                poll_interval,
                SpawnReason::Absent,
            )
            .await?;
        }
        ProbeOutcome::Incompatible { details, .. } => {
            replace_incompatible_daemon(
                &mut replacement_attempt,
                gui_owned_daemon_state,
                details,
                &mut terminate_incompatible,
                &mut spawn,
                &mut probe,
                incompatible_exit_timeout,
                timeout,
                poll_interval,
            )
            .await?;
        }
    }

    load_connection_info()
}

/// Spawns a daemon if needed, updates GUI-owned daemon state, and waits until a compatible daemon is available.
///
/// Calls the provided `spawn` closure to attempt starting a daemon. If `spawn` returns a child and pid, that spawn is recorded in `gui_owned_daemon_state`; if it returns `None`, GUI-owned state is cleared. The function then polls `probe` until a compatible daemon is observed within `timeout`, waiting `poll_interval` between attempts. If waiting fails, GUI-owned state is cleared and the error is returned.
///
/// # Parameters
/// - `gui_owned_daemon_state`: GUI-tracked daemon process state to update when a spawn occurs or when clearing state on failure.
/// - `spawn`: closure that attempts to spawn the daemon and returns `Ok(Some((child, pid)))` on a started process, `Ok(None)` if no process was started, or an error.
/// - `probe`: closure that probes daemon health and yields a `ProbeOutcome`.
/// - `timeout`: maximum duration to wait for a compatible daemon.
/// - `poll_interval`: delay between probe attempts.
/// - `spawn_reason`: reason recorded with the GUI-owned state when a spawn is recorded.
///
/// # Returns
/// `Ok(())` if a compatible daemon is observed within `timeout`, `Err(DaemonBootstrapError)` otherwise.
///
/// # Examples
///
/// ```
/// // Illustrative example (types elided for brevity).
/// // let gui_state = GuiOwnedDaemonState::default();
/// // let mut spawn = || -> Result<Option<(CommandChild, u32)>, DaemonBootstrapError> { Ok(None) };
/// // let mut probe = || async { Ok(ProbeOutcome::Compatible(default_health_response())) };
/// // spawn_and_wait_for_compatible(&gui_state, &mut spawn, &mut probe, Duration::from_secs(5), Duration::from_millis(100), SpawnReason::Absent).await?;
/// ```
async fn spawn_and_wait_for_compatible<Spawn, Probe, ProbeFuture>(
    gui_owned_daemon_state: &GuiOwnedDaemonState,
    spawn: &mut Spawn,
    probe: &mut Probe,
    timeout: Duration,
    poll_interval: Duration,
    spawn_reason: SpawnReason,
) -> Result<(), DaemonBootstrapError>
where
    Spawn: FnMut() -> Result<Option<(CommandChild, u32)>, DaemonBootstrapError>,
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
{
    match spawn()? {
        Some((child, pid)) => {
            gui_owned_daemon_state.record_spawned(child, pid, spawn_reason);
        }
        None => {
            let _ = gui_owned_daemon_state.clear();
        }
    }

    let wait_result = wait_for_daemon_health(probe, timeout, poll_interval).await;
    if wait_result.is_err() {
        let _ = gui_owned_daemon_state.clear();
    }
    wait_result
}

/// Attempts to replace a running but incompatible daemon and ensure a compatible one is started.
///
/// If the maximum number of replacement attempts has been reached, this returns
/// `DaemonBootstrapError::IncompatibleDaemon` with the provided `details`. Otherwise it:
/// increments the attempt counter, calls the provided termination hook for the
/// incompatible daemon, waits for the daemon's endpoint to disappear within
/// `incompatible_exit_timeout` (polling every `poll_interval`), clears GUI-owned
/// daemon state, then invokes the spawn hook and waits for the spawned daemon to
/// become healthy within `timeout`.
///
/// # Errors
///
/// Returns `DaemonBootstrapError::IncompatibleDaemon` when the replacement-attempt
/// limit is exceeded. Other `DaemonBootstrapError` variants are propagated from
/// the provided hooks and probe/wait helpers.
///
/// # Examples
///
/// ```
/// # use std::time::Duration;
/// # use std::future;
/// # use uc_daemon_local::daemon_bootstrap::{replace_incompatible_daemon, ProbeOutcome, DaemonBootstrapError};
/// # // Minimal stand-ins for the example (real code uses crate types).
/// struct GuiOwnedDaemonState;
/// impl GuiOwnedDaemonState { fn clear(&self) -> Result<(), ()> { Ok(()) } }
///
/// #[tokio::main]
/// async fn main() -> Result<(), DaemonBootstrapError> {
///     let mut attempts = 0u8;
///     let gui_state = GuiOwnedDaemonState;
///     let mut terminate = || -> Result<(), DaemonBootstrapError> { Ok(()) };
///     let mut spawn = || -> Result<Option<(uc_daemon_local::CommandChild, u32)>, DaemonBootstrapError> {
///         Ok(None)
///     };
///     let mut probe = || future::ready(Ok(ProbeOutcome::Absent));
///
///     replace_incompatible_daemon(
///         &mut attempts,
///         &gui_state,
///         "incompatible daemon".to_string(),
///         &mut terminate,
///         &mut spawn,
///         &mut probe,
///         Duration::from_secs(1),
///         Duration::from_secs(5),
///         Duration::from_millis(100),
///     ).await?;
///     Ok(())
/// }
/// ```
async fn replace_incompatible_daemon<Terminate, Spawn, Probe, ProbeFuture>(
    replacement_attempt: &mut u8,
    gui_owned_daemon_state: &GuiOwnedDaemonState,
    details: String,
    terminate_incompatible: &mut Terminate,
    spawn: &mut Spawn,
    probe: &mut Probe,
    incompatible_exit_timeout: Duration,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<(), DaemonBootstrapError>
where
    Terminate: FnMut() -> Result<(), DaemonBootstrapError>,
    Spawn: FnMut() -> Result<Option<(CommandChild, u32)>, DaemonBootstrapError>,
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
{
    if *replacement_attempt >= MAX_INCOMPATIBLE_REPLACEMENT_ATTEMPTS {
        return Err(DaemonBootstrapError::IncompatibleDaemon { details });
    }

    *replacement_attempt += 1;
    terminate_incompatible()?;
    wait_for_endpoint_absent(probe, incompatible_exit_timeout, poll_interval, &details).await?;
    let _ = gui_owned_daemon_state.clear();
    spawn_and_wait_for_compatible(
        gui_owned_daemon_state,
        spawn,
        probe,
        timeout,
        poll_interval,
        SpawnReason::Replacement,
    )
    .await
}

// `wait_for_daemon_health` and `wait_for_endpoint_absent` were moved to
// `crate::health_wait` so non-sidecar consumers (uc-desktop) can use them
// without enabling `sidecar-lifecycle`. They are re-exported at the top of
// this module for backward compatibility.

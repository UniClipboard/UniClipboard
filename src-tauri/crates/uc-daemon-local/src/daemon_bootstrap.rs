use anyhow::Result;
use std::future::Future;
use std::time::Duration;

use tauri_plugin_shell::process::CommandChild;
use thiserror::Error;
use uc_daemon_contract::api::auth::DaemonConnectionInfo;
use uc_daemon_contract::api::types::HealthResponse;

use crate::daemon_lifecycle::{GuiOwnedDaemonState, SpawnReason};

const MAX_INCOMPATIBLE_REPLACEMENT_ATTEMPTS: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    Absent,
    Compatible(HealthResponse),
    Incompatible {
        details: String,
        observed_package_version: Option<String>,
        observed_api_revision: Option<String>,
    },
}

#[derive(Debug, Error)]
pub enum DaemonBootstrapError {
    #[error("failed to initialize daemon HTTP probe client: {0}")]
    Client(anyhow::Error),
    #[error("failed to probe daemon health: {0}")]
    Probe(anyhow::Error),
    #[error("incompatible daemon is already running: {details}")]
    IncompatibleDaemon { details: String },
    #[error("failed to spawn uniclipboard-daemon: {0}")]
    Spawn(anyhow::Error),
    #[error("daemon startup timed out after {timeout_ms}ms")]
    StartupTimeout { timeout_ms: u64 },
    #[error("failed to load daemon connection info: {0}")]
    ConnectionInfo(anyhow::Error),
}

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

pub async fn wait_for_daemon_health<Probe, ProbeFuture>(
    probe: &mut Probe,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<(), DaemonBootstrapError>
where
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match probe().await? {
            ProbeOutcome::Compatible(_) => return Ok(()),
            ProbeOutcome::Absent => {}
            ProbeOutcome::Incompatible { details, .. } => {
                return Err(DaemonBootstrapError::IncompatibleDaemon { details });
            }
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(DaemonBootstrapError::StartupTimeout {
                timeout_ms: timeout.as_millis() as u64,
            });
        }

        tokio::time::sleep(poll_interval).await;
    }
}

async fn wait_for_endpoint_absent<Probe, ProbeFuture>(
    probe: &mut Probe,
    timeout: Duration,
    poll_interval: Duration,
    last_reason: &str,
) -> Result<(), DaemonBootstrapError>
where
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
{
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        match probe().await? {
            ProbeOutcome::Absent => return Ok(()),
            ProbeOutcome::Compatible(_) | ProbeOutcome::Incompatible { .. } => {}
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(DaemonBootstrapError::IncompatibleDaemon {
                details: format!(
                    "incompatible daemon did not exit within {}ms after replacement attempt: {}",
                    timeout.as_millis(),
                    last_reason
                ),
            });
        }

        tokio::time::sleep(poll_interval).await;
    }
}

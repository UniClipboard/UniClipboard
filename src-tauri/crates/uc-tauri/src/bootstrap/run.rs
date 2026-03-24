use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use thiserror::Error;
use tokio_util::sync::CancellationToken;
use uc_daemon::api::auth::{resolve_daemon_token_path, DaemonConnectionInfo};
use uc_daemon::api::types::HealthResponse;
use uc_daemon::process_metadata::read_pid_file;
use uc_daemon::socket::{resolve_daemon_socket_path, try_resolve_daemon_http_addr};
use uc_daemon::DAEMON_API_REVISION;
use uc_daemon_client::DaemonConnectionState;

use super::daemon_lifecycle::{GuiOwnedDaemonState, SpawnReason};
use super::runtime::DaemonBootstrapOwnershipState;
use crate::commands::startup::StartupBarrier;

pub const DAEMON_CONNECTION_EVENT: &str = "daemon://connection-info";
const DAEMON_BINARY_NAME: &str = "uniclipboard-daemon";
const HEALTH_PATH: &str = "/health";
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(8);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(200);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const INCOMPATIBLE_DAEMON_EXIT_TIMEOUT: Duration = Duration::from_millis(1500);
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
    #[error("main webview window is not available")]
    MainWindowUnavailable,
    #[error("failed to emit daemon connection info event: {0}")]
    Emit(anyhow::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DaemonConnectionPayload {
    base_url: String,
    ws_url: String,
    token: String,
}

impl From<&DaemonConnectionInfo> for DaemonConnectionPayload {
    fn from(value: &DaemonConnectionInfo) -> Self {
        Self {
            base_url: value.base_url.clone(),
            ws_url: value.ws_url.clone(),
            token: value.token.clone(),
        }
    }
}

pub async fn bootstrap_daemon_connection(
    state: &DaemonConnectionState,
    gui_owned_daemon_state: &GuiOwnedDaemonState,
) -> Result<DaemonConnectionInfo, DaemonBootstrapError> {
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| {
            DaemonBootstrapError::Client(
                anyhow::Error::new(error).context("failed to build daemon probe client"),
            )
        })?;

    let ownership = DaemonBootstrapOwnershipState::default();
    bootstrap_daemon_connection_with_hooks(
        state,
        &ownership,
        gui_owned_daemon_state,
        || spawn_daemon_process().map(Some),
        || probe_daemon_health(&client),
        load_daemon_connection_info,
        terminate_incompatible_daemon_from_pid_file,
        INCOMPATIBLE_DAEMON_EXIT_TIMEOUT,
        HEALTH_CHECK_TIMEOUT,
        HEALTH_POLL_INTERVAL,
    )
    .await
}

const SUPERVISOR_POLL_INTERVAL: Duration = Duration::from_secs(5);
const SUPERVISOR_RESPAWN_BACKOFF_INITIAL: Duration = Duration::from_secs(2);
const SUPERVISOR_RESPAWN_BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Continuously monitors the owned daemon process and respawns it if it dies.
///
/// Runs until the cancellation token is triggered (app exit). After a successful
/// respawn, updates `DaemonConnectionState` so the WS bridge can reconnect.
pub async fn supervise_daemon(
    state: &DaemonConnectionState,
    gui_owned_daemon_state: &GuiOwnedDaemonState,
    token: CancellationToken,
) {
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .expect("reqwest client should build");

    let mut respawn_backoff = SUPERVISOR_RESPAWN_BACKOFF_INITIAL;

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                tracing::debug!("Daemon supervisor shutting down");
                return;
            }
            _ = tokio::time::sleep(SUPERVISOR_POLL_INTERVAL) => {}
        }

        if gui_owned_daemon_state.exit_cleanup_in_progress() {
            continue;
        }

        // Check if our owned daemon is still alive via health probe.
        let health = match probe_daemon_health(&client).await {
            Ok(ProbeOutcome::Compatible(_)) => {
                respawn_backoff = SUPERVISOR_RESPAWN_BACKOFF_INITIAL;
                continue;
            }
            Ok(outcome) => outcome,
            Err(err) => {
                tracing::warn!(error = %err, "Daemon supervisor health probe error");
                continue;
            }
        };

        // Daemon is absent or incompatible — only respawn if we previously owned one.
        if gui_owned_daemon_state.snapshot_pid().is_none() {
            continue;
        }

        tracing::warn!(
            outcome = ?health,
            "Daemon supervisor detected owned daemon is gone; attempting respawn"
        );

        match spawn_daemon_process() {
            Ok(child) => {
                let ownership = DaemonBootstrapOwnershipState::default();
                gui_owned_daemon_state.record_spawned(child, SpawnReason::Replacement);
                ownership.record_spawned_child(gui_owned_daemon_state.snapshot_pid());

                // Wait for it to become healthy.
                let mut probe_fn = || probe_daemon_health(&client);
                match wait_for_daemon_health(
                    &mut probe_fn,
                    HEALTH_CHECK_TIMEOUT,
                    HEALTH_POLL_INTERVAL,
                )
                .await
                {
                    Ok(()) => {
                        match load_daemon_connection_info() {
                            Ok(info) => {
                                state.set(info);
                                tracing::info!("Daemon supervisor respawned daemon successfully");
                            }
                            Err(err) => {
                                tracing::error!(error = %err, "Daemon supervisor respawned daemon but failed to load connection info");
                            }
                        }
                        respawn_backoff = SUPERVISOR_RESPAWN_BACKOFF_INITIAL;
                    }
                    Err(err) => {
                        tracing::error!(error = %err, "Daemon supervisor respawned daemon but health check failed");
                    }
                }
            }
            Err(err) => {
                tracing::error!(
                    error = %err,
                    backoff_ms = respawn_backoff.as_millis() as u64,
                    "Daemon supervisor failed to respawn daemon"
                );
                tokio::select! {
                    _ = token.cancelled() => return,
                    _ = tokio::time::sleep(respawn_backoff) => {}
                }
                respawn_backoff = (respawn_backoff * 2).min(SUPERVISOR_RESPAWN_BACKOFF_MAX);
            }
        }
    }
}

pub fn emit_daemon_connection_info_if_ready<R: Runtime>(
    app_handle: &AppHandle<R>,
    state: &DaemonConnectionState,
    startup_barrier: &StartupBarrier,
) -> Result<bool, DaemonBootstrapError> {
    if !startup_barrier.frontend_ready() {
        return Ok(false);
    }

    let connection_info = match state.get() {
        Some(connection_info) => connection_info,
        None => return Ok(false),
    };

    if !startup_barrier.try_begin_daemon_connection_emit() {
        return Ok(false);
    }

    let window = match app_handle.get_webview_window("main") {
        Some(window) => window,
        None => {
            startup_barrier.release_daemon_connection_emit();
            return Err(DaemonBootstrapError::MainWindowUnavailable);
        }
    };

    let payload = DaemonConnectionPayload::from(&connection_info);
    if let Err(error) = window.emit(DAEMON_CONNECTION_EVENT, payload) {
        startup_barrier.release_daemon_connection_emit();
        return Err(DaemonBootstrapError::Emit(anyhow::Error::new(error)));
    }

    Ok(true)
}

pub async fn bootstrap_daemon_connection_with_hooks<
    Spawn,
    Probe,
    ProbeFuture,
    LoadInfo,
    Terminate,
>(
    state: &DaemonConnectionState,
    ownership: &DaemonBootstrapOwnershipState,
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
    Spawn: FnMut() -> Result<Option<Child>, DaemonBootstrapError>,
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
    LoadInfo: Fn() -> Result<DaemonConnectionInfo, DaemonBootstrapError>,
    Terminate: FnMut() -> Result<(), DaemonBootstrapError>,
{
    match probe().await? {
        ProbeOutcome::Compatible(_) => {
            let _ = gui_owned_daemon_state.clear();
        }
        ProbeOutcome::Absent => {
            spawn_and_wait_for_compatible(
                ownership,
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
                ownership,
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

    let connection_info = load_connection_info()?;
    state.set(connection_info.clone());
    Ok(connection_info)
}

async fn spawn_and_wait_for_compatible<Spawn, Probe, ProbeFuture>(
    ownership: &DaemonBootstrapOwnershipState,
    gui_owned_daemon_state: &GuiOwnedDaemonState,
    spawn: &mut Spawn,
    probe: &mut Probe,
    timeout: Duration,
    poll_interval: Duration,
    spawn_reason: SpawnReason,
) -> Result<(), DaemonBootstrapError>
where
    Spawn: FnMut() -> Result<Option<Child>, DaemonBootstrapError>,
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
{
    match spawn()? {
        Some(child) => {
            let child_pid = child.id();
            gui_owned_daemon_state.record_spawned(child, spawn_reason);
            ownership.record_spawned_child(Some(child_pid));
        }
        None => {
            let _ = gui_owned_daemon_state.clear();
            ownership.clear_spawned_child();
        }
    }

    let wait_result = wait_for_daemon_health(probe, timeout, poll_interval).await;
    if wait_result.is_err() {
        let _ = gui_owned_daemon_state.clear();
        ownership.clear_spawned_child();
    }
    wait_result
}

async fn replace_incompatible_daemon<Terminate, Spawn, Probe, ProbeFuture>(
    ownership: &DaemonBootstrapOwnershipState,
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
    Spawn: FnMut() -> Result<Option<Child>, DaemonBootstrapError>,
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
{
    if ownership.snapshot().replacement_attempt >= MAX_INCOMPATIBLE_REPLACEMENT_ATTEMPTS {
        return Err(DaemonBootstrapError::IncompatibleDaemon { details });
    }

    ownership.record_replacement_attempt(details.clone());
    terminate_incompatible()?;
    wait_for_endpoint_absent(probe, incompatible_exit_timeout, poll_interval, &details).await?;
    let _ = gui_owned_daemon_state.clear();
    ownership.clear_spawned_child();
    spawn_and_wait_for_compatible(
        ownership,
        gui_owned_daemon_state,
        spawn,
        probe,
        timeout,
        poll_interval,
        SpawnReason::Replacement,
    )
    .await
}

async fn wait_for_daemon_health<Probe, ProbeFuture>(
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

async fn probe_daemon_health(
    client: &reqwest::Client,
) -> Result<ProbeOutcome, DaemonBootstrapError> {
    let addr = try_resolve_daemon_http_addr().map_err(|error| {
        DaemonBootstrapError::Probe(
            error.context("failed to resolve profile-aware daemon HTTP address"),
        )
    })?;
    probe_daemon_health_at(client, addr).await
}

async fn probe_daemon_health_at(
    client: &reqwest::Client,
    addr: std::net::SocketAddr,
) -> Result<ProbeOutcome, DaemonBootstrapError> {
    let url = format!("http://{}:{}{}", addr.ip(), addr.port(), HEALTH_PATH);

    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => return Ok(ProbeOutcome::Absent),
        Err(error) => {
            return Err(DaemonBootstrapError::Probe(
                anyhow::Error::new(error).context("daemon health probe request failed"),
            ))
        }
    };

    if !response.status().is_success() {
        return Ok(ProbeOutcome::Incompatible {
            details: format!("daemon health probe returned HTTP {}", response.status()),
            observed_package_version: None,
            observed_api_revision: None,
        });
    }

    let body = response.text().await.map_err(|error| {
        DaemonBootstrapError::Probe(
            anyhow::Error::new(error).context("failed to read daemon health response body"),
        )
    })?;
    let health = match serde_json::from_str::<HealthResponse>(&body) {
        Ok(health) => health,
        Err(error) => {
            return Ok(ProbeOutcome::Incompatible {
                details: format!("failed to decode daemon health response: {error}"),
                observed_package_version: None,
                observed_api_revision: None,
            });
        }
    };

    Ok(classify_health_response(health))
}

fn classify_health_response(health: HealthResponse) -> ProbeOutcome {
    let observed_package_version = Some(health.package_version.clone());
    let observed_api_revision = Some(health.api_revision.clone());

    if health.status != "ok" {
        return ProbeOutcome::Incompatible {
            details: format!("daemon reported unhealthy status {}", health.status),
            observed_package_version,
            observed_api_revision,
        };
    }

    if health.package_version.trim().is_empty() {
        return ProbeOutcome::Incompatible {
            details: "daemon health response missing packageVersion".to_string(),
            observed_package_version,
            observed_api_revision,
        };
    }

    if health.api_revision.trim().is_empty() {
        return ProbeOutcome::Incompatible {
            details: "daemon health response missing apiRevision".to_string(),
            observed_package_version,
            observed_api_revision,
        };
    }

    if health.package_version != env!("CARGO_PKG_VERSION") {
        return ProbeOutcome::Incompatible {
            details: format!(
                "daemon packageVersion {} does not match GUI packageVersion {}",
                health.package_version,
                env!("CARGO_PKG_VERSION")
            ),
            observed_package_version,
            observed_api_revision,
        };
    }

    if health.api_revision != DAEMON_API_REVISION {
        return ProbeOutcome::Incompatible {
            details: format!(
                "daemon apiRevision {} does not match required {}",
                health.api_revision, DAEMON_API_REVISION
            ),
            observed_package_version,
            observed_api_revision,
        };
    }

    ProbeOutcome::Compatible(health)
}

fn load_daemon_connection_info() -> Result<DaemonConnectionInfo, DaemonBootstrapError> {
    let token_path = resolve_token_path();
    let token = std::fs::read_to_string(&token_path).map_err(|error| {
        DaemonBootstrapError::ConnectionInfo(anyhow::Error::new(error).context(format!(
            "failed to read daemon auth token at {}",
            token_path.display()
        )))
    })?;
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(DaemonBootstrapError::ConnectionInfo(anyhow::anyhow!(
            "daemon auth token at {} is empty",
            token_path.display()
        )));
    }

    let addr = try_resolve_daemon_http_addr().map_err(|error| {
        DaemonBootstrapError::ConnectionInfo(
            error.context("failed to resolve profile-aware daemon HTTP address"),
        )
    })?;
    Ok(DaemonConnectionInfo {
        base_url: format!("http://{}:{}", addr.ip(), addr.port()),
        ws_url: format!("ws://{}:{}/ws", addr.ip(), addr.port()),
        token,
    })
}

fn terminate_incompatible_daemon_from_pid_file() -> Result<(), DaemonBootstrapError> {
    let pid = read_pid_file()
        .map_err(|error| DaemonBootstrapError::IncompatibleDaemon {
            details: format!("failed to read daemon pid metadata: {error}"),
        })?
        .ok_or_else(|| DaemonBootstrapError::IncompatibleDaemon {
            details: "expected incompatible daemon pid metadata was missing".to_string(),
        })?;

    terminate_local_daemon_pid(pid)
}

pub(crate) fn terminate_local_daemon_pid(pid: u32) -> Result<(), DaemonBootstrapError> {
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("kill");
        command.arg("-TERM").arg(pid.to_string());
        command
    };

    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("taskkill");
        command.arg("/PID").arg(pid.to_string()).arg("/T").arg("/F");
        command
    };

    let output = command
        .output()
        .map_err(|error| DaemonBootstrapError::IncompatibleDaemon {
            details: format!(
                "failed to launch incompatible daemon terminator for pid {pid}: {error}"
            ),
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(DaemonBootstrapError::IncompatibleDaemon {
        details: format!(
            "failed to terminate incompatible daemon pid {pid}: status={} stdout={} stderr={}",
            output.status,
            stdout.trim(),
            stderr.trim()
        ),
    })
}

fn spawn_daemon_process() -> Result<Child, DaemonBootstrapError> {
    let daemon_binary = resolve_daemon_binary_path().map_err(|error| {
        DaemonBootstrapError::Spawn(
            anyhow::Error::new(error).context("failed to resolve daemon binary path"),
        )
    })?;

    Command::new(&daemon_binary)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            DaemonBootstrapError::Spawn(anyhow::Error::new(error).context(format!(
                "failed to spawn {} from {}",
                DAEMON_BINARY_NAME,
                daemon_binary.display()
            )))
        })
}

fn resolve_daemon_binary_path() -> std::io::Result<PathBuf> {
    let current_exe = std::env::current_exe()?;
    let binary_name = daemon_binary_name();
    let sibling = current_exe
        .parent()
        .map(|parent| parent.join(binary_name))
        .filter(|candidate| candidate.exists());

    Ok(sibling.unwrap_or_else(|| PathBuf::from(binary_name)))
}

fn daemon_binary_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "uniclipboard-daemon.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        DAEMON_BINARY_NAME
    }
}

fn resolve_token_path() -> PathBuf {
    let socket_path = resolve_daemon_socket_path();
    let token_base_dir = socket_path.parent().unwrap_or_else(|| Path::new("/tmp"));
    resolve_daemon_token_path(token_base_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::sync::{Mutex, OnceLock};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn with_daemon_env<T>(
        profile: Option<&str>,
        xdg_runtime_dir: Option<&Path>,
        f: impl FnOnce() -> T,
    ) -> T {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_profile = std::env::var("UC_PROFILE").ok();
        let previous_xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok();

        match profile {
            Some(profile) => std::env::set_var("UC_PROFILE", profile),
            None => std::env::remove_var("UC_PROFILE"),
        }
        match xdg_runtime_dir {
            Some(path) => std::env::set_var("XDG_RUNTIME_DIR", path),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }

        let result = f();

        match previous_profile {
            Some(profile) => std::env::set_var("UC_PROFILE", profile),
            None => std::env::remove_var("UC_PROFILE"),
        }
        match previous_xdg_runtime_dir {
            Some(path) => std::env::set_var("XDG_RUNTIME_DIR", path),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }

        result
    }

    async fn spawn_health_server(status_line: &str, body: &str) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let status_line = status_line.to_string();
        let body = body.to_string();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer).await.unwrap();
            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        addr
    }

    #[tokio::test]
    async fn probe_helper_returns_success_on_healthy_health_endpoint() {
        let body = format!(
            r#"{{"status":"ok","packageVersion":"{}","apiRevision":"{}"}}"#,
            env!("CARGO_PKG_VERSION"),
            DAEMON_API_REVISION
        );
        let addr = spawn_health_server("200 OK", &body).await;

        let client = reqwest::Client::builder()
            .timeout(PROBE_TIMEOUT)
            .build()
            .unwrap();
        let outcome = probe_daemon_health_at(&client, addr).await.unwrap();

        assert!(matches!(
            outcome,
            ProbeOutcome::Compatible(HealthResponse {
                status,
                package_version,
                api_revision,
            }) if status == "ok"
                && package_version == env!("CARGO_PKG_VERSION")
                && api_revision == DAEMON_API_REVISION
        ));
    }

    #[tokio::test]
    async fn startup_helper_treats_http_response_with_503_as_incompatible() {
        let addr = spawn_health_server("503 Service Unavailable", r#"{"status":"starting"}"#).await;
        let client = reqwest::Client::builder()
            .timeout(PROBE_TIMEOUT)
            .build()
            .unwrap();

        let outcome = probe_daemon_health_at(&client, addr).await.unwrap();

        assert!(matches!(
            outcome,
            ProbeOutcome::Incompatible {
                observed_package_version: None,
                observed_api_revision: None,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn startup_helper_treats_malformed_health_payload_as_incompatible() {
        let addr = spawn_health_server("200 OK", r#"{"status":"ok","version":"0.1.0"}"#).await;
        let client = reqwest::Client::builder()
            .timeout(PROBE_TIMEOUT)
            .build()
            .unwrap();

        let outcome = probe_daemon_health_at(&client, addr).await.unwrap();

        assert!(matches!(
            outcome,
            ProbeOutcome::Incompatible { details, .. }
                if details.contains("failed to decode daemon health response")
        ));
    }

    #[tokio::test]
    async fn startup_helper_rejects_healthy_but_incompatible_daemon() {
        let body = format!(
            r#"{{"status":"ok","packageVersion":"{}-stale","apiRevision":"{}"}}"#,
            env!("CARGO_PKG_VERSION"),
            DAEMON_API_REVISION
        );
        let addr = spawn_health_server("200 OK", &body).await;
        let client = reqwest::Client::builder()
            .timeout(PROBE_TIMEOUT)
            .build()
            .unwrap();

        let incompatible_outcome = probe_daemon_health_at(&client, addr).await.unwrap();
        let state = DaemonConnectionState::default();
        let ownership = DaemonBootstrapOwnershipState::default();
        let gui_owned_daemon_state = GuiOwnedDaemonState::default();
        let result = bootstrap_daemon_connection_with_hooks(
            &state,
            &ownership,
            &gui_owned_daemon_state,
            || panic!("spawn should not run when an incompatible daemon is already listening"),
            || {
                let incompatible_outcome = incompatible_outcome.clone();
                async move { Ok(incompatible_outcome) }
            },
            || unreachable!(),
            || unreachable!(),
            Duration::from_millis(10),
            Duration::from_millis(10),
            Duration::from_millis(1),
        )
        .await;

        assert!(matches!(
            result,
            Err(DaemonBootstrapError::IncompatibleDaemon { details })
                if details.contains("does not match GUI packageVersion")
        ));
    }

    #[tokio::test]
    async fn startup_helper_rejects_healthy_but_api_incompatible_daemon() {
        let body = format!(
            r#"{{"status":"ok","packageVersion":"{}","apiRevision":"legacy-v0"}}"#,
            env!("CARGO_PKG_VERSION")
        );
        let addr = spawn_health_server("200 OK", &body).await;
        let client = reqwest::Client::builder()
            .timeout(PROBE_TIMEOUT)
            .build()
            .unwrap();

        let outcome = probe_daemon_health_at(&client, addr).await.unwrap();

        assert!(matches!(
            outcome,
            ProbeOutcome::Incompatible {
                observed_package_version: Some(observed_package_version),
                observed_api_revision: Some(observed_api_revision),
                ..
            } if observed_package_version == env!("CARGO_PKG_VERSION")
                && observed_api_revision == "legacy-v0"
        ));
    }

    #[tokio::test]
    async fn startup_helper_treats_spawn_failure_as_error() {
        let state = DaemonConnectionState::default();
        let ownership = DaemonBootstrapOwnershipState::default();
        let gui_owned_daemon_state = GuiOwnedDaemonState::default();
        let result = bootstrap_daemon_connection_with_hooks(
            &state,
            &ownership,
            &gui_owned_daemon_state,
            || Err(DaemonBootstrapError::Spawn(anyhow::anyhow!("spawn failed"))),
            || async { Ok(ProbeOutcome::Absent) },
            || unreachable!(),
            || unreachable!(),
            Duration::from_millis(10),
            Duration::from_millis(10),
            Duration::from_millis(1),
        )
        .await;

        assert!(matches!(result, Err(DaemonBootstrapError::Spawn(_))));
    }

    #[tokio::test]
    async fn startup_helper_treats_timeout_as_error() {
        let state = DaemonConnectionState::default();
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_probe = attempts.clone();
        let ownership = DaemonBootstrapOwnershipState::default();
        let gui_owned_daemon_state = GuiOwnedDaemonState::default();
        let result = bootstrap_daemon_connection_with_hooks(
            &state,
            &ownership,
            &gui_owned_daemon_state,
            || Ok(None),
            move || {
                let attempts_for_probe = attempts_for_probe.clone();
                async move {
                    attempts_for_probe.fetch_add(1, Ordering::SeqCst);
                    Ok(ProbeOutcome::Absent)
                }
            },
            || unreachable!(),
            || unreachable!(),
            Duration::from_millis(10),
            Duration::from_millis(20),
            Duration::from_millis(5),
        )
        .await;

        assert!(matches!(
            result,
            Err(DaemonBootstrapError::StartupTimeout { .. })
        ));
        assert!(attempts.load(Ordering::SeqCst) >= 2);
    }

    #[test]
    fn emitted_event_payload_uses_camel_case_keys() {
        let payload = DaemonConnectionPayload::from(&DaemonConnectionInfo {
            base_url: "http://127.0.0.1:42715".to_string(),
            ws_url: "ws://127.0.0.1:42715/ws".to_string(),
            token: "secret".to_string(),
        });

        let value = serde_json::to_value(payload).unwrap();

        assert_eq!(value["baseUrl"], "http://127.0.0.1:42715");
        assert_eq!(value["wsUrl"], "ws://127.0.0.1:42715/ws");
        assert_eq!(value["token"], "secret");
        assert!(value.get("base_url").is_none());
        assert!(value.get("ws_url").is_none());
    }

    #[test]
    fn resolve_token_path_tracks_uc_profile() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");

        let token_path_a = with_daemon_env(Some("a"), Some(tempdir.path()), resolve_token_path);
        let token_path_b = with_daemon_env(Some("b"), Some(tempdir.path()), resolve_token_path);

        assert_eq!(
            token_path_a.file_name().and_then(std::ffi::OsStr::to_str),
            Some("uniclipboard-daemon-a.token")
        );
        assert_eq!(
            token_path_b.file_name().and_then(std::ffi::OsStr::to_str),
            Some("uniclipboard-daemon-b.token")
        );
        assert_ne!(token_path_a, token_path_b);
    }

    #[test]
    fn load_daemon_connection_info_uses_profile_specific_urls() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");

        let connection_a = with_daemon_env(Some("a"), Some(tempdir.path()), || {
            std::fs::write(
                tempdir.path().join("uniclipboard-daemon-a.token"),
                "token-a",
            )
            .expect("token fixture should be written");
            load_daemon_connection_info().expect("profile a connection info should load")
        });
        let connection_b = with_daemon_env(Some("b"), Some(tempdir.path()), || {
            std::fs::write(
                tempdir.path().join("uniclipboard-daemon-b.token"),
                "token-b",
            )
            .expect("token fixture should be written");
            load_daemon_connection_info().expect("profile b connection info should load")
        });

        assert_eq!(connection_a.base_url, "http://127.0.0.1:42716");
        assert_eq!(connection_a.ws_url, "ws://127.0.0.1:42716/ws");
        assert_eq!(connection_a.token, "token-a");
        assert_eq!(connection_b.base_url, "http://127.0.0.1:42717");
        assert_eq!(connection_b.ws_url, "ws://127.0.0.1:42717/ws");
        assert_eq!(connection_b.token, "token-b");
    }
}

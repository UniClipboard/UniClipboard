use anyhow::Result;
use std::time::Duration;

use reqwest::header::AUTHORIZATION;
use tauri::{AppHandle, Runtime};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;
use tokio_util::sync::CancellationToken;
use uc_daemon_client::http::{clear_session_token_cache, exchange_session_token};
use uc_daemon_client::DaemonConnectionState;
use uc_daemon_contract::api::auth::DaemonConnectionInfo;
use uc_daemon_contract::api::types::HealthResponse;
use uc_daemon_contract::DAEMON_API_REVISION;
use uc_daemon_local::daemon_bootstrap::{
    bootstrap_daemon_connection_with_hooks, wait_for_daemon_health, DaemonBootstrapError,
    ProbeOutcome,
};
use uc_daemon_local::daemon_lifecycle::terminate_local_daemon_pid;
use uc_daemon_local::daemon_lifecycle::{GuiOwnedDaemonState, SpawnReason};
use uc_daemon_local::process_metadata::read_pid_file;
use uc_daemon_local::socket::try_resolve_daemon_http_addr;

const HEALTH_PATH: &str = "/health";
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(8);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(200);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const INCOMPATIBLE_DAEMON_EXIT_TIMEOUT: Duration = Duration::from_millis(1500);

/// Bootstraps a connection to the local daemon, starting or reconnecting the daemon as needed and recording the resulting connection information in `state`.
///
/// This function builds an HTTP probe client, uses the daemon-bootstrap helpers to ensure a compatible daemon is running (spawning one when necessary), and then stores the established `DaemonConnectionInfo` into `state`.
///
/// # Returns
///
/// `Ok(DaemonConnectionInfo)` with the established connection information on success, or a `DaemonBootstrapError` describing why bootstrapping failed.
///
/// # Examples
///
/// ```no_run
/// # use uc_tauri::bootstrap::run::bootstrap_daemon_connection;
/// # use tauri::AppHandle;
/// # use uc_tauri::state::DaemonConnectionState;
/// # use uc_tauri::state::GuiOwnedDaemonState;
/// # async fn example(app: AppHandle<tauri::Wry>, state: DaemonConnectionState, gui_state: GuiOwnedDaemonState) {
/// let conn = bootstrap_daemon_connection(&app, &state, &gui_state).await;
/// match conn {
///     Ok(info) => println!("Connected to daemon at {}", info.base_url),
///     Err(e) => eprintln!("Failed to bootstrap daemon: {}", e),
/// }
/// # }
/// ```
pub async fn bootstrap_daemon_connection<R: Runtime>(
    app: &AppHandle<R>,
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

    let app = app.clone();
    let connection_info = bootstrap_daemon_connection_with_hooks(
        gui_owned_daemon_state,
        || {
            let (child, pid) = spawn_daemon_process(&app)?;
            Ok(Some((child, pid)))
        },
        || probe_daemon_health(&client),
        load_daemon_connection_info,
        terminate_incompatible_daemon_from_pid_file,
        INCOMPATIBLE_DAEMON_EXIT_TIMEOUT,
        HEALTH_CHECK_TIMEOUT,
        HEALTH_POLL_INTERVAL,
    )
    .await?;

    state.set(connection_info.clone());
    Ok(connection_info)
}

const SUPERVISOR_POLL_INTERVAL: Duration = Duration::from_secs(5);
const SUPERVISOR_RESPAWN_BACKOFF_INITIAL: Duration = Duration::from_secs(2);
const SUPERVISOR_RESPAWN_BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Supervises the GUI-owned daemon: monitors its health and respawns it if it stops running.
///
/// The function runs until the provided cancellation token is cancelled. After successfully
/// respawning and verifying the daemon's health, it updates `DaemonConnectionState` so other
/// components (for example the WebSocket bridge) can reconnect to the new daemon instance.
///
/// # Examples
///
/// ```
/// # use tokio::time::{sleep, Duration};
/// # use tokio_util::sync::CancellationToken;
/// # async fn example() {
/// // `app`, `state`, and `gui_owned_daemon_state` need to be created according to application setup.
/// // Here we only illustrate supervisor lifecycle control with a cancellation token.
/// let token = CancellationToken::new();
/// let cancel_child = token.clone();
///
/// // Spawn supervisor (types omitted for brevity).
/// // tokio::spawn(supervise_daemon(&app, &state, &gui_owned_daemon_state, token));
///
/// // Let supervisor run briefly, then request shutdown.
/// sleep(Duration::from_millis(100)).await;
/// cancel_child.cancel();
/// # }
/// ```
pub async fn supervise_daemon<R: Runtime>(
    app: &AppHandle<R>,
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

        match spawn_daemon_process(app) {
            Ok((child, pid)) => {
                gui_owned_daemon_state.record_spawned(child, pid, SpawnReason::Replacement);

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
                                // Replay the GUI's one-shot `/lifecycle/ready` so the
                                // new daemon exits its deferred state (clipboard
                                // watcher + inbound clipboard sync). Without this,
                                // the GUI's own ready latch never refires after a
                                // respawn and sync stays silently dead until the
                                // user restarts the GUI.
                                replay_lifecycle_ready_after_respawn(state, &client).await;
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

/// Re-issue `POST /lifecycle/ready` to a freshly respawned daemon so its
/// deferred services (clipboard watcher, inbound clipboard sync, etc.) start.
///
/// The GUI signals `/lifecycle/ready` exactly once at boot and latches that
/// success in a React ref, so it never reissues after a daemon respawn.
/// Each respawned daemon process boots a fresh JWT secret, so we also clear
/// the cached session token before exchanging a new one.
///
/// All errors are logged and swallowed — the supervisor's main loop must
/// keep running even if this best-effort signal fails.
async fn replay_lifecycle_ready_after_respawn(
    state: &DaemonConnectionState,
    client: &reqwest::Client,
) {
    clear_session_token_cache().await;

    let pid = std::process::id();
    let session_token = match exchange_session_token(client, state, pid, "gui").await {
        Ok(token) => token,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "Daemon supervisor failed to exchange session token after respawn; \
                 deferred services will stay dormant until the GUI re-signals ready"
            );
            return;
        }
    };

    let connection = match state.get() {
        Some(c) => c,
        None => {
            tracing::warn!(
                "Daemon supervisor missing connection info after respawn; \
                 cannot replay /lifecycle/ready"
            );
            return;
        }
    };

    let url = format!("{}/lifecycle/ready", connection.base_url);
    match client
        .post(&url)
        .header(AUTHORIZATION, format!("Session {}", session_token))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            tracing::info!(
                "Daemon supervisor replayed /lifecycle/ready after respawn; \
                 deferred services should now start"
            );
        }
        Ok(response) => {
            tracing::warn!(
                status = %response.status(),
                "Daemon supervisor /lifecycle/ready replay returned non-success"
            );
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "Daemon supervisor /lifecycle/ready replay failed"
            );
        }
    }
}

/// Probes the daemon HTTP health endpoint for the active profile and classifies its health.
///
/// Resolves the profile-aware daemon HTTP address and performs an HTTP probe; on success returns a `ProbeOutcome` describing whether the daemon is compatible, incompatible, or absent, and on failure returns `DaemonBootstrapError::Probe` with context about the resolution or probe error.
///
/// # Examples
///
/// ```no_run
/// # tokio_test::block_on(async {
/// let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(2)).build().unwrap();
/// match probe_daemon_health(&client).await {
///     Ok(outcome) => println!("probe outcome: {:?}", outcome),
///     Err(err) => eprintln!("probe error: {}", err),
/// }
/// # });
/// ```
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
    uc_daemon_client::resolve_connection_info_from_env()
        .map_err(|e| DaemonBootstrapError::ConnectionInfo(e))
}

fn terminate_incompatible_daemon_from_pid_file() -> Result<(), DaemonBootstrapError> {
    let pid = read_pid_file()
        .map_err(|error| DaemonBootstrapError::IncompatibleDaemon {
            details: format!("failed to read daemon pid metadata: {error}"),
        })?
        .ok_or_else(|| DaemonBootstrapError::IncompatibleDaemon {
            details: "expected incompatible daemon pid metadata was missing".to_string(),
        })?;

    terminate_local_daemon_pid(pid).map_err(|e| DaemonBootstrapError::IncompatibleDaemon {
        details: e.to_string(),
    })?;
    Ok(())
}

fn spawn_daemon_process<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(CommandChild, u32), DaemonBootstrapError> {
    let mut sidecar_cmd = app
        .shell()
        .sidecar("uniclipboard-daemon")
        .map_err(|e| {
            DaemonBootstrapError::Spawn(anyhow::Error::msg(format!("sidecar create: {e}")))
        })?
        .args(["--gui-managed"]);

    // Tauri v2 sidecar does NOT inherit the parent environment by default.
    // Forward observability-related env vars so the daemon can initialize its
    // own Seq / Sentry / log-profile layers and emit structured events
    // directly — otherwise the only daemon logs reaching Seq would be the
    // stdout-forwarding wrapper below, which loses target/level/span/fields.
    for key in [
        "UC_SEQ_URL",
        "UC_SEQ_API_KEY",
        "UC_LOG_PROFILE",
        "SENTRY_DSN",
        "RUST_LOG",
        "RUST_BACKTRACE",
    ] {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                sidecar_cmd = sidecar_cmd.env(key, value);
            }
        }
    }

    let (rx, child) = sidecar_cmd.spawn().map_err(|e| {
        DaemonBootstrapError::Spawn(anyhow::Error::msg(format!("sidecar spawn: {e}")))
    })?;

    let pid = child.pid();
    tracing::info!(pid, "daemon sidecar spawned successfully");

    // Drain stdout/stderr events to prevent pipe blocking.
    //
    // The daemon owns its own tracing subscriber (console + JSON file + Seq
    // when UC_SEQ_URL is forwarded above) — it has already done filtering and
    // pretty-formatting for the console output before writing to its stdout.
    // Structured events reach Seq directly from the daemon process.
    //
    // Tauri's sidecar API pipes the child's stdio (it never inherits the
    // parent tty), so we must actively drain `rx`. Write the already-formatted
    // daemon lines to our own stdout/stderr VERBATIM — do NOT re-wrap them in
    // `tracing::*!` events: doing so flattens target/level/span into a `line`
    // string field and produces a noise event in Seq that duplicates the real
    // structured record the daemon already sent.
    //
    // CommandChild holds stdin open, maintaining the D-06 stdin tether.
    tauri::async_runtime::spawn(async move {
        use std::io::Write;
        let mut rx = rx;
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    let mut out = std::io::stdout().lock();
                    let _ = out.write_all(&line);
                    let _ = out.write_all(b"\n");
                    let _ = out.flush();
                }
                CommandEvent::Stderr(line) => {
                    let mut err = std::io::stderr().lock();
                    let _ = err.write_all(&line);
                    let _ = err.write_all(b"\n");
                    let _ = err.flush();
                }
                CommandEvent::Terminated(payload) => {
                    tracing::warn!(?payload, "daemon sidecar terminated");
                    break;
                }
                CommandEvent::Error(err) => {
                    tracing::error!(error = %err, "daemon sidecar error event");
                }
                _ => {}
            }
        }
    });

    Ok((child, pid))
}

use anyhow::Result;
use std::time::Duration;

use tauri::{AppHandle, Runtime};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;
use tokio_util::sync::CancellationToken;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
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
        let previous_token_path = std::env::var("UNICLIPBOARD_DAEMON_TOKEN_PATH").ok();

        match profile {
            Some(profile) => std::env::set_var("UC_PROFILE", profile),
            None => std::env::remove_var("UC_PROFILE"),
        }
        match xdg_runtime_dir {
            Some(path) => std::env::set_var("XDG_RUNTIME_DIR", path),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }
        // Point UNICLIPBOARD_DAEMON_TOKEN_PATH directly at the fixture token file so
        // resolve_token_path() doesn't fall back to the real app_data_root on the CI runner.
        // Fixture filename convention: uniclipboard-daemon-{profile}.token
        match (profile, xdg_runtime_dir) {
            (Some(p), Some(dir)) => {
                let token_path = dir.join(format!("uniclipboard-daemon-{p}.token"));
                std::env::set_var("UNICLIPBOARD_DAEMON_TOKEN_PATH", token_path);
            }
            _ => std::env::remove_var("UNICLIPBOARD_DAEMON_TOKEN_PATH"),
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
        match previous_token_path {
            Some(path) => std::env::set_var("UNICLIPBOARD_DAEMON_TOKEN_PATH", path),
            None => std::env::remove_var("UNICLIPBOARD_DAEMON_TOKEN_PATH"),
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
        let gui_owned_daemon_state = GuiOwnedDaemonState::default();
        // terminate_incompatible is called but daemon stays incompatible (probe never returns
        // Absent), so wait_for_endpoint_absent times out with IncompatibleDaemon.
        // spawn is never reached because the replacement path fails first.
        let result = bootstrap_daemon_connection_with_hooks(
            &gui_owned_daemon_state,
            || panic!("spawn should not run when incompatible daemon does not exit"),
            || {
                let incompatible_outcome = incompatible_outcome.clone();
                async move { Ok(incompatible_outcome) }
            },
            || unreachable!(),
            || Ok(()),
            Duration::from_millis(10),
            Duration::from_millis(10),
            Duration::from_millis(1),
        )
        .await;

        assert!(matches!(
            result,
            Err(DaemonBootstrapError::IncompatibleDaemon { details })
                if details.contains("did not exit within 10ms")
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
        let gui_owned_daemon_state = GuiOwnedDaemonState::default();
        let result = bootstrap_daemon_connection_with_hooks(
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
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_probe = attempts.clone();
        let gui_owned_daemon_state = GuiOwnedDaemonState::default();
        let result = bootstrap_daemon_connection_with_hooks(
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
    fn load_daemon_connection_info_uses_profile_specific_urls() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");

        let token_path_a = tempdir.path().join("token-a");
        let token_path_b = tempdir.path().join("token-b");

        let connection_a = with_daemon_env(Some("a"), Some(tempdir.path()), || {
            std::fs::write(&token_path_a, "token-a").expect("token fixture should be written");
            std::env::set_var("UNICLIPBOARD_DAEMON_TOKEN_PATH", &token_path_a);
            load_daemon_connection_info().expect("profile a connection info should load")
        });
        let connection_b = with_daemon_env(Some("b"), Some(tempdir.path()), || {
            std::fs::write(&token_path_b, "token-b").expect("token fixture should be written");
            std::env::set_var("UNICLIPBOARD_DAEMON_TOKEN_PATH", &token_path_b);
            load_daemon_connection_info().expect("profile b connection info should load")
        });

        assert_eq!(connection_a.base_url, "http://127.0.0.1:42716");
        assert_eq!(connection_a.ws_url, "http://127.0.0.1:42716/ws");
        assert_eq!(connection_a.token, "token-a");
        assert_eq!(connection_b.base_url, "http://127.0.0.1:42717");
        assert_eq!(connection_b.ws_url, "http://127.0.0.1:42717/ws");
        assert_eq!(connection_b.token, "token-b");
    }
}

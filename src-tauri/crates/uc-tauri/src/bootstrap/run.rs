//! Tauri 端 daemon sidecar 的拉起、监督与替换。
//!
//! 这是 `uc-tauri` shell 特有的实现——通过 `tauri-plugin-shell` 拉
//! sidecar、用 `tauri::AppHandle` 持有上下文。GUI-framework agnostic 的
//! 健康探测 / 连接信息加载 / `/lifecycle/ready` 重放等 helper 在
//! [`uc_desktop::daemon_probe`]，本文件只保留 Tauri 拉起编排部分。

use anyhow::Result;

use tauri::{AppHandle, Runtime};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;
use tokio_util::sync::CancellationToken;
use uc_daemon_client::DaemonConnectionState;
use uc_daemon_contract::api::auth::DaemonConnectionInfo;
use uc_daemon_local::contract::{DaemonBootstrapError, ProbeOutcome, SpawnReason};
use uc_daemon_local::daemon_bootstrap::bootstrap_daemon_connection_with_hooks;
use uc_daemon_local::daemon_lifecycle::GuiOwnedDaemonState;
use uc_daemon_local::health_wait::wait_for_daemon_health;

use uc_desktop::daemon_probe::{
    load_daemon_connection_info, probe_daemon_health, replay_lifecycle_ready_after_respawn,
    terminate_incompatible_daemon_from_pid_file, HEALTH_CHECK_TIMEOUT, HEALTH_POLL_INTERVAL,
    INCOMPATIBLE_DAEMON_EXIT_TIMEOUT, PROBE_TIMEOUT, SUPERVISOR_POLL_INTERVAL,
    SUPERVISOR_RESPAWN_BACKOFF_INITIAL, SUPERVISOR_RESPAWN_BACKOFF_MAX,
};

/// 这个 GUI shell 期望 daemon 上报的 `packageVersion`——`probe_daemon_health`
/// 用它做版本兼容性判断。`env!` 拿的是 `uc-tauri` 自己的 cargo 版本，
/// workspace 共享版本号所以与 `uniclipboard` bin 一致。
const EXPECTED_PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

async fn probe(client: &reqwest::Client) -> Result<ProbeOutcome, DaemonBootstrapError> {
    probe_daemon_health(client, EXPECTED_PACKAGE_VERSION).await
}

/// Bootstraps a connection to the local daemon, starting or reconnecting the daemon as needed and recording the resulting connection information in `state`.
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
        || probe(&client),
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

/// Supervises the GUI-owned daemon: monitors its health and respawns it if it stops running.
///
/// The function runs until the provided cancellation token is cancelled. After successfully
/// respawning and verifying the daemon's health, it updates `DaemonConnectionState` so other
/// components (for example the WebSocket bridge) can reconnect to the new daemon instance.
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
        let health = match probe(&client).await {
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
                let mut probe_fn = || probe(&client);
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

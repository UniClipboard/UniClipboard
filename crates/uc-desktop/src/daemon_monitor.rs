//! Daemon crash detection and automatic restart for desktop GUI hosts.
//!
//! [`run`] is a long-lived background task registered with the process
//! [`TaskRegistry`](uc_core::TaskRegistry) after the initial daemon bootstrap
//! succeeds. It probes `/health` every [`CHECK_INTERVAL`]; after
//! [`FAILURE_THRESHOLD`] consecutive misses it calls [`restart_local_daemon`]
//! and refreshes the live [`DaemonConnectionState`], allowing the WS bridge to
//! reconnect automatically without user intervention.
//!
//! ## Design notes
//!
//! **Start-after-bootstrap only.** This task must be spawned only after
//! [`bootstrap_daemon_in_process`](crate::daemon_probe::bootstrap_daemon_in_process)
//! returns `Ok`. Starting before bootstrap completes risks:
//! - treating normal startup latency as a crash, or
//! - calling [`restart_local_daemon`] against a daemon whose version was
//!   legitimately refused (e.g. `RefusedNewerDaemon`), which would kill the
//!   higher-version daemon.
//!
//! **Concurrency with the `restart_daemon` command.** Both this monitor and the
//! `restart_daemon` Tauri command may call [`restart_local_daemon`] concurrently
//! if a manual restart is triggered while the monitor is in its own restart
//! path. The worst outcome is two overlapping restart sequences: both SIGTERM
//! the same PID (idempotent), both wait for process exit, both attempt to
//! spawn a new daemon (the second one fails the instance lock and exits
//! immediately), and both poll `/health` until the winner is healthy. The
//! resulting `connection_state.set()` calls carry the same connection info and
//! are therefore idempotent. This race is acceptable given the low frequency
//! of manual restarts and the idempotency of every step.
//!
//! **Riding out a manual restart.** With [`CHECK_INTERVAL`] = 5 s and
//! [`FAILURE_THRESHOLD`] = 3, the monitor needs 15 s of uninterrupted absence
//! before acting. A manual `restart_daemon` completes in ≤ 10 s end-to-end,
//! so the monitor typically sees 2 absent probes followed by 1 healthy probe
//! and resets its counter — never triggering its own restart.
//!
//! **Shutdown safety.** The task is cancelled via its [`CancellationToken`]
//! (propagated from the process `TaskRegistry`) when the GUI exits. The token
//! is checked at the start of every loop iteration and again immediately before
//! the long-running restart call.

use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use uc_daemon_client::DaemonConnectionState;
use uc_daemon_contract::probe::ProbeOutcome;

use crate::daemon_probe::{probe_daemon_health, restart_local_daemon, PROBE_TIMEOUT};
use crate::daemon_recovery::recover_after_restart;

/// How often to probe the daemon `/health` endpoint.
const CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// Number of consecutive probe failures that trigger an automatic daemon restart.
///
/// With [`CHECK_INTERVAL`] = 5 s, three failures = 15 s of confirmed absence
/// before action is taken. This provides enough headroom to ride out a manual
/// `restart_daemon` command (which takes ≤ ~10 s end-to-end) without reacting
/// to a transient probe hiccup or a deliberate in-progress restart.
const FAILURE_THRESHOLD: u32 = 3;

/// Drive the daemon liveness monitor until `token` is cancelled.
///
/// **Must be started only after the initial daemon bootstrap has succeeded**
/// and `connection_state` holds a valid `DaemonConnectionInfo`. See the
/// module-level documentation for the rationale.
///
/// `expected_package_version` must be the shell crate's own
/// `env!("CARGO_PKG_VERSION")` — the same value passed to
/// [`bootstrap_daemon_in_process`](crate::daemon_probe::bootstrap_daemon_in_process).
/// The `'static` bound ensures the future produced by this async function is
/// `'static` and can be registered with the `TaskRegistry`.
pub async fn run(
    expected_package_version: &'static str,
    connection_state: DaemonConnectionState,
    token: CancellationToken,
) {
    let client = match reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(c) => c,
        Err(error) => {
            warn!(%error, "daemon_monitor: failed to build HTTP probe client; monitor disabled");
            return;
        }
    };

    let mut consecutive_failures: u32 = 0;

    loop {
        // Wait for the next probe window or exit immediately on cancellation.
        // `biased` ensures the cancellation branch is always checked first so
        // the monitor reacts promptly to a shutdown signal.
        tokio::select! {
            biased;
            _ = token.cancelled() => {
                info!("daemon_monitor: cancellation token fired, stopping");
                return;
            }
            _ = tokio::time::sleep(CHECK_INTERVAL) => {}
        }

        let probe_result = probe_daemon_health(&client, expected_package_version).await;

        let alive = match &probe_result {
            Ok(ProbeOutcome::Compatible(_)) => true,
            Ok(other) => {
                debug!(
                    outcome = ?other,
                    "daemon_monitor: probe returned non-Compatible outcome"
                );
                false
            }
            Err(error) => {
                warn!(%error, "daemon_monitor: probe request error (treating as absence)");
                false
            }
        };

        if alive {
            if consecutive_failures > 0 {
                info!(
                    consecutive_failures,
                    "daemon_monitor: daemon is responding again; resetting failure counter"
                );
            }
            consecutive_failures = 0;
            continue;
        }

        consecutive_failures += 1;
        warn!(
            consecutive_failures,
            threshold = FAILURE_THRESHOLD,
            "daemon_monitor: daemon health probe failed"
        );

        if consecutive_failures < FAILURE_THRESHOLD {
            // Not yet at threshold — keep accumulating failures before acting.
            continue;
        }

        // Failure threshold reached — attempt automatic recovery.
        info!(
            consecutive_failures,
            "daemon_monitor: failure threshold reached; triggering daemon restart"
        );

        // Reset the counter unconditionally: if the restart fails we want
        // another full failure sequence before the next attempt, rather than
        // retrying in the very next loop iteration.
        consecutive_failures = 0;

        // Check the token one more time before committing to the potentially
        // long restart sequence. The exit handler handles daemon teardown
        // through its own path, so there is no benefit in racing with it.
        if token.is_cancelled() {
            info!("daemon_monitor: shutdown requested before restart; aborting");
            return;
        }

        match restart_local_daemon(expected_package_version).await {
            Ok(new_info) => {
                info!("daemon_monitor: daemon restarted successfully; refreshing connection state");
                connection_state.set(new_info);

                // The new daemon process has a fresh JWT secret; any cached
                // session token from the old daemon will be rejected. Clear it
                // so the next HTTP request mints a fresh session against the
                // restarted daemon.
                uc_daemon_client::http::clear_session_token_cache().await;

                // Best-effort: re-unlock encryption and advance lifecycle
                // services so the user's clipboard sync session is restored
                // automatically without requiring manual intervention.
                recover_after_restart(connection_state.clone()).await;
            }
            Err(error) => {
                warn!(
                    %error,
                    "daemon_monitor: restart failed; will retry after the next failure sequence"
                );
            }
        }
    }
}

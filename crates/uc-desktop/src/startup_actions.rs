//! Daemon-dependent cold-launch startup orchestration (issue #1169).
//!
//! Bundles the RPC sequence a desktop shell runs once the daemon connection
//! is confirmed at process launch: auto-unlock + lifecycle retry (delegated
//! to [`crate::daemon_recovery::recover_after_cold_launch`]), optionally
//! restoring the most recent clipboard entry, and reporting whether the
//! shell should hand off to its background-only running state. Every step
//! here is a plain RPC over the daemon HTTP API — nothing here is specific
//! to any one GUI framework, so a future non-Tauri shell can reuse it
//! as-is; only the actual background-only-mode transition (window/tray
//! teardown) stays in the shell.

use std::time::Duration;

use uc_daemon_client::{
    DaemonClipboardClient, DaemonConnectionState, DaemonSearchClient, DaemonSettingsClient,
    SearchQueryRequest,
};

use crate::daemon_recovery::recover_after_cold_launch;

/// GUI-local slack on top of the daemon's own startup budget: covers the
/// probe round-trip, the incompatible-daemon replacement wait, and
/// `DaemonConnectionState` population after the daemon turns healthy.
const READY_LOCAL_MARGIN: Duration = Duration::from_secs(15);

/// Default upper bound for [`wait_for_daemon_connection`]/
/// [`run_cold_launch_actions`]: the dominant term is the daemon's own
/// startup budget (`timing::DAEMON_STARTUP_TIMEOUT`, which already covers a
/// replacement waiting out a predecessor's instance lock and then
/// bootstrapping), plus the local margin above. Every caller runs in a
/// detached background task, so a longer bound costs nothing; expiry only
/// skips the startup action (auto-unlock / restore / background-only
/// handoff).
pub const DEFAULT_READY_TIMEOUT: Duration = Duration::from_millis(
    uc_daemon_process::timing::DAEMON_STARTUP_TIMEOUT.as_millis() as u64
        + READY_LOCAL_MARGIN.as_millis() as u64,
);

/// Default poll interval for [`wait_for_daemon_connection`]/
/// [`run_cold_launch_actions`].
pub const DEFAULT_READY_POLL: Duration = Duration::from_millis(200);

/// Poll `connection_state` until it is populated or `timeout` elapses.
/// Returns `true` when the connection info is ready.
pub async fn wait_for_daemon_connection(
    state: &DaemonConnectionState,
    timeout: Duration,
    poll_interval: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if state.get().is_some() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// What the shell should do once [`run_cold_launch_actions`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColdLaunchOutcome {
    /// Whether the shell should transition to its background-only running
    /// state now that the daemon is confirmed ready and settings have been
    /// read. The shell decides how ("background-only mode" mechanics are
    /// framework-specific); this flag only carries the user's preference.
    pub enter_background_only_mode: bool,
}

/// Run the daemon-dependent cold-launch sequence: wait for the daemon
/// connection, fetch settings once, auto-unlock + lifecycle retry, and
/// optionally restore the most recent clipboard entry. Returns `None` when
/// the daemon never became reachable or settings could not be read — the
/// caller should skip any further startup action in that case.
///
/// Wrapped in its own span so the whole sequence's `info!`/`warn!` events
/// (here and in `daemon_recovery`) correlate under one trace instead of
/// reading as unrelated log lines.
#[tracing::instrument(name = "startup.cold_launch_actions", level = "info", skip_all)]
pub async fn run_cold_launch_actions(
    connection_state: DaemonConnectionState,
    daemon_ready_timeout: Duration,
    daemon_ready_poll: Duration,
) -> Option<ColdLaunchOutcome> {
    if !wait_for_daemon_connection(&connection_state, daemon_ready_timeout, daemon_ready_poll).await
    {
        tracing::warn!(
            timeout_secs = daemon_ready_timeout.as_secs(),
            "daemon connection not ready in time; skipping cold-launch startup actions"
        );
        return None;
    }

    let settings_client = DaemonSettingsClient::new(connection_state.clone());
    let settings = match settings_client.get_settings().await {
        Ok(settings) => settings,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "failed to load settings; skipping auto-unlock/restore/background-only handoff"
            );
            return None;
        }
    };

    recover_after_cold_launch(
        connection_state.clone(),
        settings.security.auto_unlock_enabled,
    )
    .await;

    if settings.general.restore_last_entry_on_startup {
        restore_last_entry(connection_state).await;
    }

    Some(ColdLaunchOutcome {
        enter_background_only_mode: settings.general.lightweight_start,
    })
}

/// Restore the most recent clipboard history entry onto the OS clipboard.
///
/// Preserves whatever is currently on the OS clipboard as its own history
/// entry FIRST — a concurrent write during the daemon's startup window
/// (e.g. the user pasting a Wi-Fi password during login) would otherwise be
/// silently clobbered by the restore below. The target entry id is resolved
/// before that preservation step runs, so the capture can never make itself
/// the "most recent" entry and short-circuit the restore into a no-op.
///
/// Browses via `GET /search/query` with an empty query (same path the
/// history UI uses) rather than the deprecated `GET /clipboard/entries`: an
/// empty-query browse is explicitly served even while the encryption
/// session is locked (degrading to a direct main-store read), whereas the
/// deprecated list endpoint was observed to return an empty page in that
/// case instead of the real most-recent entry.
async fn restore_last_entry(connection_state: DaemonConnectionState) {
    let search_client = DaemonSearchClient::new(connection_state.clone());
    let clipboard_client = DaemonClipboardClient::new(connection_state);

    let query = SearchQueryRequest {
        query: String::new(),
        operator: None,
        time_preset: None,
        from_ms: None,
        to_ms: None,
        content_types: Vec::new(),
        tags: Vec::new(),
        extensions: Vec::new(),
        source_devices: Vec::new(),
        limit: 1,
        offset: 0,
    };
    let target_entry_id = match search_client.query(query).await {
        Ok(page) => match page.items.first() {
            Some(latest) => latest.entry_id.clone(),
            None => {
                tracing::info!("no clipboard history entry to restore");
                return;
            }
        },
        Err(error) => {
            tracing::warn!(error = %error, "failed to list clipboard entries for startup restore");
            return;
        }
    };

    if let Err(error) = clipboard_client.capture_current_clipboard().await {
        tracing::warn!(
            error = %error,
            "failed to preserve current clipboard content before startup restore"
        );
    }

    if let Err(error) = clipboard_client
        .restore_clipboard_entry(&target_entry_id)
        .await
    {
        tracing::warn!(error = %error, "failed to restore most recent clipboard entry");
    } else {
        tracing::info!("restored most recent clipboard entry to the OS clipboard");
    }
}

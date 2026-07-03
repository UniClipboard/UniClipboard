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
    DaemonClipboardClient, DaemonConnectionState, DaemonQueryClient, DaemonSearchClient,
    DaemonSettingsClient, SearchQueryRequest,
};
use uc_daemon_contract::api::dto::settings::StartupModeDto;

use crate::daemon::DaemonLaunchOrigin;
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

/// Upper bound for [`wait_for_encryption_session_ready`] before startup
/// restore gives up. A cold launch frequently has not unlocked the encryption
/// session yet: auto-unlock may be disabled, and even when it is enabled the
/// session is only resumed asynchronously by the frontend's silent keyring
/// unlock a beat after the webview loads (observed ~1.6s after the daemon
/// turns healthy). This budget also tolerates the user taking a moment to
/// unlock manually. Expiry only *skips* the restore (logged), so a generous
/// bound is cheap — the GUI is already fully running while this waits.
const ENCRYPTION_READY_TIMEOUT: Duration = Duration::from_secs(30);

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

/// Poll the daemon's encryption state until the session is unlocked
/// (`session_ready`) or `timeout` elapses. Returns `true` once the session is
/// ready to encrypt/decrypt.
///
/// Startup restore reads encrypted history (decrypt) and, before that,
/// captures the current OS clipboard into history (encrypt); both fail with a
/// 500 while the session is still locked. A cold launch is often locked at
/// this point — auto-unlock may be off, and even then the frontend resumes the
/// session from the keyring only a beat later — so restore must wait for the
/// session rather than firing once and failing.
///
/// Returns `false` (skip restore) on two terminal conditions: `timeout`
/// elapses, or the store is not `initialized` (no encrypted history exists /
/// can ever exist this session, so the session will never become ready). A
/// transient state-query error is not terminal — it is logged and retried
/// until the deadline.
pub async fn wait_for_encryption_session_ready(
    connection_state: DaemonConnectionState,
    timeout: Duration,
    poll_interval: Duration,
) -> bool {
    let client = DaemonQueryClient::new(connection_state);
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match client.get_encryption_state().await {
            Ok(state) if state.session_ready => return true,
            Ok(state) if !state.initialized => {
                tracing::info!("encryption store not initialized; skipping startup restore wait");
                return false;
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "failed to query encryption state while waiting to restore; will retry"
                );
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(poll_interval).await;
    }
}

/// What the shell should do to its main window once
/// [`run_cold_launch_actions`] returns. The shell decides *how* (window/tray
/// mechanics are framework-specific); this only carries the decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupWindowAction {
    /// Nothing to do — the boot-time window state already matches. Covers
    /// Normal mode (window already shown) and Silent mode (stays hidden).
    None,
    /// Tear down to background-only running (Lightweight Mode, issue #1169):
    /// a genuine cold start where this launch spawned the daemon. The shell
    /// notifies the user and exits the GUI process, leaving the daemon up.
    EnterBackgroundOnly,
    /// Show the main window: a Lightweight-Mode *reopen*, where a previous
    /// background-only session left the daemon running and the user relaunched
    /// the app to get the window back. Boot hid the window (Lightweight always
    /// hides at boot until the origin is known), so it must be shown now.
    ShowWindow,
}

/// Resolve the boot-time window action from whether this launch spawned the
/// daemon and whether Lightweight Mode is selected.
///
/// Only Lightweight Mode has a distinct reopen behavior, because it is the
/// only mode that exits the GUI process (Normal/Silent keep it alive with a
/// tray, so a "reopen" reuses the live process via the tray/Dock rather than a
/// fresh launch). For Normal/Silent the boot-time visibility already matches,
/// so there is nothing to do here.
fn resolve_window_action(spawned_this_launch: bool, is_lightweight: bool) -> StartupWindowAction {
    match (is_lightweight, spawned_this_launch) {
        (true, true) => StartupWindowAction::EnterBackgroundOnly,
        (true, false) => StartupWindowAction::ShowWindow,
        (false, _) => StartupWindowAction::None,
    }
}

/// What the shell should do once [`run_cold_launch_actions`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColdLaunchOutcome {
    /// How the shell should reconcile its main window now that the daemon is
    /// confirmed ready, settings have been read, and the launch origin (cold
    /// start vs reopen) is known.
    pub window_action: StartupWindowAction,
}

/// Run the daemon-dependent cold-launch sequence: wait for the daemon
/// connection, fetch settings once, and — only on a genuine cold start
/// (`launch_origin` reports this launch spawned the daemon) — auto-unlock +
/// lifecycle retry and optionally restore the most recent clipboard entry.
/// Returns `None` when the daemon never became reachable or settings could
/// not be read — the caller should skip any further startup action then.
///
/// The `launch_origin` split matters two ways (issue #1169):
/// - A **reopen** (a compatible daemon was already running — typically a
///   previous Lightweight-Mode session that left it up) must NOT re-run the
///   restore: it already ran when the daemon first started, and re-running it
///   would clobber the OS clipboard every time the user reopens the window.
/// - Under Lightweight Mode, a cold start hands off to background-only running
///   ([`StartupWindowAction::EnterBackgroundOnly`]) while a reopen shows the
///   window ([`StartupWindowAction::ShowWindow`]) — the whole point of the
///   reopen.
///
/// Wrapped in its own span so the whole sequence's `info!`/`warn!` events
/// (here and in `daemon_recovery`) correlate under one trace instead of
/// reading as unrelated log lines.
#[tracing::instrument(name = "startup.cold_launch_actions", level = "info", skip_all)]
pub async fn run_cold_launch_actions(
    connection_state: DaemonConnectionState,
    launch_origin: DaemonLaunchOrigin,
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

    let is_lightweight = matches!(settings.general.startup_mode, StartupModeDto::Lightweight);
    // The connection is ready, so the probe has settled the origin.
    let spawned_this_launch = launch_origin.spawned_this_launch();

    if !spawned_this_launch {
        // Reopen: a compatible daemon was already running (e.g. a previous
        // Lightweight-Mode session), so this launch only connected. Skip the
        // cold-start recovery and restore entirely — they belong to the
        // daemon's original start, and re-running the restore would overwrite
        // the OS clipboard on every window reopen (issue #1169).
        tracing::info!("daemon already running (reopen); skipping cold-start recovery and restore");
        return Some(ColdLaunchOutcome {
            window_action: resolve_window_action(false, is_lightweight),
        });
    }

    recover_after_cold_launch(
        connection_state.clone(),
        settings.security.auto_unlock_enabled,
    )
    .await;

    if settings.general.restore_last_entry_on_startup {
        // Both the pre-restore capture-current (encrypt) and the restore
        // itself (decrypt) touch encrypted history, so they need the
        // encryption session unlocked. `recover_after_cold_launch` above does
        // NOT guarantee that: it skips unlocking entirely when auto-unlock is
        // off, and even with it on the session may be resumed by the frontend
        // a beat later. Wait for the session to become ready before restoring;
        // on timeout skip (not fail) so we never fire the doomed 500-ing
        // capture+restore into a locked session (issue #1169).
        if wait_for_encryption_session_ready(
            connection_state.clone(),
            ENCRYPTION_READY_TIMEOUT,
            daemon_ready_poll,
        )
        .await
        {
            restore_last_entry(connection_state).await;
        } else {
            tracing::warn!(
                timeout_secs = ENCRYPTION_READY_TIMEOUT.as_secs(),
                "encryption session not ready in time; skipping startup restore of the most recent clipboard entry"
            );
        }
    }

    Some(ColdLaunchOutcome {
        window_action: resolve_window_action(true, is_lightweight),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lightweight_cold_start_enters_background() {
        // Scenario 2 (auto-start) and scenario 1's first manual launch collapse
        // to the same path: this launch spawned the daemon under Lightweight
        // Mode, so hand off to background-only running.
        assert_eq!(
            resolve_window_action(true, true),
            StartupWindowAction::EnterBackgroundOnly
        );
    }

    #[test]
    fn lightweight_reopen_shows_window() {
        // A later click on the app icon: the daemon a previous Lightweight
        // session left running is found already up, so show the window.
        assert_eq!(
            resolve_window_action(false, true),
            StartupWindowAction::ShowWindow
        );
    }

    #[test]
    fn non_lightweight_never_acts_regardless_of_origin() {
        // Normal/Silent keep the GUI process alive with a tray, so their
        // boot-time visibility already matches — nothing to reconcile here,
        // whether this launch spawned the daemon or found one running.
        assert_eq!(
            resolve_window_action(true, false),
            StartupWindowAction::None
        );
        assert_eq!(
            resolve_window_action(false, false),
            StartupWindowAction::None
        );
    }
}

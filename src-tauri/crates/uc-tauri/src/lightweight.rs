//! ADR-008 D3 (P4-3): lightweight mode + the three-state quit intent.
//!
//! Three exit behaviors, all distinguished here:
//!
//! - **关窗** (window close) → hide to tray; handled in `run.rs`, never reaches here.
//! - **轻量模式** (tray "Lightweight") → GUI process fully exits, the external
//!   `uniclipd` keeps running. A one-time system notification tells the user it
//!   is still alive and how to reopen it ([`enter_lightweight_mode`]).
//! - **彻底退出** (tray "Quit" **or Cmd-Q**) → GUI exits AND stops the connected
//!   daemon regardless of who spawned it. Two triggers feed the same decision in
//!   `run.rs`'s `ExitRequested` handler via [`exit_should_stop_daemon`]: the tray
//!   action flips [`QuitIntent`]; an OS-level Cmd-Q / app-Quit arrives as
//!   `ExitRequested { code: None }`. Both mean "quit the whole app". Identity +
//!   legacy-in-process safety carve-outs live in `stop_local_daemon_on_full_quit`.
//!
//! Window-close (hide to tray), lightweight mode, and restart leave the daemon
//! running: window-close never reaches `ExitRequested`; lightweight and restart
//! exit programmatically with `code: Some(_)` (`app.exit(0)` /
//! `app.restart()`→`RESTART_EXIT_CODE`) and never flip the intent. Only a
//! deliberate quit — tray "Quit" or Cmd-Q — stops the daemon.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use tracing::{info, warn};

use crate::bootstrap::TauriAppRuntime;

/// Whether the explicit tray "Quit (彻底退出)" action was chosen.
///
/// Default `false`; only the tray "Quit" flips it. This is one of the two inputs
/// to [`exit_should_stop_daemon`] — the other is the OS-level Cmd-Q, which Tauri
/// signals as `ExitRequested { code: None }`. Window-close, lightweight mode, and
/// restart neither flip this nor arrive with a `None` code, so they keep the
/// daemon running.
#[derive(Default)]
pub struct QuitIntent(AtomicBool);

impl QuitIntent {
    fn request_full_quit(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether the tray "彻底退出" action was chosen. Read by `run.rs`'s
    /// `ExitRequested` handler and fed into [`exit_should_stop_daemon`].
    pub fn should_stop_daemon(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Decide whether a pending app exit should also stop the connected daemon.
///
/// `true` when the user deliberately quit the whole app, via either trigger:
/// - the tray "彻底退出" flipped [`QuitIntent`] (`quit_intent == true`), or
/// - an OS/user-initiated quit — macOS Cmd-Q or the app Quit menu — which Tauri
///   reports as `ExitRequested { code: None }`.
///
/// A `Some(_)` exit code is reserved for programmatic exits that must NOT stop
/// the daemon: `app.exit(0)` (lightweight mode) and `app.restart()` (carries
/// `RESTART_EXIT_CODE`). Neither flips the intent, so both keep the daemon alive.
pub fn exit_should_stop_daemon(quit_intent: bool, exit_code: Option<i32>) -> bool {
    quit_intent || exit_code.is_none()
}

/// Mark the pending exit as a full quit (stop the connected daemon), then exit.
/// The actual stop happens in `run.rs`'s `ExitRequested` handler. Triggered by
/// the tray "彻底退出" and by terminal/OS terminate signals (Ctrl-C / SIGTERM),
/// which the GUI routes here so the detached daemon is not orphaned.
pub fn request_full_quit(app: &AppHandle) {
    app.state::<QuitIntent>().request_full_quit();
    info!("full quit requested — connected daemon will be stopped");
    app.exit(0);
}

/// Tray "轻量模式": show the one-time discoverability notification, then exit
/// the GUI process. The daemon keeps running (default [`QuitIntent`]).
pub fn enter_lightweight_mode(app: &AppHandle) {
    let app_data_root = app
        .state::<Arc<TauriAppRuntime>>()
        .storage_paths()
        .app_data_root_dir
        .clone();
    notify_lightweight_once(app, &app_data_root);
    info!("entering lightweight mode — GUI exiting, daemon stays running");
    app.exit(0);
}

const LIGHTWEIGHT_FLAG_FILE: &str = "lightweight-notified.json";

/// Send the one-time "still running in the background" notification
/// (OQ-lightweight-discoverability). Bilingual (中 + EN). No-op once the
/// per-profile flag file exists; deleting that file re-arms the notification
/// (self-healing — it lives in `app_data_root`, NOT settings.json).
pub fn notify_lightweight_once(app: &AppHandle, app_data_root: &Path) {
    let flag = app_data_root.join(LIGHTWEIGHT_FLAG_FILE);
    if flag.exists() {
        return;
    }

    let result = app
        .notification()
        .builder()
        .title("UniClipboard")
        .body(
            "UniClipboard 仍在后台运行，点应用图标可重新打开窗口。\n\
             Still running in the background — open it from the app icon to show the window again.",
        )
        .show();

    match result {
        Ok(()) => {
            mark_notified(app_data_root);
            info!("lightweight-mode discoverability notification shown");
        }
        Err(error) => {
            // Don't write the flag — retry next time so the user isn't left
            // with zero on-screen trace of a running background process.
            warn!(%error, "failed to show lightweight-mode notification");
        }
    }
}

/// Persist the "notification shown" flag atomically (temp + rename) so a torn
/// write never corrupts it — at worst the notification shows once more.
fn mark_notified(app_data_root: &Path) {
    let flag = app_data_root.join(LIGHTWEIGHT_FLAG_FILE);
    let tmp = app_data_root.join(format!("{LIGHTWEIGHT_FLAG_FILE}.tmp"));
    let write =
        std::fs::write(&tmp, b"{\"notified\":true}\n").and_then(|()| std::fs::rename(&tmp, &flag));
    if let Err(error) = write {
        warn!(%error, "failed to persist lightweight-notified flag");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_intent_defaults_to_leaving_daemon() {
        let intent = QuitIntent::default();
        assert!(
            !intent.should_stop_daemon(),
            "default intent must NOT stop the daemon — only explicit 彻底退出 flips it"
        );
        intent.request_full_quit();
        assert!(intent.should_stop_daemon());
    }

    #[test]
    fn cmd_q_stops_daemon_even_without_tray_intent() {
        // macOS Cmd-Q / app-Quit arrives as ExitRequested { code: None }. It is a
        // deliberate "quit the app", so it stops the daemon just like tray Quit —
        // even though no tray action flipped QuitIntent.
        assert!(exit_should_stop_daemon(false, None));
    }

    #[test]
    fn tray_quit_stops_daemon_via_intent() {
        // Tray "彻底退出" → app.exit(0) → code Some(0); the intent flag carries it.
        assert!(exit_should_stop_daemon(true, Some(0)));
    }

    #[test]
    fn lightweight_and_restart_keep_daemon() {
        // Lightweight: app.exit(0) without intent → Some(0) → keep.
        assert!(!exit_should_stop_daemon(false, Some(0)));
        // Restart: app.restart() → Some(RESTART_EXIT_CODE = i32::MAX) → keep.
        assert!(!exit_should_stop_daemon(false, Some(i32::MAX)));
    }

    #[test]
    fn mark_notified_writes_flag_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let flag = dir.path().join(LIGHTWEIGHT_FLAG_FILE);
        assert!(!flag.exists());

        mark_notified(dir.path());

        assert!(flag.exists(), "flag file must exist after mark_notified");
        assert!(
            !dir.path()
                .join(format!("{LIGHTWEIGHT_FLAG_FILE}.tmp"))
                .exists(),
            "atomic rename must not leave the temp file behind"
        );
    }
}

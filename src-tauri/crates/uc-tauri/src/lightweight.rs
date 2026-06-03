//! ADR-008 D3 (P4-3): lightweight mode + the three-state quit intent.
//!
//! Three exit behaviors, all distinguished here:
//!
//! - **关窗** (window close) → hide to tray; handled in `run.rs`, never reaches here.
//! - **轻量模式** (tray "Lightweight") → GUI process fully exits, the external
//!   `uniclipd` keeps running. A one-time system notification tells the user it
//!   is still alive and how to reopen it ([`enter_lightweight_mode`]).
//! - **彻底退出** (tray "Quit") → GUI exits AND stops the daemon *if a GUI
//!   spawned it* ([`request_full_quit`] sets [`QuitIntent`]; `run.rs`'s
//!   `ExitRequested` reads it and calls `stop_gui_spawned_daemon`).
//!
//! Default intent is "leave the daemon running", so Cmd-Q / restart / any other
//! exit never kills the daemon — only the explicit tray "Quit" does.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use tracing::{info, warn};

use crate::bootstrap::TauriAppRuntime;

/// Whether the pending app exit should also stop the GUI-spawned daemon.
///
/// Default `false`: window-close, lightweight mode, Cmd-Q, and restart all leave
/// the daemon running. Only the tray "Quit (彻底退出)" action flips it, so the
/// GUI never stops a daemon unless the user explicitly asked for a full quit.
#[derive(Default)]
pub struct QuitIntent(AtomicBool);

impl QuitIntent {
    fn request_full_quit(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Read by `run.rs`'s `ExitRequested` handler.
    pub fn should_stop_daemon(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Tray "彻底退出": mark the exit as a full quit (stop the GUI-spawned daemon),
/// then exit. The actual stop happens in the `ExitRequested` handler.
pub fn request_full_quit(app: &AppHandle) {
    app.state::<QuitIntent>().request_full_quit();
    info!("full quit requested from tray — daemon will be stopped if GUI-spawned");
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
            "default quit must NOT stop the daemon — only explicit 彻底退出 does"
        );
        intent.request_full_quit();
        assert!(intent.should_stop_daemon());
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

//! ADR-008 D3 (P4-3): lightweight mode + the quit / daemon-teardown decision.
//!
//! The external `uniclipd` is always a separate process. Four exit behaviors,
//! distinguished here and in `run.rs`'s `RunEvent` handlers:
//!
//! - **关窗** (window close) → hide to tray; intercepted in `run.rs`, never
//!   reaches the exit handlers, daemon untouched.
//! - **轻量模式** (tray "Lightweight") → GUI process exits, the daemon keeps
//!   running. [`enter_lightweight_mode`] shows a reassurance notification (so the
//!   user knows it is still alive) then `app.exit(0)`.
//! - **重启** (restart) → GUI process exits and respawns; daemon keeps running.
//! - **彻底退出** → GUI exits AND stops the connected daemon regardless of who
//!   spawned it. Triggers: tray "彻底退出", **macOS Cmd-Q / app-Quit menu**,
//!   terminal Ctrl-C, and SIGTERM. Identity + legacy-in-process safety carve-outs
//!   live in `stop_local_daemon_on_full_quit`.
//!
//! ## Why the decision lives in the `Exit` handler, not `ExitRequested`
//!
//! `RunEvent::ExitRequested` does NOT fire for every quit. In tao 0.35 / Tauri
//! 2.11 on macOS, Cmd-Q and the app "Quit" menu go through
//! `applicationWillTerminate` → `AppState::exit()`, which emits ONLY
//! `RunEvent::Exit` — no `ExitRequested`. (`ExitRequested { code: None }` is
//! reserved for the *last window being destroyed*, which never happens here
//! because window-close is intercepted to hide-to-tray.) So a teardown decided
//! solely in `ExitRequested` silently skips Cmd-Q and orphans the daemon — the
//! bug this module now fixes.
//!
//! Instead, `RunEvent::Exit` — which fires for every clean termination — stops
//! the daemon by DEFAULT. The only exits that keep it (lightweight / restart) are
//! the programmatic `app.exit(0)` / `app.restart()` paths; those arrive first as
//! `ExitRequested { code: Some(_) }` *without* a full-quit request and flag
//! [`QuitIntent::note_exit_requested`] to keep the daemon. A GUI crash or SIGKILL
//! never reaches `Exit` cleanly, so the daemon still survives those.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use tracing::{info, warn};

/// Process-wide exit state, read by `run.rs`'s `RunEvent::ExitRequested` /
/// `RunEvent::Exit` handlers to decide whether to also stop the connected daemon.
///
/// Two flags, both default `false`:
/// - `full_quit`: flipped by [`request_full_quit`] (tray "彻底退出", Ctrl-C,
///   SIGTERM).
/// - `keep_daemon`: flipped by [`Self::note_exit_requested`] for programmatic
///   keep-alive exits (lightweight `app.exit(0)` / restart). When set, the `Exit`
///   handler leaves the daemon running.
///
/// The default (`keep_daemon == false`) means "stop the daemon", so the Cmd-Q
/// path — which reaches `RunEvent::Exit` without any prior `ExitRequested` —
/// correctly tears the daemon down.
#[derive(Default)]
pub struct QuitIntent {
    full_quit: AtomicBool,
    keep_daemon: AtomicBool,
}

impl QuitIntent {
    fn request_full_quit(&self) {
        self.full_quit.store(true, Ordering::SeqCst);
    }

    fn full_quit_requested(&self) -> bool {
        self.full_quit.load(Ordering::SeqCst)
    }

    /// Record an `ExitRequested { code }` event. Programmatic keep-alive exits
    /// (lightweight / restart) arrive as `Some(_)` without a full-quit request;
    /// flag them so the later `Exit` handler keeps the daemon. Tray "彻底退出"
    /// (full quit) and the last-window-destroyed `None` case leave the flag unset
    /// → daemon stopped. See [`exit_keeps_daemon`].
    pub fn note_exit_requested(&self, exit_code: Option<i32>) {
        if exit_keeps_daemon(self.full_quit_requested(), exit_code) {
            self.keep_daemon.store(true, Ordering::SeqCst);
        }
    }

    /// Read by `run.rs`'s `RunEvent::Exit` handler. The daemon is stopped on every
    /// clean quit EXCEPT the programmatic keep-alive exits recorded by
    /// [`Self::note_exit_requested`]. macOS Cmd-Q / app-Quit reach `Exit` WITHOUT
    /// a prior `ExitRequested`, so the flag stays unset and the daemon is stopped.
    pub fn should_stop_daemon_on_exit(&self) -> bool {
        !self.keep_daemon.load(Ordering::SeqCst)
    }
}

/// Whether a programmatic `ExitRequested { code }` should KEEP the external daemon
/// running.
///
/// `app.exit(0)` (lightweight) and `app.restart()` (→ `RESTART_EXIT_CODE`) arrive
/// as `Some(_)`; they keep the daemon UNLESS the user asked for a full quit
/// (`request_full_quit` → tray "彻底退出" / Ctrl-C / SIGTERM). A `None` code means
/// the last window was destroyed — a real quit — so it does NOT keep the daemon.
///
/// NOTE: macOS Cmd-Q does NOT produce an `ExitRequested` at all (tao's
/// `applicationWillTerminate` emits only `RunEvent::Exit`), so it never reaches
/// this helper — it falls through to the default-stop behavior in the `Exit`
/// handler ([`QuitIntent::should_stop_daemon_on_exit`]).
pub fn exit_keeps_daemon(full_quit_requested: bool, exit_code: Option<i32>) -> bool {
    exit_code.is_some() && !full_quit_requested
}

/// Mark the pending exit as a full quit (stop the connected daemon), then exit.
/// The actual stop happens in `run.rs`'s `RunEvent::Exit` handler. Triggered by
/// the tray "彻底退出" and by terminal/OS terminate signals (Ctrl-C / SIGTERM),
/// which the GUI routes here so the detached daemon is not orphaned.
pub fn request_full_quit(app: &AppHandle) {
    app.state::<QuitIntent>().request_full_quit();
    info!("full quit requested — connected daemon will be stopped");
    app.exit(0);
}

/// Tray "轻量模式": show the reassurance notification, then exit the GUI
/// process. The daemon keeps running (default [`QuitIntent`]).
pub fn enter_lightweight_mode(app: &AppHandle) {
    notify_lightweight(app);
    info!("entering lightweight mode — GUI exiting, daemon stays running");
    app.exit(0);
}

/// Send the "still running in the background" notification EVERY time the user
/// enters lightweight mode. Bilingual (中 + EN).
///
/// Showing it on every entry — not just once — is the fix for issue #1129: the
/// GUI process exiting looked like a crash to users whose one-time toast had
/// already been consumed on an earlier run, leaving zero on-screen trace of the
/// still-running background daemon. The user explicitly invoked this menu item,
/// so a notification per entry is expected feedback, not noise. A failed
/// `.show()` degrades to a `warn!` and never blocks the exit.
pub fn notify_lightweight(app: &AppHandle) {
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
        Ok(()) => info!("lightweight-mode discoverability notification shown"),
        Err(error) => warn!(%error, "failed to show lightweight-mode notification"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_q_stops_daemon_no_exit_requested_fired() {
        // macOS Cmd-Q reaches RunEvent::Exit WITHOUT any prior ExitRequested, so
        // `note_exit_requested` is never called and the default (stop) holds.
        let state = QuitIntent::default();
        assert!(
            state.should_stop_daemon_on_exit(),
            "Cmd-Q (no ExitRequested) must stop the daemon by default"
        );
    }

    #[test]
    fn tray_quit_stops_daemon_despite_some_code() {
        // Tray "彻底退出" → request_full_quit() flips full_quit, then app.exit(0)
        // → ExitRequested { code: Some(0) }. The full-quit request keeps the
        // keep-daemon flag unset → Exit stops the daemon.
        let state = QuitIntent::default();
        state.request_full_quit();
        state.note_exit_requested(Some(0));
        assert!(state.should_stop_daemon_on_exit());
    }

    #[test]
    fn lightweight_keeps_daemon() {
        // Lightweight: app.exit(0) WITHOUT a full-quit request → keep.
        let state = QuitIntent::default();
        state.note_exit_requested(Some(0));
        assert!(!state.should_stop_daemon_on_exit());
    }

    #[test]
    fn restart_keeps_daemon() {
        // Restart via the event loop: app.restart() → ExitRequested
        // { code: Some(RESTART_EXIT_CODE = i32::MAX) } without a full-quit request.
        let state = QuitIntent::default();
        state.note_exit_requested(Some(i32::MAX));
        assert!(!state.should_stop_daemon_on_exit());
    }

    #[test]
    fn last_window_destroyed_stops_daemon() {
        // The last window being destroyed surfaces as ExitRequested { code: None }
        // — a real quit, so the daemon is stopped.
        let state = QuitIntent::default();
        state.note_exit_requested(None);
        assert!(state.should_stop_daemon_on_exit());
    }

    #[test]
    fn exit_keeps_daemon_truth_table() {
        // Programmatic Some(_) without full-quit → keep.
        assert!(exit_keeps_daemon(false, Some(0)));
        assert!(exit_keeps_daemon(false, Some(i32::MAX)));
        // Full-quit request overrides → stop.
        assert!(!exit_keeps_daemon(true, Some(0)));
        // None code (last window destroyed) → stop.
        assert!(!exit_keeps_daemon(false, None));
    }
}

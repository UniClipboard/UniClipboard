//! Selection of the quick panel implementation.
//!
//! Either the WebView panel in this module tree owns the quick panel, or a separate native GPUI
//! helper process does. Exactly one of them does, and the choice is made once at startup, here,
//! so no code path has to ask "is the other one running too?".
//!
//! The helper takes over the global shortcut, the modifier double-tap trigger and the window, so
//! while it is selected the GUI registers none of them. The GUI only starts, restarts and stops it.
//!
//! The native panel is the default on macOS; `UC_GPUI_QUICK_PANEL=0` turns it off. On Windows it
//! stays opt-in (`UC_GPUI_QUICK_PANEL=1`) until that port exists.
//!
//! The content lock lives inside the GUI process and the daemon does not enforce it, so the GUI
//! gates the helper: it runs only while the setting is on **and** content is unlocked. Locking
//! stops the process, which closes any window it had open. This is a GUI-side safeguard, not a
//! security boundary against other local processes that talk to the daemon.

#[cfg(any(target_os = "macos", target_os = "windows"))]
use tracing::{error, info};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use uc_desktop::quick_panel_helper::{resolve_helper_exe_path, ProcessLauncher, SupervisedHelper};

/// Environment variable that selects the native quick panel (`1`) or the WebView one (`0`).
pub const NATIVE_QUICK_PANEL_ENV: &str = "UC_GPUI_QUICK_PANEL";

/// Whether the native panel is wanted, given the variable and the platform default.
fn native_wanted(value: Option<&str>, default_on: bool) -> bool {
    match value {
        Some("1") => true,
        Some("0") => false,
        _ => default_on,
    }
}

/// The helper runs only while the setting is on and content is unlocked.
#[derive(Default)]
struct RunGate {
    enabled: bool,
    unlocked: bool,
}

impl RunGate {
    fn should_run(&self) -> bool {
        self.enabled && self.unlocked
    }

    /// Records the setting and returns whether the helper should run afterwards.
    fn set_enabled(&mut self, enabled: bool) -> bool {
        self.enabled = enabled;
        self.should_run()
    }

    /// Records the lock state. Returns the new run state only when it changed, so a steady
    /// answer from the observer never restarts a running helper.
    fn set_unlocked(&mut self, unlocked: bool) -> Option<bool> {
        let before = self.should_run();
        self.unlocked = unlocked;
        let after = self.should_run();
        (before != after).then_some(after)
    }
}

/// Carries out what the helper asks of the GUI. Window work goes to the main thread.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn request_handler(app: tauri::AppHandle) -> uc_desktop::quick_panel_helper::RequestHandler {
    use uc_desktop::quick_panel_helper::HelperRequest;
    std::sync::Arc::new(move |request| {
        let handle = app.clone();
        let result = app.run_on_main_thread(move || match request {
            HelperRequest::ShowMainWindow => crate::main_window::show_main_window(&handle),
            HelperRequest::OpenSettings => crate::main_window::show_settings_window(&handle),
        });
        if let Err(error) = result {
            error!(error = %error, "Could not carry out a request of the quick panel helper");
        }
    })
}

/// Managed state holding the selected implementation.
pub struct QuickPanelBackend {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    helper: Option<SupervisedHelper<ProcessLauncher>>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    gate: std::sync::Mutex<RunGate>,
}

impl QuickPanelBackend {
    /// Chooses the implementation from the environment. Falls back to the WebView panel when the
    /// helper executable is missing, so the user is never left without a quick panel.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn select(app: &tauri::AppHandle) -> Self {
        let value = std::env::var(NATIVE_QUICK_PANEL_ENV).ok();
        if !native_wanted(value.as_deref(), cfg!(target_os = "macos")) {
            return Self::without_helper();
        }
        match resolve_helper_exe_path() {
            Some(executable) => {
                info!(path = %executable.display(), "Using the native quick panel helper");
                Self {
                    helper: Some(SupervisedHelper::new(
                        ProcessLauncher::for_helper(executable)
                            .with_request_handler(request_handler(app.clone())),
                    )),
                    gate: std::sync::Mutex::new(RunGate::default()),
                }
            }
            None => {
                error!(
                    "The native quick panel is selected but the helper executable was not found \
                     next to the application; using the WebView quick panel"
                );
                Self::without_helper()
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn without_helper() -> Self {
        Self {
            helper: None,
            gate: std::sync::Mutex::new(RunGate::default()),
        }
    }

    /// Other platforms only have the WebView panel.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub fn select(_: &tauri::AppHandle) -> Self {
        Self {}
    }

    /// Whether the helper implements the modifier double-tap trigger on this platform.
    pub fn supports_double_tap() -> bool {
        cfg!(target_os = "macos")
    }

    /// True when the native helper owns the quick panel.
    pub fn is_native(&self) -> bool {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.helper.is_some()
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            false
        }
    }

    /// Records the persisted `quick_panel.enabled` setting. The helper runs while it is on and
    /// content is unlocked. Does nothing when the WebView panel is in use.
    pub fn set_enabled(&self, enabled: bool) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(helper) = &self.helper {
            let run = self
                .gate
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .set_enabled(enabled);
            helper.set_enabled(run);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = enabled;
    }

    /// Tells the backend whether content is unlocked. Locking stops the helper, which closes its
    /// window whether it was shown or hidden; unlocking starts it again if the setting is on.
    pub fn set_content_unlocked(&self, unlocked: bool) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(helper) = &self.helper {
            let change = {
                let mut gate = self.gate.lock().unwrap_or_else(|e| e.into_inner());
                gate.set_unlocked(unlocked)
            };
            if let Some(run) = change {
                info!(
                    unlocked,
                    run, "Content lock changed; adjusting the quick panel helper"
                );
                helper.set_enabled(run);
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = unlocked;
    }

    /// Restarts a running helper so it re-reads settings it only applies at startup, such as the
    /// global shortcut. Does nothing when the WebView panel is in use.
    pub fn restart(&self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(helper) = &self.helper {
            helper.restart();
        }
    }

    /// Stops the helper for good. Called when the GUI exits.
    pub fn shutdown(&self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(helper) = &self.helper {
            helper.shutdown();
        }
    }
}

/// Follows the content lock for as long as the GUI runs. Locking is noticed within one interval;
/// an answer that cannot be obtained counts as locked, so the helper never outlives a doubt.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn watch_content_lock(app: tauri::AppHandle) {
    use std::time::Duration;
    use tauri::Manager;
    use uc_daemon_client::{DaemonConnectionState, DaemonQueryClient};

    if !app.state::<QuickPanelBackend>().is_native() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        loop {
            let unlocked = crate::commands::content_lock::resolve_content_unlocked(
                &app.state::<crate::commands::content_lock::ContentLockState>(),
                &app.state::<DaemonConnectionState>(),
                &app.state::<DaemonQueryClient>(),
            )
            .await
            .unwrap_or(false);
            app.state::<QuickPanelBackend>()
                .set_content_unlocked(unlocked);
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_defaults_to_native_and_the_variable_overrides() {
        assert!(native_wanted(None, true));
        assert!(native_wanted(Some("1"), true));
        assert!(!native_wanted(Some("0"), true));
        assert!(native_wanted(Some("other"), true));
        assert!(!native_wanted(None, false));
        assert!(native_wanted(Some("1"), false));
    }

    #[test]
    fn the_helper_needs_the_setting_and_an_unlocked_gui() {
        let mut gate = RunGate::default();
        // Started locked: enabling the setting does not run it.
        assert!(!gate.set_enabled(true));
        assert_eq!(gate.set_unlocked(true), Some(true));
        // A steady answer changes nothing.
        assert_eq!(gate.set_unlocked(true), None);
        // Locking stops it, unlocking starts it again.
        assert_eq!(gate.set_unlocked(false), Some(false));
        assert_eq!(gate.set_unlocked(false), None);
        assert_eq!(gate.set_unlocked(true), Some(true));
    }

    #[test]
    fn turning_the_setting_off_stops_it_even_when_unlocked() {
        let mut gate = RunGate::default();
        gate.set_unlocked(true);
        assert!(gate.set_enabled(true));
        assert!(!gate.set_enabled(false));
        // Unlock changes while the setting is off never start it.
        assert_eq!(gate.set_unlocked(false), None);
        assert_eq!(gate.set_unlocked(true), None);
    }
}

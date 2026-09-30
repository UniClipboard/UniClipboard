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
//! The content lock is not decided here. The daemon refuses history-derived content to GUI-class
//! clients (this GUI and the helper) until the user has unlocked, so the helper can run all the
//! time: while locked it shows the locked page, and it drops what it holds when the daemon says
//! content got locked.

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
        Self { helper: None }
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

    /// Starts or stops the helper to match the persisted `quick_panel.enabled` setting.
    /// Does nothing when the WebView panel is in use.
    pub fn set_enabled(&self, enabled: bool) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(helper) = &self.helper {
            helper.set_enabled(enabled);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = enabled;
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
}

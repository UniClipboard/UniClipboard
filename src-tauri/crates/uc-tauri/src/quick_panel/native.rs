//! Selection of the quick panel implementation.
//!
//! Either the WebView panel in this module tree owns the quick panel, or a separate native GPUI
//! helper process does. Exactly one of them does, and the choice is made once at startup, here,
//! so no code path has to ask "is the other one running too?".
//!
//! The helper takes over the global shortcut, the modifier double-tap trigger and the window, so
//! while it is selected the GUI registers none of them. The GUI only starts, restarts and stops it.
//!
//! The native panel is opt-in for now: the content-lock authority still lives inside the GUI
//! process, which the helper cannot see, so it must not be the default yet.

#[cfg(any(target_os = "macos", target_os = "windows"))]
use tracing::{error, info};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use uc_desktop::quick_panel_helper::{resolve_helper_exe_path, ProcessLauncher, SupervisedHelper};

/// Environment variable that selects the native quick panel (`1`) on macOS and Windows.
pub const NATIVE_QUICK_PANEL_ENV: &str = "UC_GPUI_QUICK_PANEL";

/// Managed state holding the selected implementation.
pub struct QuickPanelBackend {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    helper: Option<SupervisedHelper<ProcessLauncher>>,
}

impl QuickPanelBackend {
    /// Chooses the implementation from the environment. Falls back to the WebView panel when the
    /// helper executable is missing, so the user is never left without a quick panel.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    pub fn select() -> Self {
        if std::env::var(NATIVE_QUICK_PANEL_ENV).as_deref() != Ok("1") {
            return Self { helper: None };
        }
        match resolve_helper_exe_path() {
            Some(executable) => {
                info!(path = %executable.display(), "Using the native quick panel helper");
                Self {
                    helper: Some(SupervisedHelper::new(ProcessLauncher::for_helper(
                        executable,
                    ))),
                }
            }
            None => {
                error!(
                    "{NATIVE_QUICK_PANEL_ENV} is set but the helper executable was not found \
                     next to the application; using the WebView quick panel"
                );
                Self { helper: None }
            }
        }
    }

    /// Other platforms only have the WebView panel.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub fn select() -> Self {
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

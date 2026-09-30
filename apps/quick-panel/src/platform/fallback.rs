//! Platforms without a native implementation yet.
//!
//! Every operation that needs the operating system reports [`PlatformError::Unsupported`], and
//! [`CAPABILITIES`] says so, so the interface hides what does not work instead of failing on it.

use std::rc::Rc;

use quick_panel_core::geometry::window_pair::PreviewPlacement;
use quick_panel_core::ports::{Capabilities, PasteTarget, PlatformError};
use uc_desktop::modifier_double_tap_monitor::ModifierKeyState;

pub const CAPABILITIES: Capabilities = Capabilities {
    auto_paste: false,
    cursor_anchor: false,
    shaped_preview: false,
    open_and_reveal: false,
};

struct NoPasteTarget;

impl PasteTarget for NoPasteTarget {
    fn name(&self) -> Option<String> {
        None
    }

    fn check(&self) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }

    fn return_focus(&self) {}

    fn paste(&self) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }

    fn type_text(&self, _: &str) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
}

pub fn capture_paste_target() -> Rc<dyn PasteTarget> {
    Rc::new(NoPasteTarget)
}

pub fn panel_anchor(_: bool, _: f64, _: f64) -> Result<(f64, f64), PlatformError> {
    Err(PlatformError::Unsupported)
}

pub fn run_as_background_app() {}

pub fn open_target(_: &str) -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported)
}

pub fn reveal_path(_: &str) -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported)
}

pub fn set_visible(window: &gpui::Window, visible: bool) -> Result<(), PlatformError> {
    if visible {
        window.activate_window();
        Ok(())
    } else {
        Err(PlatformError::Unsupported)
    }
}

pub fn show_without_focus(window: &gpui::Window) -> Result<(), PlatformError> {
    set_visible(window, true)
}

pub fn set_frame(
    _: &mut gpui::Window,
    _: f64,
    _: f64,
    _: f64,
    _: f64,
    _: &gpui::App,
) -> Result<(), PlatformError> {
    Err(PlatformError::Unsupported)
}

/// Nothing to configure: the preview is an ordinary rectangular window here.
pub fn configure_shaped_preview(_: &gpui::Window, _: &gpui::App) -> Result<(), PlatformError> {
    Ok(())
}

/// Nothing to clip: see [`configure_shaped_preview`].
pub fn clip_preview_shape(
    _: &gpui::Window,
    _: PreviewPlacement,
    _: f64,
    _: &gpui::App,
) -> Result<(), PlatformError> {
    Ok(())
}

/// The panel then simply has no double-tap trigger.
pub fn new_key_state() -> Result<Box<dyn ModifierKeyState>, String> {
    Err("modifier double-tap is not available on this platform yet".to_string())
}

/// The language the system is set to, as read from the locale variables.
pub fn system_language() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
}

//! Windows: Win32 windowing, input injection and shell calls.

mod focus;
mod locale;
mod modifier_state;
mod paste;
mod screen;
mod shell;
mod window;

use std::rc::Rc;

use quick_panel_core::ports::{Capabilities, PasteTarget};

pub use locale::system_language;
pub use modifier_state::new_key_state;
pub use screen::panel_anchor;
pub use shell::{open_target, reveal_path};
pub use window::{
    clip_preview_shape, configure_shaped_preview, set_frame, set_visible, show_without_focus,
};

/// There is no window-shape mask on Windows, so the preview stays a rectangular window and the
/// pointer arrow is not drawn (`shaped_preview: false`).
pub const CAPABILITIES: Capabilities = Capabilities {
    auto_paste: true,
    cursor_anchor: true,
    shaped_preview: false,
    open_and_reveal: true,
};

/// Captures the window in front, which the panel is about to cover.
pub fn capture_paste_target(cx: &gpui::App) -> Rc<dyn PasteTarget> {
    Rc::new(paste::ForegroundWindow::capture(cx))
}

/// A console-subsystem process owns no application activation state, so there is nothing to
/// demote: the panel window is a tool window and has no taskbar button.
pub fn run_as_background_app() {}

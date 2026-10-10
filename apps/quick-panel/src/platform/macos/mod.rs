//! macOS: AppKit, Core Graphics and Accessibility.

mod app;
mod modifier_state;
mod paste;
mod screen;
mod shell;
mod window;

use std::rc::Rc;

use quick_panel_core::ports::{Capabilities, PasteTarget};

pub use app::run_as_background_app;
pub use modifier_state::new_key_state;
pub use screen::panel_anchor;
pub use shell::{open_target, reveal_path};
pub use window::{
    clip_preview_shape, configure_shaped_preview, set_frame, set_visible, show_without_focus,
};

pub const CAPABILITIES: Capabilities = Capabilities {
    auto_paste: true,
    cursor_anchor: true,
    shaped_preview: true,
    open_and_reveal: true,
};

/// Captures the application in front, which the panel is about to cover.
pub fn capture_paste_target(_: &gpui::App) -> Rc<dyn PasteTarget> {
    Rc::new(paste::FrontApplication::capture())
}

/// The language the system is set to, as a BCP 47 tag.
pub fn system_language() -> Option<String> {
    use objc2_foundation::NSLocale;
    NSLocale::preferredLanguages()
        .firstObject()
        .map(|language| language.to_string())
}

//! Everything that differs between operating systems.
//!
//! This is the only place that selects an implementation by target. Each implementation exports
//! the same functions and constants; adding a platform means adding a module with that surface and
//! one line here.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(not(target_os = "macos"))]
mod fallback;
#[cfg(not(target_os = "macos"))]
use fallback as imp;

pub use imp::{
    capture_paste_target, clip_preview_shape, configure_shaped_preview, new_key_state, open_target,
    panel_anchor, reveal_path, run_as_background_app, set_frame, set_visible, show_without_focus,
    system_language, CAPABILITIES,
};

//! Everything that differs between operating systems.
//!
//! This is the only place that selects an implementation by target. Each implementation exports
//! the same functions and constants; adding a platform means adding a module with that surface and
//! one line here.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as imp;

#[cfg(target_os = "windows")]
mod win32;
#[cfg(target_os = "windows")]
use win32 as imp;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod fallback;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use fallback as imp;

pub use imp::{
    capture_paste_target, clip_preview_shape, configure_shaped_preview, new_key_state, open_target,
    panel_anchor, reveal_path, run_as_background_app, set_frame, set_visible, show_without_focus,
    system_language, CAPABILITIES, WINDOW_CORNER_RADIUS,
};

/// Only shaped native windows reserve space for a pointer outside the preview body.
pub const PREVIEW_POINTER_DEPTH: f64 = if CAPABILITIES.shaped_preview {
    quick_panel_core::geometry::window_pair::POINTER_DEPTH
} else {
    0.
};

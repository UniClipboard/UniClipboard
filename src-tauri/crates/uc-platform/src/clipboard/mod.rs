// `common.rs` wraps `clipboard_rs::ClipboardContext`. Phase 4 narrowed
// `clipboard-rs` to macOS/Windows targets, so `common` follows; Linux's
// native Wayland + x11rb backends don't need it.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod common;
pub mod event_loop;
#[cfg(target_os = "windows")]
pub mod image_convert;
pub mod noop;
pub mod platform;
pub mod watcher;

pub use event_loop::{
    build_event_loop, shutdown_channel, PlatformClipboardEventLoop, ShutdownRx, ShutdownTx,
};
pub use noop::NoopSystemClipboard;
pub use platform::LocalClipboard;
pub use watcher::{PlatformEvent, PlatformEventSender};

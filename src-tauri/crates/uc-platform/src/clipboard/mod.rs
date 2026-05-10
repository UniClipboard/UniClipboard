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

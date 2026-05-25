// `common.rs` wraps `clipboard_rs::ClipboardContext`. Phase 4 narrowed
// `clipboard-rs` to macOS/Windows targets, so `common` follows; Linux's
// native Wayland + x11rb backends don't need it.
// CF_HTML wrapper normalization. Pure string logic, but only called from the
// Windows write path — gate the module with `any(test, target_os = "windows")`
// so non-Windows release builds don't trip `-D dead-code` (CI runs that on
// Linux), while `cargo test` on every host still compiles and runs the unit
// tests (cfg(test) is enabled during the test build).
#[cfg(any(test, target_os = "windows"))]
pub(crate) mod cf_html;
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod common;
pub mod event_loop;
#[cfg(target_os = "windows")]
pub mod image_convert;
pub mod noop;
// `payload.rs` 是跨平台 rep payload helper（按 source 分流读字节），三平台写入
// 路径都依赖它来消化入站的 `LocalFile` source rep。它独立于 `clipboard-rs`，
// 因此不与 `common` 共享 cfg gate（Linux Wayland / X11 写入器也调用它）。
pub(crate) mod payload;
pub mod platform;
pub mod watcher;

pub use event_loop::{
    build_event_loop, shutdown_channel, PlatformClipboardEventLoop, ShutdownRx, ShutdownTx,
};
pub use noop::NoopSystemClipboard;
pub use platform::LocalClipboard;
pub use watcher::{PlatformEvent, PlatformEventSender};

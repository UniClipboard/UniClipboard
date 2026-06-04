// `common.rs` wraps `clipboard_rs::ClipboardContext`. Phase 4 narrowed
// `clipboard-rs` to macOS/Windows targets, so `common` follows; Linux's
// native Wayland + x11rb backends don't need it.
// CF_HTML wrapper normalization. Pure string logic, but only called from the
// Windows write path — gate the module with `any(test, target_os = "windows")`
// so non-Windows release builds don't trip `-D dead-code` (CI runs that on
// Linux), while `cargo test` on every host still compiles and runs the unit
// tests (cfg(test) is enabled during the test build).
#[cfg(any(test, all(target_os = "windows", feature = "desktop")))]
pub(crate) mod cf_html;
// `common` wraps `clipboard_rs::ClipboardContext`; only macOS/Windows still
// use it (Linux switched to native wayland/x11rb in Phase 4) and only the
// `desktop` feature pulls `clipboard-rs` in. Mobile / portable hosts implement
// their own adapter against `SystemClipboardPort` and do not need this.
#[cfg(all(any(target_os = "macos", target_os = "windows"), feature = "desktop"))]
pub mod common;
pub mod event_loop;
pub mod format_id_mime;
// `image_convert::dib_to_png` is pure `image`-crate logic but is only invoked
// by the Windows desktop adapter. Compile it together with the Windows
// adapter to avoid `-D dead-code` warnings under `desktop`.
#[cfg(all(target_os = "windows", feature = "desktop"))]
pub mod image_convert;
pub mod noop;
// `payload.rs` 是跨平台 rep payload helper（按 source 分流读字节），桌面三平台
// 写入路径都依赖它来消化入站的 `LocalFile` source rep。逻辑纯 std + uc-core，
// 形态上是 portable 的，但目前唯一的消费者都在 desktop 适配器内（macOS/Windows
// `common.rs` + Linux `platform/linux/*` 的 wayland/x11rb 写入器）；portable 构建
// 里没有人调用它，全部会触发 `dead_code` 警告，因此随 `desktop` feature 一并编入。
// 未来 mobile 写入路径若需要按 source 分流读盘，再把这层 gate 撤掉。
#[cfg(feature = "desktop")]
pub(crate) mod payload;
#[cfg(feature = "desktop")]
pub mod platform;
pub mod watcher;

pub use event_loop::{shutdown_channel, PlatformClipboardEventLoop, ShutdownRx, ShutdownTx};
pub use noop::NoopSystemClipboard;
pub use watcher::{PlatformEvent, PlatformEventSender};

// Concrete OS-bound adapter + factory only exist when the `desktop` feature
// is on. Portable / mobile hosts wire their own `SystemClipboardPort`
// implementation and their own event loop (e.g. driven by host lifecycle
// callbacks on iOS / Android), so these re-exports stay desktop-only.
#[cfg(feature = "desktop")]
pub use event_loop::build_event_loop;
#[cfg(feature = "desktop")]
pub use platform::LocalClipboard;

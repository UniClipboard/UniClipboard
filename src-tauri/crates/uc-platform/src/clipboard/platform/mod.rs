// `clipboard_rs_adapter` wraps `clipboard_rs::ClipboardWatcherContext`,
// so Phase 4 narrowed it to macOS/Windows. Linux uses native
// Wayland + x11rb event loops under `linux::build_event_loop`.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod clipboard_rs_adapter;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

// macOS exports
#[cfg(target_os = "macos")]
pub use macos::MacOSClipboard as LocalClipboard;

// Windows exports
#[cfg(target_os = "windows")]
pub use windows::WindowsClipboard as LocalClipboard;

// Unix exports
#[cfg(target_os = "linux")]
pub use linux::LinuxClipboard as LocalClipboard;

/// Default platform clipboard event loop factory.
///
/// - Linux: delegates to [`linux::build_event_loop`], which runtime-selects
///   the native Wayland implementation (when `WAYLAND_DISPLAY` is set and
///   the compositor advertises `ext`- or `wlr-data-control`) or the
///   native x11rb implementation. The legacy `clipboard_rs` adapter was
///   removed in Phase 4.
/// - macOS / Windows: wraps `clipboard_rs::ClipboardWatcherContext` via
///   [`clipboard_rs_adapter::ClipboardRsEventLoop`].
pub fn build_event_loop(
) -> anyhow::Result<Box<dyn crate::clipboard::event_loop::PlatformClipboardEventLoop>> {
    #[cfg(target_os = "linux")]
    {
        return linux::build_event_loop();
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        Ok(Box::new(clipboard_rs_adapter::ClipboardRsEventLoop::new()))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        anyhow::bail!(
            "No clipboard event loop implementation available for target_os = {}",
            std::env::consts::OS
        )
    }
}

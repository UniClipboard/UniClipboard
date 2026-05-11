//! Linux clipboard backend with runtime Wayland / X11 dispatch.
//!
//! `LinuxClipboard` is the public type plugged in as `LocalClipboard` for
//! `target_os = "linux"`. At construction time it probes the session and
//! selects between two backends:
//!
//! - [`legacy::LegacyLinuxClipboard`] — `clipboard_rs`-based X11 path. Used
//!   for X11 sessions today and (Phase 2 transitional behavior) also as the
//!   read/write path on Wayland sessions until [`wayland::WaylandClipboard`]
//!   is fully wired up in Phase 2b.
//! - `wayland::WaylandClipboard` — native `wlr-data-control` /
//!   `ext-data-control` client that talks directly to the compositor.
//!
//! The Wayland *event loop* (clipboard change watcher) is selected
//! independently in [`super::build_event_loop`] — Phase 2a lights up
//! `WaylandEventLoop` ahead of `WaylandClipboard` so users on Wayland get
//! correct change notifications immediately while reads/writes still go
//! through the legacy path. The two halves get unified in Phase 2b.

mod legacy;
pub(super) mod wayland;

use anyhow::Result;
use async_trait::async_trait;
use tracing::{debug, info, warn};
use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::SystemClipboardPort;

pub use legacy::LegacyLinuxClipboard;

use crate::clipboard::event_loop::PlatformClipboardEventLoop;

pub enum LinuxClipboard {
    Legacy(LegacyLinuxClipboard),
    Wayland(wayland::WaylandClipboard),
}

impl LinuxClipboard {
    pub fn new() -> Result<Self> {
        // Wayland session AND compositor advertises wlr-data-control →
        // native Wayland clipboard. Otherwise fall back to the legacy
        // clipboard_rs/X11 path. Phase 3 swaps the X11 path to native
        // x11rb.
        if is_wayland_session() {
            match wayland::WaylandClipboard::try_new() {
                Ok(Some(wl)) => {
                    info!("Linux clipboard: native Wayland (data-control)");
                    return Ok(Self::Wayland(wl));
                }
                Ok(None) => {
                    debug!(
                        "Linux clipboard: Wayland session but no data-control protocol; \
                         falling back to clipboard_rs adapter"
                    );
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "Linux clipboard: Wayland clipboard probe failed; falling back"
                    );
                }
            }
        }
        info!("Linux clipboard: clipboard_rs (X11) adapter");
        Ok(Self::Legacy(LegacyLinuxClipboard::new()?))
    }
}

#[async_trait]
impl SystemClipboardPort for LinuxClipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        match self {
            Self::Legacy(c) => c.read_snapshot(),
            Self::Wayland(c) => c.read_snapshot(),
        }
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        match self {
            Self::Legacy(c) => c.write_snapshot(snapshot),
            Self::Wayland(c) => c.write_snapshot(snapshot),
        }
    }
}

/// Returns true if the current process is running under a Wayland session.
///
/// Strict check: the `WAYLAND_DISPLAY` environment variable must point at a
/// reachable wayland socket. We do not look at `XDG_SESSION_TYPE` because
/// that's a session manager hint, not a runtime guarantee — XWayland-only
/// processes can have `XDG_SESSION_TYPE=wayland` but no `WAYLAND_DISPLAY`.
pub(super) fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// Build the platform clipboard event loop for Linux.
///
/// Runtime selection:
///
/// 1. If `WAYLAND_DISPLAY` is set, attempt to bring up
///    [`wayland::WaylandEventLoop`]. The constructor itself probes the
///    compositor for `zwlr_data_control_manager_v1`; if the protocol isn't
///    advertised it returns `Ok(None)` and we fall back transparently.
/// 2. Otherwise (or on Wayland without data-control), wrap
///    [`crate::clipboard::platform::clipboard_rs_adapter::ClipboardRsEventLoop`]
///    which goes through `clipboard_rs::ClipboardWatcherContext` (X11
///    XFIXES). Phase 3 replaces this branch with a native `x11rb`
///    implementation.
pub(super) fn build_event_loop() -> Result<Box<dyn PlatformClipboardEventLoop>> {
    if is_wayland_session() {
        match wayland::WaylandEventLoop::try_new() {
            Ok(Some(wl)) => {
                info!("Linux clipboard event loop: native Wayland (data-control)");
                return Ok(Box::new(wl));
            }
            Ok(None) => {
                debug!(
                    "Linux clipboard event loop: Wayland session but no data-control \
                     protocol; falling back to clipboard_rs adapter"
                );
            }
            Err(e) => {
                warn!(error = %e, "Linux clipboard event loop: Wayland probe failed; falling back");
            }
        }
    }
    info!("Linux clipboard event loop: clipboard_rs (X11) adapter");
    Ok(Box::new(
        super::clipboard_rs_adapter::ClipboardRsEventLoop::new(),
    ))
}

//! Linux clipboard backend with runtime Wayland / X11 dispatch.
//!
//! `LinuxClipboard` is the public type plugged in as `LocalClipboard` for
//! `target_os = "linux"`. At construction time it probes the session and
//! selects between two native backends:
//!
//! - `wayland::WaylandClipboard` — native `wlr-data-control` /
//!   `ext-data-control` client that talks directly to the compositor.
//! - [`x11::X11Clipboard`] — native `x11rb` ICCCM selection client used
//!   on X11 sessions and as the XWayland fallback when Wayland
//!   data-control is unavailable.
//!
//! [`build_event_loop`] follows the same precedence (Wayland → X11) for
//! the change watcher. Each layer's `try_new` returns `Ok(None)` when its
//! probe says "not available here", so the cascade is transparent.
//!
//! Phase 4 removed the legacy `clipboard_rs` fallback — every Linux
//! environment we support is covered by one of the two native backends.

pub(super) mod wayland;
pub(super) mod x11;

use anyhow::Result;
use async_trait::async_trait;
use tracing::{info, warn};
use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::SystemClipboardPort;

use crate::clipboard::event_loop::PlatformClipboardEventLoop;

pub enum LinuxClipboard {
    Wayland(wayland::WaylandClipboard),
    X11(x11::X11Clipboard),
}

impl LinuxClipboard {
    pub fn new() -> Result<Self> {
        // Preference order:
        //   1. Wayland (when WAYLAND_DISPLAY is set AND a data-control
        //      protocol — ext or wlr — is advertised).
        //   2. Native x11rb (whenever an X display is reachable, including
        //      XWayland under a Wayland session without data-control).
        if is_wayland_session() {
            match wayland::WaylandClipboard::try_new() {
                Ok(Some(wl)) => {
                    info!("Linux clipboard: native Wayland (data-control)");
                    return Ok(Self::Wayland(wl));
                }
                Ok(None) => {
                    info!(
                        "Linux clipboard: Wayland session but no data-control protocol; \
                         falling through to native x11rb"
                    );
                }
                Err(e) => {
                    warn!(
                        error = %e,
                        "Linux clipboard: Wayland probe failed; falling through to native x11rb"
                    );
                }
            }
        }

        match x11::try_new_clipboard() {
            Ok(Some(x)) => {
                info!(
                    wayland_session = is_wayland_session(),
                    "Linux clipboard: native X11 (x11rb)"
                );
                Ok(Self::X11(x))
            }
            Ok(None) => Err(anyhow::anyhow!(
                "Linux clipboard: no usable backend — Wayland data-control unavailable and \
                 no X display reachable (DISPLAY={:?}, WAYLAND_DISPLAY={:?})",
                std::env::var_os("DISPLAY"),
                std::env::var_os("WAYLAND_DISPLAY")
            )),
            Err(e) => Err(e.context("Linux clipboard: native X11 init failed")),
        }
    }
}

#[async_trait]
impl SystemClipboardPort for LinuxClipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        match self {
            Self::Wayland(c) => c.read_snapshot(),
            Self::X11(c) => c.read_snapshot(),
        }
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        match self {
            Self::Wayland(c) => c.write_snapshot(snapshot),
            Self::X11(c) => c.write_snapshot(snapshot),
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
/// Runtime selection mirrors [`LinuxClipboard::new`]:
///
/// 1. Wayland data-control via [`wayland::WaylandEventLoop`] if the
///    compositor advertises ext- or wlr-data-control.
/// 2. Native x11rb via [`x11::X11EventLoop`] whenever an X display is
///    reachable (covers XWayland sessions whose compositor doesn't expose
///    data-control).
///
/// Returns `Err` if neither backend can be brought up — that means the
/// process is on a Linux box with neither a Wayland nor an X display, which
/// is unusual for desktop deployments (headless containers should not be
/// running the daemon).
pub(super) fn build_event_loop() -> Result<Box<dyn PlatformClipboardEventLoop>> {
    if is_wayland_session() {
        match wayland::WaylandEventLoop::try_new() {
            Ok(Some(wl)) => {
                info!("Linux clipboard event loop: native Wayland (data-control)");
                return Ok(Box::new(wl));
            }
            Ok(None) => {
                info!(
                    "Linux clipboard event loop: Wayland session but no data-control \
                     protocol; falling through to native x11rb"
                );
            }
            Err(e) => {
                warn!(error = %e, "Linux clipboard event loop: Wayland probe failed; falling through");
            }
        }
    }

    match x11::try_new_event_loop() {
        Ok(Some(loop_)) => {
            info!(
                wayland_session = is_wayland_session(),
                "Linux clipboard event loop: native X11 (x11rb + XFIXES)"
            );
            Ok(Box::new(loop_))
        }
        Ok(None) => Err(anyhow::anyhow!(
            "Linux clipboard event loop: no usable backend — Wayland data-control \
             unavailable and no X display reachable (DISPLAY={:?}, WAYLAND_DISPLAY={:?})",
            std::env::var_os("DISPLAY"),
            std::env::var_os("WAYLAND_DISPLAY")
        )),
        Err(e) => Err(e.context("Linux clipboard event loop: native X11 probe failed")),
    }
}

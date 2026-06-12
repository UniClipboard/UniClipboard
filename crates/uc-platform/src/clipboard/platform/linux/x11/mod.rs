//! Native X11 clipboard backend for Linux (Phase 3).
//!
//! Replaces the `clipboard_rs` X11 path for X11 sessions (and the XWayland
//! fallback when Wayland data-control is unavailable). Direct `x11rb`
//! bindings keep us in the same ecosystem family as the Wayland code
//! (`smithay-rs`'s `wayland-client`) and let us own the INCR streaming /
//! selection-owner state machine instead of inheriting whatever quirks
//! `clipboard_rs` may surface.
//!
//! Two halves:
//!
//! - [`event_loop::X11EventLoop`] — drives [`crate::clipboard::watcher::ClipboardWatcher`]
//!   from XFIXES `SELECTION_NOTIFY` events on `CLIPBOARD`. Used by the
//!   daemon clipboard watcher worker.
//! - [`clipboard::X11Clipboard`] — implements [`uc_core::ports::SystemClipboardPort`]
//!   on top of ICCCM selection ownership. The reader half handles `TARGETS`,
//!   per-mime `convert_selection`, and INCR streaming receive; the writer
//!   half takes selection ownership and services `SelectionRequest` events
//!   on a dedicated worker thread.
//!
//! Both halves are constructed via [`try_new_event_loop`] / [`try_new_clipboard`],
//! which connect to `$DISPLAY` and check that `XFIXES` is available. If the
//! environment has no X display reachable at all, both return `Ok(None)` and
//! the parent [`super::super::build_event_loop`] / `LinuxClipboard::new`
//! fall back to the legacy `clipboard_rs` adapter.

mod atoms;
mod clipboard;
mod connection;
mod event_loop;
mod reader;
mod writer;

pub(crate) use clipboard::X11Clipboard;
pub(crate) use event_loop::X11EventLoop;

use anyhow::Result;
use tracing::{debug, info};

use connection::X11Server;

/// Try to bring up the X11 event loop. Returns:
///
/// - `Ok(Some(_))` — X server reachable + XFIXES available; caller drives `run()`.
/// - `Ok(None)`   — no usable X display; caller falls back to the legacy
///   `clipboard_rs` adapter.
/// - `Err(_)`     — connect/probe failure that should bubble up.
pub(crate) fn try_new_event_loop() -> Result<Option<X11EventLoop>> {
    match X11Server::connect() {
        Ok(server) => {
            info!("x11 event loop: connected (display ready, XFIXES available)");
            Ok(Some(X11EventLoop::new(server)))
        }
        Err(e) => {
            debug!(error = %e, "x11 event loop: connect failed");
            Ok(None)
        }
    }
}

/// Try to bring up the X11 clipboard read/write backend. Same return
/// semantics as [`try_new_event_loop`].
pub(crate) fn try_new_clipboard() -> Result<Option<X11Clipboard>> {
    // The clipboard worker constructs its own connection internally so it can
    // own the selection. We still do a connect-probe here so the caller can
    // distinguish "no X server" from "X server present but later failure".
    if X11Server::connect().is_err() {
        debug!("x11 clipboard: connect probe failed, skipping x11 backend");
        return Ok(None);
    }
    info!("x11 clipboard: connected (display ready, XFIXES available)");
    let c = X11Clipboard::spawn()?;
    Ok(Some(c))
}

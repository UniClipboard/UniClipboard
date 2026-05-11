//! `PlatformClipboardEventLoop` facade for the Wayland data-control client.
//!
//! Delegates to whichever protocol-specific event loop was selected at
//! construction time (see [`super::protocol::try_new_event_loop`]).

use anyhow::Result;

use crate::clipboard::event_loop::{PlatformClipboardEventLoop, ShutdownRx};
use crate::clipboard::watcher::ClipboardWatcher;

use super::protocol::{ext::ExtEventLoop, try_new_event_loop, wlr::WlrEventLoop};

pub(crate) struct WaylandEventLoop {
    pub(super) inner: WaylandEventLoopInner,
}

pub(super) enum WaylandEventLoopInner {
    Wlr(WlrEventLoop),
    Ext(ExtEventLoop),
}

impl WaylandEventLoop {
    /// Connect to the running wayland session and pick a backend.
    ///
    /// - `Ok(Some(_))` — manager bind succeeded; caller drives [`Self::run`].
    /// - `Ok(None)` — wayland connect succeeded but no data-control protocol
    ///   is advertised (e.g. plain GNOME mutter < 47); caller falls back to
    ///   the legacy adapter.
    /// - `Err(_)` — hard probe failure; caller falls back.
    pub(crate) fn try_new() -> Result<Option<Self>> {
        try_new_event_loop()
    }
}

impl PlatformClipboardEventLoop for WaylandEventLoop {
    fn run(self: Box<Self>, handler: ClipboardWatcher, shutdown_rx: ShutdownRx) -> Result<()> {
        match self.inner {
            WaylandEventLoopInner::Wlr(loop_) => Box::new(loop_).run(handler, shutdown_rx),
            WaylandEventLoopInner::Ext(loop_) => Box::new(loop_).run(handler, shutdown_rx),
        }
    }
}

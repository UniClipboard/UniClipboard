//! `SystemClipboardPort` facade for the Wayland data-control client.
//!
//! Delegates to whichever protocol-specific worker was selected at
//! construction time (see [`super::protocol::try_new_clipboard`]).

use anyhow::Result;
use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::SystemClipboardPort;

use super::protocol::{ext::ExtClipboard, try_new_clipboard, wlr::WlrClipboard};

pub struct WaylandClipboard {
    pub(super) inner: WaylandClipboardInner,
}

pub(super) enum WaylandClipboardInner {
    Wlr(WlrClipboard),
    Ext(ExtClipboard),
}

impl WaylandClipboard {
    /// Connect to the running wayland session and pick a backend.
    ///
    /// - `Ok(Some(_))` — backend ready; calls into the trait return promptly.
    /// - `Ok(None)` — connect succeeded but no data-control protocol is
    ///   advertised; caller falls back to the legacy clipboard.
    /// - `Err(_)` — hard probe / spawn failure.
    pub(crate) fn try_new() -> Result<Option<Self>> {
        try_new_clipboard()
    }
}

#[async_trait::async_trait]
impl SystemClipboardPort for WaylandClipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        match &self.inner {
            WaylandClipboardInner::Wlr(c) => c.read_snapshot(),
            WaylandClipboardInner::Ext(c) => c.read_snapshot(),
        }
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        match &self.inner {
            WaylandClipboardInner::Wlr(c) => c.write_snapshot(snapshot),
            WaylandClipboardInner::Ext(c) => c.write_snapshot(snapshot),
        }
    }
}

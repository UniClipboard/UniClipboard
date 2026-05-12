//! Adapter that drives the platform clipboard event loop on top of
//! `clipboard_rs::ClipboardWatcherContext`.
//!
//! Used by macOS, Windows, and (Phase 1) Linux X11. Phase 2 introduces a
//! native `wayland-client`-backed implementation for Wayland sessions; Phase 3
//! adds a native `x11rb`-backed implementation that replaces this adapter on
//! Linux entirely; Phase 4 then narrows the `clipboard-rs` dependency to
//! `cfg(any(target_os = "macos", target_os = "windows"))`.
//!
//! Bridging notes:
//!
//! - `ClipboardWatcherContext::start_watch` is a synchronous blocking C-style
//!   loop. We run it on the same thread the worker spawned us on (via
//!   `tokio::task::spawn_blocking`).
//! - Cross-thread shutdown: `WatcherShutdown` is a thin `Sender<()>` wrapper
//!   whose `stop(self)` is just `drop(self)`. Move it onto a tiny helper
//!   thread that blocks on [`ShutdownRx::wait`] and drops the handle to
//!   propagate the signal back into the watcher's own internal channel.

use anyhow::Result;
use clipboard_rs::{ClipboardWatcher as RSClipboardWatcher, ClipboardWatcherContext};
use tracing::{debug, info, warn};

use crate::clipboard::event_loop::{PlatformClipboardEventLoop, ShutdownRx};
use crate::clipboard::watcher::ClipboardWatcher;

pub struct ClipboardRsEventLoop;

impl ClipboardRsEventLoop {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClipboardRsEventLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformClipboardEventLoop for ClipboardRsEventLoop {
    fn run(self: Box<Self>, handler: ClipboardWatcher, shutdown_rx: ShutdownRx) -> Result<()> {
        let mut watcher_ctx = ClipboardWatcherContext::new()
            .map_err(|e| anyhow::anyhow!("Failed to create ClipboardWatcherContext: {}", e))?;

        let shutdown_handle = watcher_ctx.add_handler(handler).get_shutdown_channel();

        // Helper thread that blocks until our `ShutdownRx` fires, then drops
        // `WatcherShutdown` (== sender.send(()) == stop). Joining the helper
        // after `start_watch` returns guarantees the handle isn't dropped on
        // the watcher thread itself.
        let helper = std::thread::Builder::new()
            .name("clipboard-watcher-shutdown".into())
            .spawn(move || {
                shutdown_rx.wait();
                debug!("clipboard_rs adapter: shutdown signal received, stopping watcher_ctx");
                shutdown_handle.stop();
            })
            .map_err(|e| anyhow::anyhow!("Failed to spawn shutdown helper thread: {}", e))?;

        info!("clipboard_rs adapter: starting watcher loop");
        watcher_ctx.start_watch();
        info!("clipboard_rs adapter: watcher loop returned");

        if let Err(panic) = helper.join() {
            warn!(
                ?panic,
                "clipboard_rs adapter: shutdown helper thread panicked"
            );
        }
        Ok(())
    }
}

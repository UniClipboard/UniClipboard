//! `PlatformClipboardEventLoop` impl for the Wayland data-control client.
//!
//! Connects to the running wayland session, binds `wl_seat` +
//! `zwlr_data_control_manager_v1`, gets a per-seat data device, and then
//! pumps the event queue while polling alongside the shutdown eventfd.
//! Each `Selection` event captured by [`super::state::State`] produces a
//! `SystemClipboardSnapshot` which we forward to the
//! `ClipboardWatcher::notify_with_snapshot` dedup pipeline.

use anyhow::{Context, Result};
use rustix::event::{poll, PollFd, PollFlags};
use std::os::fd::{AsRawFd, BorrowedFd};
use tracing::{debug, info, warn};
use wayland_client::Connection;

use crate::clipboard::event_loop::{PlatformClipboardEventLoop, ShutdownRx};
use crate::clipboard::watcher::ClipboardWatcher;

use super::state::State;

/// Fallback poll interval when `ShutdownRx::raw_fd()` is unavailable. Keeps
/// shutdown latency bounded without spinning. Real path always has an
/// eventfd and never hits this.
const FALLBACK_POLL_TIMEOUT_MS: i32 = 250;

pub(crate) struct WaylandEventLoop {
    conn: Connection,
}

impl WaylandEventLoop {
    /// Probe the running wayland session for `zwlr_data_control_manager_v1`.
    /// Returns:
    ///
    /// - `Ok(Some(_))` — manager found, caller should drive [`Self::run`].
    /// - `Ok(None)` — the wayland connection succeeded but the compositor
    ///   does not advertise the protocol (e.g. plain GNOME mutter < 47);
    ///   caller should fall back to the legacy adapter.
    /// - `Err(_)` — hard wayland connect failure; caller should fall back.
    ///
    /// The probe spends one roundtrip on a throwaway event queue, then
    /// reuses the underlying `Connection` for the real run loop. This
    /// avoids a second TCP/socket connection and keeps the compositor's
    /// view of clients unchanged.
    pub(crate) fn try_new() -> Result<Option<Self>> {
        let conn = match Connection::connect_to_env() {
            Ok(c) => c,
            Err(e) => {
                debug!(error = %e, "wayland: cannot connect; skipping wayland backend");
                return Ok(None);
            }
        };

        let mut probe_queue = conn.new_event_queue::<State>();
        let qh = probe_queue.handle();
        let _registry = conn.display().get_registry(&qh, ());

        let mut state = State::new();
        // Two roundtrips: one to receive Global events, one to let any
        // bind-time follow-ups settle. The second is cheap and protects
        // against compositors that send some globals only after the client
        // shows interest.
        probe_queue
            .roundtrip(&mut state)
            .context("wayland probe roundtrip 1 failed")?;
        probe_queue
            .roundtrip(&mut state)
            .context("wayland probe roundtrip 2 failed")?;

        if state.manager.is_none() {
            debug!("wayland: zwlr_data_control_manager_v1 not advertised");
            return Ok(None);
        }
        if state.seat.is_none() {
            warn!("wayland: data-control manager present but no wl_seat — falling back");
            return Ok(None);
        }

        debug!("wayland: zwlr_data_control_manager_v1 detected, using native backend");
        Ok(Some(Self { conn }))
    }
}

impl PlatformClipboardEventLoop for WaylandEventLoop {
    fn run(self: Box<Self>, mut handler: ClipboardWatcher, shutdown_rx: ShutdownRx) -> Result<()> {
        info!("wayland event loop: starting");

        let conn = self.conn;
        let mut event_queue = conn.new_event_queue::<State>();
        let qh = event_queue.handle();
        let _registry = conn.display().get_registry(&qh, ());

        let mut state = State::new();

        // Bootstrap: bind globals.
        event_queue
            .roundtrip(&mut state)
            .context("wayland startup roundtrip failed")?;

        let manager = state
            .manager
            .clone()
            .context("wlr-data-control manager disappeared after probe")?;
        let seat = state
            .seat
            .clone()
            .context("wl_seat disappeared after probe")?;

        // Bind device for this seat. The device is what delivers
        // DataOffer / Selection events.
        state.device = Some(manager.get_data_device(&seat, &qh, ()));

        // Initial roundtrip so the compositor sends us the *current*
        // Selection (if any) — without this we'd only see future copies.
        event_queue
            .roundtrip(&mut state)
            .context("wayland device-bind roundtrip failed")?;

        // Drain whatever the device-bind roundtrip produced.
        for snap in state.pending_snapshots.drain(..) {
            handler.notify_with_snapshot(snap);
        }

        // Main loop: dispatch_pending → flush → poll → read.
        loop {
            event_queue
                .dispatch_pending(&mut state)
                .context("wayland dispatch_pending failed")?;
            for snap in state.pending_snapshots.drain(..) {
                handler.notify_with_snapshot(snap);
            }

            if shutdown_rx.is_signaled() {
                debug!("wayland event loop: shutdown observed before poll");
                break;
            }

            event_queue
                .flush()
                .context("wayland event_queue flush failed")?;

            // Reserve the right to read from the wayland socket.
            let read_guard = match conn.prepare_read() {
                Some(g) => g,
                None => {
                    // Events arrived between dispatch_pending and prepare_read;
                    // loop back to dispatch them.
                    continue;
                }
            };

            let wl_raw_fd = read_guard.connection_fd().as_raw_fd();
            let shutdown_raw_fd = shutdown_rx.raw_fd();

            // Build poll set. Borrow lifetimes are tied to the surrounding
            // scope; we copy out the raw fds first then borrow_raw fresh
            // BorrowedFds for the poll call.
            //
            // SAFETY: the wayland fd lives at least as long as `read_guard`
            // (the guard holds an internal lock); the shutdown eventfd is
            // owned by `ShutdownInner` which is `Arc`-shared with the
            // sender, so it stays open for the duration of the loop.
            let wl_borrow = unsafe { BorrowedFd::borrow_raw(wl_raw_fd) };

            let poll_result;
            let wl_revents;
            let shutdown_woke;

            if let Some(s_raw) = shutdown_raw_fd {
                let s_borrow = unsafe { BorrowedFd::borrow_raw(s_raw) };
                let mut pfds = [
                    PollFd::new(&wl_borrow, PollFlags::IN),
                    PollFd::new(&s_borrow, PollFlags::IN),
                ];
                poll_result = poll(&mut pfds, -1);
                wl_revents = pfds[0].revents();
                shutdown_woke = pfds[1].revents().contains(PollFlags::IN);
            } else {
                let mut pfds = [PollFd::new(&wl_borrow, PollFlags::IN)];
                poll_result = poll(&mut pfds, FALLBACK_POLL_TIMEOUT_MS);
                wl_revents = pfds[0].revents();
                shutdown_woke = false;
            }

            match poll_result {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => {
                    drop(read_guard);
                    continue;
                }
                Err(e) => return Err(e.into()),
            }

            if shutdown_woke || shutdown_rx.is_signaled() {
                drop(read_guard);
                debug!("wayland event loop: shutdown signal received");
                break;
            }

            if wl_revents.contains(PollFlags::IN) {
                // Pull events from socket into the queue. This consumes the
                // guard and releases the read lock.
                if let Err(e) = read_guard.read() {
                    return Err(anyhow::anyhow!("wayland read events failed: {e:?}"));
                }
            } else {
                // Spurious wakeup or poll timed out (fallback path); release
                // the guard and try again.
                drop(read_guard);
            }
        }

        info!("wayland event loop: stopped");
        Ok(())
    }
}

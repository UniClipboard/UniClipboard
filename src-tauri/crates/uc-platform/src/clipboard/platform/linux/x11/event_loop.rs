//! XFIXES-driven `PlatformClipboardEventLoop` for X11.
//!
//! Subscribes the watcher window to `xfixes::select_selection_input` on
//! `CLIPBOARD` with the same mask `clipboard_rs` used. Each
//! `XfixesSelectionNotify` triggers a fresh `read_snapshot()` on the
//! watcher's own connection (separate from the `X11Clipboard` worker's
//! connection, so a slow read can't block paste-serving).
//!
//! The poll loop waits on `[conn.fd, shutdown_rx.raw_fd]` so the shutdown
//! signal wakes us instantly without a helper thread. When `raw_fd()`
//! returns `None` (eventfd unavailable) we fall back to a short poll
//! timeout + Condvar check.

use std::os::fd::{AsFd, BorrowedFd};

use anyhow::{Context, Result};
use rustix::event::{poll, PollFd, PollFlags};
use tracing::{debug, info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::{self, SelectionEventMask};
use x11rb::protocol::Event;

use crate::clipboard::event_loop::{PlatformClipboardEventLoop, ShutdownRx};
use crate::clipboard::watcher::ClipboardWatcher;

use super::connection::X11Server;
use super::reader::read_snapshot;

/// Used when the shutdown channel didn't manage to allocate an eventfd
/// (extremely unusual). 250 ms keeps us reactive without burning CPU.
const FALLBACK_POLL_TIMEOUT_MS: i32 = 250;

pub(crate) struct X11EventLoop {
    server: X11Server,
}

impl X11EventLoop {
    pub(super) fn new(server: X11Server) -> Self {
        Self { server }
    }
}

impl PlatformClipboardEventLoop for X11EventLoop {
    fn run(self: Box<Self>, mut handler: ClipboardWatcher, shutdown_rx: ShutdownRx) -> Result<()> {
        info!("x11 event loop: starting");

        let server = self.server;
        let conn = &server.conn;
        let atoms = server.atoms;

        // Subscribe to CLIPBOARD ownership-change notifications. Mask
        // matches what clipboard_rs / xclip / klipper register so we catch:
        //  - new owner taking over,
        //  - current owner's window being destroyed,
        //  - current owner's client disconnecting.
        xfixes::select_selection_input(
            conn,
            server.screen_root,
            atoms.CLIPBOARD,
            SelectionEventMask::SET_SELECTION_OWNER
                | SelectionEventMask::SELECTION_WINDOW_DESTROY
                | SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )
        .context("x11 watcher: xfixes::select_selection_input failed")?
        .check()
        .context("x11 watcher: xfixes::select_selection_input check failed")?;
        conn.flush().context("x11 watcher: initial flush failed")?;

        // Emit a baseline snapshot so consumers see the current clipboard
        // state without waiting for the next change — matches what the
        // wayland watcher does after the device-bind roundtrip.
        match read_snapshot(&server, None) {
            Ok(snap) if !snap.representations.is_empty() => {
                handler.notify_with_snapshot(snap);
            }
            Ok(_) => debug!("x11 watcher: baseline read returned empty snapshot"),
            Err(e) => warn!(error = %e, "x11 watcher: baseline read failed"),
        }

        loop {
            // Drain anything currently buffered. We process every event so
            // we don't miss a change that arrived while we were reading.
            let mut saw_change = false;
            while let Some(event) = conn
                .poll_for_event()
                .context("x11 watcher: poll_for_event failed")?
            {
                if matches!(event, Event::XfixesSelectionNotify(_)) {
                    saw_change = true;
                }
            }

            if saw_change {
                match read_snapshot(&server, None) {
                    Ok(snap) if !snap.representations.is_empty() => {
                        handler.notify_with_snapshot(snap);
                    }
                    Ok(_) => debug!("x11 watcher: selection-notify produced empty snapshot"),
                    Err(e) => warn!(error = %e, "x11 watcher: read_snapshot failed"),
                }
            }

            if shutdown_rx.is_signaled() {
                debug!("x11 watcher: shutdown observed before poll");
                break;
            }

            // Wait for either the X11 fd to become readable or the shutdown
            // eventfd to fire.
            let stream = conn.stream().as_fd();
            let shutdown_raw_fd = shutdown_rx.raw_fd();

            let poll_result;
            let shutdown_woke;
            if let Some(s_raw) = shutdown_raw_fd {
                // SAFETY: the shutdown eventfd lives inside `ShutdownInner`
                // (Arc-shared with the sender); it outlives this poll.
                let s_borrow = unsafe { BorrowedFd::borrow_raw(s_raw) };
                let mut pfds = [
                    PollFd::new(&stream, PollFlags::IN),
                    PollFd::new(&s_borrow, PollFlags::IN),
                ];
                poll_result = poll(&mut pfds, -1);
                shutdown_woke = pfds[1].revents().contains(PollFlags::IN);
            } else {
                let mut pfds = [PollFd::new(&stream, PollFlags::IN)];
                poll_result = poll(&mut pfds, FALLBACK_POLL_TIMEOUT_MS);
                shutdown_woke = false;
            }

            match poll_result {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(e) => return Err(e.into()),
            }

            if shutdown_woke || shutdown_rx.is_signaled() {
                debug!("x11 watcher: shutdown signal received");
                break;
            }
        }

        info!("x11 event loop: stopped");
        Ok(())
    }
}

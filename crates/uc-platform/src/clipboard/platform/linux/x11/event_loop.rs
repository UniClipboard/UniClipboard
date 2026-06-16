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
use std::time::Duration;

use anyhow::{Context, Result};
use rustix::event::{poll, PollFd, PollFlags};
use tracing::{debug, info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::{self, SelectionEventMask};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};
use x11rb::protocol::Event;

use crate::clipboard::event_loop::{PlatformClipboardEventLoop, ShutdownRx};
use crate::clipboard::watcher::ClipboardWatcher;

use super::connection::X11Server;
use super::reader::{read_snapshot, ReadContext};

/// Used when the shutdown channel didn't manage to allocate an eventfd
/// (extremely unusual). 250 ms keeps us reactive without burning CPU.
const FALLBACK_POLL_TIMEOUT_MS: i32 = 250;

/// Total read attempts after a selection-change notification when the read
/// keeps coming back empty. Chromium (reached through the XWayland
/// selection bridge) is known to refuse or serve empty payloads for a short
/// window right after a copy; a couple of short retries absorbs that
/// (issue #1029).
const CHANGE_READ_ATTEMPTS: u32 = 3;

/// Pause between those attempts. Long enough for the owner to finish
/// installing its offer, short enough to keep capture latency negligible.
const CHANGE_READ_RETRY_DELAY: Duration = Duration::from_millis(150);

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
        // wayland watcher does after the device-bind roundtrip. A change
        // landing during this read is flagged and serviced by the first
        // loop iteration instead of being dropped.
        let baseline_ctx = ReadContext::new(None);
        match read_snapshot(&server, &baseline_ctx) {
            Ok(snap) if !snap.representations.is_empty() => {
                handler.notify_with_snapshot(snap);
            }
            Ok(_) => debug!("x11 watcher: baseline read returned empty snapshot"),
            Err(e) => warn!(error = %e, "x11 watcher: baseline read failed"),
        }
        let mut pending_change = baseline_ctx.take_selection_changed();
        // Last CLIPBOARD owner we logged, to surface ownership churn without
        // spamming: issue #957's storm is driven by an EXTERNAL client
        // re-asserting ownership on a cadence, so logging the owner each time it
        // changes names the culprit (when it carries WM_CLASS) and exposes a
        // rapid A↔B ownership war directly in the daemon log.
        let mut last_owner: Option<u32> = None;

        loop {
            // Drain anything currently buffered. We process every event so
            // we don't miss a change that arrived while we were reading —
            // including ones flagged mid-read by the previous iteration.
            let mut saw_change = std::mem::take(&mut pending_change);
            let mut new_owner = None;
            while let Some(event) = conn
                .poll_for_event()
                .context("x11 watcher: poll_for_event failed")?
            {
                if let Event::XfixesSelectionNotify(notify) = event {
                    saw_change = true;
                    new_owner = Some(notify.owner);
                }
            }

            // Diagnostic only (DEBUG, throttled to ownership changes) — does not
            // affect capture. Resolved after draining so the WM_CLASS query
            // doesn't interleave with the event drain above.
            if let Some(owner) = new_owner {
                if last_owner != Some(owner) {
                    last_owner = Some(owner);
                    debug!(
                        owner,
                        owner_info = %describe_window(conn, owner),
                        "x11 watcher: CLIPBOARD selection owner changed"
                    );
                }
            }

            if saw_change {
                pending_change = read_changed_selection(&server, &mut handler, &shutdown_rx);
            }

            if shutdown_rx.is_signaled() {
                debug!("x11 watcher: shutdown observed before poll");
                break;
            }

            if pending_change {
                // A change was flagged while we were reading; service it now
                // instead of blocking in poll (its event was already consumed,
                // so the fd would stay quiet).
                debug!("x11 watcher: selection changed during read; re-reading");
                continue;
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

/// Read the selection after a change notification, retrying a bounded
/// number of times when the read comes back empty while an owner exists.
///
/// Returns true when a further selection change was observed (and
/// necessarily consumed) during one of the reads — the caller must loop
/// around and read again rather than block in poll.
fn read_changed_selection(
    server: &X11Server,
    handler: &mut ClipboardWatcher,
    shutdown_rx: &ShutdownRx,
) -> bool {
    let ctx = ReadContext::new(None);
    for attempt in 1..=CHANGE_READ_ATTEMPTS {
        match read_snapshot(server, &ctx) {
            Ok(snap) if !snap.representations.is_empty() => {
                if attempt > 1 {
                    info!(attempt, "x11 watcher: selection read recovered after retry");
                }
                handler.notify_with_snapshot(snap);
                return ctx.take_selection_changed();
            }
            Ok(_) => {
                // Empty with no owner is a legitimate cleared clipboard —
                // not worth retrying or warning about.
                if current_selection_owner(server) == x11rb::NONE {
                    info!("x11 watcher: selection has no owner (cleared); nothing to capture");
                    return ctx.take_selection_changed();
                }
                if shutdown_rx.is_signaled() {
                    return ctx.take_selection_changed();
                }
                if attempt == CHANGE_READ_ATTEMPTS {
                    break;
                }
                debug!(
                    attempt,
                    retry_delay_ms = CHANGE_READ_RETRY_DELAY.as_millis() as u64,
                    "x11 watcher: empty snapshot after selection change; retrying"
                );
                std::thread::sleep(CHANGE_READ_RETRY_DELAY);
            }
            Err(e) => {
                warn!(
                    error = %e,
                    error_kind = "x11_selection_read_failed",
                    retryable = true,
                    "x11 watcher: read_snapshot failed after selection change"
                );
                return ctx.take_selection_changed();
            }
        }
    }
    warn!(
        attempts = CHANGE_READ_ATTEMPTS,
        error_kind = "x11_selection_read_empty",
        "x11 watcher: selection changed but no readable content (owner refused, timed out, \
         or offered no interesting mimes) — clipboard change lost"
    );
    ctx.take_selection_changed()
}

/// Best-effort, diagnostics-only human description of an X11 window: its
/// `WM_CLASS` when set, otherwise just the hex id. Selection-owner windows are
/// often unmanaged utility windows with no `WM_CLASS`, so an empty class is
/// expected and simply yields the bare id. Never fails — any X error degrades
/// to the id (or "none" for the cleared selection).
fn describe_window<C: Connection>(conn: &C, window: u32) -> String {
    if window == x11rb::NONE {
        return "none (selection cleared)".to_string();
    }
    let class = conn
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        // WM_CLASS is "instance\0class\0"; render the NULs as spaces.
        .map(|reply| {
            String::from_utf8_lossy(&reply.value)
                .replace('\0', " ")
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty());
    match class {
        Some(c) => format!("0x{window:x} ({c})"),
        None => format!("0x{window:x}"),
    }
}

/// Current owner window of `CLIPBOARD`, or `x11rb::NONE` when unowned or
/// the query fails (treated as unowned — the caller only uses this to
/// decide whether an empty read is worth retrying).
fn current_selection_owner(server: &X11Server) -> u32 {
    server
        .conn
        .get_selection_owner(server.atoms.CLIPBOARD)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.owner)
        .unwrap_or(x11rb::NONE)
}

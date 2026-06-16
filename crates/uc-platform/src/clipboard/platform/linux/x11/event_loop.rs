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
use std::time::{Duration, Instant};

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

/// Hard upper bound for polling a single selection ownership after a change
/// notification. X11/ICCCM emits no event when an owner that already holds
/// the selection later *adds* targets — Chromium reached through the
/// XWayland bridge advertises a private target first and only fills in
/// `text/plain` a beat later (issue #1029). So once an owner takes over we
/// poll it ourselves until it serves readable content, releases the
/// selection, or this budget elapses. The cap stops us polling forever when
/// an owner only ever offers private / undecodable formats.
const CHANGE_POLL_DEADLINE: Duration = Duration::from_secs(3);

/// First backoff between empty reads while the same owner keeps holding the
/// selection. Short enough that the common case (data ready within a couple
/// hundred ms) is captured with negligible latency.
const CHANGE_POLL_INITIAL_DELAY: Duration = Duration::from_millis(150);

/// Backoff cap. The interval doubles (150 → 300 → 500 …) up to this value so
/// the tail of the poll window stays dense — a late `text/plain` is picked
/// up within at most this long of becoming available.
const CHANGE_POLL_MAX_DELAY: Duration = Duration::from_millis(500);

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

/// Read the selection after a change notification, polling the triggering
/// owner across its lifetime until it serves readable content, releases the
/// selection, or [`CHANGE_POLL_DEADLINE`] elapses.
///
/// X11 fires no event when an owner that already holds the selection later
/// adds targets, so a fixed short retry window misses an owner that supplies
/// data lazily (issue #1029). Instead we record the owner this notification
/// is about and re-read with exponential backoff for as long as that same
/// owner keeps the selection. A *different* owner (or a clear) ends the
/// round: its own `XfixesSelectionNotify` drives a fresh read, so chasing it
/// here would be redundant.
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
    // The owner this notification is about. A query failure reads as NONE,
    // which the first empty read treats as a legitimate clear.
    let initial_owner = current_selection_owner(server);
    let deadline = Instant::now() + CHANGE_POLL_DEADLINE;
    let mut delay = CHANGE_POLL_INITIAL_DELAY;
    let mut attempt: u32 = 0;

    loop {
        if shutdown_rx.is_signaled() {
            return ctx.take_selection_changed();
        }
        attempt += 1;
        match read_snapshot(server, &ctx) {
            Ok(snap) if !snap.representations.is_empty() => {
                if attempt > 1 {
                    info!(attempt, "x11 watcher: selection read recovered after retry");
                }
                handler.notify_with_snapshot(snap);
                return ctx.take_selection_changed();
            }
            Ok(_) => {
                let current_owner = current_selection_owner(server);
                let now = Instant::now();
                match poll_outcome(initial_owner, current_owner, now >= deadline) {
                    PollOutcome::Cleared => {
                        info!("x11 watcher: selection has no owner (cleared); nothing to capture");
                        return ctx.take_selection_changed();
                    }
                    PollOutcome::OwnerChanged => {
                        debug!(
                            "x11 watcher: selection owner changed during poll; deferring to \
                             the new owner's change notification"
                        );
                        return ctx.take_selection_changed();
                    }
                    PollOutcome::Expired => break,
                    PollOutcome::KeepPolling => {
                        // Never sleep past the deadline so the final read lands
                        // right at the budget edge rather than beyond it.
                        let nap = delay.min(deadline.saturating_duration_since(now));
                        debug!(
                            attempt,
                            retry_delay_ms = nap.as_millis() as u64,
                            "x11 watcher: empty snapshot; owner still holds selection, backing off"
                        );
                        std::thread::sleep(nap);
                        delay = next_poll_delay(delay);
                    }
                }
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
        attempts = attempt,
        budget_ms = CHANGE_POLL_DEADLINE.as_millis() as u64,
        error_kind = "x11_selection_read_empty",
        "x11 watcher: owner held the selection for the whole poll window but never served \
         readable content (offered only private / undecodable mimes, refused, or timed out) \
         — clipboard change lost"
    );
    ctx.take_selection_changed()
}

/// What to do after an empty read while polling a single ownership. See
/// [`poll_outcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PollOutcome {
    /// No owner holds the selection — a legitimate clear; nothing to capture.
    Cleared,
    /// A different client now owns the selection; its own change
    /// notification will drive a fresh read, so this round stops.
    OwnerChanged,
    /// The triggering owner still holds the selection but hasn't served any
    /// readable content yet — keep waiting.
    KeepPolling,
    /// The triggering owner held the selection for the whole budget without
    /// ever serving readable content — give up (treated as a lost change).
    Expired,
}

/// Decide what to do after an empty read while polling a single ownership.
///
/// `initial_owner` is the owner observed when the poll round began (the one
/// the triggering notification is about); `current_owner` is the owner now.
/// Clear and owner-change are reported ahead of budget expiry so the log
/// names the real reason the round ended rather than a misleading timeout.
fn poll_outcome(initial_owner: u32, current_owner: u32, budget_exhausted: bool) -> PollOutcome {
    if current_owner == x11rb::NONE {
        PollOutcome::Cleared
    } else if current_owner != initial_owner {
        PollOutcome::OwnerChanged
    } else if budget_exhausted {
        PollOutcome::Expired
    } else {
        PollOutcome::KeepPolling
    }
}

/// Next backoff interval: double the current one, capped at
/// [`CHANGE_POLL_MAX_DELAY`].
fn next_poll_delay(current: Duration) -> Duration {
    (current * 2).min(CHANGE_POLL_MAX_DELAY)
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

/// Current owner window of `CLIPBOARD`, or `x11rb::NONE` when unowned or the
/// query fails (treated as unowned). The poll loop uses this both to detect a
/// legitimate clear and to tell whether the owner it is polling has changed.
fn current_selection_owner(server: &X11Server) -> u32 {
    server
        .conn
        .get_selection_owner(server.atoms.CLIPBOARD)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| reply.owner)
        .unwrap_or(x11rb::NONE)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Distinct non-NONE owner window ids for the decision-function tests.
    const OWNER_A: u32 = 0x0100_0001;
    const OWNER_B: u32 = 0x0200_0002;

    #[test]
    fn poll_outcome_keeps_polling_same_owner_within_budget() {
        // The lazy-supply case (#1029): same owner, still empty, budget left.
        assert_eq!(
            poll_outcome(OWNER_A, OWNER_A, false),
            PollOutcome::KeepPolling
        );
    }

    #[test]
    fn poll_outcome_expires_when_same_owner_exhausts_budget() {
        assert_eq!(poll_outcome(OWNER_A, OWNER_A, true), PollOutcome::Expired);
    }

    #[test]
    fn poll_outcome_reports_clear_regardless_of_budget() {
        assert_eq!(
            poll_outcome(OWNER_A, x11rb::NONE, false),
            PollOutcome::Cleared
        );
        assert_eq!(
            poll_outcome(OWNER_A, x11rb::NONE, true),
            PollOutcome::Cleared
        );
    }

    #[test]
    fn poll_outcome_defers_to_new_owner_regardless_of_budget() {
        // A different live owner took over: stop polling the old ownership
        // even with budget to spare — the new owner fires its own notify.
        assert_eq!(
            poll_outcome(OWNER_A, OWNER_B, false),
            PollOutcome::OwnerChanged
        );
        assert_eq!(
            poll_outcome(OWNER_A, OWNER_B, true),
            PollOutcome::OwnerChanged
        );
    }

    #[test]
    fn poll_outcome_clear_takes_precedence_over_expiry() {
        // Ordering guarantee: an owner that vanished right as the budget ran
        // out is a clear (not a lost change), so the log isn't misleading.
        assert_eq!(
            poll_outcome(OWNER_A, x11rb::NONE, true),
            PollOutcome::Cleared
        );
    }

    #[test]
    fn next_poll_delay_doubles_then_caps() {
        let d0 = CHANGE_POLL_INITIAL_DELAY;
        assert_eq!(d0, Duration::from_millis(150));
        let d1 = next_poll_delay(d0);
        assert_eq!(d1, Duration::from_millis(300));
        // 600 ms would exceed the cap, so it clamps to CHANGE_POLL_MAX_DELAY…
        let d2 = next_poll_delay(d1);
        assert_eq!(d2, CHANGE_POLL_MAX_DELAY);
        // …and stays there.
        let d3 = next_poll_delay(d2);
        assert_eq!(d3, CHANGE_POLL_MAX_DELAY);
    }

    #[test]
    fn poll_schedule_fits_several_dense_reads_inside_budget() {
        // Sanity-check the schedule the loop will follow: count the reads
        // that start before the deadline. The capped tail must keep reads
        // dense enough that late data is caught within CHANGE_POLL_MAX_DELAY.
        let mut delay = CHANGE_POLL_INITIAL_DELAY;
        let mut elapsed = Duration::ZERO;
        let mut reads = 1; // the first read is immediate
        while elapsed + delay <= CHANGE_POLL_DEADLINE {
            elapsed += delay;
            reads += 1;
            delay = next_poll_delay(delay);
        }
        assert!(
            reads >= 7,
            "expected the 3s window to allow >=7 reads, got {reads}"
        );
        assert!(CHANGE_POLL_MAX_DELAY <= Duration::from_millis(500));
    }
}

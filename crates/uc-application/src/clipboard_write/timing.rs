//! Self-write echo attribution budget for [`ClipboardWriteCoordinator`].
//!
//! When the daemon writes to the OS clipboard (restore / inbound sync / file
//! copy) the platform watcher fires a change event for that very write. To
//! avoid re-capturing and re-broadcasting our own write, the coordinator arms
//! an attribution record before writing and the watcher consumes it when the
//! echo arrives.
//!
//! ## Consumption is event-driven; time is only a GC backstop
//!
//! The authority for resolving a self-write echo is the **next watcher
//! event**, not the clock: a content-keyed record is consumed the moment a
//! change with the matching hash is observed, and a next-change record is
//! consumed by the very next observed change. The window below does NOT decide
//! attribution — it only garbage-collects a record whose echo never arrives (a
//! write may legitimately produce no clipboard event at all: identical content,
//! or a failed write). Without the GC backstop a record left armed forever
//! would eventually mis-attribute an unrelated user copy.
//!
//! ## One window, not two
//!
//! Local writes (restore / file copy) and remote pushes (inbound sync) both
//! encode the SAME physical quantity — the worst-case round-trip latency for a
//! programmatic write to come back as a watcher callback, including OS
//! re-encoding (Windows PNG→DIB→PNG) and file URI/path rewrites. They used to
//! carry two different windows (2s local, 60s remote) purely because remote
//! pushes were rare and a longer backstop felt cheap; with consumption now
//! firmly event-driven the backstop value is no longer load-bearing for
//! correctness, so the two collapse into a single budget. Naming it once keeps
//! us honest about the project rule against scattered timeout literals (mirrors
//! the pattern in `uc-daemon-process::timing`).

use std::time::Duration;

/// Worst-case latency between a programmatic clipboard write and the watcher
/// callback for that write, covering OS re-encoding and file URI/path rewrites
/// that change the bytes between write and echo.
///
/// This is the single self-write echo budget for both local writes (restore /
/// file copy) and remote pushes (inbound sync). It is a garbage-collection
/// backstop, not an attribution authority: the next observed change consumes
/// the armed record regardless of how much of this window remains (see the
/// module docs). Sized generously so a slow echo (weak network, large image
/// re-encode) is still recognised as our own write rather than re-broadcast; a
/// genuine echo that somehow arrives later is backstopped by inbound idempotency.
pub(crate) const CLIPBOARD_ECHO_RTT_MAX: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the echo budget so a change to it is a deliberate, reviewed act
    /// rather than a silent drift.
    #[test]
    fn budget_is_pinned() {
        assert_eq!(CLIPBOARD_ECHO_RTT_MAX, Duration::from_secs(5));
    }
}

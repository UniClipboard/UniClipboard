//! Self-write echo attribution budgets for [`ClipboardWriteCoordinator`].
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
//! consumed by the very next observed change. The windows below do NOT decide
//! attribution — they only garbage-collect a record whose echo never arrives (a
//! write may legitimately produce no clipboard event at all: identical content,
//! or a failed write). Without the GC backstop a record left armed forever
//! would eventually mis-attribute an unrelated user copy.
//!
//! ## Two budgets, because the echoes are different physical quantities
//!
//! For a *content-keyed* record the backstop value is not load-bearing: the
//! matching hash consumes it regardless of how much window remains. But the
//! *next-change fallback* exists precisely for the case where the bytes change
//! between write and echo, so the content hash provably cannot match — and there
//! the time bound is the ONLY thing that recognises the echo as our own write.
//! That case is asymmetric between local and remote:
//!
//! - **Local writes** (history restore / file copy) are not OS-re-encoded; at
//!   most a file restore comes back with rewritten URI/path bytes, and that echo
//!   returns fast. Kept short so a lingering record (a write that produced no
//!   echo because identical content was already on the clipboard) cannot swallow
//!   a user's deliberate identical re-copy for long.
//! - **Remote pushes** (inbound sync) can be re-encoded by the platform (Windows
//!   PNG→DIB→PNG) AND can be large/slow on a contended host. For a re-encoded
//!   image the content record cannot match, so the next-change fallback is the
//!   sole suppression; sizing it short risks bouncing the peer's image back as a
//!   fresh local capture (re-broadcast loop). It must cover the worst-case
//!   write→echo round-trip, not the typical one.
//!
//! Naming each budget once keeps us honest about the project rule against
//! scattered timeout literals (mirrors the pattern in
//! `uc-daemon-process::timing`). They are NOT derived from a shared base —
//! deriving two unrelated worst-case latencies from each other would be fake
//! coupling — so each is pinned independently.

use std::time::Duration;

/// Echo backstop for LOCAL programmatic writes (history restore / file copy).
///
/// Local echoes are fast and never OS-re-encoded, so this only needs to outlast
/// a same-process write→watcher round-trip. Kept short so a record whose echo
/// never arrives (identical content already on the clipboard) does not swallow a
/// user's deliberate identical re-copy. GC backstop only — see the module docs.
pub(crate) const LOCAL_ECHO_RTT_MAX: Duration = Duration::from_secs(2);

/// Echo backstop for REMOTE pushes (inbound sync).
///
/// Sized for the worst case the next-change fallback exists to cover: OS image
/// re-encoding (Windows PNG→DIB→PNG) on a contended host, where the content
/// record provably cannot match and this time-bounded fallback is the ONLY
/// suppression. A re-encode echo that arrives after this window is mis-read as a
/// fresh local capture and re-dispatched back to the sender, so the budget is
/// generous on purpose. GC backstop only — see the module docs.
pub(crate) const REMOTE_ECHO_RTT_MAX: Duration = Duration::from_secs(60);

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the echo budgets so a change to either is a deliberate, reviewed act
    /// rather than a silent drift.
    #[test]
    fn budgets_are_pinned() {
        assert_eq!(LOCAL_ECHO_RTT_MAX, Duration::from_secs(2));
        assert_eq!(REMOTE_ECHO_RTT_MAX, Duration::from_secs(60));
    }
}

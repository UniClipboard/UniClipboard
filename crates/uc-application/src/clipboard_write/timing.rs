//! Self-write echo attribution budgets for [`ClipboardWriteCoordinator`].
//!
//! When the daemon writes to the OS clipboard (restore / inbound sync / file
//! copy) the platform watcher fires a change event for that very write. To
//! avoid re-capturing and re-broadcasting our own write, the coordinator arms
//! an attribution guard before writing and the watcher consumes it when the
//! echo arrives. The guard needs a staleness window: a write may produce no
//! observable event at all (identical content, or a failed write), so a guard
//! left armed forever would mis-attribute a later, unrelated user copy.
//!
//! These windows used to live as inline `Duration::from_secs(2 | 60)` literals
//! spread across `coordinator.rs` (the local hash guard, the remote hash
//! guard, and their two `set_next_origin` fallbacks). They all encode the SAME
//! physical quantity — the worst-case round-trip latency for a programmatic
//! write to come back as a watcher callback, including OS re-encoding
//! (Windows PNG→DIB→PNG) and file URI/path rewrites. Naming the base once and
//! DERIVING the rest makes that relation load-bearing instead of a comment, in
//! line with the project rule against scattered timeout literals (mirrors the
//! pattern in `uc-daemon-process::timing`).
//!
//! Dependency chain (base feeds derived):
//!
//! ```text
//! OS clipboard echo round-trip   = CLIPBOARD_ECHO_RTT_MAX          (base)
//! remote-push staleness backstop = SELF_WRITE_STALENESS_BACKSTOP   (= 30 × base)
//! ```

use std::time::Duration;

/// Multiply a duration by an integer factor at compile time.
///
/// Operates on whole milliseconds so the result stays a `const`.
const fn scale(d: Duration, factor: u32) -> Duration {
    Duration::from_millis(d.as_millis() as u64 * factor as u64)
}

/// Base budget: the worst-case latency between a programmatic clipboard write
/// and the watcher callback for that write, covering OS re-encoding and file
/// URI/path rewrites that change the bytes between write and echo.
///
/// Used for local writes (restore / file copy), where the originating user
/// action is local and a mis-merge window must stay short.
pub(crate) const CLIPBOARD_ECHO_RTT_MAX: Duration = Duration::from_secs(2);

/// Staleness backstop for remote-push (inbound sync) self-write guards.
///
/// Remote pushes are comparatively rare and the cost of a false merge (treating
/// a real user copy as an echo) is low, so the guard tolerates a longer round
/// trip than [`CLIPBOARD_ECHO_RTT_MAX`] before it is garbage-collected. Derived
/// from the base so the two move together.
///
/// Note: S2 of this refactor will revisit whether local and remote genuinely
/// need different windows; until then this preserves the historical 60s value
/// exactly (`30 × 2s`).
pub(crate) const SELF_WRITE_STALENESS_BACKSTOP: Duration = scale(CLIPBOARD_ECHO_RTT_MAX, 30);

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the derived budgets to their historical values so a change to the
    /// base (or the factor) is a deliberate, reviewed act rather than a silent
    /// drift. These numbers are the literals S0 replaced in `coordinator.rs`.
    #[test]
    fn budgets_are_pinned() {
        assert_eq!(CLIPBOARD_ECHO_RTT_MAX, Duration::from_secs(2));
        assert_eq!(SELF_WRITE_STALENESS_BACKSTOP, Duration::from_secs(60));
    }
}

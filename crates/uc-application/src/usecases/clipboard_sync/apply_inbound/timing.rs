//! Inbound idempotency dedup horizons for `ApplyInboundClipboardUseCase`.
//!
//! These windows answer ONE question: *"have we already applied this inbound
//! clip recently?"* They deduplicate the SAME logical clip arriving more than
//! once — the same peer re-pushing identical bytes, a peer re-sending the same
//! visible content with extended representations, or a single file copy
//! arriving via two delivery channels (direct dispatch + active-state pull)
//! under different snapshot hashes.
//!
//! ## Not the self-write echo story
//!
//! This is INBOUND idempotency, and it is a different concern from self-write
//! echo suppression. When the daemon writes an inbound clip to the OS
//! clipboard, the platform watcher fires for that very write; suppressing THAT
//! echo is the job of the self-write ledger and the coordinator's single echo
//! budget (`crate::clipboard_write::timing`). The two never share a window: one
//! bounds *"our own programmatic write coming back as a watcher event"*, the
//! other bounds *"the same peer clip being delivered twice"*. Keeping the
//! budgets in separate homes is deliberate — it stops the two narratives from
//! being conflated back into one tangled "loopback" story.
//!
//! ## Two independent horizons (not derived from a shared base)
//!
//! Unlike the self-write echo budget, these two are genuinely different
//! physical quantities and do NOT derive from one base, so each is named and
//! pinned independently rather than scaled off a common root (deriving
//! unrelated budgets from each other would be fake coupling). They are
//! centralised here, not inlined at the use case, per the project rule against
//! scattered timeout literals.
//!
//! A former third horizon — a sender-`entry_id` source-entry window — was
//! retired once the per-identity coordinator made content-hash dedup atomic:
//! the two channels now carry the same canonical hash, so a `find` by hash
//! (under the identity lock) collapses them without a separate entry_id cache.

use std::time::Duration;

/// Byte-identical echo-frame window: a peer re-pushing the exact same
/// `snapshot_hash` within this interval is treated as one logical clip.
///
/// Sized to the network re-push cadence (retries / duplicate frames arrive
/// within a few hundred milliseconds), short enough that a deliberate repeat
/// copy of identical bytes by the user is not swallowed.
pub(crate) const RAPID_DUPLICATE_WINDOW: Duration = Duration::from_millis(200);

/// Visible-content window: the same visible content arriving under a *different*
/// `snapshot_hash` (e.g. a peer re-sending with extended representations) is
/// collapsed to one logical clip within this interval.
///
/// Sized to a human re-copy window — long enough to absorb a peer's re-send of
/// the same thing the user just saw, short enough not to merge a genuinely new
/// copy of similar content.
pub(crate) const VISIBLE_DUPLICATE_WINDOW: Duration = Duration::from_secs(2);

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the dedup horizons so a change to any of them is a deliberate,
    /// reviewed act rather than a silent drift.
    #[test]
    fn dedup_windows_are_pinned() {
        assert_eq!(RAPID_DUPLICATE_WINDOW, Duration::from_millis(200));
        assert_eq!(VISIBLE_DUPLICATE_WINDOW, Duration::from_secs(2));
    }
}

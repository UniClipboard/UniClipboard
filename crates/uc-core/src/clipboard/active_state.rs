//! The cross-device "currently active clipboard" register value object.

use crate::ids::{DeviceId, EntryId};

/// Which clipboard content is currently the active OS-clipboard content,
/// modelled as a last-writer-wins (LWW) register that converges across
/// devices.
///
/// The register identity (the thing that is "the same content" on any
/// device) is [`content_hash`](Self::content_hash). The LWW order is the
/// pair `(activated_at_ms, activated_by)`: a higher `activated_at_ms`
/// wins, ties break on the lexicographically greater `activated_by`.
///
/// `entry_id` is a per-device local handle and is deliberately *not* part
/// of the cross-device identity or the LWW comparison — the same content
/// has a different `entry_id` on each device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveClipboardState {
    /// Stable, cross-device content identity string (`"blake3v1:<hex>"`).
    /// Two devices holding identical clipboard content compute the same
    /// value; equals the content's snapshot hash.
    pub content_hash: String,
    /// Local entry handle for the content on this device. Per-device only;
    /// never compared across devices.
    pub entry_id: EntryId,
    /// Wall-clock milliseconds of the activation event itself (the moment
    /// the content became the active clipboard), independent of when the
    /// underlying entry was first created. Primary LWW key.
    pub activated_at_ms: i64,
    /// The device that performed the activation. LWW tiebreaker and
    /// attribution only.
    pub activated_by: DeviceId,
}

impl ActiveClipboardState {
    pub fn new(
        content_hash: impl Into<String>,
        entry_id: EntryId,
        activated_at_ms: i64,
        activated_by: DeviceId,
    ) -> Self {
        Self {
            content_hash: content_hash.into(),
            entry_id,
            activated_at_ms,
            activated_by,
        }
    }

    /// Whether `self` supersedes `current` under the LWW order.
    ///
    /// True iff `self.activated_at_ms` is greater, or the timestamps are
    /// equal and `self.activated_by` is lexicographically greater. Two
    /// values with the same `(activated_at_ms, activated_by)` never
    /// supersede each other (the register is already converged on them).
    pub fn supersedes(&self, current: &ActiveClipboardState) -> bool {
        use std::cmp::Ordering;
        match self.activated_at_ms.cmp(&current.activated_at_ms) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => self.activated_by.as_str() > current.activated_by.as_str(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(hash: &str, ts: i64, by: &str) -> ActiveClipboardState {
        ActiveClipboardState::new(hash, EntryId::new(), ts, DeviceId::new(by))
    }

    #[test]
    fn newer_timestamp_supersedes() {
        let older = state("blake3v1:aa", 100, "dev-a");
        let newer = state("blake3v1:bb", 200, "dev-a");
        assert!(newer.supersedes(&older));
        assert!(!older.supersedes(&newer));
    }

    #[test]
    fn equal_timestamp_breaks_on_activator() {
        let lo = state("blake3v1:aa", 100, "dev-a");
        let hi = state("blake3v1:bb", 100, "dev-b");
        assert!(hi.supersedes(&lo));
        assert!(!lo.supersedes(&hi));
    }

    #[test]
    fn fully_equal_key_does_not_supersede() {
        let a = state("blake3v1:aa", 100, "dev-a");
        let b = state("blake3v1:aa", 100, "dev-a");
        assert!(!a.supersedes(&b));
        assert!(!b.supersedes(&a));
    }
}

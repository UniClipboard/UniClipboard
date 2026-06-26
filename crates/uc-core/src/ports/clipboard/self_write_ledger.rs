use crate::ClipboardChangeOrigin;
use async_trait::async_trait;
use std::time::Duration;

/// How a recorded self-write is matched against a later observed change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfWriteMatch {
    /// Match a later change whose snapshot hash equals this key. Used when the
    /// written bytes are expected to come back unchanged.
    ByContent(String),
    /// Match the very next observed change regardless of its content. Used when
    /// the bytes may be re-encoded between write and observation, so a content
    /// key cannot be relied on.
    ///
    /// The carried key is the content guard key of the same write this fallback
    /// backs — the key that write also armed as [`ByContent`]. It pairs the
    /// fallback to its write so a content match can retire exactly the
    /// now-redundant fallback (and not a concurrent write's), and so repeated
    /// arms of one write coalesce instead of accumulating duplicate fallbacks.
    /// The key never gates matching: consumption still resolves the next change
    /// regardless of its hash.
    ByNextChange(String),
}

/// What a matched self-write should attribute the observed change to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfWriteAttribution {
    /// A local write (history restore / file copy).
    Local,
    /// A push originating from a remote device.
    Remote,
}

/// Records programmatic writes to the system clipboard so that a subsequent
/// observed change can be attributed to the write that caused it rather than
/// mistaken for a fresh, user-initiated capture.
///
/// This is the single source of attribution truth for clipboard write-back
/// loop prevention: a writer arms a record before writing, and the observer
/// attributes each change against the armed records.
#[async_trait]
pub trait SelfWriteLedgerPort: Send + Sync {
    /// Arm a record that a self-write has occurred (or is about to).
    ///
    /// `matching` selects how a later change is recognised as this write's
    /// echo; `attribution` selects what origin the matched change resolves to.
    /// The record expires after `ttl` if no matching change is observed (a
    /// write may legitimately produce no clipboard event at all).
    async fn record_self_write(
        &self,
        matching: SelfWriteMatch,
        attribution: SelfWriteAttribution,
        ttl: Duration,
    );

    /// Attribute an observed clipboard change identified by `snapshot_hash`.
    ///
    /// Resolves against armed records: a content-keyed record for this hash
    /// takes precedence, otherwise a pending next-change record applies. The
    /// matched record is consumed. When nothing matches, the change is a
    /// genuine, unguarded event and resolves to
    /// [`ClipboardChangeOrigin::LocalCapture`].
    ///
    /// Codomain: the ledger only ever produces one of three values —
    /// `LocalCapture` (no record matched), `LocalRestore` (a `Local`-attributed
    /// record matched), or `RemotePush { from_device: None }` (a `Remote`-
    /// attributed record matched). It never produces `Resend`, and never carries
    /// a `from_device` — the ledger tracks only local-vs-remote attribution, not
    /// peer identity. Consumers that need the originating device must source it
    /// elsewhere.
    async fn attribute_observed_change(&self, snapshot_hash: &str) -> ClipboardChangeOrigin;
}

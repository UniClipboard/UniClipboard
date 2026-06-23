use crate::ClipboardChangeOrigin;
use async_trait::async_trait;
use std::time::Duration;

#[async_trait]
pub trait ClipboardChangeOriginPort: Send + Sync {
    /// Arm a positional fallback: the NEXT observed clipboard change is
    /// attributed to `origin` regardless of its content hash, until `ttl`
    /// elapses. Covers writes whose bytes are re-encoded by the OS between
    /// write and watcher callback (so a content-hash guard cannot match).
    async fn set_next_origin(&self, origin: ClipboardChangeOrigin, ttl: Duration);

    /// Arm a content-keyed guard attributing a later change with the same
    /// `snapshot_hash` to a remote push, until `ttl` elapses.
    async fn remember_remote_snapshot_hash(&self, _snapshot_hash: String, _ttl: Duration) {}

    /// Arm a content-keyed guard attributing a later change with the same
    /// `snapshot_hash` to a local write, until `ttl` elapses.
    async fn remember_local_snapshot_hash(&self, _snapshot_hash: String, _ttl: Duration) {}

    /// Attribute an observed clipboard change identified by `snapshot_hash`.
    ///
    /// Matches an armed content guard first; if none matches, falls back to a
    /// positional guard set by [`Self::set_next_origin`]; if neither applies,
    /// returns `default_origin` (the change is a genuine, unguarded event).
    /// Consumes whichever guard matched.
    async fn consume_origin_for_snapshot_or_default(
        &self,
        snapshot_hash: &str,
        default_origin: ClipboardChangeOrigin,
    ) -> ClipboardChangeOrigin;
}

//! SearchIndexMaintenancePort — one-shot storage maintenance for the search index.
//!
//! Implemented by uc-infra. Kept separate from `SearchIndexPort` (query/index
//! surface) because it is a background-only, storage-level concern with a
//! different lifecycle: it runs once after a schema-changing rebuild.

use crate::search::SearchError;
use async_trait::async_trait;

/// Port for reclaiming on-disk residue and recording that the reclaim ran.
#[async_trait]
pub trait SearchIndexMaintenancePort: Send + Sync {
    /// Reclaim on-disk residue left by dropped plaintext columns: checkpoint the
    /// write-ahead log, compact the database, and sweep any leftover rebuild
    /// scratch tables.
    ///
    /// Idempotent and safe to re-run. Returns `SearchError::Internal` if the
    /// storage engine reports a durable failure; transient contention is retried
    /// internally with a bounded backoff rather than surfaced.
    async fn purge_plaintext_residue(&self) -> Result<(), SearchError>;

    /// Record that the one-shot plaintext-residue purge completed at `ts_ms`
    /// (milliseconds since the Unix epoch) for the active profile.
    async fn mark_plaintext_purge_done(&self, ts_ms: i64) -> Result<(), SearchError>;
}

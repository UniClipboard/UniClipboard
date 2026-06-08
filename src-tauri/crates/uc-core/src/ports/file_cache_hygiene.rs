//! Port for file-cache hygiene operations (reconcile + cleanup).
//!
//! Desktop host crates schedule these operations at startup without
//! needing to know the concrete facade implementation.

use async_trait::async_trait;

/// Result of reconciling DB entries against disk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileResult {
    pub entries_scanned: u32,
    pub entries_deleted: u32,
    pub errors: u32,
}

/// Result of cleaning up expired file-cache entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupResult {
    pub files_removed: u32,
    pub bytes_reclaimed: u64,
    pub entries_deleted: u32,
    pub orphans_removed: u32,
    pub errors: u32,
}

/// Error from file-cache hygiene operations.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
#[error("{0}")]
pub struct FileCacheHygieneError(pub String);

/// Port for file-cache reconciliation and cleanup.
///
/// Implementors walk the local file cache and the database to keep them
/// in sync: reconcile drops DB entries whose cached files are gone,
/// cleanup removes expired files that outlived their retention TTL.
#[async_trait]
pub trait FileCacheHygienePort: Send + Sync {
    /// Drop DB entries whose cache-managed file paths no longer exist on disk.
    async fn reconcile_missing_files(&self) -> Result<ReconcileResult, FileCacheHygieneError>;

    /// Remove cached files past their retention TTL.
    async fn cleanup_expired_files(&self) -> Result<CleanupResult, FileCacheHygieneError>;
}

//! Intent ports for clipboard entry persistence.
//!
//! Each trait exposes a single responsibility direction (query vs command) so
//! a consumer depends only on the capability it actually uses. The underlying
//! adapter implements all of them; the composition root coerces the single
//! adapter into each intent port.

use async_trait::async_trait;

use crate::clipboard::{ClipboardEntry, ClipboardRepositoryError, ClipboardSelectionDecision};
use crate::ids::EntryId;

/// Fetch a single clipboard entry by id.
#[async_trait]
pub trait GetClipboardEntryPort: Send + Sync {
    /// Returns the entry, or `None` when no entry with `entry_id` exists.
    async fn get_entry(
        &self,
        entry_id: &EntryId,
    ) -> Result<Option<ClipboardEntry>, ClipboardRepositoryError>;
}

/// List clipboard entries in reverse-chronological pages.
#[async_trait]
pub trait ListClipboardEntriesPort: Send + Sync {
    /// Returns up to `limit` entries starting at `offset`. An empty vector
    /// means no entries exist in that range.
    async fn list_entries(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ClipboardEntry>, ClipboardRepositoryError>;
}

/// Persist a clipboard entry together with its selection decision atomically.
#[async_trait]
pub trait SaveClipboardEntryPort: Send + Sync {
    /// Stores `entry` and `selection` as one atomic unit. Replaces any prior
    /// record sharing the same entry identity.
    async fn save_entry_and_selection(
        &self,
        entry: &ClipboardEntry,
        selection: &ClipboardSelectionDecision,
    ) -> Result<(), ClipboardRepositoryError>;
}

/// Update the last-active timestamp of an existing entry.
#[async_trait]
pub trait TouchClipboardEntryPort: Send + Sync {
    /// Sets the entry's active time to `active_time_ms`. Returns `true` when a
    /// row was updated, `false` when no entry with `entry_id` exists.
    async fn touch_entry(
        &self,
        entry_id: &EntryId,
        active_time_ms: i64,
    ) -> Result<bool, ClipboardRepositoryError>;
}

/// Delete a clipboard entry.
#[async_trait]
pub trait DeleteClipboardEntryPort: Send + Sync {
    /// Removes the entry identified by `entry_id`. Succeeds even when no such
    /// entry exists (idempotent delete).
    async fn delete_entry(&self, entry_id: &EntryId) -> Result<(), ClipboardRepositoryError>;
}

/// Resolve an entry id from a previously observed content snapshot hash.
#[async_trait]
pub trait FindEntryIdBySnapshotHashPort: Send + Sync {
    /// Returns the entry whose event carries `snapshot_hash`, or `None` when no
    /// prior capture persisted that exact hash. The hash is the wire
    /// `content_hash` string (formatted as `"blake3v1:<hex>"`).
    async fn find_entry_id_by_snapshot_hash(
        &self,
        snapshot_hash: &str,
    ) -> Result<Option<EntryId>, ClipboardRepositoryError>;
}

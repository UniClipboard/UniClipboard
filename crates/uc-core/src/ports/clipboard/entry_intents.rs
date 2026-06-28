//! Intent ports for clipboard entry persistence.
//!
//! Each trait exposes a single responsibility direction (query vs command) so
//! a consumer depends only on the capability it actually uses. The underlying
//! adapter implements all of them; the composition root coerces the single
//! adapter into each intent port.

use async_trait::async_trait;

use crate::clipboard::{
    ClipboardEntry, ClipboardEvent, ClipboardRepositoryError, ClipboardSelectionDecision,
    PersistedClipboardRepresentation,
};
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

/// Set the favorite flag of an existing entry.
#[async_trait]
pub trait SetClipboardEntryFavoritePort: Send + Sync {
    /// Sets the entry's favorite flag to `is_favorited`. Returns `true` when a
    /// row was updated, `false` when no entry with `entry_id` exists.
    /// Idempotent with respect to the stored value.
    async fn set_favorite(
        &self,
        entry_id: &EntryId,
        is_favorited: bool,
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
    /// prior capture persisted that exact hash. The hash is carried on the wire
    /// as the `snapshot_hash` string (formatted as `"blake3v1:<hex>"`).
    async fn find_entry_id_by_snapshot_hash(
        &self,
        snapshot_hash: &str,
    ) -> Result<Option<EntryId>, ClipboardRepositoryError>;
}

/// Report whether an entry's content is fully held and usable locally.
#[async_trait]
pub trait CheckEntryAvailabilityPort: Send + Sync {
    /// Returns `true` only when every representation of `entry_id` is ready —
    /// no placeholder for not-yet-materialized payload, none in a `Failed` or
    /// `Lost` state — and, for a file-backed entry, the local files its
    /// file-list points at actually exist, are readable, and are regular files.
    ///
    /// Returns `false` for a partially materialized entry (e.g. a cancelled
    /// transfer left a missing-payload placeholder) or one whose local files
    /// have since been removed or replaced. Returns `false` when no entry with
    /// `entry_id` exists.
    ///
    /// Availability is derived live on each call rather than read from a stored
    /// flag, because representation state is rewritten asynchronously by
    /// materialization and reconciliation; a denormalized column would go
    /// stale. Callers gate "do I already hold this content?" decisions on this:
    /// a hash match alone is not enough — a matched-but-unavailable entry is not
    /// held.
    async fn is_entry_available(
        &self,
        entry_id: &EntryId,
    ) -> Result<bool, ClipboardRepositoryError>;
}

/// Replace the content of an existing entry in place, reusing its identity.
#[async_trait]
pub trait ReplaceEntryContentPort: Send + Sync {
    /// Atomically swap the content behind `entry_id`: remove the entry's
    /// current event, representations, selection, thumbnails, delivery and
    /// transfer associations, then rebuild them from the supplied event,
    /// representations and selection. The entry keeps its `entry_id`, and its
    /// sticky state — `pinned`, `active_time_ms`, `created_at_ms` — is
    /// preserved; only its content pointer (`event_id`), `title` and
    /// `total_size` are updated to the new event.
    ///
    /// The whole operation is one transaction: on any failure nothing changes.
    /// `new_event` carries the authoritative content identity (`snapshot_hash`)
    /// the replaced entry will be known by. `new_selection` must reference
    /// `entry_id`.
    ///
    /// Returns an error if no entry with `entry_id` exists — replace never
    /// implicitly creates.
    async fn replace_entry_content(
        &self,
        entry_id: &EntryId,
        new_event: &ClipboardEvent,
        new_representations: &[PersistedClipboardRepresentation],
        new_selection: &ClipboardSelectionDecision,
        new_title: Option<String>,
        new_total_size: i64,
    ) -> Result<(), ClipboardRepositoryError>;
}

/// Resolve the persisted snapshot hash recorded for a given entry.
#[async_trait]
pub trait GetEntrySnapshotHashPort: Send + Sync {
    /// Returns the `"blake3v1:<hex>"` snapshot hash persisted for `entry_id`'s
    /// content identity, or `None` when no entry with `entry_id` exists. This
    /// is the stored identity, the inverse of
    /// [`FindEntryIdBySnapshotHashPort::find_entry_id_by_snapshot_hash`];
    /// callers must not recompute it from a materialized snapshot, because a
    /// rebuilt file snapshot hashes a different representation than the
    /// captured original.
    async fn get_entry_snapshot_hash(
        &self,
        entry_id: &EntryId,
    ) -> Result<Option<String>, ClipboardRepositoryError>;
}

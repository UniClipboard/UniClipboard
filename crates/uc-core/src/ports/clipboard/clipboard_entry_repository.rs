use anyhow::Result;

use crate::{clipboard::ClipboardEntry, ids::EntryId, ClipboardSelectionDecision};

#[async_trait::async_trait]
pub trait ClipboardEntryStore: Send + Sync {
    async fn save_entry_and_selection(
        &self,
        entry: &ClipboardEntry,
        selection: &ClipboardSelectionDecision,
    ) -> Result<()>;
    async fn get_entry(&self, entry_id: &EntryId) -> Result<Option<ClipboardEntry>>;

    /// List clipboard entries with pagination
    /// 列出剪贴板条目（分页）
    async fn list_entries(&self, limit: usize, offset: usize) -> Result<Vec<ClipboardEntry>>;

    /// Update the entry active time.
    /// 更新条目的活跃时间。
    async fn touch_entry(&self, _entry_id: &EntryId, _active_time_ms: i64) -> Result<bool> {
        Ok(false)
    }

    /// Set the favorite flag of an existing entry.
    /// 设置已存在条目的收藏标记。
    ///
    /// Returns `true` when a row was updated, `false` when no entry with
    /// `entry_id` exists. Idempotent: setting the same value again still
    /// reports `true` as long as the row exists.
    async fn set_favorite(&self, _entry_id: &EntryId, _is_favorited: bool) -> Result<bool> {
        Ok(false)
    }

    /// Delete a clipboard entry.
    /// 删除剪贴板条目。
    ///
    /// # Arguments
    /// * `entry_id` - The entry ID to delete
    ///
    /// # Errors
    /// Returns error if database operation fails
    async fn delete_entry(&self, entry_id: &EntryId) -> Result<()>;

    /// Look up an existing entry by its event's `snapshot_hash` (carried on
    /// the wire as the `snapshot_hash` string, formatted as
    /// `"blake3v1:<hex>"`).
    ///
    /// Returns `Some(EntryId)` when a prior capture (local or remote push)
    /// persisted a `ClipboardEvent` carrying this exact hash; returns
    /// `None` when no match exists. Used by
    /// `ApplyInboundClipboardUseCase` (Slice 2 Phase 3 · T4) as the dedup
    /// short-circuit before falling through to persist + OS-clipboard
    /// write.
    ///
    /// Implementation note: this is a read-only join across
    /// `clipboard_entry` + `clipboard_event`. Adapters that do not support
    /// the join (e.g. in-memory test fakes) may return `Ok(None)`
    /// unconditionally, which degrades Phase 3 dedup to "wire-level only"
    /// but is safe.
    async fn find_entry_id_by_snapshot_hash(
        &self,
        _snapshot_hash: &str,
    ) -> Result<Option<EntryId>> {
        Ok(None)
    }

    /// Return the snapshot hash persisted for `entry_id`'s event (the
    /// `"blake3v1:<hex>"` content identity recorded when the content was first
    /// captured), or `None` when no entry with `entry_id` exists.
    ///
    /// Inverse of [`Self::find_entry_id_by_snapshot_hash`]: that resolves a
    /// hash to an entry, this resolves an entry to its persisted hash. The
    /// value is the stored identity, returned verbatim; it must never be
    /// recomputed from a materialized snapshot, because rebuilding file
    /// content yields a different representation — and thus a different hash —
    /// than the captured original.
    ///
    /// Implementation note: a read-only join across `clipboard_entry` +
    /// `clipboard_event`. Adapters without the join (in-memory test fakes) may
    /// return `Ok(None)` unconditionally.
    async fn get_entry_snapshot_hash(&self, _entry_id: &EntryId) -> Result<Option<String>> {
        Ok(None)
    }
}

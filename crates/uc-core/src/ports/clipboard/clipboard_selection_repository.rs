use crate::{clipboard::ClipboardSelectionDecision, ids::EntryId};
use anyhow::Result;

#[async_trait::async_trait]
pub trait ClipboardSelectionRepositoryPort: Send + Sync {
    async fn get_selection(&self, entry_id: &EntryId)
        -> Result<Option<ClipboardSelectionDecision>>;

    /// Delete the clipboard selection for a given entry.
    /// 删除给定条目的剪贴板选择。
    ///
    /// # Arguments
    /// * `entry_id` - The entry ID to delete selection for
    ///
    /// # Errors
    /// Returns error if database operation fails
    async fn delete_selection(&self, entry_id: &EntryId) -> Result<()>;
}

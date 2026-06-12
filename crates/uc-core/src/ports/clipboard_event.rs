use crate::clipboard::{ClipboardEvent, PersistedClipboardRepresentation};
use crate::ids::EventId;
use anyhow::Result;

#[async_trait::async_trait]
pub trait ClipboardEventWriterPort: Send + Sync {
    async fn insert_event(
        &self,
        event: &ClipboardEvent,
        representations: &Vec<PersistedClipboardRepresentation>,
    ) -> Result<()>;

    /// Delete an event and all its associated representations.
    /// 删除事件及其所有关联的表示形式。
    ///
    /// # Arguments
    /// * `event_id` - The event ID to delete
    ///
    /// # Behavior
    /// - Deletes all snapshot representations first (they reference the event)
    /// - Then deletes the event itself
    /// - Executed within a database transaction
    ///
    /// # Errors
    /// Returns error if database operation fails
    async fn delete_event_and_representations(&self, event_id: &EventId) -> Result<()>;
}

#[async_trait::async_trait]
pub trait ClipboardEventReaderPort: Send + Sync {
    async fn get_event(&self, id: &EventId) -> Result<Option<ClipboardEvent>>;
}

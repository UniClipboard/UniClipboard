use super::event::FileTransferEvent;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait FileTransferEventStorePort: Send + Sync {
    async fn load(&self, transfer_id: &str) -> Result<Vec<FileTransferEvent>>;

    async fn append(&self, event: FileTransferEvent) -> Result<()>;
}

#[async_trait]
pub trait FileTransferEventPublisherPort: Send + Sync {
    async fn publish(&self, event: FileTransferEvent) -> Result<()>;
}

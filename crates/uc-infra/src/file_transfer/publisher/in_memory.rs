use std::sync::RwLock;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use uc_core::file_transfer::{FileTransferEvent, FileTransferEventPublisherPort};

#[derive(Debug, Default)]
pub struct InMemoryEventPublisher {
    published: RwLock<Vec<FileTransferEvent>>,
}

impl InMemoryEventPublisher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn published_events(&self) -> Result<Vec<FileTransferEvent>> {
        let published = self
            .published
            .read()
            .map_err(|_| anyhow!("in-memory file transfer publisher read lock poisoned"))?;

        Ok(published.clone())
    }
}

#[async_trait]
impl FileTransferEventPublisherPort for InMemoryEventPublisher {
    async fn publish(&self, event: FileTransferEvent) -> Result<()> {
        let mut published = self
            .published
            .write()
            .map_err(|_| anyhow!("in-memory file transfer publisher write lock poisoned"))?;

        published.push(event);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_records_events_in_order() {
        let publisher = InMemoryEventPublisher::new();
        let started = FileTransferEvent::started("transfer-1", "peer-1", "report.pdf", Some(128));
        let completed = FileTransferEvent::completed("transfer-1", "peer-1");

        publisher.publish(started.clone()).await.unwrap();
        publisher.publish(completed.clone()).await.unwrap();

        assert_eq!(
            publisher.published_events().unwrap(),
            vec![started, completed]
        );
    }
}

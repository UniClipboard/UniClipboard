use std::collections::HashMap;
use std::sync::RwLock;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use uc_core::file_transfer::{FileTransferEvent, FileTransferEventStorePort};

#[derive(Debug, Default)]
pub struct InMemoryEventStore {
    events: RwLock<HashMap<String, Vec<FileTransferEvent>>>,
}

impl InMemoryEventStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl FileTransferEventStorePort for InMemoryEventStore {
    async fn load(&self, transfer_id: &str) -> Result<Vec<FileTransferEvent>> {
        let events = self
            .events
            .read()
            .map_err(|_| anyhow!("in-memory file transfer event store read lock poisoned"))?;

        Ok(events.get(transfer_id).cloned().unwrap_or_default())
    }

    async fn append(&self, event: FileTransferEvent) -> Result<()> {
        let transfer_id = transfer_id_of(&event).to_owned();
        let mut events = self
            .events
            .write()
            .map_err(|_| anyhow!("in-memory file transfer event store write lock poisoned"))?;

        events.entry(transfer_id).or_default().push(event);
        Ok(())
    }
}

fn transfer_id_of(event: &FileTransferEvent) -> &str {
    match event {
        FileTransferEvent::Started { transfer_id, .. }
        | FileTransferEvent::Progress { transfer_id, .. }
        | FileTransferEvent::Completed { transfer_id, .. }
        | FileTransferEvent::Failed { transfer_id, .. }
        | FileTransferEvent::Cancelled { transfer_id, .. } => transfer_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::{FileTransferDirection, FileTransferProgress};

    #[tokio::test]
    async fn append_and_load_only_returns_matching_transfer_history() {
        let store = InMemoryEventStore::new();
        let first_started =
            FileTransferEvent::started("transfer-1", "peer-1", "report.pdf", Some(128));
        let second_started =
            FileTransferEvent::started("transfer-2", "peer-2", "archive.zip", Some(512));
        let first_progress = FileTransferEvent::Progress {
            transfer_id: "transfer-1".into(),
            peer_id: "peer-1".into(),
            progress: FileTransferProgress {
                direction: FileTransferDirection::Sending,
                bytes_transferred: 64,
                total_bytes: Some(128),
            },
        };

        store.append(first_started.clone()).await.unwrap();
        store.append(second_started).await.unwrap();
        store.append(first_progress.clone()).await.unwrap();

        assert_eq!(
            store.load("transfer-1").await.unwrap(),
            vec![first_started, first_progress]
        );
        assert!(store.load("missing").await.unwrap().is_empty());
    }
}

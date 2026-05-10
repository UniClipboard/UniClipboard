use std::sync::Arc;

use uc_core::file_transfer::{FileTransferEventPublisherPort, FileTransferEventStorePort};
use uc_core::{FileTransferEvent, FileTransferProgress};

use crate::file_transfer::errors::FileTransferApplicationError;
use crate::file_transfer::timeline::{load_timeline, persist_and_publish};

/// Input for reporting transfer progress.
///
/// 上报传输进度时的应用层输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportTransferProgress {
    pub transfer_id: String,
    pub peer_id: String,
    pub progress: FileTransferProgress,
}

/// Record transfer progress and emit a `Progress` event.
///
/// 记录传输进度，并产出 `Progress` 事件。
pub struct ReportTransferProgressUseCase {
    store: Arc<dyn FileTransferEventStorePort>,
    publisher: Arc<dyn FileTransferEventPublisherPort>,
}

impl ReportTransferProgressUseCase {
    pub fn new(
        store: Arc<dyn FileTransferEventStorePort>,
        publisher: Arc<dyn FileTransferEventPublisherPort>,
    ) -> Self {
        Self { store, publisher }
    }

    pub async fn execute(
        &self,
        input: ReportTransferProgress,
    ) -> Result<FileTransferEvent, FileTransferApplicationError> {
        let timeline = load_timeline(self.store.as_ref(), &input.transfer_id).await?;
        timeline.ensure_active(&input.transfer_id, &input.peer_id)?;

        if let Some(previous_bytes) = timeline.last_progress_bytes {
            if input.progress.bytes_transferred < previous_bytes {
                return Err(FileTransferApplicationError::ProgressWentBackwards {
                    transfer_id: input.transfer_id,
                    previous_bytes,
                    new_bytes: input.progress.bytes_transferred,
                });
            }
        }

        let event = FileTransferEvent::Progress {
            transfer_id: input.transfer_id,
            peer_id: input.peer_id,
            progress: input.progress,
        };

        persist_and_publish(self.store.as_ref(), self.publisher.as_ref(), event).await
    }
}

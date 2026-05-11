use std::sync::Arc;

use uc_core::file_transfer::{FileTransferEventPublisherPort, FileTransferEventStorePort};
use uc_core::FileTransferEvent;

use crate::file_transfer::errors::FileTransferApplicationError;
use crate::file_transfer::timeline::{load_timeline, persist_and_publish};

/// Input for completing a transfer.
///
/// 标记传输完成时的应用层输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteTransfer {
    pub transfer_id: String,
    pub peer_id: String,
}

/// Mark a transfer as completed and emit a `Completed` event.
///
/// 将传输标记为完成，并产出 `Completed` 事件。
pub struct CompleteTransferUseCase {
    store: Arc<dyn FileTransferEventStorePort>,
    publisher: Arc<dyn FileTransferEventPublisherPort>,
}

impl CompleteTransferUseCase {
    pub fn new(
        store: Arc<dyn FileTransferEventStorePort>,
        publisher: Arc<dyn FileTransferEventPublisherPort>,
    ) -> Self {
        Self { store, publisher }
    }

    pub async fn execute(
        &self,
        input: CompleteTransfer,
    ) -> Result<FileTransferEvent, FileTransferApplicationError> {
        let timeline = load_timeline(self.store.as_ref(), &input.transfer_id).await?;
        timeline.ensure_active(&input.transfer_id, &input.peer_id)?;

        let event = FileTransferEvent::completed(input.transfer_id, input.peer_id);
        persist_and_publish(self.store.as_ref(), self.publisher.as_ref(), event).await
    }
}

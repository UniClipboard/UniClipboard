use std::sync::Arc;

use uc_core::file_transfer::{FileTransferEventPublisherPort, FileTransferEventStorePort};
use uc_core::{FileTransferCancellationReason, FileTransferEvent};

use crate::file_transfer::errors::FileTransferApplicationError;
use crate::file_transfer::timeline::{load_timeline, persist_and_publish};

/// Input for cancelling a transfer.
///
/// 取消传输时的应用层输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelTransfer {
    pub transfer_id: String,
    pub peer_id: String,
    pub reason: FileTransferCancellationReason,
}

/// Cancel an active transfer and emit a `Cancelled` event.
///
/// 取消一个仍在进行中的传输，并产出 `Cancelled` 事件。
pub struct CancelTransferUseCase {
    store: Arc<dyn FileTransferEventStorePort>,
    publisher: Arc<dyn FileTransferEventPublisherPort>,
}

impl CancelTransferUseCase {
    pub fn new(
        store: Arc<dyn FileTransferEventStorePort>,
        publisher: Arc<dyn FileTransferEventPublisherPort>,
    ) -> Self {
        Self { store, publisher }
    }

    pub async fn execute(
        &self,
        input: CancelTransfer,
    ) -> Result<FileTransferEvent, FileTransferApplicationError> {
        let timeline = load_timeline(self.store.as_ref(), &input.transfer_id).await?;
        timeline.ensure_active(&input.transfer_id, &input.peer_id)?;

        let event = FileTransferEvent::cancelled(input.transfer_id, input.peer_id, input.reason);
        persist_and_publish(self.store.as_ref(), self.publisher.as_ref(), event).await
    }
}

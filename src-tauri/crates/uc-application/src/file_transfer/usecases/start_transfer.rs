use std::sync::Arc;

use uc_core::file_transfer::{FileTransferEventPublisherPort, FileTransferEventStorePort};
use uc_core::FileTransferEvent;

use crate::file_transfer::errors::FileTransferApplicationError;
use crate::file_transfer::timeline::{load_timeline, persist_and_publish};

/// Input for starting a transfer.
///
/// 启动传输时的应用层输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartTransfer {
    pub transfer_id: String,
    pub peer_id: String,
    pub filename: String,
    pub file_size: Option<u64>,
}

/// Start a new transfer and emit a `Started` event.
///
/// 启动一个新的传输，并产出 `Started` 事件。
pub struct StartTransferUseCase {
    store: Arc<dyn FileTransferEventStorePort>,
    publisher: Arc<dyn FileTransferEventPublisherPort>,
}

impl StartTransferUseCase {
    pub fn new(
        store: Arc<dyn FileTransferEventStorePort>,
        publisher: Arc<dyn FileTransferEventPublisherPort>,
    ) -> Self {
        Self { store, publisher }
    }

    pub async fn execute(
        &self,
        input: StartTransfer,
    ) -> Result<FileTransferEvent, FileTransferApplicationError> {
        let timeline = load_timeline(self.store.as_ref(), &input.transfer_id).await?;

        if timeline.started {
            return Err(FileTransferApplicationError::TransferAlreadyStarted {
                transfer_id: input.transfer_id,
            });
        }

        let event = FileTransferEvent::started(
            input.transfer_id,
            input.peer_id,
            input.filename,
            input.file_size,
        );

        persist_and_publish(self.store.as_ref(), self.publisher.as_ref(), event).await
    }
}

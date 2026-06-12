use std::sync::Arc;

use uc_core::file_transfer::{FileTransferEventPublisherPort, FileTransferEventStorePort};
use uc_core::{FileTransferEvent, FileTransferFailureReason};

use crate::file_transfer::errors::FileTransferApplicationError;
use crate::file_transfer::timeline::{load_timeline, persist_and_publish};

/// Input for failing a transfer.
///
/// 标记传输失败时的应用层输入。
///
/// `detail` 承载失败现场的自由文本上下文（例如底层 I/O 错误信息）。
/// 它与类型化的 `reason` 一同传到 UI 层，让用户既看到业务分类，也能看到
/// 具体原因。应用层不解释格式，只负责透传。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailTransfer {
    pub transfer_id: String,
    pub peer_id: String,
    pub reason: FileTransferFailureReason,
    pub detail: Option<String>,
}

/// Mark a transfer as failed and emit a `Failed` event.
///
/// 将传输标记为失败，并产出 `Failed` 事件。
pub struct FailTransferUseCase {
    store: Arc<dyn FileTransferEventStorePort>,
    publisher: Arc<dyn FileTransferEventPublisherPort>,
}

impl FailTransferUseCase {
    pub fn new(
        store: Arc<dyn FileTransferEventStorePort>,
        publisher: Arc<dyn FileTransferEventPublisherPort>,
    ) -> Self {
        Self { store, publisher }
    }

    pub async fn execute(
        &self,
        input: FailTransfer,
    ) -> Result<FileTransferEvent, FileTransferApplicationError> {
        let timeline = load_timeline(self.store.as_ref(), &input.transfer_id).await?;
        timeline.ensure_active(&input.transfer_id, &input.peer_id)?;

        let event =
            FileTransferEvent::failed(input.transfer_id, input.peer_id, input.reason, input.detail);
        persist_and_publish(self.store.as_ref(), self.publisher.as_ref(), event).await
    }
}

use std::sync::Arc;

use tracing::instrument;
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

    // 跨设备可观测性(PR2):文件传输是用户感知最强的"跨设备动作"之一,
    // 但启动入口此前完全没有 tracing instrumentation —— Sentry 上看不到
    // "谁向谁传了什么文件,什么时候开始的",失败时只能靠下游错误日志倒推。
    // 这条 #[instrument] 把 `transfer.id` / `peer.device_id` /
    // `transfer.bytes_total` / `flow.kind` 钉到 root span,后续的
    // ReportProgress / Fail / Cancel use case 共享同一个 `transfer.id`
    // 上下文,Sentry 上一个传输的完整时间线一目了然。
    //
    // `transfer.id` 直接复用业务 ID(`input.transfer_id`),它已经是全
    // wire 唯一的相关 ID —— 跨设备 join 时不需要再独立生成 `flow.id`。
    #[instrument(
        name = "file_transfer.start",
        skip_all,
        fields(
            transfer.id = %input.transfer_id,
            peer.device_id = %input.peer_id,
            transfer.bytes_total = input.file_size.unwrap_or(0),
            flow.kind = "file_transfer",
        ),
    )]
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

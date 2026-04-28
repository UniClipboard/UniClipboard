use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use bytes::Bytes;
use tracing::warn;

use uc_core::file_transfer::FileTransferDirection;
use uc_core::ids::EntryId;
use uc_core::ports::blob::{
    BlobDigest, BlobProgressSink, BlobReferenceRepositoryPort, BlobTicket, BlobTransferPort,
    PlaintextHash,
};
use uc_core::ports::ContentHashPort;

use crate::facade::host_event::{HostEvent, HostEventEmitterPort, TransferHostEvent};
use crate::usecases::blob_transfer::{
    FetchBlobInput, FetchBlobUseCase, PublishBlobInput, PublishBlobUseCase,
};

/// 共享的 host event emitter cell。
///
/// daemon 启动早期注入真实 emitter,在此之前事件会落到 noop 实现上,
/// 与 `FileTransferHostEventPublisher` 共用同一个 cell,保证启动顺序无关。
pub type SharedHostEventEmitter = Arc<RwLock<Arc<dyn HostEventEmitterPort>>>;

pub struct BlobTransferDeps {
    pub hash: Arc<dyn ContentHashPort>,
    pub blob_transfer: Arc<dyn BlobTransferPort>,
    pub blob_reference: Arc<dyn BlobReferenceRepositoryPort>,
    /// 可选 host event emitter。提供时,带 `transfer_context` 的 fetch_blob
    /// 会发出 status_changed + progress 事件;不提供则 fetch_blob 退化为静默拉取。
    pub host_event_emitter: Option<SharedHostEventEmitter>,
}

#[derive(Debug, Clone)]
pub struct PublishBlobCommand {
    pub plaintext: Bytes,
    pub entry_id: Option<EntryId>,
}

#[derive(Debug, Clone)]
pub struct PublishBlobResult {
    pub ticket: BlobTicket,
    pub entry_id: EntryId,
    pub plaintext_hash: PlaintextHash,
    pub digest: BlobDigest,
    pub reused_existing: bool,
}

/// fetch_blob 期间向上报告进度时所需的传输上下文。
///
/// 由调用方提供:
/// - `transfer_id` 通常等于 entry_id(每个 entry 一次传输);
/// - `peer_id` 是来源设备 ID,前端用它做"来自谁"的展示;
/// - `total_bytes` 来自 V3 envelope 的 advertised size,用于前端进度百分比与 ETA。
///
/// 不提供 transfer_context 时 fetch_blob 表现等同于改造前——只拉数据,不发事件。
#[derive(Debug, Clone)]
pub struct FetchTransferContext {
    pub transfer_id: String,
    pub peer_id: String,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct FetchBlobCommand {
    pub ticket: BlobTicket,
    pub entry_id: EntryId,
    /// Some 时 fetch_blob 会发出 status_changed + progress host events;
    /// None 时退化为静默拉取(用于不需要 UI 反馈的内部场景,例如 CLI 工具)。
    pub transfer_context: Option<FetchTransferContext>,
}

#[derive(Debug, Clone)]
pub struct FetchBlobResult {
    pub plaintext: Bytes,
    pub entry_id: EntryId,
    pub plaintext_hash: PlaintextHash,
    pub digest: BlobDigest,
}

#[derive(Debug, thiserror::Error)]
pub enum BlobTransferError {
    #[error("publish blob failed: {0}")]
    Publish(String),
    #[error("fetch blob failed: {0}")]
    Fetch(String),
}

pub struct BlobTransferFacade {
    publish_uc: Arc<PublishBlobUseCase>,
    fetch_uc: Arc<FetchBlobUseCase>,
    host_event_emitter: Option<SharedHostEventEmitter>,
}

impl BlobTransferFacade {
    pub fn new(deps: BlobTransferDeps) -> Self {
        let publish_uc = Arc::new(PublishBlobUseCase::new(
            Arc::clone(&deps.hash),
            Arc::clone(&deps.blob_transfer),
            Arc::clone(&deps.blob_reference),
        ));
        let fetch_uc = Arc::new(FetchBlobUseCase::new(
            deps.hash,
            deps.blob_transfer,
            deps.blob_reference,
        ));
        Self {
            publish_uc,
            fetch_uc,
            host_event_emitter: deps.host_event_emitter,
        }
    }

    fn emit_host_event(&self, event: HostEvent) {
        let Some(cell) = self.host_event_emitter.as_ref() else {
            return;
        };
        let emitter = cell.read().unwrap_or_else(|p| p.into_inner()).clone();
        if let Err(err) = emitter.emit(event) {
            warn!(error = %err, "blob fetch: failed to emit host event");
        }
    }

    fn emit_status_changed(
        &self,
        ctx: &FetchTransferContext,
        entry_id: &EntryId,
        status: &'static str,
        reason: Option<String>,
    ) {
        self.emit_host_event(HostEvent::Transfer(TransferHostEvent::StatusChanged {
            transfer_id: ctx.transfer_id.clone(),
            entry_id: entry_id.as_ref().to_string(),
            status: status.to_string(),
            reason,
        }));
    }

    fn emit_progress(
        &self,
        ctx: &FetchTransferContext,
        entry_id: &EntryId,
        bytes_transferred: u64,
        total_bytes: Option<u64>,
    ) {
        self.emit_host_event(HostEvent::Transfer(TransferHostEvent::Progress {
            transfer_id: ctx.transfer_id.clone(),
            entry_id: Some(entry_id.as_ref().to_string()),
            peer_id: ctx.peer_id.clone(),
            direction: FileTransferDirection::Receiving,
            bytes_transferred,
            total_bytes,
        }));
    }

    pub async fn publish_blob(
        &self,
        command: PublishBlobCommand,
    ) -> Result<PublishBlobResult, BlobTransferError> {
        let outcome = self
            .publish_uc
            .execute(PublishBlobInput {
                plaintext: command.plaintext,
                entry_id: command.entry_id.unwrap_or_default(),
            })
            .await
            .map_err(|e| BlobTransferError::Publish(e.to_string()))?;
        Ok(PublishBlobResult {
            ticket: outcome.ticket,
            entry_id: outcome.entry_id,
            plaintext_hash: outcome.plaintext_hash,
            digest: outcome.digest,
            reused_existing: outcome.reused_existing,
        })
    }

    pub async fn fetch_blob(
        &self,
        command: FetchBlobCommand,
    ) -> Result<FetchBlobResult, BlobTransferError> {
        let entry_id = command.entry_id.clone();
        let progress_sink: Option<Arc<dyn BlobProgressSink>> = command
            .transfer_context
            .as_ref()
            .filter(|_| self.host_event_emitter.is_some())
            .map(|ctx| {
                let sink: Arc<dyn BlobProgressSink> = Arc::new(HostEventProgressSink {
                    emitter_cell: self.host_event_emitter.clone().unwrap(),
                    transfer_id: ctx.transfer_id.clone(),
                    entry_id: entry_id.as_ref().to_string(),
                    peer_id: ctx.peer_id.clone(),
                    fallback_total: ctx.total_bytes,
                });
                sink
            });

        // 发出 'transferring' 状态 + 0 字节 progress,让前端立刻显示进度条
        // (即便 adapter 命中本地缓存也会发: completed 事件会马上覆盖,
        // 不会让 UI 出现"卡在 0%")。
        if let Some(ctx) = command.transfer_context.as_ref() {
            self.emit_status_changed(ctx, &entry_id, "transferring", None);
            self.emit_progress(ctx, &entry_id, 0, ctx.total_bytes);
        }

        let result = self
            .fetch_uc
            .execute(FetchBlobInput {
                ticket: command.ticket,
                entry_id: entry_id.clone(),
                progress: progress_sink,
            })
            .await;

        match result {
            Ok(outcome) => {
                if let Some(ctx) = command.transfer_context.as_ref() {
                    let final_size = outcome.plaintext.len() as u64;
                    let total = ctx.total_bytes.or(Some(final_size));
                    self.emit_progress(ctx, &entry_id, final_size, total);
                    self.emit_status_changed(ctx, &entry_id, "completed", None);
                }
                Ok(FetchBlobResult {
                    plaintext: outcome.plaintext,
                    entry_id: outcome.entry_id,
                    plaintext_hash: outcome.plaintext_hash,
                    digest: outcome.digest,
                })
            }
            Err(e) => {
                let msg = e.to_string();
                if let Some(ctx) = command.transfer_context.as_ref() {
                    self.emit_status_changed(ctx, &entry_id, "failed", Some(msg.clone()));
                }
                Err(BlobTransferError::Fetch(msg))
            }
        }
    }
}

/// 把 adapter 字节级进度上报转发为 host event 的 sink 实现。
///
/// adapter 已经做了字节阈值/时间窗节流,这里只负责把每次回调翻译成
/// `TransferHostEvent::Progress`,并填充上下文字段(transfer_id / entry_id /
/// peer_id / direction)。`fallback_total` 用于补全 adapter 不知道总大小
/// (`total_bytes == None`)的场景——iroh 拉取过程中 size 通常要等到
/// PartComplete 才已知,所以前端的进度百分比依赖这个 fallback。
struct HostEventProgressSink {
    emitter_cell: SharedHostEventEmitter,
    transfer_id: String,
    entry_id: String,
    peer_id: String,
    fallback_total: Option<u64>,
}

#[async_trait]
impl BlobProgressSink for HostEventProgressSink {
    async fn report(&self, bytes_transferred: u64, total_bytes: Option<u64>) {
        let total = total_bytes.or(self.fallback_total);
        let event = HostEvent::Transfer(TransferHostEvent::Progress {
            transfer_id: self.transfer_id.clone(),
            entry_id: Some(self.entry_id.clone()),
            peer_id: self.peer_id.clone(),
            direction: FileTransferDirection::Receiving,
            bytes_transferred,
            total_bytes: total,
        });
        let emitter = self
            .emitter_cell
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Err(err) = emitter.emit(event) {
            warn!(error = %err, "blob fetch: failed to emit progress event");
        }
    }
}

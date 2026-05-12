use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use bytes::Bytes;
use tracing::warn;

use uc_core::file_transfer::{
    FileTransferDirection, FileTransferFailureReason, OutboundProgressReporterPort,
    OutboundProgressStatus,
};
use uc_core::ids::{DeviceId, EntryId};
use uc_core::ports::blob::{
    BlobDigest, BlobProgressSink, BlobReferenceRepositoryPort, BlobTicket, BlobTransferPort,
    PlaintextHash,
};
use uc_core::ports::ContentHashPort;

use crate::facade::file_transfer::{
    CompleteTransfer, FailTransfer, FileTransferFacade, SeedReceiverContext, StartTransfer,
};
use crate::facade::host_event::{HostEvent, HostEventEmitterPort, TransferHostEvent};
use crate::usecases::blob_transfer::{
    FetchBlobInput, FetchBlobPathInput, FetchBlobUseCase, PublishBlobInput, PublishBlobUseCase,
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
    /// 会发出 progress 事件;不提供则 fetch_blob 退化为静默拉取。
    /// 状态变更(transferring / completed / failed)统一通过
    /// [`FileTransferFacade`] 走 lifecycle,不再由本 facade 直发。
    pub host_event_emitter: Option<SharedHostEventEmitter>,
    /// 可选反向进度上报端口。提供时,fetch_blob 会在每次本地进度回调上额外
    /// 推一帧给数据来源端(sender),让 sender UI 能实时展示对端接收字节进度。
    /// 不提供则 fetch_blob 退化为只发本地 host event。
    pub outbound_progress_reporter: Option<Arc<dyn OutboundProgressReporterPort>>,
    /// 可选 file-transfer lifecycle facade。提供时,带 `transfer_context` 的
    /// fetch_blob / fetch_blob_to_path 会调 `start` / `complete` / `fail`
    /// 让事件落进 file_transfer 表与 domain timeline,前端 status 切换
    /// 由 `FileTransferHostEventPublisher` 统一发出。不提供则状态变更被
    /// 静默忽略(用于不需要 lifecycle 跟踪的内部场景,例如 CLI 工具)。
    pub file_transfer: Option<Arc<FileTransferFacade>>,
}

#[derive(Debug, Clone)]
pub struct PublishBlobCommand {
    pub plaintext: Bytes,
    pub entry_id: Option<EntryId>,
}

/// 用磁盘文件路径作为 blob 来源 publish。GH#487 P1: 让 outbound 的大文件
/// 走 iroh-blobs `add_path` 流式入库,避免 `tokio::fs::read` 把整文件读到
/// 内存,把 1GB 文件 publish 期间的 RSS 峰值从 ~2GB 降到 chunk 量级。
///
/// 与 [`PublishBlobCommand`] 在协议层等价(产出同样的 [`PublishBlobResult`]),
/// 但内存/IO 行为不同:`PublishBlobCommand` 适合已经在内存里的小 inline
/// payload(剪贴板文本扩展 rep / 小图);`PublishBlobPathCommand` 适合磁盘
/// 上的大文件 outbound。
#[derive(Debug, Clone)]
pub struct PublishBlobPathCommand {
    pub path: PathBuf,
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
/// - `transfer_id`: 接收端协议层这次传输的唯一关联 key。本工程的入站
///   pipeline 约定 `transfer_id == receiver_entry_id`(`ApplyInbound`
///   在流程入口预生成,贯穿占位卡片 → progress → `clipboard.new_content`),
///   所以 host event 里的 `transfer_id` 和 `entry_id` 字段最终发出的
///   是**同一个值**。前端 `useTransferProgress` 用它定位 UI,
///   `entryStatusById` 用它做 list row 关联。这两个字段在协议层职责
///   不同,但接收端值相等,是有意为之的对齐(避免前端做映射)。
/// - `peer_id` 是来源设备 ID,前端用它做"来自谁"的展示;
/// - `total_bytes` 来自 V3 envelope 的 advertised size,用于前端进度百分比与 ETA。
/// - `filename` 是 receiver-side projection 已经 seed 好的真实文件名;
///   `BlobTransferFacade::fetch_*` 调 `FileTransferFacade::start` 时会把
///   它一起塞进 `Started` 事件,projection apply 时会把这个值原样
///   写回 `file_transfer.filename`(避免覆盖 seed 时填入的真实值)。
///   rep-bound blob / 没有显式文件名的场景填空字符串。
/// - `outbound_transfer_id` / `outbound_target`:可选的反向上报上下文。
///   设置时,sink 在每次进度回调上会通过 `OutboundProgressReporterPort`
///   把 (bytes, total, status) 推回数据来源端(sender),让 sender UI
///   实时显示对端接收字节进度。两个字段必须成对设置:
///   * `outbound_transfer_id` 是 sender 端的 entry_id(来自 V3 envelope
///     `blob_refs[i].entry_id`),sender 端用它索引本地 entry。
///   * `outbound_target` 是 sender 的 DeviceId,reporter 用它定位反向
///     连接目标(同 `peer_id` 的语义,但强类型化以避免重复字符串解析)。
///   只设置一个会被忽略 —— sink 拒绝出向上报。
///
/// 不提供 transfer_context 时 fetch_blob 表现等同于改造前——只拉数据,不发事件。
#[derive(Debug, Clone)]
pub struct FetchTransferContext {
    pub transfer_id: String,
    pub peer_id: String,
    pub total_bytes: Option<u64>,
    pub filename: String,
    pub outbound_transfer_id: Option<String>,
    pub outbound_target: Option<DeviceId>,
}

#[derive(Debug, Clone)]
pub struct FetchBlobCommand {
    pub ticket: BlobTicket,
    /// **发送端**侧的 entry_id —— 仅用于 iroh blob tag 与 fetch use case
    /// 内部记录,不会出现在 host event 里。前端关联用的是
    /// `transfer_context.transfer_id`(== receiver_entry_id)。
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

/// Streaming counterpart of [`FetchBlobCommand`] — the blob lands on disk
/// at `target_path` instead of being returned as `Bytes`. GH#487 Phase 2.
///
/// Used by the inbound materializer for free-standing files so the
/// receive side stops materialising 800 MB+ payloads in memory only to
/// `tokio::fs::write` them out again.
#[derive(Debug, Clone)]
pub struct FetchBlobToPathCommand {
    pub ticket: BlobTicket,
    pub entry_id: EntryId,
    pub target_path: PathBuf,
    pub transfer_context: Option<FetchTransferContext>,
}

#[derive(Debug, Clone)]
pub struct FetchBlobToPathResult {
    pub entry_id: EntryId,
    pub plaintext_hash: PlaintextHash,
    pub digest: BlobDigest,
    /// Final file size on disk (from `tokio::fs::metadata` after the
    /// streaming export completed). Useful for callers that didn't get a
    /// `total_bytes` from the protocol layer and want to log the real size.
    pub bytes_written: u64,
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
    outbound_progress_reporter: Option<Arc<dyn OutboundProgressReporterPort>>,
    file_transfer: Option<Arc<FileTransferFacade>>,
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
            outbound_progress_reporter: deps.outbound_progress_reporter,
            file_transfer: deps.file_transfer,
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

    /// 发一帧 receiving-direction Progress host event。
    ///
    /// fetch 入口的"0 字节起始帧"和 fetch 收尾的"final-size 帧"显式
    /// 通过这条路径发——`HostEventProgressSink` 已经做了字节阈值/时间窗
    /// 节流,通常不会刚好落在 0 字节起点和最后一个字节,所以这两帧由
    /// facade 主路径直接补,确保前端进度条立刻显示和最终停在 100%。
    /// `entry_id` 字段直接复用 `ctx.transfer_id`(协议约定 == receiver_entry_id)。
    fn emit_progress(
        &self,
        ctx: &FetchTransferContext,
        bytes_transferred: u64,
        total_bytes: Option<u64>,
    ) {
        self.emit_host_event(HostEvent::Transfer(TransferHostEvent::Progress {
            transfer_id: ctx.transfer_id.clone(),
            entry_id: Some(ctx.transfer_id.clone()),
            peer_id: ctx.peer_id.clone(),
            direction: FileTransferDirection::Receiving,
            bytes_transferred,
            total_bytes,
        }));
    }

    /// 在 receiver-side projection 表里 upsert 一条 `pending` 行,让
    /// `FileTransferHostEventPublisher::resolve_entry_id` 能在后续
    /// `Started` / `Completed` / `Failed` 事件里查到 entry_id 把
    /// `StatusChanged` host event 发出去。iroh 路径里
    /// `transfer_id == receiver_entry_id`,所以两个字段填同一个值。
    /// `cached_path` 仅 fetch_blob_to_path 路径有意义(blob 落盘的目标
    /// 路径);fetch_blob 写回 representation bytes,留空。
    async fn seed_lifecycle(&self, ctx: &FetchTransferContext, cached_path: String) {
        let Some(facade) = self.file_transfer.as_ref() else {
            return;
        };
        if let Err(err) = facade
            .seed_receiver_context(SeedReceiverContext {
                transfer_id: ctx.transfer_id.clone(),
                entry_id: ctx.transfer_id.clone(),
                origin_device_id: ctx.peer_id.clone(),
                filename: ctx.filename.clone(),
                cached_path,
            })
            .await
        {
            warn!(
                transfer_id = %ctx.transfer_id,
                error = %err,
                "blob fetch: seed receiver context failed"
            );
        }
    }

    /// 调 `FileTransferFacade::start` 让 `Started` 事件落进 store。
    /// 失败时只 warn,不让 fetch 主路径感知—— lifecycle 错误不应该
    /// 阻塞 blob 拉取本身,projection 后续 sweep / reconcile 会继续兜底。
    async fn start_lifecycle(&self, ctx: &FetchTransferContext) {
        let Some(facade) = self.file_transfer.as_ref() else {
            return;
        };
        if let Err(err) = facade
            .start(StartTransfer {
                transfer_id: ctx.transfer_id.clone(),
                peer_id: ctx.peer_id.clone(),
                filename: ctx.filename.clone(),
                file_size: ctx.total_bytes,
            })
            .await
        {
            warn!(
                transfer_id = %ctx.transfer_id,
                error = %err,
                "blob fetch: start lifecycle failed"
            );
        }
    }

    async fn complete_lifecycle(&self, ctx: &FetchTransferContext) {
        let Some(facade) = self.file_transfer.as_ref() else {
            return;
        };
        if let Err(err) = facade
            .complete(CompleteTransfer {
                transfer_id: ctx.transfer_id.clone(),
                peer_id: ctx.peer_id.clone(),
            })
            .await
        {
            warn!(
                transfer_id = %ctx.transfer_id,
                error = %err,
                "blob fetch: complete lifecycle failed"
            );
        }
    }

    async fn fail_lifecycle(&self, ctx: &FetchTransferContext, detail: String) {
        let Some(facade) = self.file_transfer.as_ref() else {
            return;
        };
        if let Err(err) = facade
            .fail(FailTransfer {
                transfer_id: ctx.transfer_id.clone(),
                peer_id: ctx.peer_id.clone(),
                reason: FileTransferFailureReason::Unknown,
                detail: Some(detail),
            })
            .await
        {
            warn!(
                transfer_id = %ctx.transfer_id,
                error = %err,
                "blob fetch: fail lifecycle failed"
            );
        }
    }

    pub async fn publish_blob(
        &self,
        command: PublishBlobCommand,
    ) -> Result<PublishBlobResult, BlobTransferError> {
        let outcome = self
            .publish_uc
            .execute(PublishBlobInput::Plaintext {
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

    /// 流式 publish:从磁盘文件读取并入库,避免把整文件加载到内存。GH#487 P1。
    pub async fn publish_blob_path(
        &self,
        command: PublishBlobPathCommand,
    ) -> Result<PublishBlobResult, BlobTransferError> {
        let outcome = self
            .publish_uc
            .execute(PublishBlobInput::Path {
                path: command.path,
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
        let iroh_tag_entry_id = command.entry_id.clone();
        let progress_sink: Option<Arc<dyn BlobProgressSink>> = command
            .transfer_context
            .as_ref()
            .filter(|_| self.host_event_emitter.is_some())
            .map(|ctx| {
                let outbound = match (
                    self.outbound_progress_reporter.clone(),
                    ctx.outbound_transfer_id.clone(),
                    ctx.outbound_target.clone(),
                ) {
                    (Some(reporter), Some(tid), Some(target)) => Some(OutboundReportContext {
                        reporter,
                        transfer_id: tid,
                        target,
                    }),
                    _ => None,
                };
                let sink: Arc<dyn BlobProgressSink> = Arc::new(HostEventProgressSink {
                    emitter_cell: self.host_event_emitter.clone().unwrap(),
                    transfer_id: ctx.transfer_id.clone(),
                    peer_id: ctx.peer_id.clone(),
                    fallback_total: ctx.total_bytes,
                    outbound,
                });
                sink
            });

        // seed 让 receiver projection 先有一行 pending,publisher 后续
        // 发 `StatusChanged` 时才能 resolve 出 entry_id。fetch_blob 写回
        // representation bytes,blob 不落本地文件,所以 cached_path 留空。
        // start 让 lifecycle 落 `Started` 事件 + projection 行翻成
        // `transferring`,publisher 据此发 `StatusChanged transferring`;
        // 紧跟一帧 0 字节 Progress 是 sink 节流的兜底——即便 adapter 命中
        // 本地缓存瞬间完成,前端也能先看到进度条 placeholder,后续 completed
        // 事件再覆盖。
        if let Some(ctx) = command.transfer_context.as_ref() {
            self.seed_lifecycle(ctx, String::new()).await;
            self.start_lifecycle(ctx).await;
            self.emit_progress(ctx, 0, ctx.total_bytes);
        }

        let result = self
            .fetch_uc
            .execute(FetchBlobInput {
                ticket: command.ticket,
                entry_id: iroh_tag_entry_id,
                progress: progress_sink,
            })
            .await;

        match result {
            Ok(outcome) => {
                if let Some(ctx) = command.transfer_context.as_ref() {
                    let final_size = outcome.plaintext.len() as u64;
                    let total = ctx.total_bytes.or(Some(final_size));
                    // 进度回调 throttle 通常不会刚好落在最后一个字节,
                    // 所以 final-size 帧由 facade 显式推一次,确保前端
                    // 进度条停在 100%;然后 lifecycle complete 让 publisher
                    // 发 `StatusChanged completed`。
                    self.emit_progress(ctx, final_size, total);
                    self.complete_lifecycle(ctx).await;
                    // 把"传输完成"也通知 sender —— 进度回调 throttle 通常
                    // 不会刚好落在最后一个字节,所以最终一帧由 facade 显式
                    // 推送,确保 sender UI 看到 100%。
                    self.report_outbound_terminal(
                        ctx,
                        final_size,
                        total,
                        OutboundProgressStatus::Completed,
                    )
                    .await;
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
                    self.fail_lifecycle(ctx, msg.clone()).await;
                    self.report_outbound_terminal(
                        ctx,
                        0,
                        ctx.total_bytes,
                        OutboundProgressStatus::Failed,
                    )
                    .await;
                }
                Err(BlobTransferError::Fetch(msg))
            }
        }
    }

    /// 流式 fetch:把 blob 从 iroh store 直接 export 到 `target_path`,
    /// 不经过 `Bytes`。GH#487 Phase 2:与 [`fetch_blob`](Self::fetch_blob) 在
    /// progress sink、status events、outbound 反向上报上行为完全一致 ——
    /// 只是不返回 plaintext。inbound materializer 用这个入口避免 800 MB+
    /// 文件先全量进内存再 `tokio::fs::write` 的双写盘问题。
    pub async fn fetch_blob_to_path(
        &self,
        command: FetchBlobToPathCommand,
    ) -> Result<FetchBlobToPathResult, BlobTransferError> {
        let iroh_tag_entry_id = command.entry_id.clone();
        let progress_sink: Option<Arc<dyn BlobProgressSink>> = command
            .transfer_context
            .as_ref()
            .filter(|_| self.host_event_emitter.is_some())
            .map(|ctx| {
                let outbound = match (
                    self.outbound_progress_reporter.clone(),
                    ctx.outbound_transfer_id.clone(),
                    ctx.outbound_target.clone(),
                ) {
                    (Some(reporter), Some(tid), Some(target)) => Some(OutboundReportContext {
                        reporter,
                        transfer_id: tid,
                        target,
                    }),
                    _ => None,
                };
                let sink: Arc<dyn BlobProgressSink> = Arc::new(HostEventProgressSink {
                    emitter_cell: self.host_event_emitter.clone().unwrap(),
                    transfer_id: ctx.transfer_id.clone(),
                    peer_id: ctx.peer_id.clone(),
                    fallback_total: ctx.total_bytes,
                    outbound,
                });
                sink
            });

        // seed 用 target_path 当 cached_path —— blob 落盘的实际位置,
        // dashboard `cached_path` 字段直接显示为本地副本路径。
        if let Some(ctx) = command.transfer_context.as_ref() {
            let cached_path = command.target_path.to_string_lossy().into_owned();
            self.seed_lifecycle(ctx, cached_path).await;
            self.start_lifecycle(ctx).await;
            self.emit_progress(ctx, 0, ctx.total_bytes);
        }

        let result = self
            .fetch_uc
            .execute_to_path(FetchBlobPathInput {
                ticket: command.ticket,
                entry_id: iroh_tag_entry_id,
                target_path: command.target_path,
                progress: progress_sink,
            })
            .await;

        match result {
            Ok(outcome) => {
                if let Some(ctx) = command.transfer_context.as_ref() {
                    let final_size = outcome.bytes_written;
                    let total = ctx.total_bytes.or(Some(final_size));
                    self.emit_progress(ctx, final_size, total);
                    self.complete_lifecycle(ctx).await;
                    self.report_outbound_terminal(
                        ctx,
                        final_size,
                        total,
                        OutboundProgressStatus::Completed,
                    )
                    .await;
                }
                Ok(FetchBlobToPathResult {
                    entry_id: outcome.entry_id,
                    plaintext_hash: outcome.plaintext_hash,
                    digest: outcome.digest,
                    bytes_written: outcome.bytes_written,
                })
            }
            Err(e) => {
                let msg = e.to_string();
                if let Some(ctx) = command.transfer_context.as_ref() {
                    self.fail_lifecycle(ctx, msg.clone()).await;
                    self.report_outbound_terminal(
                        ctx,
                        0,
                        ctx.total_bytes,
                        OutboundProgressStatus::Failed,
                    )
                    .await;
                }
                Err(BlobTransferError::Fetch(msg))
            }
        }
    }

    /// fetch 收尾时把最终状态(Completed/Failed)推回 sender。中间进度
    /// 由 sink 在 adapter 节流回调里推送,这里只补"最后一帧"。
    async fn report_outbound_terminal(
        &self,
        ctx: &FetchTransferContext,
        bytes_transferred: u64,
        total_bytes: Option<u64>,
        status: OutboundProgressStatus,
    ) {
        let (Some(reporter), Some(tid), Some(target)) = (
            self.outbound_progress_reporter.as_ref(),
            ctx.outbound_transfer_id.as_ref(),
            ctx.outbound_target.as_ref(),
        ) else {
            return;
        };
        reporter
            .report(target, tid, bytes_transferred, total_bytes, status)
            .await;
    }
}

/// 反向上报上下文。同时配齐三个字段才会触发 outbound report。
struct OutboundReportContext {
    reporter: Arc<dyn OutboundProgressReporterPort>,
    transfer_id: String,
    target: DeviceId,
}

/// 把 adapter 字节级进度上报转发为 host event 的 sink 实现。
///
/// adapter 已经做了字节阈值/时间窗节流,这里只负责把每次回调翻译成
/// `TransferHostEvent::Progress`,并填充上下文字段(transfer_id /
/// peer_id / direction)。`entry_id` 字段直接复用 `transfer_id`(协议
/// 约定 == receiver_entry_id)。`fallback_total` 用于补全 adapter 不知
/// 道总大小(`total_bytes == None`)的场景——iroh 拉取过程中 size 通
/// 常要等到 PartComplete 才已知,所以前端的进度百分比依赖这个 fallback。
///
/// 同一次回调还会通过 `outbound`(若配置)把 progress 推回数据来源端,
/// 让 sender UI 看到对端真实接收字节进度。reporter 自己会处理失败
/// (内部 log + return),不会让 fetch 主路径感知。
struct HostEventProgressSink {
    emitter_cell: SharedHostEventEmitter,
    transfer_id: String,
    peer_id: String,
    fallback_total: Option<u64>,
    outbound: Option<OutboundReportContext>,
}

#[async_trait]
impl BlobProgressSink for HostEventProgressSink {
    async fn report(&self, bytes_transferred: u64, total_bytes: Option<u64>) {
        let total = total_bytes.or(self.fallback_total);
        let event = HostEvent::Transfer(TransferHostEvent::Progress {
            transfer_id: self.transfer_id.clone(),
            entry_id: Some(self.transfer_id.clone()),
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

        if let Some(ob) = self.outbound.as_ref() {
            ob.reporter
                .report(
                    &ob.target,
                    &ob.transfer_id,
                    bytes_transferred,
                    total,
                    OutboundProgressStatus::InProgress,
                )
                .await;
        }
    }
}

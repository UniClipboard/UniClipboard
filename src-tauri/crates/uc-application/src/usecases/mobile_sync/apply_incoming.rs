//! `ApplyIncomingMobileClipUseCase` —— mobile sync 入站剪贴板的应用层入口。
//!
//! 把 iPhone 经 SyncClipboard 协议 (`PUT /SyncClipboard.json` +
//! `PUT /file/{name}`) 上传的 text / image / file 翻成项目内部的
//! [`SystemClipboardSnapshot`], 编码成 V3 envelope, 喂给已有的
//! [`ApplyInboundClipboardUseCase`] 复用整套入站管线 ——
//! dedup / capture / OS 写回 + 60s 写回环防御。
//!
//! ## 两步 PUT 协议
//!
//! SyncClipboard EX 在上传非纯文本时, 客户端先 `PUT /file/{dataName}` 把
//! 字节 buffer 上来, 再 `PUT /SyncClipboard.json` 提交元数据(含 `dataName`
//! 指向那条 file)。daemon 在第一步必须把字节缓存住 —— 这就是
//! [`IncomingMobileBuffer`] 的角色, 不是 stub, 而是协议必要部件。
//! 第二步触发时再从 buffer 取出, 拼 `SystemClipboardSnapshot` 走 capture。
//!
//! ## v1 范围 (P5a.3 → P5a.3.5)
//!
//! - **Text** ✅ 直接编码成 `text/plain` rep
//! - **Image** ✅ 从 buffer 取 `(mime, bytes)` 拼 `image/*` rep
//! - **File** ✅ (P5a.3.5) 从 buffer 取 `(mime, bytes)` 经
//!   [`MobileFileStagingPort`] 物化到 cache_dir,把得到的 `file:///...` URI
//!   拼成 `text/uri-list` rep(format_id=`files` / mime=`text/uri-list`)
//! - **Group** ⏭ 返回 `DecodeFailed`(SyncClipboard 协议本身保留语义,
//!   shortcut EX 客户端不会发, 不在 v1 实现)
//!
//! ## 写回环防御复用
//!
//! 不需要在本 use case 里做 hash guard / next-origin override —— 这些
//! 在 [`ApplyInboundClipboardUseCase`] 内部 `InboundWrite` (调
//! `ClipboardWriteCoordinator::write(.., RemotePush)`) 已经备齐, 60s
//! 防御链对 mobile sync 入站同样自动生效。
//!
//! ## 命名空间隔离
//!
//! 写回 `clipboard_event.from_device` 的伪 DeviceId 是
//! `mobile_sync:<device_id>` 形态, 让日志 / UI 一眼能区分"这条不是 P2P
//! 来的"。不会与现有 P2P DeviceId(libp2p PeerId 派生)冲突。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use thiserror::Error;
use tracing::{debug, info, instrument, warn};

use uc_core::file_transfer::FileTransferFailureReason;
use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::mobile_sync::{MobileDeviceId, StagedFile};
use uc_core::ports::mobile_sync::{MobileFileStagingError, MobileFileStagingPort};
use uc_core::ports::ClockPort;
use uc_core::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_observability::analytics::{AnalyticsPort, Direction, Event, PayloadSizeBucket};

use crate::facade::file_transfer::{
    CompleteTransfer, FailTransfer, FileTransferFacade, LinkTransferToEntry,
};
use crate::usecases::clipboard_sync::apply_inbound::{
    ApplyInboundClipboardUseCase, ApplyInboundError, ApplyInboundInput, ApplyOutcome,
};
use crate::usecases::clipboard_sync::payload_codec::encode_snapshot_to_v3_bytes;
use crate::usecases::mobile_sync::clipboard_doc::SyncClipboardItemType;

// ─── Fan-out port (use-case-local) ───────────────────────────────────────

/// 移动端入站完成后, 把刚应用到本机的 snapshot 传播给 Space 内其他已配
/// 对设备的能力抽象。
///
/// ## 为什么是 use-case-local trait, 而不是直接持 `Arc<ClipboardOutboundFacade>`
///
/// 设计上 use case 不应该跨层去拿一个 facade(facade 是给外部 crate 看
/// 的对外门面, 不是 use case 的依赖类型)。让本 use case 通过 trait 持
/// 一个最小的领域端口, 而把"调出站 dispatcher / 异步 spawn / 错误降级
/// 成 warn / 编辑日志字段"等所有传输细节封到生产 adapter 里, 换来:
///
/// - **测试可注入**:fake 实现直接 record 调用, use case 单测可断言
///   `Applied` 触发一次、`DuplicateSkipped` / 错误分支不触发, 完全脱
///   离 iroh / blob / outbound dispatcher 整条装配链;
/// - **依赖收口**:use case 的对外依赖仍只盯"自己关心的领域 collaborator"
///   (inbound / staging / file_transfer / clock / fan_out), 不随出站
///   管线演化而膨胀;
/// - **adapter 可独立演化**:未来要加 telemetry / 按 source_device 走
///   不同策略 / 复用到非移动端入口, 都改 adapter, 不动 use case。
///
/// ## 调用契约
///
/// - `fan_out` 是 fire-and-forget:调用立即返回, 真实分发在实现内异步
///   执行。失败由实现层自己消化(典型 `warn!`), 不抛回 use case ——
///   mobile 上传 HTTP 响应只取决于本机入站是否生效, fan-out 是事后传
///   播, 网络出口故障不应倒灌成 4xx/5xx。
/// - 调用方仅在本机入站产生新 entry(`Applied` 分支)时调用一次, 不在
///   `DuplicateSkipped` / `DecodeFailed` / `Err(_)` 分支调用。
/// - `source_device_id` 只用于日志 / telemetry, 实现不应据此做路由决策。
pub(crate) trait MobileInboundFanOutPort: Send + Sync {
    fn fan_out(
        &self,
        entry_id: EntryId,
        snapshot: SystemClipboardSnapshot,
        source_device_id: MobileDeviceId,
    );
}

// ─── Public types (pub(crate) per AGENTS.md §11.4) ──────────────────────

/// Input to [`ApplyIncomingMobileClipUseCase::execute`].
#[derive(Debug, Clone)]
pub struct ApplyIncomingMobileClipInput {
    /// Authenticated mobile device id (registered iPhone). Becomes the
    /// suffix of the pseudo `DeviceId` written into the clipboard event.
    pub source_device_id: MobileDeviceId,
    /// What kind of incoming event this is. See [`IncomingMobileClipEvent`].
    pub event: IncomingMobileClipEvent,
}

/// Two-shape input — one variant per HTTP route that calls into us.
#[derive(Debug, Clone)]
pub enum IncomingMobileClipEvent {
    /// Triggered by `PUT /SyncClipboard.json`. Commits to apply the clip
    /// into the local clipboard (capture + OS write).
    SyncDoc {
        item_type: SyncClipboardItemType,
        /// `meta.text` from SyncClipboard wire schema. For Text type
        /// it carries the actual content; for Image / File it's the
        /// original filename (purely informational, not used by us).
        text: String,
        /// `meta.dataName` from SyncClipboard wire schema. Required
        /// for Image / File types — points to the file-buffer entry.
        data_name: Option<String>,
    },
    /// Triggered by `PUT /file/{data_name}` 已经落盘 staging 之后。
    /// 把"已 staged 文件 + transfer_id"挂进 buffer 等 SyncDoc 配对,返回
    /// `Buffered` outcome —— 路由层应回 HTTP 200。
    ///
    /// 注意:本事件**不再携带字节**。handler 在收 body 期间已经通过
    /// streaming staging API 把字节边收边写到 cache_dir, `staged` 字段携
    /// 带最终的 `file:///...` URI。SyncDoc apply 阶段:
    /// - File 类型直接用 URI 拼 `text/uri-list` rep;
    /// - Image 类型按 URI `read_by_uri` 读回字节内联到 image rep。
    ///
    /// `transfer_id` 由 handler 生成(`mobile-lan:<uuid>` 或 `?upload_id=`
    /// 客户端提供),贯穿到 SyncDoc apply 阶段做 link_transfer_to_entry +
    /// complete;handler 已经在流式收 body 期间发过 `Started` / `Progress`
    /// lifecycle 事件,本 use case 不重复发。
    BufferFile {
        data_name: String,
        mime: String,
        staged: StagedFile,
        transfer_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyIncomingMobileClipOutcome {
    /// New content — persisted via capture + OS clipboard written.
    Applied { entry_id: EntryId },
    /// `content_hash` already exists locally — no persist, no OS write.
    /// Mirrors `ApplyOutcome::DuplicateSkipped`.
    DuplicateSkipped {
        content_hash: String,
        existing_entry_id: EntryId,
    },
    /// Decode-time / contract failure (unsupported type, missing
    /// `data_name`, buffer miss, ...). Routes layer maps to HTTP 400.
    DecodeFailed { reason: String },
    /// Bytes buffered, awaiting sibling `PUT /SyncClipboard.json`.
    /// Routes layer responds HTTP 200 — this is the protocol's expected
    /// shape, not an error.
    Buffered,
}

#[derive(Debug, Error)]
pub enum ApplyIncomingMobileClipError {
    /// Underlying [`ApplyInboundClipboardUseCase`] failed in a way that
    /// is **not** expressible as a domain outcome (DB error, capture
    /// pipeline crash, OS write coord failure).
    #[error("inbound apply failed: {0}")]
    Inbound(#[from] ApplyInboundError),
    /// V3 envelope encode failed.
    #[error("V3 envelope encode failed: {0}")]
    EncodeFailed(String),
    /// Catch-all for use-case-internal logic errors.
    #[error("internal: {0}")]
    Internal(String),
}

/// `build_*_snapshot` 内部错误形态:区分"协议输入不合法"(`Decode`,
/// 上抛 outcome `DecodeFailed`)与"基础设施真出问题"(`Internal`,
/// 上抛 application error `Internal`)。
enum BuildSnapshotFailure {
    Decode(String),
    Internal(String),
}

/// `build_*_snapshot` 成功后的"快照 + transfer_id"组合。
///
/// Text 分支没有 PUT /file 阶段, `transfer_id` 永远是 `None`;
/// Image / File 分支命中 buffer 时取出 buffered.transfer_id, 让 SyncDoc
/// apply 后能 `link_transfer_to_entry` + `complete` 收尾。
struct BuiltSnapshot {
    snapshot: SystemClipboardSnapshot,
    transfer_id: Option<String>,
}

// ─── IncomingMobileBuffer ────────────────────────────────────────────────

const MAX_BUFFERED_FILES: usize = 16;

#[derive(Debug, Clone)]
struct BufferedFile {
    mime: String,
    /// 已经流式落盘的 staging 文件引用。File 类型 SyncDoc apply 时直接
    /// 用 URI 拼 uri-list rep;Image 类型按 URI `read_by_uri` 把字节读回
    /// 内联到 image rep。
    staged: StagedFile,
    /// 协议层 transfer_id —— 由 `PUT /file` handler 在入口处生成
    /// (`mobile-lan:<uuid-v4>` 或客户端通过 `?upload_id=` 提供)。
    /// 让 SyncDoc 阶段拿到真实 entry_id 后能 `link_transfer_to_entry`
    /// + `complete` 这条 lifecycle。
    transfer_id: String,
}

/// In-process staging buffer for `PUT /file/{name}` bytes between the
/// two-step PUT (file → json) protocol.
///
/// **Bounded** by [`MAX_BUFFERED_FILES`] entries — when full and an
/// unseen `data_name` arrives, one existing entry is dropped to keep
/// the bound.
///
/// v1 trade-off: orphaned entries (a `PUT /file` with no follow-up
/// `PUT /json`) leak until the size cap kicks in or daemon restarts.
/// P5a.10 真机回归会确认是否需要加 TTL sweep —— 在那之前, 16 条上限
/// 给"快速连续上传几张图片"留够余量, 又不至于让 OOM 风险窜起来。
pub struct IncomingMobileBuffer {
    inner: Mutex<HashMap<String, BufferedFile>>,
}

impl IncomingMobileBuffer {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn store(&self, data_name: String, mime: String, staged: StagedFile, transfer_id: String) {
        let mut guard = self.inner.lock().unwrap();
        if guard.len() >= MAX_BUFFERED_FILES && !guard.contains_key(&data_name) {
            // Cap reached + new key. Drop one arbitrary existing entry
            // to make room. v1: HashMap iter order; if real-world traffic
            // shows orphan accumulation we'll switch to LRU in P5a.3.5+.
            if let Some(victim) = guard.keys().next().cloned() {
                warn!(
                    victim = %victim,
                    cap = MAX_BUFFERED_FILES,
                    "mobile_sync IncomingMobileBuffer full; dropping oldest entry"
                );
                guard.remove(&victim);
            }
        }
        guard.insert(
            data_name,
            BufferedFile {
                mime,
                staged,
                transfer_id,
            },
        );
    }

    fn take(&self, data_name: &str) -> Option<BufferedFile> {
        self.inner.lock().unwrap().remove(data_name)
    }

    /// 主动删除一个 buffer slot 并把 transfer_id 返回出去。
    ///
    /// 调用方场景:PUT /file 流式接收过程中 body 中断 / 请求被取消,
    /// handler 需要清掉之前 reserved 的 slot 并对外发 `fail` lifecycle。
    /// 不存在时返回 None。
    pub fn remove(&self, data_name: &str) -> Option<String> {
        self.inner
            .lock()
            .unwrap()
            .remove(data_name)
            .map(|file| file.transfer_id)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
}

impl Default for IncomingMobileBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Use Case ────────────────────────────────────────────────────────────

pub(crate) struct ApplyIncomingMobileClipUseCase {
    inbound: Arc<ApplyInboundClipboardUseCase>,
    buffer: Arc<IncomingMobileBuffer>,
    file_staging: Arc<dyn MobileFileStagingPort>,
    clock: Arc<dyn ClockPort>,
    /// 可选 file-transfer lifecycle facade。装配处提供时,SyncDoc apply
    /// 后调 `link_transfer_to_entry` + `complete` 把 mobile_lan 路径生成
    /// 的 transfer 关联到真实 entry_id;`None` 时静默降级(测试装配 / CLI
    /// fallback)。BufferFile 分支由 handler 在收 body 期间已经发过
    /// `Started` / `Progress`,本 use case 不重复发。
    file_transfer: Option<Arc<FileTransferFacade>>,
    /// 可选 fan-out port。装配处提供实现时, SyncDoc apply 成功后(仅
    /// `Applied` 分支)把刚应用到本机的 snapshot 异步传播给 Space 内
    /// 其他已配对设备。
    ///
    /// 本 use case **只关心"调一下 fan_out"** —— 具体走 iroh 直发 / 走
    /// 出站 dispatcher / 走文件 blob 发布 / 大图自动剥成 blob ref / 用户
    /// settings 过滤等所有传输细节, 都封在生产 adapter 里, use case 不感
    /// 知。这样:
    ///
    /// - use case 单测只需 fake 实现 [`MobileInboundFanOutPort`], 不必
    ///   拉真实 dispatcher / blob facade / iroh adapter, 可断言 fan-out
    ///   触发时机与参数;
    /// - 未来扩展(例如同时打点 telemetry、按 source_device 做策略)
    ///   都改 adapter, 不改 use case;
    /// - use case 的对外依赖始终只盯"自己关心的领域 collaborator", 不
    ///   随出站管线演化膨胀。
    ///
    /// ## 仅 `Applied` 分支调用
    ///
    /// `DuplicateSkipped` 命中本机 dedup —— 这条 content_hash 此前已被
    /// 本设备处理过, 上次处理时若已 fan-out 过, 重复广播只会浪费带宽
    /// 并扰乱对端 dedup 时序;`DecodeFailed` / `Err(...)` 表示本机入站
    /// 根本没成功, 没有"已应用的内容"可广播。
    ///
    /// `None` 时静默降级(facade 自测装配 / CLI fallback 等不接出站
    /// 的入口):mobile 上传仅落地本机, 不传播 —— 与本字段引入前的行
    /// 为完全一致, 不退化。
    fan_out: Option<Arc<dyn MobileInboundFanOutPort>>,
    /// schema doc §7.6 / §12.2 P1：iPhone → 桌面剪贴板实际落地的 inbound
    /// 计数。**仅** `SyncDoc` arm 的 `Applied` outcome emit
    /// `MobileClipboardSynced { direction: Inbound, payload_size_bucket }`；
    /// `Buffered` / `DuplicateSkipped` / `DecodeFailed` / `Err` 都不上报，
    /// 沿用 `ClipboardEntryCaptured` 防 RemotePush 双计的红线哲学。
    analytics: Arc<dyn AnalyticsPort>,
}

impl ApplyIncomingMobileClipUseCase {
    pub(crate) fn new(
        inbound: Arc<ApplyInboundClipboardUseCase>,
        buffer: Arc<IncomingMobileBuffer>,
        file_staging: Arc<dyn MobileFileStagingPort>,
        clock: Arc<dyn ClockPort>,
        file_transfer: Option<Arc<FileTransferFacade>>,
        fan_out: Option<Arc<dyn MobileInboundFanOutPort>>,
        analytics: Arc<dyn AnalyticsPort>,
    ) -> Self {
        Self {
            inbound,
            buffer,
            file_staging,
            clock,
            file_transfer,
            fan_out,
            analytics,
        }
    }

    #[instrument(
        name = "mobile_sync.apply_incoming",
        skip_all,
        fields(source_device_id = %input.source_device_id),
    )]
    pub(crate) async fn execute(
        &self,
        input: ApplyIncomingMobileClipInput,
    ) -> Result<ApplyIncomingMobileClipOutcome, ApplyIncomingMobileClipError> {
        match input.event {
            IncomingMobileClipEvent::BufferFile {
                data_name,
                mime,
                staged,
                transfer_id,
            } => {
                let uri = staged.uri.as_str().to_string();
                let sanitized = staged.sanitized_name.clone();
                self.buffer
                    .store(data_name.clone(), mime.clone(), staged, transfer_id.clone());
                info!(
                    data_name = %data_name,
                    mime = %mime,
                    sanitized_name = %sanitized,
                    staged_uri = %uri,
                    transfer_id = %transfer_id,
                    "mobile_sync apply_incoming: buffered staged file"
                );
                Ok(ApplyIncomingMobileClipOutcome::Buffered)
            }
            IncomingMobileClipEvent::SyncDoc {
                item_type,
                text,
                data_name,
            } => {
                let source_device_id = input.source_device_id.clone();
                let build_result: Result<BuiltSnapshot, BuildSnapshotFailure> = match item_type {
                    SyncClipboardItemType::Text => self.build_text_snapshot(text),
                    SyncClipboardItemType::Image => self.build_image_snapshot(data_name).await,
                    SyncClipboardItemType::File => self.build_file_snapshot(data_name),
                    SyncClipboardItemType::Group => Err(BuildSnapshotFailure::Decode(
                        "Group 类型不在 v1 范围内 (SyncClipboard 协议保留)".into(),
                    )),
                };

                let built = match build_result {
                    Ok(s) => s,
                    Err(BuildSnapshotFailure::Decode(reason)) => {
                        warn!(
                            item_type = ?item_type,
                            reason = %reason,
                            "mobile_sync apply_incoming: decode failed"
                        );
                        // decode 失败时 transfer 已经被 handler 起过 lifecycle,
                        // 这里要补一发 fail 把它收尾,免得 sweep 5 min 后才动。
                        // build_*_snapshot 在 decode 失败前不会 `buffer.take`,
                        // 所以原 transfer_id 还在 buffer 里 —— 通过 data_name
                        // 反查不可行(它已经被 move 进入 event 解构)。本 use case
                        // 无法在 decode 路径上拿到 transfer_id,handler 端的
                        // ?upload_id 反向查询是未来增强;现在先靠 sweep 兜底。
                        return Ok(ApplyIncomingMobileClipOutcome::DecodeFailed { reason });
                    }
                    Err(BuildSnapshotFailure::Internal(msg)) => {
                        warn!(
                            item_type = ?item_type,
                            error = %msg,
                            "mobile_sync apply_incoming: internal failure (file staging)"
                        );
                        return Err(ApplyIncomingMobileClipError::Internal(msg));
                    }
                };

                let BuiltSnapshot {
                    snapshot,
                    transfer_id,
                } = built;
                // 仅当本装配真的接了 fan-out port 时才克隆 snapshot ——
                // Image / File 分支的 snapshot 内含完整字节, 无 fan-out
                // 装配的场景(CLI fallback / 单测)不应该白付一份克隆开销。
                let snapshot_for_fanout = self.fan_out.as_ref().map(|_| snapshot.clone());
                // analytics 用：在 dispatch 消费 snapshot 之前把字节总数
                // 取出来，避免在 outcome 分支里再保留一份 snapshot 引用。
                let payload_bytes = snapshot.total_size_bytes().max(0) as u64;
                let dispatch_outcome = self
                    .dispatch_inbound(source_device_id.clone(), snapshot)
                    .await;
                self.maybe_emit_inbound_synced(&dispatch_outcome, payload_bytes);
                self.maybe_fan_out_to_paired_peers(
                    &source_device_id,
                    &dispatch_outcome,
                    snapshot_for_fanout,
                );
                self.finalize_transfer_lifecycle(transfer_id, source_device_id, &dispatch_outcome)
                    .await;
                dispatch_outcome
            }
        }
    }

    /// schema doc §7.6 / §12.2 P1：iPhone → 桌面剪贴板实际落地 inbound。
    ///
    /// **仅** `Applied` outcome emit；`Buffered` / `DuplicateSkipped` /
    /// `DecodeFailed` / `Err` 全不上报：
    ///
    /// - `DuplicateSkipped` 命中本机 dedup——内容此前已存在，重复埋点会
    ///   让 dashboard 频率口径双计（沿用 `ClipboardEntryCaptured` RemotePush
    ///   红线哲学）。
    /// - `Buffered` 是文件两步 PUT 协议的中间态，不代表用户可感知的同步。
    /// - `DecodeFailed` / `Err` 本机入站没成功，与产品视角的"sync 成功
    ///   一次"语义不一致。
    fn maybe_emit_inbound_synced(
        &self,
        outcome: &Result<ApplyIncomingMobileClipOutcome, ApplyIncomingMobileClipError>,
        payload_bytes: u64,
    ) {
        if let Ok(ApplyIncomingMobileClipOutcome::Applied { .. }) = outcome {
            self.analytics.capture(Event::MobileClipboardSynced {
                direction: Direction::Inbound,
                payload_size_bucket: PayloadSizeBucket::from_bytes(payload_bytes),
            });
        }
    }

    /// 把刚刚应用到本机的移动端 snapshot 交给 [`MobileInboundFanOutPort`]
    /// 传播到 Space 内其他已配对设备。
    ///
    /// 仅在 `Applied` 分支调用一次:`DuplicateSkipped` 命中本机 dedup
    /// (内容已存在, 不该重复广播);`DecodeFailed` / `Err(...)` 本机
    /// 入站没成功, 没"已应用的内容"可播。`Buffered` 由 `BufferFile`
    /// 分支产生, 不走到这里。
    ///
    /// 传输细节(走 iroh / 大图剥成 blob ref / 文件流式 publish_blob_path
    /// / `OutboundSyncPlanner` settings 过滤 / `tokio::spawn` fire-and-forget
    /// / 失败仅 `warn!`)全部封在生产 adapter 里, 本 use case 不感知 ——
    /// 见 [`MobileInboundFanOutPort`] 设计意图。
    fn maybe_fan_out_to_paired_peers(
        &self,
        source_device_id: &MobileDeviceId,
        dispatch_outcome: &Result<ApplyIncomingMobileClipOutcome, ApplyIncomingMobileClipError>,
        snapshot_for_fanout: Option<SystemClipboardSnapshot>,
    ) {
        let Some(fan_out) = self.fan_out.as_ref() else {
            return;
        };
        let Ok(ApplyIncomingMobileClipOutcome::Applied { entry_id }) = dispatch_outcome else {
            return;
        };
        let Some(snapshot) = snapshot_for_fanout else {
            // fan_out = Some 时上游一定克隆过 snapshot; 走到这里说明上
            // 游 `snapshot_for_fanout` 被错误构造成 None, 属于编程错误。
            // 沉默而不是 panic —— 这条 fan-out 只是"事后传播", 不应让
            // mobile 上传整体失败。
            warn!("mobile_sync fan-out: fan_out wired but snapshot_for_fanout=None, skipping");
            return;
        };
        fan_out.fan_out(entry_id.clone(), snapshot, source_device_id.clone());
    }

    /// SyncDoc apply 完成后把 mobile_lan 路径预先打开的 transfer 关闭。
    ///
    /// - `Applied { entry_id }`:把 transfer 行从占位 entry_id 改挂到真实
    ///   entry_id,然后发 `Completed` 事件。这是 mobile_lan 路径独有的
    ///   "buffered 期间没有 entry_id → SyncDoc apply 后 backfill" 模式。
    /// - `DuplicateSkipped { existing_entry_id }`:重定向到已存在的 entry,
    ///   仍然 complete —— transfer 字节已经收齐, dedup 命中只是没产生新
    ///   entry, 不应让 transfer 永久卡在 transferring。
    /// - `DecodeFailed`:几乎不可能 (我们刚 encode 出来的 envelope),但若
    ///   发生应 fail —— 否则 transfer 也会卡在 transferring。
    /// - 应用层错误 (`Err(...)`):capture / write 链路真出问题, fail。
    async fn finalize_transfer_lifecycle(
        &self,
        transfer_id: Option<String>,
        source_device_id: MobileDeviceId,
        dispatch: &Result<ApplyIncomingMobileClipOutcome, ApplyIncomingMobileClipError>,
    ) {
        let Some(facade) = self.file_transfer.as_ref() else {
            return;
        };
        let Some(transfer_id) = transfer_id else {
            return;
        };
        let peer_id = format!("mobile:{}", source_device_id);
        match dispatch {
            Ok(ApplyIncomingMobileClipOutcome::Applied { entry_id }) => {
                self.link_then_complete(facade, &transfer_id, entry_id.as_ref(), &peer_id)
                    .await;
            }
            Ok(ApplyIncomingMobileClipOutcome::DuplicateSkipped {
                existing_entry_id, ..
            }) => {
                self.link_then_complete(facade, &transfer_id, existing_entry_id.as_ref(), &peer_id)
                    .await;
            }
            Ok(ApplyIncomingMobileClipOutcome::DecodeFailed { reason }) => {
                self.fail_transfer(facade, &transfer_id, &peer_id, reason.clone())
                    .await;
            }
            Ok(ApplyIncomingMobileClipOutcome::Buffered) => {
                // SyncDoc 路径不会产 Buffered;若 dispatch 真返 Buffered 是
                // bug 但不影响 lifecycle 状态。沉默即可。
            }
            Err(err) => {
                self.fail_transfer(facade, &transfer_id, &peer_id, err.to_string())
                    .await;
            }
        }
    }

    async fn link_then_complete(
        &self,
        facade: &FileTransferFacade,
        transfer_id: &str,
        entry_id: &str,
        peer_id: &str,
    ) {
        if let Err(err) = facade
            .link_transfer_to_entry(LinkTransferToEntry {
                transfer_id: transfer_id.to_string(),
                entry_id: entry_id.to_string(),
            })
            .await
        {
            warn!(
                transfer_id,
                error = %err,
                "mobile_sync apply_incoming: link_transfer_to_entry failed"
            );
        }
        if let Err(err) = facade
            .complete(CompleteTransfer {
                transfer_id: transfer_id.to_string(),
                peer_id: peer_id.to_string(),
            })
            .await
        {
            warn!(
                transfer_id,
                error = %err,
                "mobile_sync apply_incoming: complete lifecycle failed"
            );
        }
    }

    async fn fail_transfer(
        &self,
        facade: &FileTransferFacade,
        transfer_id: &str,
        peer_id: &str,
        detail: String,
    ) {
        if let Err(err) = facade
            .fail(FailTransfer {
                transfer_id: transfer_id.to_string(),
                peer_id: peer_id.to_string(),
                reason: FileTransferFailureReason::Unknown,
                detail: Some(detail),
            })
            .await
        {
            warn!(
                transfer_id,
                error = %err,
                "mobile_sync apply_incoming: fail lifecycle failed"
            );
        }
    }

    fn build_text_snapshot(&self, text: String) -> Result<BuiltSnapshot, BuildSnapshotFailure> {
        if text.is_empty() {
            return Err(BuildSnapshotFailure::Decode(
                "Text item with empty body".into(),
            ));
        }
        let bytes = text.into_bytes();
        let rep = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            bytes,
        );
        Ok(BuiltSnapshot {
            snapshot: SystemClipboardSnapshot {
                ts_ms: self.clock.now_ms(),
                representations: vec![rep],
            },
            transfer_id: None,
        })
    }

    async fn build_image_snapshot(
        &self,
        data_name: Option<String>,
    ) -> Result<BuiltSnapshot, BuildSnapshotFailure> {
        let name = data_name
            .ok_or_else(|| BuildSnapshotFailure::Decode("Image item without dataName".into()))?;
        let buffered = self.buffer.take(&name).ok_or_else(|| {
            BuildSnapshotFailure::Decode(format!(
                "file buffer miss for `{}` (PUT /file may have arrived late or never)",
                name
            ))
        })?;
        let transfer_id = buffered.transfer_id.clone();
        // PUT /file 阶段已经把字节流式落盘到 staging,这里按 URI 把字节读
        // 回来 —— image rep 标准形态是字节内联(系统剪贴板 image type 要求
        // rep 持字节),不能像 file 分支那样只挂 URI。来回一次盘换 PUT /file
        // 阶段不再吃满内存 buffer。
        let bytes = self
            .file_staging
            .read_by_uri(buffered.staged.uri.as_str())
            .await
            .map_err(|err| match err {
                MobileFileStagingError::NotFound => BuildSnapshotFailure::Internal(format!(
                    "staged image file missing for `{}`: {err}",
                    name
                )),
                _ => BuildSnapshotFailure::Internal(format!(
                    "read staged image bytes for `{}` failed: {err}",
                    name
                )),
            })?;
        let rep = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("image"),
            Some(MimeType(buffered.mime)),
            bytes,
        );
        Ok(BuiltSnapshot {
            snapshot: SystemClipboardSnapshot {
                ts_ms: self.clock.now_ms(),
                representations: vec![rep],
            },
            transfer_id: Some(transfer_id),
        })
    }

    /// `File` 分支:从 buffer 取已 staged 文件引用,直接拼成 `text/uri-list`
    /// rep。
    ///
    /// 与 image 分支的差异:
    /// - image rep 必须持字节(系统剪贴板 image type 要求);本分支只挂 URI,
    ///   不读盘 —— file-list 的 wire 形态本来就是 URI 字符串。
    /// - PUT /file 阶段已经把字节流式落盘,这里不再触发额外的 stage_file 调用。
    fn build_file_snapshot(
        &self,
        data_name: Option<String>,
    ) -> Result<BuiltSnapshot, BuildSnapshotFailure> {
        let name = data_name
            .ok_or_else(|| BuildSnapshotFailure::Decode("File item without dataName".into()))?;
        let buffered = self.buffer.take(&name).ok_or_else(|| {
            BuildSnapshotFailure::Decode(format!(
                "file buffer miss for `{}` (PUT /file may have arrived late or never)",
                name
            ))
        })?;
        let transfer_id = buffered.transfer_id.clone();

        let uri_list = format!("{}\n", buffered.staged.uri.as_str());
        let rep = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            uri_list.into_bytes(),
        );
        info!(
            data_name = %name,
            sanitized_name = %buffered.staged.sanitized_name,
            uri = %buffered.staged.uri,
            "mobile_sync apply_incoming: file staged into uri-list rep"
        );
        Ok(BuiltSnapshot {
            snapshot: SystemClipboardSnapshot {
                ts_ms: self.clock.now_ms(),
                representations: vec![rep],
            },
            transfer_id: Some(transfer_id),
        })
    }

    async fn dispatch_inbound(
        &self,
        source_device_id: MobileDeviceId,
        snapshot: SystemClipboardSnapshot,
    ) -> Result<ApplyIncomingMobileClipOutcome, ApplyIncomingMobileClipError> {
        let (plaintext, content_hash) = encode_snapshot_to_v3_bytes(&snapshot)
            .map_err(|e| ApplyIncomingMobileClipError::EncodeFailed(e.to_string()))?;

        // 伪 DeviceId: `mobile_sync:<id>` 前缀让日志 / clipboard_event.from_device
        // 一眼看出"这条不是 P2P 来的", 不污染真实 P2P DeviceId 命名空间。
        let pseudo_from = DeviceId::new(format!("mobile_sync:{}", source_device_id));

        debug!(
            content_hash = %content_hash,
            plaintext_len = plaintext.len(),
            from_device = %pseudo_from,
            "mobile_sync apply_incoming: dispatching to ApplyInbound"
        );

        let outcome = self
            .inbound
            .execute(ApplyInboundInput {
                from_device: pseudo_from,
                content_hash: content_hash.clone(),
                plaintext,
                flow_id: None,
            })
            .await?;

        Ok(match outcome {
            ApplyOutcome::Applied { entry_id } => {
                info!(entry_id = %entry_id, "mobile_sync apply_incoming: applied");
                ApplyIncomingMobileClipOutcome::Applied { entry_id }
            }
            ApplyOutcome::DuplicateSkipped {
                content_hash: hash,
                existing_entry_id,
            } => {
                debug!(
                    content_hash = %hash,
                    existing_entry_id = %existing_entry_id,
                    "mobile_sync apply_incoming: dedup hit, skipping"
                );
                ApplyIncomingMobileClipOutcome::DuplicateSkipped {
                    content_hash: hash,
                    existing_entry_id,
                }
            }
            ApplyOutcome::DecodeFailed { reason } => {
                // 我们刚 encode 出来的 envelope 又被 inbound decode 失败 ——
                // 几乎不可能, 但为了类型完备保留这条路径 + warn 日志。
                warn!(
                    reason = %reason,
                    "mobile_sync apply_incoming: inbound decode failed (unexpected — we just encoded it)"
                );
                ApplyIncomingMobileClipOutcome::DecodeFailed { reason }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    //! Use-case 单测。
    //!
    //! 用 mockall 把 [`ApplyInboundClipboardUseCase`] 的 3 个内部 collaborator
    //! (entry repo / capture / write) mock 掉, 这样我们就能 assert "本 use
    //! case 在每个分支上正确驱动了 inbound 管线", 不必拉真实 sqlite + OS
    //! clipboard。
    //!
    //! 覆盖矩阵:
    //!
    //! | 分支 | 期望 outcome | inbound 调用 |
    //! |---|---|---|
    //! | Text(non-empty) | Applied | dedup miss + capture + write |
    //! | Text(empty) | DecodeFailed | 不进 inbound |
    //! | BufferFile | Buffered | 不进 inbound (buffer 增量 1) |
    //! | Image(buffer hit) | Applied | 上面同 happy path |
    //! | Image(buffer miss) | DecodeFailed | 不进 inbound |
    //! | Image(没 dataName) | DecodeFailed | 不进 inbound |
    //! | File(buffer hit + staging OK) | Applied + uri-list rep | dedup miss + capture + write |
    //! | File(没 dataName) | DecodeFailed | 不进 inbound |
    //! | File(buffer miss) | DecodeFailed | 不进 inbound |
    //! | File(staging IO err) | Internal err | 不进 inbound |
    //! | File(跨平台 URI) | Applied + URI bytes 校验 | dedup miss + capture + write |
    //! | Group | DecodeFailed | 不进 inbound |
    //! | Text dedup hit | DuplicateSkipped | dedup hit + 无 capture/write |

    use super::*;

    use anyhow::Result as AnyResult;
    use async_trait::async_trait;
    use mockall::predicate::*;

    use uc_core::ports::ClipboardEntryRepositoryPort;

    use crate::usecases::clipboard_sync::apply_inbound::{InboundCapture, InboundWrite};

    use uc_core::mobile_sync::{StagedFile, StagedFileUri};
    use uc_observability::analytics::NoopAnalyticsSink;

    // MobileFileStagingPort mock(get_file 的 read_by_uri 路径共用)与
    // CapturingAnalyticsSink 都在 test_support 模块集中维护。
    use super::super::test_support::{CapturingAnalyticsSink, MockStaging};

    /// 默认 noop sink——绝大多数已有测试只需要"event 不污染断言"，
    /// 不在乎是否 emit。新增的 mobile_clipboard_synced 红线测试用
    /// `capturing_analytics()` 拿到可断言句柄。
    fn noop_analytics() -> Arc<dyn AnalyticsPort> {
        Arc::new(NoopAnalyticsSink::default())
    }

    fn capturing_analytics() -> Arc<CapturingAnalyticsSink> {
        Arc::new(CapturingAnalyticsSink::default())
    }

    /// "未配置任何期望"的 staging mock —— mockall strict mode 下任何方法
    /// 被调到都会 panic, 用于断言"本测试根本不应该触达 staging"。
    fn staging_never_called() -> Arc<MockStaging> {
        Arc::new(MockStaging::new())
    }

    /// 配置 `read_by_uri` 返回指定字节,其它方法仍为"被调即 panic"。
    /// 用于 image 分支测试 —— `build_image_snapshot` 会按 staged URI 读字节
    /// 内联到 image rep。
    fn staging_with_image_bytes(bytes: Vec<u8>) -> Arc<MockStaging> {
        let mut mock = MockStaging::new();
        mock.expect_read_by_uri()
            .times(1)
            .return_once(move |_| Ok(bytes));
        Arc::new(mock)
    }

    // ── mockall: same 3 collaborator surfaces apply_inbound's tests use ──

    mockall::mock! {
        EntryRepo {}
        #[async_trait]
        impl ClipboardEntryRepositoryPort for EntryRepo {
            async fn save_entry_and_selection(
                &self,
                entry: &uc_core::ClipboardEntry,
                selection: &uc_core::ClipboardSelectionDecision,
            ) -> AnyResult<()>;
            async fn get_entry(&self, entry_id: &EntryId) -> AnyResult<Option<uc_core::ClipboardEntry>>;
            async fn list_entries(&self, limit: usize, offset: usize) -> AnyResult<Vec<uc_core::ClipboardEntry>>;
            async fn touch_entry(&self, entry_id: &EntryId, active_time_ms: i64) -> AnyResult<bool>;
            async fn delete_entry(&self, entry_id: &EntryId) -> AnyResult<()>;
            async fn find_entry_id_by_snapshot_hash(&self, snapshot_hash: &str) -> AnyResult<Option<EntryId>>;
        }
    }

    mockall::mock! {
        Capture {}
        #[async_trait]
        impl InboundCapture for Capture {
            async fn capture(
                &self,
                preset_entry_id: EntryId,
                from_device: DeviceId,
                snapshot: SystemClipboardSnapshot,
            ) -> AnyResult<Option<EntryId>>;
        }
    }

    mockall::mock! {
        Write {}
        #[async_trait]
        impl InboundWrite for Write {
            async fn write(&self, snapshot: SystemClipboardSnapshot) -> AnyResult<()>;
        }
    }

    struct FixedClock;
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            1_700_000_000_000
        }
    }

    /// Build a use case where the inner ApplyInbound expects exactly:
    /// dedup-miss + 1 capture + 1 write, returning the supplied entry id.
    /// Used by the "happy path → Applied" tests.
    fn build_uc_expect_applied(entry_id: &str) -> ApplyIncomingMobileClipUseCase {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(None));

        let mut capture = MockCapture::new();
        let id_for_capture = EntryId::from(entry_id);
        capture
            .expect_capture()
            .times(1)
            .returning(move |_, _, _| Ok(Some(id_for_capture.clone())));

        let mut write = MockWrite::new();
        write.expect_write().times(1).returning(|_| Ok(()));

        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            None,
            noop_analytics(),
        )
    }

    /// Build a use case where the inner ApplyInbound is **never** called
    /// (zero expectations on all 3 collaborators). Used by branches that
    /// short-circuit via DecodeFailed / Buffered.
    fn build_uc_expect_no_inbound() -> ApplyIncomingMobileClipUseCase {
        let repo = MockEntryRepo::new();
        let capture = MockCapture::new();
        let write = MockWrite::new();
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            None,
            noop_analytics(),
        )
    }

    /// 装 inbound = 一条 happy path,buffer + staging mock 由调用方提供。
    /// image 分支测试需要注入 `read_by_uri` 响应让 build_image_snapshot 拿到
    /// 字节;其它分支用 `staging_never_called()` 即可。
    fn build_uc_with_buffer_and_image_bytes_expect_applied(
        entry_id: &str,
        staging: Arc<MockStaging>,
    ) -> (ApplyIncomingMobileClipUseCase, Arc<IncomingMobileBuffer>) {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(None));

        let mut capture = MockCapture::new();
        let id_for_capture = EntryId::from(entry_id);
        capture
            .expect_capture()
            .times(1)
            .returning(move |_, _, _| Ok(Some(id_for_capture.clone())));

        let mut write = MockWrite::new();
        write.expect_write().times(1).returning(|_| Ok(()));

        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        let buffer = Arc::new(IncomingMobileBuffer::new());
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            buffer.clone(),
            staging,
            Arc::new(FixedClock),
            None,
            None,
            noop_analytics(),
        );
        (uc, buffer)
    }

    /// Build a use case with a shared buffer + a controllable staging port.
    /// inbound expects exactly 1 happy path (capture + write) returning
    /// `entry_id`. Used by File-branch happy-path tests.
    fn build_uc_with_buffer_and_staging_expect_applied(
        entry_id: &str,
        staging: Arc<MockStaging>,
    ) -> (ApplyIncomingMobileClipUseCase, Arc<IncomingMobileBuffer>) {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(None));
        let mut capture = MockCapture::new();
        let id_for_capture = EntryId::from(entry_id);
        capture
            .expect_capture()
            .times(1)
            .returning(move |_, _, _| Ok(Some(id_for_capture.clone())));
        let mut write = MockWrite::new();
        write.expect_write().times(1).returning(|_| Ok(()));

        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        let buffer = Arc::new(IncomingMobileBuffer::new());
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            buffer.clone(),
            staging,
            Arc::new(FixedClock),
            None,
            None,
            noop_analytics(),
        );
        (uc, buffer)
    }

    /// Build a use case where staging port is preset, but inbound is **never**
    /// called (decode failure path). Used by File 缺 dataName / staging 错误
    /// 等测试。
    fn build_uc_with_staging_expect_no_inbound(
        staging: Arc<MockStaging>,
        buffer: Arc<IncomingMobileBuffer>,
    ) -> ApplyIncomingMobileClipUseCase {
        let repo = MockEntryRepo::new();
        let capture = MockCapture::new();
        let write = MockWrite::new();
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            buffer,
            staging,
            Arc::new(FixedClock),
            None,
            None,
            noop_analytics(),
        )
    }

    fn input_sync_doc(
        item_type: SyncClipboardItemType,
        text: &str,
        data_name: Option<&str>,
    ) -> ApplyIncomingMobileClipInput {
        ApplyIncomingMobileClipInput {
            source_device_id: MobileDeviceId::new("did_seed"),
            event: IncomingMobileClipEvent::SyncDoc {
                item_type,
                text: text.to_string(),
                data_name: data_name.map(|s| s.to_string()),
            },
        }
    }

    fn input_buffer_file(
        data_name: &str,
        mime: &str,
        staged_uri: &str,
        sanitized_name: &str,
    ) -> ApplyIncomingMobileClipInput {
        ApplyIncomingMobileClipInput {
            source_device_id: MobileDeviceId::new("did_seed"),
            event: IncomingMobileClipEvent::BufferFile {
                data_name: data_name.to_string(),
                mime: mime.to_string(),
                staged: StagedFile {
                    uri: StagedFileUri::new(staged_uri),
                    sanitized_name: sanitized_name.to_string(),
                },
                transfer_id: format!("mobile-lan:test-{data_name}"),
            },
        }
    }

    // ── verdicts ────────────────────────────────────────────────────────

    /// Text happy path: dedup miss → capture → write → `Applied`.
    #[tokio::test]
    async fn text_applied_on_new_content() {
        let uc = build_uc_expect_applied("entry-text-1");
        let outcome = uc
            .execute(input_sync_doc(SyncClipboardItemType::Text, "hello", None))
            .await
            .expect("text happy path returns Ok");
        assert_eq!(
            outcome,
            ApplyIncomingMobileClipOutcome::Applied {
                entry_id: EntryId::from("entry-text-1")
            }
        );
    }

    /// Text with empty body → `DecodeFailed`, no inbound calls.
    #[tokio::test]
    async fn text_decode_failed_on_empty_body() {
        let uc = build_uc_expect_no_inbound();
        let outcome = uc
            .execute(input_sync_doc(SyncClipboardItemType::Text, "", None))
            .await
            .expect("empty text returns Ok with DecodeFailed variant");
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { .. }
        ));
    }

    /// `BufferFile` event → `Buffered`, no inbound calls. Verify the
    /// buffer actually grew by 1.
    #[tokio::test]
    async fn buffer_file_returns_buffered_and_grows_buffer() {
        // build_uc_expect_no_inbound ignores buffer size — make our own
        // so we can assert on it.
        let repo = MockEntryRepo::new();
        let capture = MockCapture::new();
        let write = MockWrite::new();
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        let buffer = Arc::new(IncomingMobileBuffer::new());
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            buffer.clone(),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            None,
            noop_analytics(),
        );
        assert_eq!(buffer.len(), 0);

        let outcome = uc
            .execute(input_buffer_file(
                "photo.png",
                "image/png",
                "file:///tmp/mobile_inbound/buf01/photo.png",
                "photo.png",
            ))
            .await
            .expect("buffer-file path returns Ok");

        assert_eq!(outcome, ApplyIncomingMobileClipOutcome::Buffered);
        assert_eq!(buffer.len(), 1, "buffer should now have the staged file");
    }

    /// Image happy path: pre-seed via BufferFile, then SyncDoc Image →
    /// `Applied`. Verifies the two-step PUT protocol's contract.
    #[tokio::test]
    async fn image_applied_after_buffer_then_sync_doc() {
        // build_image_snapshot 走 read_by_uri 拿 staged image 字节, 测试注入
        // 一段 PNG 魔数让链路跑通。
        let staging = staging_with_image_bytes(vec![0x89, 0x50, 0x4E, 0x47]);
        let (uc, buffer) =
            build_uc_with_buffer_and_image_bytes_expect_applied("entry-image-1", staging);

        // step 1: PUT /file/{name} —— 在新架构里 handler 已经把字节流式落盘,
        // 这里直接构造一个"已 staged"的 BufferFile event 模拟 facade 入口的结果。
        let buf_outcome = uc
            .execute(input_buffer_file(
                "photo.png",
                "image/png",
                "file:///tmp/mobile_inbound/img01/photo.png",
                "photo.png",
            ))
            .await
            .unwrap();
        assert_eq!(buf_outcome, ApplyIncomingMobileClipOutcome::Buffered);
        assert_eq!(buffer.len(), 1);

        // step 2: PUT /SyncClipboard.json with type=Image, dataName=photo.png
        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::Image,
                "photo.png",
                Some("photo.png"),
            ))
            .await
            .unwrap();

        assert_eq!(
            outcome,
            ApplyIncomingMobileClipOutcome::Applied {
                entry_id: EntryId::from("entry-image-1")
            }
        );
        // buffer should be drained after take()
        assert_eq!(
            buffer.len(),
            0,
            "image branch should consume the buffered entry"
        );
    }

    /// Image without prior PUT /file → `DecodeFailed`, no inbound calls.
    #[tokio::test]
    async fn image_decode_failed_on_buffer_miss() {
        let uc = build_uc_expect_no_inbound();
        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::Image,
                "photo.png",
                Some("photo.png"),
            ))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { reason } if reason.contains("file buffer miss")
        ));
    }

    /// Image without `dataName` field → `DecodeFailed`, no inbound calls.
    #[tokio::test]
    async fn image_decode_failed_on_missing_data_name() {
        let uc = build_uc_expect_no_inbound();
        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::Image,
                "photo.png",
                None,
            ))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { reason } if reason.contains("without dataName")
        ));
    }

    // ── File 分支测试 ────────────────────────────────────────────────────
    //
    // 流式落盘改造后:
    // - PUT /file 阶段把字节流式 stage 到 cache_dir(由 webserver 触发,本
    //   use case 不参与)。BufferFile event 不再携带字节, 只挂"已 staged
    //   文件"引用。
    // - SyncDoc File 分支不再调 staging,直接拿 buffer 里的 `StagedFile`
    //   拼 uri-list rep。原来的"staging IO 失败"测试因此不再属于本 use case
    //   的语义边界(staging 失败由 facade 流式入口翻译,已在 facade 单测覆盖)。

    /// File happy path: BufferFile (PUT /file 已 stage) → SyncDoc File →
    /// 拼 file-list rep → capture → `Applied`。校验 buffer 被 drain。
    #[tokio::test]
    async fn file_applied_after_buffer_then_sync_doc() {
        let (uc, buffer) =
            build_uc_with_buffer_and_staging_expect_applied("entry-file-1", staging_never_called());

        // step 1: PUT /file/{name} 的最终结果 —— 字节已被 staging 流式落盘,
        // event 只挂 URI 入 buffer。
        uc.execute(input_buffer_file(
            "doc.pdf",
            "application/pdf",
            "file:///tmp/mobile_inbound/abcdef012345/doc.pdf",
            "doc.pdf",
        ))
        .await
        .unwrap();
        assert_eq!(buffer.len(), 1);

        // step 2: PUT /SyncClipboard.json type=File
        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::File,
                "doc.pdf",
                Some("doc.pdf"),
            ))
            .await
            .unwrap();

        assert_eq!(
            outcome,
            ApplyIncomingMobileClipOutcome::Applied {
                entry_id: EntryId::from("entry-file-1")
            }
        );
        assert_eq!(buffer.len(), 0, "file branch should drain buffer");
    }

    /// File 缺 dataName → `DecodeFailed`, 不进 inbound。
    #[tokio::test]
    async fn file_decode_failed_on_missing_data_name() {
        let staging = staging_never_called();
        let buffer = Arc::new(IncomingMobileBuffer::new());
        let uc = build_uc_with_staging_expect_no_inbound(staging, buffer);
        let outcome = uc
            .execute(input_sync_doc(SyncClipboardItemType::File, "doc.pdf", None))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { reason } if reason.contains("without dataName")
        ));
    }

    /// File buffer miss(没有先 PUT /file 就 PUT /json)→ `DecodeFailed`,
    /// 不进 inbound。
    #[tokio::test]
    async fn file_decode_failed_on_buffer_miss() {
        let staging = staging_never_called();
        let buffer = Arc::new(IncomingMobileBuffer::new());
        let uc = build_uc_with_staging_expect_no_inbound(staging, buffer);
        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::File,
                "doc.pdf",
                Some("doc.pdf"),
            ))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { reason } if reason.contains("file buffer miss")
        ));
    }

    /// 跨平台 URI 编码 plumbing:BufferFile 注入"Windows-shape" URI,
    /// 校验 file-list rep bytes 严格按 `\n` 分隔单条 URI 写出。验证 use
    /// case 不对 URI 形态做任何假设(平台细节由 staging adapter 决定)。
    #[tokio::test]
    async fn file_uri_list_rep_propagates_adapter_uri_verbatim() {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(None));

        let mut capture = MockCapture::new();
        capture
            .expect_capture()
            .withf(|_id, _from_device, snapshot| {
                let rep = snapshot
                    .representations
                    .iter()
                    .find(|r| r.format_id.eq_ignore_ascii_case("files"));
                let Some(rep) = rep else { return false };
                let mime_ok = rep
                    .mime
                    .as_ref()
                    .map(|m| m.as_str() == "text/uri-list")
                    .unwrap_or(false);
                let body = std::str::from_utf8(rep.expect_inline_bytes()).unwrap_or("");
                mime_ok
                    && body == "file:///C:/Users/mark/AppData/Local/uc/mobile_inbound/abc/My%20Photo.png\n"
            })
            .times(1)
            .returning(|_, _, _| Ok(Some(EntryId::from("entry-file-win"))));

        let mut write = MockWrite::new();
        write.expect_write().times(1).returning(|_| Ok(()));

        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        let buffer = Arc::new(IncomingMobileBuffer::new());
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            buffer.clone(),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            None,
            noop_analytics(),
        );

        // BufferFile event 注入 Windows-shape URI
        uc.execute(input_buffer_file(
            "My Photo.png",
            "application/octet-stream",
            "file:///C:/Users/mark/AppData/Local/uc/mobile_inbound/abc/My%20Photo.png",
            "My Photo.png",
        ))
        .await
        .unwrap();

        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::File,
                "My Photo.png",
                Some("My Photo.png"),
            ))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::Applied { .. }
        ));
    }

    /// Group type → `DecodeFailed`, no inbound calls.
    #[tokio::test]
    async fn group_decode_failed() {
        let uc = build_uc_expect_no_inbound();
        let outcome = uc
            .execute(input_sync_doc(SyncClipboardItemType::Group, "", None))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { .. }
        ));
    }

    /// Dedup hit: text body whose hash matches an existing local entry →
    /// `DuplicateSkipped` propagated, capture + write **not** called.
    /// Pins the "remote re-PUT doesn't double-write" property.
    #[tokio::test]
    async fn duplicate_skipped_when_text_hash_already_local() {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(Some(EntryId::from("entry-existing"))));

        // Zero expectations on capture + write — mockall panics on Drop
        // if either gets called.
        let capture = MockCapture::new();
        let write = MockWrite::new();
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            None,
            noop_analytics(),
        );

        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::Text,
                "already-here",
                None,
            ))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DuplicateSkipped {
                existing_entry_id,
                ..
            } if existing_entry_id == EntryId::from("entry-existing")
        ));
    }

    // ── fan-out trait wire-up ──────────────────────────────────────────
    //
    // 验证 `ApplyIncomingMobileClipUseCase` 与 `MobileInboundFanOutPort`
    // 之间的接线契约。这一层不验证"传播到 paired peers 的真实行为"——
    // 那是 `ClipboardOutboundFanOutAdapter` 的责任,改 adapter 不动这里
    // 的断言;use case 只该保证"在对的分支以对的参数调一次 trait"。

    /// `MobileInboundFanOutPort` 的 in-memory fake, 单测断言用。
    #[derive(Default)]
    struct RecordingFanOut {
        calls: std::sync::Mutex<Vec<(EntryId, SystemClipboardSnapshot, MobileDeviceId)>>,
    }

    impl RecordingFanOut {
        fn calls(&self) -> Vec<(EntryId, SystemClipboardSnapshot, MobileDeviceId)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl MobileInboundFanOutPort for RecordingFanOut {
        fn fan_out(
            &self,
            entry_id: EntryId,
            snapshot: SystemClipboardSnapshot,
            source_device_id: MobileDeviceId,
        ) {
            self.calls
                .lock()
                .unwrap()
                .push((entry_id, snapshot, source_device_id));
        }
    }

    /// `Applied` 分支必须把 `(entry_id, snapshot, source_device_id)` 完整
    /// 透传给 fan-out port —— 后续 adapter 才有足够信息复用本机捕获出站
    /// 管线(snapshot 用来抽文件路径 / 发布 blob;entry_id 用作 blob 的
    /// 发送端归属;source_device_id 给日志做来源识别)。
    #[tokio::test]
    async fn applied_branch_invokes_fan_out_with_full_context() {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(None));
        let mut capture = MockCapture::new();
        capture
            .expect_capture()
            .times(1)
            .returning(|_, _, _| Ok(Some(EntryId::from("entry-fanout-1"))));
        let mut write = MockWrite::new();
        write.expect_write().times(1).returning(|_| Ok(()));
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));

        let recorder = Arc::new(RecordingFanOut::default());
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            Some(Arc::clone(&recorder) as Arc<dyn MobileInboundFanOutPort>),
            noop_analytics(),
        );

        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::Text,
                "hello from mobile",
                None,
            ))
            .await
            .expect("text applied ok");
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::Applied { .. }
        ));

        let calls = recorder.calls();
        assert_eq!(calls.len(), 1, "Applied 必须触发 fan_out 一次");
        let (entry_id, snapshot, source) = &calls[0];
        assert_eq!(*entry_id, EntryId::from("entry-fanout-1"));
        assert_eq!(*source, MobileDeviceId::new("did_seed"));
        // snapshot 至少要携带原 rep, 让 adapter 能基于内容抽文件路径 /
        // 计算 blob ref。这里仅断言 rep 数量, 内容细节由其他 use case
        // 测试覆盖, 不重复绑定。
        assert_eq!(snapshot.representations.len(), 1);
    }

    /// `DuplicateSkipped` 命中本机 dedup —— 这条 content_hash 之前已被
    /// 本设备处理过, **绝对不能**再 fan-out: 重复广播浪费带宽且可能扰
    /// 乱对端 dedup 时序。
    #[tokio::test]
    async fn duplicate_skipped_does_not_invoke_fan_out() {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(Some(EntryId::from("entry-existing"))));
        let capture = MockCapture::new();
        let write = MockWrite::new();
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));

        let recorder = Arc::new(RecordingFanOut::default());
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            Some(Arc::clone(&recorder) as Arc<dyn MobileInboundFanOutPort>),
            noop_analytics(),
        );

        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::Text,
                "already-here",
                None,
            ))
            .await
            .expect("dedup hit ok");
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DuplicateSkipped { .. }
        ));
        assert_eq!(
            recorder.calls().len(),
            0,
            "DuplicateSkipped 分支不得触发 fan_out"
        );
    }

    /// `DecodeFailed` 分支(此例:Text 空 body 在 `build_text_snapshot`
    /// 阶段被拒绝)本机入站根本没成功 → 没"已应用的内容"可广播 →
    /// 不调 fan-out。inbound 链全程零调用是顺带钉死的不变量。
    #[tokio::test]
    async fn decode_failed_does_not_invoke_fan_out() {
        let recorder = Arc::new(RecordingFanOut::default());
        let repo = MockEntryRepo::new();
        let capture = MockCapture::new();
        let write = MockWrite::new();
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            Some(Arc::clone(&recorder) as Arc<dyn MobileInboundFanOutPort>),
            noop_analytics(),
        );

        let outcome = uc
            .execute(input_sync_doc(SyncClipboardItemType::Text, "", None))
            .await
            .expect("decode failure surfaces as outcome, not Err");
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { .. }
        ));
        assert_eq!(
            recorder.calls().len(),
            0,
            "DecodeFailed 分支不得触发 fan_out"
        );
    }

    /// `BufferFile` 分支只把字节挂进 buffer, 不产生"已应用到本机的 entry"——
    /// SyncDoc 还没到, 当前没有内容可广播。这条 outcome (`Buffered`) 的不变量
    /// 在 `maybe_fan_out_to_paired_peers` 文档里被显式承诺(只有 `Applied`
    /// 才触发), 这里用一个 fake fan-out port 把它钉死, 防止未来重构在 PUT
    /// /file 阶段提前 fan-out 一份"空 envelope"出去。
    #[tokio::test]
    async fn buffer_file_does_not_invoke_fan_out() {
        let recorder = Arc::new(RecordingFanOut::default());
        // BufferFile 分支不进 inbound 链, mocks 留空 —— 任何意外调用都会
        // 让 mockall 在 Drop 时 panic, 这本身就是个额外的不变量校验。
        let repo = MockEntryRepo::new();
        let capture = MockCapture::new();
        let write = MockWrite::new();
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            Some(Arc::clone(&recorder) as Arc<dyn MobileInboundFanOutPort>),
            noop_analytics(),
        );

        let outcome = uc
            .execute(input_buffer_file(
                "photo.png",
                "image/png",
                "file:///tmp/mobile_inbound/buf-fanout/photo.png",
                "photo.png",
            ))
            .await
            .expect("buffer-file path returns Ok");
        assert_eq!(outcome, ApplyIncomingMobileClipOutcome::Buffered);
        assert_eq!(
            recorder.calls().len(),
            0,
            "Buffered 分支不得触发 fan_out (PUT /file 阶段没有 entry 可广播)"
        );
    }

    /// `mobile_sync:<id>` 伪 DeviceId 的 plumbing: 通过 capture mock 的
    /// `withf` 校验 snapshot 的 ts_ms 来自 FixedClock。`from_device` 的
    /// 字符串校验留给 inbound 的 ports —— 我们已经在 dispatch_inbound 里
    /// `format!("mobile_sync:{}", source_device_id)`, 编译期保证。
    #[tokio::test]
    async fn text_snapshot_uses_clock_ts_ms() {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(None));

        let mut capture = MockCapture::new();
        capture
            .expect_capture()
            .withf(|_id, _from_device, snapshot| snapshot.ts_ms == 1_700_000_000_000)
            .times(1)
            .returning(|_, _, _| Ok(Some(EntryId::from("entry-clock"))));

        let mut write = MockWrite::new();
        write.expect_write().times(1).returning(|_| Ok(()));

        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            None,
            noop_analytics(),
        );

        let outcome = uc
            .execute(input_sync_doc(SyncClipboardItemType::Text, "tick", None))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::Applied { .. }
        ));
    }

    // ── tests: mobile_clipboard_synced 红线 (schema doc §7.6 / §12.2 P1) ──

    /// 装配一个 happy-path use case + 可断言 analytics sink。
    /// inbound 期望恰好 1 次 dedup miss + capture + write，返回指定 entry_id。
    fn build_uc_with_analytics_expect_applied(
        entry_id: &str,
        analytics: Arc<dyn AnalyticsPort>,
    ) -> ApplyIncomingMobileClipUseCase {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(None));
        let mut capture = MockCapture::new();
        let id_for_capture = EntryId::from(entry_id);
        capture
            .expect_capture()
            .times(1)
            .returning(move |_, _, _| Ok(Some(id_for_capture.clone())));
        let mut write = MockWrite::new();
        write.expect_write().times(1).returning(|_| Ok(()));
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            None,
            analytics,
        )
    }

    /// inbound 期望恰好 1 次 dedup-hit（返回 existing_entry_id），不进
    /// capture / write。
    fn build_uc_with_analytics_expect_dedup_hit(
        existing_entry_id: &str,
        analytics: Arc<dyn AnalyticsPort>,
    ) -> ApplyIncomingMobileClipUseCase {
        let mut repo = MockEntryRepo::new();
        let id_clone = EntryId::from(existing_entry_id);
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(move |_| Ok(Some(id_clone.clone())));
        let capture = MockCapture::new();
        let write = MockWrite::new();
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            None,
            analytics,
        )
    }

    /// inbound + capture + write + staging 全无期望（短路在 use case 层）。
    fn build_uc_with_analytics_expect_no_inbound(
        analytics: Arc<dyn AnalyticsPort>,
    ) -> ApplyIncomingMobileClipUseCase {
        let repo = MockEntryRepo::new();
        let capture = MockCapture::new();
        let write = MockWrite::new();
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            staging_never_called(),
            Arc::new(FixedClock),
            None,
            None,
            analytics,
        )
    }

    #[tokio::test]
    async fn applied_emits_mobile_clipboard_synced_inbound() {
        let analytics = capturing_analytics();
        let uc = build_uc_with_analytics_expect_applied(
            "entry-text-1",
            analytics.clone() as Arc<dyn AnalyticsPort>,
        );
        uc.execute(input_sync_doc(SyncClipboardItemType::Text, "hello", None))
            .await
            .expect("happy path");
        // 5 bytes "hello" → Lt1Kb 桶。direction v1 恒为 inbound。
        assert_eq!(
            analytics.events(),
            vec![Event::MobileClipboardSynced {
                direction: Direction::Inbound,
                payload_size_bucket: PayloadSizeBucket::Lt1Kb,
            }]
        );
    }

    #[tokio::test]
    async fn duplicate_skipped_does_not_emit_synced() {
        // dedup 命中 = 本机已存在该 content_hash；重复埋点会让 dashboard
        // 频率口径双计，沿用 ClipboardEntryCaptured 防 RemotePush 双计的
        // 红线哲学。
        let analytics = capturing_analytics();
        let uc = build_uc_with_analytics_expect_dedup_hit(
            "entry-existing",
            analytics.clone() as Arc<dyn AnalyticsPort>,
        );
        let outcome = uc
            .execute(input_sync_doc(SyncClipboardItemType::Text, "hello", None))
            .await
            .expect("dedup hit returns Ok");
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DuplicateSkipped { .. }
        ));
        assert!(analytics.events().is_empty(), "{:?}", analytics.events());
    }

    #[tokio::test]
    async fn decode_failed_does_not_emit_synced() {
        // 空 text → DecodeFailed；本机入站没成功，不应 emit synced。
        let analytics = capturing_analytics();
        let uc =
            build_uc_with_analytics_expect_no_inbound(analytics.clone() as Arc<dyn AnalyticsPort>);
        let outcome = uc
            .execute(input_sync_doc(SyncClipboardItemType::Text, "", None))
            .await
            .expect("empty text returns Ok with DecodeFailed variant");
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { .. }
        ));
        assert!(analytics.events().is_empty(), "{:?}", analytics.events());
    }

    #[tokio::test]
    async fn buffer_file_does_not_emit_synced() {
        // BufferFile 是两步 PUT 协议的中间态，不是用户感知的同步。
        let analytics = capturing_analytics();
        let uc =
            build_uc_with_analytics_expect_no_inbound(analytics.clone() as Arc<dyn AnalyticsPort>);
        let outcome = uc
            .execute(input_buffer_file(
                "pic.png",
                "image/png",
                "file:///tmp/uc-staging/pic.png",
                "pic.png",
            ))
            .await
            .expect("buffer file returns Ok");
        assert!(matches!(outcome, ApplyIncomingMobileClipOutcome::Buffered));
        assert!(analytics.events().is_empty(), "{:?}", analytics.events());
    }
}

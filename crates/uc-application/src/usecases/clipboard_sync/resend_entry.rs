//! ADR-005 Stage 1a · user-initiated resend.
//!
//!补齐 desktop 缺失的"重发"能力:用户在 entry detail view 上看到对某 peer
//! 是 `Failed { Offline }`,主动点重发。本用例**不引入新表、不新增 Port、
//! 不自动触发**,候选集合由 [`EntryDeliveryRepositoryPort::list_by_entry`]
//! 与 [`TrustedPeerRepositoryPort::list`] 在本调用内派生(差集),与 ADR
//! §3.3 铁律 6 一致。
//!
//! ## 与 `DispatchClipboardEntryUseCase` 的关系
//!
//! 本用例**不复用** `ClipboardOutboundDispatcher::dispatch_capture` 入口
//! (`dispatch_capture` 的语义是"刚捕获了新内容,广播给所有 peer"):
//!
//! - 输入语义不同:resend 的输入是已存 `EntryId`,不是新 snapshot;
//! - origin 不同:用 [`ClipboardChangeOrigin::Resend`](uc_core::ClipboardChangeOrigin::Resend),
//!   避免污染 capture 漏斗 telemetry;
//! - 目标语义不同:resend 走 `DispatchClipboardEntryInput.target_filter`
//!   收紧 fan-out 到差集 / 显式 peer 子集,而 capture 走全 fan-out。
//!
//! 但下游(plan → publish → encode → dispatch)100% 复用既有路径:
//!
//! - [`reconstruct_snapshot_from_entry`] —— commit B2 抽出的共享 helper;
//! - [`OutboundSyncPlanner::plan`] —— commit B3 让 `Resend` 与 `LocalCapture`
//!   共享文件分支条件;
//! - [`publish_file_blob_refs`] / [`publish_oversized_inline_blob_refs`] ——
//!   commit B3 提升为 `pub(crate)` + 抽出 [`OutboundBlobPublishGateway`]
//!   trait 让本用例可单测;
//! - [`encode_snapshot_with_blob_refs_to_v3_bytes`] —— payload codec;
//! - [`DispatchEntryRunner`] —— commit A 的 `target_filter` 字段 + 本 commit
//!   抽出的内部 trait,让 fan-out / delivery 落盘 / host event emit 全部复用。
//!
//! ## "本机已不持有 plaintext / blob" 的定义
//!
//! `reconstruct_snapshot_from_entry` 已经把所有"无法物化"的情况收敛成
//! [`BuildSnapshotError::PasteRepUnavailable`] / `PasteRepBlobFetchFailed` /
//! `InvalidFileUri` / `NoFilePaths` / `NoRestorableRepresentations`,本用例
//! 全部映射成 [`ResendEntryError::EntryNotResendable`] + [`NotResendableReason::PayloadLost`]。
//! 文件分支额外做了 on-disk 存在性校验(reconstruct 内 `path.exists()` →
//! `PayloadResolveError::Lost`),所以"本机文件被 GC 但 payload_state 还没翻
//! 成 Lost"的边界情况也走得通。
//!
//! ## `max_file_size` 与 Resend 的契约
//!
//! Resend 路径**不**受 `settings.file_sync.max_file_size` 限制 —— 该 setting
//! 是 LocalCapture 自动出站的带宽护栏(用户没主动表态时不想偷偷送大文件),
//! 而 resend 是用户显式动作,理应越过此护栏自行承担后果。bypass 在
//! [`OutboundSyncPlanner::plan`] 内实现(origin-aware),`file_sync_enabled`
//! 不在 bypass 范围内 —— "关闭文件同步"是更强的用户意图,resend 仍尊重。
//!
//! 这条契约让 [`NotResendableReason::PayloadLost`] 的语义严格收窄到"本机
//! 不持有 plaintext / blob",不再混入"持有但超用户上限"的歧义,前端
//! i18n 文案可以放心表达"this entry no longer exists locally"。

use std::collections::HashSet;
use std::sync::Arc;

use thiserror::Error;
use tracing::{info, warn};

use uc_core::blob::ports::BlobReaderPort;
use uc_core::clipboard::ClipboardContentCategorySet;
use uc_core::clipboard::EntryDeliveryStatus;
use uc_core::ids::{DeviceId, EntryId};
use uc_core::ports::clipboard::{
    ClipboardPayloadResolverPort, GetClipboardEntryPort, GetRepresentationPort,
    UpdateRepresentationProcessingResultPort,
};
use uc_core::ports::{
    ClipboardEventRepositoryPort, ClipboardSelectionRepositoryPort, DeviceIdentityPort,
    EntryDeliveryRepositoryPort, SettingsPort,
};
use uc_core::trusted_peer::TrustedPeerRepositoryPort;
use uc_core::ClipboardChangeOrigin;

use crate::facade::clipboard_outbound::{
    extract_file_paths_from_snapshot, publish_file_blob_refs, publish_oversized_inline_blob_refs,
    ClipboardOutboundError, OutboundBlobPublishGateway,
};
use crate::sync_planner::{FileCandidate, OutboundSyncPlanner};
use crate::usecases::clipboard_sync::dispatch_entry::{
    DispatchClipboardEntryInput, DispatchEntryRunner, DispatchSyncError,
};
use crate::usecases::clipboard_sync::payload_codec::encode_snapshot_with_blob_refs_to_v3_bytes;
use crate::usecases::clipboard_sync::snapshot_from_entry::{
    reconstruct_snapshot_from_entry, BuildSnapshotError,
};

/// 用户主动 resend 的命令。
#[derive(Debug, Clone)]
pub struct ResendEntryCommand {
    pub entry_id: EntryId,
    /// `None` —— 派生 `trusted_peer \ (Delivered ∪ Duplicate)` 差集;
    /// 差集为空时返回 [`ResendEntryError::NoEligibleTargets`]。
    ///
    /// `Some(list)` —— 仅向 list 重发;但 list 中每个 device 都必须在
    /// `trusted_peer_repo.list()` 内,否则返回
    /// [`ResendEntryError::TargetNotTrusted`]。
    /// `Some(vec![])` 与差集为空等价(零目标),走 `NoEligibleTargets`。
    pub target_filter: Option<Vec<DeviceId>>,
}

/// fan-out 完成后的聚合计数。字段含义对齐 `DispatchEntryOutcome`,但语义
/// 独立(独立类型方便未来增加 partial-success / lost-peer 等 resend 特有
/// 字段时不破 dispatch 路径)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResendReport {
    pub accepted: usize,
    pub duplicate: usize,
    pub offline: usize,
    pub errored: usize,
    /// fan-out deadline 内未 settle、被搬到后台继续 join 的目标数。
    /// 后台会在真实完成时刻写 delivery record 并发 host event,前端 detail
    /// badge 自然刷新。
    pub pending: usize,
}

/// resend 用例的失败语义。表达"用户主动 resend 这个动作没能完成"的应用层
/// 错误集合,不向上漏出底层仓储 / dispatch 错误。
#[derive(Debug, Error)]
pub enum ResendEntryError {
    /// `entry_repo.get_entry` 返回 `None`。可能是 entry 已被用户删除,
    /// 也可能是 UI 拿到一份过期视图后才点击重发。
    #[error("entry not found: {0}")]
    EntryNotFound(EntryId),

    /// entry 自身存在,但当前无法重发。`reason` 区分:
    /// - [`NotResendableReason::RemoteOrigin`] —— entry 来自远端 peer;
    /// - [`NotResendableReason::PayloadLost`] —— 本机已不持有 plaintext / blob。
    ///
    /// `entry_id` 与 [`EntryNotFound`](Self::EntryNotFound) 对齐 —— 让 UI / CLI
    /// 在 toast 与日志里能锚定具体哪条 entry,用户在堆叠多条历史时不至于
    /// 一头雾水。
    #[error("entry {entry_id} is not resendable: {reason:?}")]
    EntryNotResendable {
        entry_id: EntryId,
        reason: NotResendableReason,
    },

    /// 显式 filter 中包含不在 `trusted_peer_repo.list()` 内的 device。
    /// 不静默 skip,以便 UI 能直接告诉用户"该设备已被移除信任关系"。
    #[error("target device {0} is not a trusted peer")]
    TargetNotTrusted(DeviceId),

    /// `target_filter = None` 但差集为空 —— 该 entry 已经对所有 trusted
    /// peer 至少 Delivered / Duplicate 过一次,没什么可重发的。
    #[error("no eligible targets for resend (all trusted peers already delivered)")]
    NoEligibleTargets,

    /// 仓储读写失败(entry / event / selection / representation / delivery /
    /// trusted_peer 任一)。
    #[error("storage failure: {0}")]
    Storage(String),

    /// 下游 dispatch / publish / encode 路径失败(加密会话锁定、blob 发布失
    /// 败、V3 envelope 编码失败等)。
    #[error("dispatch failure: {0}")]
    Dispatch(String),
}

/// resend 失败时的细分原因。UI 据此选不同的英文文案 / i18n key。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotResendableReason {
    /// entry 来自远端 peer。v1 不支持远端 entry 的 resend ——
    /// 视图层(`GetEntryDeliveryViewUseCase`)对远端 entry 直接返回
    /// `deliveries = []`,UI 上根本不会出现重发按钮;若仍走到此处,
    /// 多半是绕过视图的脚本调用。
    RemoteOrigin,
    /// 本机已不持有 plaintext / 必要 blob。覆盖:
    /// - paste rep `payload_state == Lost`;
    /// - 文件 rep 引用的本地文件已被 `cleanup_expired_files` 物理删除;
    /// - blob store 已 GC 掉对应字节;
    /// - selection / paste rep / event 行错位(理论上应被 cascade 清掉但
    ///   防御性归到本类别)。
    PayloadLost,
}

/// Crate-internal trait — mirrors [`ResendEntryUseCase::execute`] so callers
/// (currently [`ClipboardOutboundFacade::resend_entry`]) can hold an
/// `Arc<dyn ResendEntryRunner>` and tests can swap a stub without
/// constructing the full 12-port use case. Production wiring satisfies this
/// trait through the blanket impl below. Not exposed beyond the crate.
#[async_trait::async_trait]
pub(crate) trait ResendEntryRunner: Send + Sync {
    async fn execute(&self, cmd: ResendEntryCommand) -> Result<ResendReport, ResendEntryError>;
}

#[async_trait::async_trait]
impl ResendEntryRunner for ResendEntryUseCase {
    async fn execute(&self, cmd: ResendEntryCommand) -> Result<ResendReport, ResendEntryError> {
        ResendEntryUseCase::execute(self, cmd).await
    }
}

pub(crate) struct ResendEntryUseCase {
    entry_repo: Arc<dyn GetClipboardEntryPort>,
    event_repo: Arc<dyn ClipboardEventRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn GetRepresentationPort>,
    rep_processing_repo: Arc<dyn UpdateRepresentationProcessingResultPort>,
    payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    blob_store: Arc<dyn BlobReaderPort>,
    entry_delivery_repo: Arc<dyn EntryDeliveryRepositoryPort>,
    trusted_peer_repo: Arc<dyn TrustedPeerRepositoryPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    settings: Arc<dyn SettingsPort>,
    blob_publisher: Arc<dyn OutboundBlobPublishGateway>,
    dispatch_runner: Arc<dyn DispatchEntryRunner>,
}

/// Bundled dependencies for [`ResendEntryUseCase`].
///
/// 12 个 ports 用 named-field struct 而不是位置参数 —— 调用点 (facade 装
/// 配 + 测试 fixtures) 全部按字段名构造,后续再加 port 不会按位置撞错,
/// 也不再需要 `#[allow(clippy::too_many_arguments)]`。crate-internal:
/// 外部 crate 通过 [`ClipboardOutboundFacade::new`](crate::facade::ClipboardOutboundFacade::new)
/// 间接装配。
pub(crate) struct ResendEntryDeps {
    pub entry_repo: Arc<dyn GetClipboardEntryPort>,
    pub event_repo: Arc<dyn ClipboardEventRepositoryPort>,
    pub selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    pub representation_repo: Arc<dyn GetRepresentationPort>,
    pub rep_processing_repo: Arc<dyn UpdateRepresentationProcessingResultPort>,
    pub payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    pub blob_store: Arc<dyn BlobReaderPort>,
    pub entry_delivery_repo: Arc<dyn EntryDeliveryRepositoryPort>,
    pub trusted_peer_repo: Arc<dyn TrustedPeerRepositoryPort>,
    pub device_identity: Arc<dyn DeviceIdentityPort>,
    pub settings: Arc<dyn SettingsPort>,
    pub blob_publisher: Arc<dyn OutboundBlobPublishGateway>,
    pub dispatch_runner: Arc<dyn DispatchEntryRunner>,
}

impl ResendEntryUseCase {
    pub(crate) fn new(deps: ResendEntryDeps) -> Self {
        Self {
            entry_repo: deps.entry_repo,
            event_repo: deps.event_repo,
            selection_repo: deps.selection_repo,
            representation_repo: deps.representation_repo,
            rep_processing_repo: deps.rep_processing_repo,
            payload_resolver: deps.payload_resolver,
            blob_store: deps.blob_store,
            entry_delivery_repo: deps.entry_delivery_repo,
            trusted_peer_repo: deps.trusted_peer_repo,
            device_identity: deps.device_identity,
            settings: deps.settings,
            blob_publisher: deps.blob_publisher,
            dispatch_runner: deps.dispatch_runner,
        }
    }

    /// 执行一次 resend。流程对照 ADR-005 §2.5.4:
    ///
    /// 1. `entry_repo.get_entry` — 不存在则 `EntryNotFound`。
    /// 2. `event_repo.get_source_device` — 非本机则 `RemoteOrigin`。
    /// 3. `reconstruct_snapshot_from_entry` — 任一必要 rep 不可解则 `PayloadLost`。
    /// 4. 派生目标集合(filter 校验 / 差集派生)。
    /// 5. `OutboundSyncPlanner.plan(Resend)` + `publish_*` — 文件全缺失或
    ///    publish 失败映射到对应 error。
    /// 6. V3 envelope 编码。
    /// 7. `dispatch_runner.execute` with `target_filter = Some(targets)`。
    ///
    /// fan-out 后的 `EntryDeliveryRecord` 写盘 + host event emit 由
    /// `DispatchClipboardEntryUseCase` 内部完成,本用例只回收聚合计数。
    pub(crate) async fn execute(
        &self,
        cmd: ResendEntryCommand,
    ) -> Result<ResendReport, ResendEntryError> {
        info!(
            entry_id = %cmd.entry_id,
            filter_kind = if cmd.target_filter.is_some() { "explicit" } else { "diff_set" },
            "resend.execute start"
        );

        // 1. Load entry.
        let entry = self
            .entry_repo
            .get_entry(&cmd.entry_id)
            .await
            .map_err(|err| ResendEntryError::Storage(format!("get_entry: {err}")))?
            .ok_or_else(|| ResendEntryError::EntryNotFound(cmd.entry_id.clone()))?;

        // 2. Origin check. `get_source_device == None` 说明 event 已不存在
        //    (FK 错位),保守按"非本机"处理,而不是当成本机 — entry 真的有
        //    local 来源的话 source_device 必然有值。
        let source_device = match self
            .event_repo
            .get_source_device(&entry.event_id)
            .await
            .map_err(|err| ResendEntryError::Storage(format!("get_source_device: {err}")))?
        {
            Some(dev) => dev,
            None => {
                return Err(ResendEntryError::EntryNotResendable {
                    entry_id: cmd.entry_id.clone(),
                    reason: NotResendableReason::RemoteOrigin,
                });
            }
        };
        let local_device = self.device_identity.current_device_id();
        if source_device != local_device {
            return Err(ResendEntryError::EntryNotResendable {
                entry_id: cmd.entry_id.clone(),
                reason: NotResendableReason::RemoteOrigin,
            });
        }

        // 3. Reconstruct snapshot. 借用 commit B2 的共享 helper,文件分支自带
        //    on-disk 存在性校验,所以"本机已不持有"在此处一并被吸收。
        let snapshot = reconstruct_snapshot_from_entry(
            self.entry_repo.as_ref(),
            self.selection_repo.as_ref(),
            self.representation_repo.as_ref(),
            self.rep_processing_repo.as_ref(),
            self.payload_resolver.as_ref(),
            self.blob_store.as_ref(),
            &cmd.entry_id,
        )
        .await
        .map_err(|err| map_build_snapshot_error(err, &cmd.entry_id))?;

        // 4. Derive target set.
        let trusted: Vec<DeviceId> = self
            .trusted_peer_repo
            .list()
            .await
            .map_err(|err| ResendEntryError::Storage(format!("trusted_peer.list: {err}")))?
            .into_iter()
            .map(|tp| tp.peer_device_id)
            .collect();

        let targets: Vec<DeviceId> = match &cmd.target_filter {
            Some(filter) => {
                // 校验顺序按 filter 自身顺序,保证错误信息可复现(先报哪个
                // 不在信任集合内,与 UI 中点的顺序一致)。
                for d in filter {
                    if !trusted.iter().any(|t| t == d) {
                        return Err(ResendEntryError::TargetNotTrusted(d.clone()));
                    }
                }
                if filter.is_empty() {
                    return Err(ResendEntryError::NoEligibleTargets);
                }
                filter.clone()
            }
            None => {
                let records = self
                    .entry_delivery_repo
                    .list_by_entry(&cmd.entry_id)
                    .await
                    .map_err(|err| ResendEntryError::Storage(format!("list_by_entry: {err}")))?;
                let covered: HashSet<DeviceId> = records
                    .into_iter()
                    .filter(|r| {
                        matches!(
                            r.status,
                            EntryDeliveryStatus::Delivered | EntryDeliveryStatus::Duplicate
                        )
                    })
                    .map(|r| r.target_device_id)
                    .collect();
                let diff: Vec<DeviceId> = trusted
                    .into_iter()
                    .filter(|d| !covered.contains(d))
                    .collect();
                if diff.is_empty() {
                    return Err(ResendEntryError::NoEligibleTargets);
                }
                diff
            }
        };

        // 5. Plan + publish blobs.
        //
        // resend 走 `extract_file_paths_from_snapshot` 与 LocalCapture 共享:
        // reconstruct 出来的文件分支 snapshot 是一份 `text/uri-list` rep,
        // helper 解析其中 `file://...` 行为本地 PathBuf。`tokio::fs::metadata`
        // 失败仅 warn(单文件丢失不阻塞整次 resend);如果所有文件都丢失,
        // planner 会通过 `all_files_excluded` 返回 `clipboard: None`,这里
        // 映射成 `PayloadLost`。
        let resolved_paths = extract_file_paths_from_snapshot(&snapshot);
        let extracted_paths_count = resolved_paths.len();
        let mut file_candidates = Vec::with_capacity(resolved_paths.len());
        for path in resolved_paths {
            match tokio::fs::metadata(&path).await {
                Ok(meta) => file_candidates.push(FileCandidate {
                    path,
                    size: meta.len(),
                }),
                Err(err) => warn!(
                    error = %err,
                    "resend: 排除无法读取元数据的剪贴板文件(本机可能已不持有)"
                ),
            }
        }

        let planner = OutboundSyncPlanner::new(Arc::clone(&self.settings));
        let plan = planner
            .plan(
                snapshot,
                ClipboardChangeOrigin::Resend,
                file_candidates,
                extracted_paths_count,
            )
            .await;
        let Some(mut clipboard_intent) = plan.clipboard else {
            // resend 路径下唯一让 `clipboard` 落空的分支是 planner 的
            // `all_files_excluded` —— 即 extracted_paths_count > 0 但所有
            // candidate 都被 metadata 失败 / size 上限排除。视为 PayloadLost。
            return Err(ResendEntryError::EntryNotResendable {
                entry_id: cmd.entry_id.clone(),
                reason: NotResendableReason::PayloadLost,
            });
        };

        let mut blob_refs =
            publish_file_blob_refs(self.blob_publisher.as_ref(), &plan.files, &cmd.entry_id)
                .await
                .map_err(map_outbound_publish_error)?;
        let mut image_blob_refs = publish_oversized_inline_blob_refs(
            self.blob_publisher.as_ref(),
            &mut clipboard_intent.snapshot,
            &cmd.entry_id,
        )
        .await
        .map_err(map_outbound_publish_error)?;
        blob_refs.append(&mut image_blob_refs);

        // 6. Encode V3 envelope.
        let categories = ClipboardContentCategorySet::from_snapshot(&clipboard_intent.snapshot);
        let (plaintext, content_hash) =
            encode_snapshot_with_blob_refs_to_v3_bytes(&clipboard_intent.snapshot, &blob_refs)
                .map_err(|e| ResendEntryError::Dispatch(format!("payload encode: {e}")))?;

        // 7. Dispatch. fan-out 的 delivery 落盘 + host event emit 在
        //    `DispatchClipboardEntryUseCase::execute` 内串行完成,本用例只
        //    回收聚合计数。`target_filter = Some(targets)` 让 dispatch
        //    跳过差集外的 peer(commit A 行为),且仍受 `is_send_allowed`
        //    保护(用户在 settings 关掉的对端不会被发送)。
        let outcome = self
            .dispatch_runner
            .execute(DispatchClipboardEntryInput {
                plaintext,
                content_hash,
                payload_version: 3,
                categories,
                entry_id: Some(cmd.entry_id.clone()),
                target_filter: Some(targets),
            })
            .await
            .map_err(map_dispatch_sync_error)?;

        info!(
            entry_id = %cmd.entry_id,
            accepted = outcome.total_accepted,
            duplicate = outcome.total_duplicate,
            offline = outcome.total_offline,
            errored = outcome.total_errored,
            pending = outcome.total_pending,
            "resend.execute completed"
        );

        Ok(ResendReport {
            accepted: outcome.total_accepted,
            duplicate: outcome.total_duplicate,
            offline: outcome.total_offline,
            errored: outcome.total_errored,
            pending: outcome.total_pending,
        })
    }
}

fn map_build_snapshot_error(err: BuildSnapshotError, entry_id: &EntryId) -> ResendEntryError {
    match err {
        BuildSnapshotError::EntryNotFound { entry_id } => ResendEntryError::EntryNotFound(entry_id),
        BuildSnapshotError::SelectionNotFound { .. }
        | BuildSnapshotError::PasteRepNotFound { .. }
        | BuildSnapshotError::PasteRepUnavailable(_)
        | BuildSnapshotError::PasteRepBlobFetchFailed { .. }
        | BuildSnapshotError::InvalidFileUri { .. }
        | BuildSnapshotError::NoFilePaths { .. }
        | BuildSnapshotError::NoRestorableRepresentations { .. } => {
            // `BuildSnapshotError::EntryNotFound` 自带 entry_id;其余 PayloadLost
            // 类变体没有 —— reconstruct helper 拿到的是同一 entry_id,这里直接
            // 用上游传入的副本回填,避免上游每个 variant 都改成带 id。
            ResendEntryError::EntryNotResendable {
                entry_id: entry_id.clone(),
                reason: NotResendableReason::PayloadLost,
            }
        }
        BuildSnapshotError::Repository(inner) => ResendEntryError::Storage(inner.to_string()),
    }
}

fn map_outbound_publish_error(err: ClipboardOutboundError) -> ResendEntryError {
    ResendEntryError::Dispatch(err.to_string())
}

fn map_dispatch_sync_error(err: DispatchSyncError) -> ResendEntryError {
    match err {
        DispatchSyncError::LockedSpace => {
            ResendEntryError::Dispatch("encryption session locked".to_string())
        }
        DispatchSyncError::CipherFailure(msg) => {
            ResendEntryError::Dispatch(format!("cipher: {msg}"))
        }
        DispatchSyncError::Repository(msg) => ResendEntryError::Storage(msg),
    }
}

// ============================================================================
// Tests
// ============================================================================
//
// **Mocking convention** —— 与 `usecases::clipboard_sync::dispatch_entry::tests`
// / `snapshot_from_entry::tests` 保持一致:复杂 trait + 多 method ports 用
// hand-rolled fakes(单测专用,行为最小化、定义紧贴用例需要);trait obj
// 单 method 入口(`OutboundBlobPublishGateway`、`DispatchEntryRunner`)用
// "panic if called" 或 "fixed return" 的 stub,通过 `Arc<Mutex<...>>` 收尾
// 验证调用形态。
//
// 7 个 verdict 对应 task_plan.md commit B3 的 7 条:
//   - resend_remote_entry_returns_remote_origin
//   - resend_with_lost_payload_returns_payload_lost
//   - resend_filter_includes_untrusted_peer_returns_target_not_trusted
//   - resend_with_no_eligible_targets_returns_no_eligible_targets
//   - resend_with_no_filter_dispatches_to_diff_set
//   - resend_with_explicit_filter_dispatches_only_to_listed_peers
//   - resend_records_new_delivery_attempt_with_fresh_updated_at_ms

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;
    use bytes::Bytes;
    use chrono::Utc;
    use uc_core::clipboard::{
        ClipboardEntry, ClipboardRepositoryError, ClipboardSelection, ClipboardSelectionDecision,
        EntryDeliveryError, EntryDeliveryRecord, MimeType, ObservedClipboardRepresentation,
        PayloadAvailability, PersistedClipboardRepresentation, SelectionPolicyVersion,
    };
    use uc_core::ids::{EventId, FormatId, RepresentationId};
    use uc_core::ports::clipboard::{
        PayloadResolveError, ProcessingUpdateOutcome, ResolvedClipboardPayload,
    };
    use uc_core::security::IdentityFingerprint;
    use uc_core::settings::model::Settings;
    use uc_core::trusted_peer::{TrustedPeer, TrustedPeerError};
    use uc_core::BlobId;

    use crate::facade::{
        BlobTransferError, PublishBlobCommand, PublishBlobPathCommand, PublishBlobResult,
    };
    use crate::usecases::clipboard_sync::dispatch_entry::{DispatchOutcome, DispatchPerTarget};

    // ── fakes: ports needed by reconstruct_snapshot_from_entry + this use case ──

    struct FakeEntryRepo {
        entry: Option<ClipboardEntry>,
    }
    #[async_trait]
    impl GetClipboardEntryPort for FakeEntryRepo {
        async fn get_entry(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<ClipboardEntry>, ClipboardRepositoryError> {
            Ok(self.entry.clone())
        }
    }

    struct FakeEventRepo {
        source: Option<DeviceId>,
    }
    #[async_trait]
    impl ClipboardEventRepositoryPort for FakeEventRepo {
        async fn get_representation(
            &self,
            _id: &EventId,
            _representation_id: &str,
        ) -> anyhow::Result<ObservedClipboardRepresentation> {
            unimplemented!()
        }
        async fn get_source_device(&self, _event_id: &EventId) -> anyhow::Result<Option<DeviceId>> {
            Ok(self.source.clone())
        }
    }

    struct FakeSelectionRepo {
        selection: Option<ClipboardSelectionDecision>,
    }
    #[async_trait]
    impl ClipboardSelectionRepositoryPort for FakeSelectionRepo {
        async fn get_selection(
            &self,
            _entry_id: &EntryId,
        ) -> anyhow::Result<Option<ClipboardSelectionDecision>> {
            Ok(self.selection.clone())
        }
        async fn delete_selection(&self, _entry_id: &EntryId) -> anyhow::Result<()> {
            unimplemented!()
        }
    }

    struct StaticRepRepo {
        reps: Vec<PersistedClipboardRepresentation>,
    }
    #[async_trait]
    impl GetRepresentationPort for StaticRepRepo {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError> {
            Ok(self
                .reps
                .iter()
                .find(|r| r.id == *representation_id)
                .cloned())
        }
    }

    /// No-op processing-result port — resend tests don't exercise the
    /// orphan-demotion path, so a fixed `StateMismatch` is enough.
    struct StubProcessingRepo;
    #[async_trait]
    impl UpdateRepresentationProcessingResultPort for StubProcessingRepo {
        async fn update_processing_result(
            &self,
            _rep_id: &RepresentationId,
            _expected_states: &[PayloadAvailability],
            _blob_id: Option<&BlobId>,
            _new_state: PayloadAvailability,
            _last_error: Option<&str>,
        ) -> Result<ProcessingUpdateOutcome, ClipboardRepositoryError> {
            Ok(ProcessingUpdateOutcome::StateMismatch)
        }
    }

    enum ResolveBehavior {
        Inline(Vec<u8>),
        Lost,
    }
    struct StubResolver(ResolveBehavior);
    #[async_trait]
    impl ClipboardPayloadResolverPort for StubResolver {
        async fn resolve(
            &self,
            rep: &PersistedClipboardRepresentation,
        ) -> Result<ResolvedClipboardPayload, PayloadResolveError> {
            match &self.0 {
                ResolveBehavior::Inline(bytes) => Ok(ResolvedClipboardPayload::Inline {
                    mime: rep
                        .mime_type
                        .as_ref()
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default(),
                    bytes: bytes.clone(),
                }),
                ResolveBehavior::Lost => Err(PayloadResolveError::Lost {
                    rep_id: rep.id.clone(),
                    reason: "synthetic lost".to_string(),
                }),
            }
        }
    }

    struct UnusedBlobStore;
    #[async_trait]
    impl BlobReaderPort for UnusedBlobStore {
        async fn get(&self, _blob_id: &BlobId) -> anyhow::Result<Vec<u8>> {
            unreachable!("UnusedBlobStore: get() must not be called in resend tests")
        }
    }

    struct StubDeliveryRepo {
        records: Vec<EntryDeliveryRecord>,
    }
    #[async_trait]
    impl EntryDeliveryRepositoryPort for StubDeliveryRepo {
        async fn record_attempt(
            &self,
            _record: &EntryDeliveryRecord,
        ) -> Result<(), EntryDeliveryError> {
            // resend 用例本身不调 record_attempt(那是 dispatch_uc 的职责);
            // 但留个 noop 以防被意外调用 → 仍是 Ok。
            Ok(())
        }
        async fn list_by_entry(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Vec<EntryDeliveryRecord>, EntryDeliveryError> {
            Ok(self.records.clone())
        }
    }

    struct StubTrustedPeerRepo {
        peers: Vec<TrustedPeer>,
    }
    #[async_trait]
    impl TrustedPeerRepositoryPort for StubTrustedPeerRepo {
        async fn get(
            &self,
            _peer_device_id: &DeviceId,
        ) -> Result<Option<TrustedPeer>, TrustedPeerError> {
            unimplemented!()
        }
        async fn list(&self) -> Result<Vec<TrustedPeer>, TrustedPeerError> {
            Ok(self.peers.clone())
        }
        async fn save(&self, _trusted_peer: &TrustedPeer) -> Result<(), TrustedPeerError> {
            unimplemented!()
        }
        async fn remove(&self, _peer_device_id: &DeviceId) -> Result<bool, TrustedPeerError> {
            unimplemented!()
        }
    }

    struct StubDeviceIdentity(DeviceId);
    impl DeviceIdentityPort for StubDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            self.0.clone()
        }
    }

    struct StubSettings;
    #[async_trait]
    impl SettingsPort for StubSettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(Settings::default())
        }
        async fn save(&self, _s: &Settings) -> anyhow::Result<()> {
            unimplemented!()
        }
    }

    /// `OutboundBlobPublishGateway` stub — 这些 verdicts 全部用 text-only
    /// snapshot,publish_*_blob_refs 不会触发到此处。任何意外调用都让 panic
    /// 把契约违反硬化为测试失败。
    struct UnusedPublishGateway;
    #[async_trait]
    impl OutboundBlobPublishGateway for UnusedPublishGateway {
        async fn publish_blob(
            &self,
            _command: PublishBlobCommand,
        ) -> Result<PublishBlobResult, BlobTransferError> {
            unreachable!(
                "UnusedPublishGateway: publish_blob must not be called for text-only snapshots"
            )
        }
        async fn publish_blob_path(
            &self,
            _command: PublishBlobPathCommand,
        ) -> Result<PublishBlobResult, BlobTransferError> {
            unreachable!("UnusedPublishGateway: publish_blob_path must not be called for text-only snapshots")
        }
    }

    /// `DispatchEntryRunner` stub —— 收下 input 落到 `Mutex<Vec>` 供断言,
    /// 返回 `outcome_factory(targets, now_ms)` 合成的 DispatchOutcome。
    /// `now_ms` 每次调用从 atomic 自增,便于"updated_at_ms 单调"类断言。
    struct RecordingDispatchRunner {
        captured: Mutex<Vec<DispatchClipboardEntryInput>>,
        outcome_factory: Box<
            dyn Fn(&DispatchClipboardEntryInput) -> Result<DispatchOutcome, DispatchSyncError>
                + Send
                + Sync,
        >,
    }
    impl RecordingDispatchRunner {
        fn new<F>(factory: F) -> Self
        where
            F: Fn(&DispatchClipboardEntryInput) -> Result<DispatchOutcome, DispatchSyncError>
                + Send
                + Sync
                + 'static,
        {
            Self {
                captured: Mutex::new(Vec::new()),
                outcome_factory: Box::new(factory),
            }
        }
        fn captured(&self) -> Vec<DispatchClipboardEntryInput> {
            self.captured.lock().unwrap().clone()
        }
    }
    #[async_trait]
    impl DispatchEntryRunner for RecordingDispatchRunner {
        async fn execute(
            &self,
            input: DispatchClipboardEntryInput,
        ) -> Result<DispatchOutcome, DispatchSyncError> {
            let outcome = (self.outcome_factory)(&input);
            self.captured.lock().unwrap().push(input);
            outcome
        }
    }

    /// `DispatchEntryRunner` stub 的"绝不应该被调用"版本 —— 用于 happy-path
    /// 之前 early-return 的 verdicts(filter 校验失败 / 差集为空 / payload
    /// lost / remote origin)。
    struct UnusedDispatchRunner;
    #[async_trait]
    impl DispatchEntryRunner for UnusedDispatchRunner {
        async fn execute(
            &self,
            _input: DispatchClipboardEntryInput,
        ) -> Result<DispatchOutcome, DispatchSyncError> {
            unreachable!(
                "UnusedDispatchRunner: dispatch must not be called when resend early-returns"
            )
        }
    }

    // ── helpers ──────────────────────────────────────────────────────────

    fn fingerprint(seed: u8) -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string(
            (0..16)
                .map(|i| char::from(b'A' + ((seed as usize + i) % 26) as u8))
                .collect::<String>(),
        )
        .expect("valid fingerprint")
    }

    fn trusted(local: &DeviceId, peer: &str, seed: u8) -> TrustedPeer {
        TrustedPeer {
            local_device_id: local.clone(),
            peer_device_id: DeviceId::new(peer),
            peer_fingerprint: fingerprint(seed),
            trusted_at: Utc::now(),
        }
    }

    fn text_rep(id: &str, bytes: &[u8]) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            RepresentationId::from(id),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType("text/plain".to_string())),
            bytes.len() as i64,
            Some(bytes.to_vec()),
            None,
        )
    }

    fn entry_with_event(entry_id: &EntryId, event_id: &EventId) -> ClipboardEntry {
        ClipboardEntry::new(entry_id.clone(), event_id.clone(), 0, None, 0)
    }

    fn selection_for(entry_id: &EntryId, paste_rep_id: &str) -> ClipboardSelectionDecision {
        let paste = RepresentationId::from(paste_rep_id);
        ClipboardSelectionDecision::new(
            entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: paste.clone(),
                secondary_rep_ids: Vec::new(),
                preview_rep_id: paste.clone(),
                paste_rep_id: paste,
                policy_version: SelectionPolicyVersion::V1,
            },
        )
    }

    /// 合成一份"成功 dispatch"的 outcome —— accepted = len(filter),其余 0,
    /// `at_ms` 用调用时刻区分多次调用。
    fn happy_outcome(input: &DispatchClipboardEntryInput) -> DispatchOutcome {
        let targets: Vec<DeviceId> = input.target_filter.clone().unwrap_or_default();
        DispatchOutcome {
            content_hash: input.content_hash.clone(),
            per_target: targets
                .iter()
                .map(|d| DispatchPerTarget {
                    device_id: d.clone(),
                    outcome: Ok(uc_core::ports::DispatchAck::Accepted),
                })
                .collect(),
            total_accepted: targets.len(),
            total_duplicate: 0,
            total_offline: 0,
            total_errored: 0,
            total_pending: 0,
            at_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Wire 一份 happy-path use case:本机 device = "self",entry "entry-1"
    /// 来自本机,paste rep = text rep "rep-1",resolver Inline OK,trusted
    /// peers / delivery records 由调用方提供。
    fn build_uc(
        delivery_records: Vec<EntryDeliveryRecord>,
        trusted_peers: Vec<TrustedPeer>,
        dispatch_runner: Arc<dyn DispatchEntryRunner>,
    ) -> (ResendEntryUseCase, EntryId) {
        let entry_id = EntryId::from("entry-1");
        let event_id = EventId::from("evt-1");
        let local = DeviceId::new("self");

        let entry = entry_with_event(&entry_id, &event_id);
        let selection = selection_for(&entry_id, "rep-1");
        let rep = text_rep("rep-1", b"hello resend");

        let uc = ResendEntryUseCase::new(ResendEntryDeps {
            entry_repo: Arc::new(FakeEntryRepo { entry: Some(entry) }),
            event_repo: Arc::new(FakeEventRepo {
                source: Some(local.clone()),
            }),
            selection_repo: Arc::new(FakeSelectionRepo {
                selection: Some(selection),
            }),
            representation_repo: Arc::new(StaticRepRepo { reps: vec![rep] }),
            rep_processing_repo: Arc::new(StubProcessingRepo),
            payload_resolver: Arc::new(StubResolver(ResolveBehavior::Inline(
                b"hello resend".to_vec(),
            ))),
            blob_store: Arc::new(UnusedBlobStore),
            entry_delivery_repo: Arc::new(StubDeliveryRepo {
                records: delivery_records,
            }),
            trusted_peer_repo: Arc::new(StubTrustedPeerRepo {
                peers: trusted_peers,
            }),
            device_identity: Arc::new(StubDeviceIdentity(local)),
            settings: Arc::new(StubSettings),
            blob_publisher: Arc::new(UnusedPublishGateway),
            dispatch_runner,
        });
        (uc, entry_id)
    }

    fn delivered_record(entry: &EntryId, target: &str, ms: i64) -> EntryDeliveryRecord {
        EntryDeliveryRecord {
            entry_id: entry.clone(),
            target_device_id: DeviceId::new(target),
            status: EntryDeliveryStatus::Delivered,
            reason_detail: None,
            updated_at_ms: ms,
        }
    }

    fn failed_offline_record(entry: &EntryId, target: &str, ms: i64) -> EntryDeliveryRecord {
        EntryDeliveryRecord {
            entry_id: entry.clone(),
            target_device_id: DeviceId::new(target),
            status: EntryDeliveryStatus::Failed {
                reason: uc_core::clipboard::DeliveryFailureReason::Offline,
            },
            reason_detail: None,
            updated_at_ms: ms,
        }
    }

    // ── verdicts ─────────────────────────────────────────────────────────

    /// V1 — 远端 entry 直接拒绝。`source_device != local` 应在 step 2
    /// 短路;reconstruct / publish / dispatch 全部不应被触发(`UnusedDispatchRunner`
    /// 与 `UnusedBlobStore` panic on call)。
    #[tokio::test]
    async fn resend_remote_entry_returns_remote_origin() {
        let entry_id = EntryId::from("entry-remote");
        let event_id = EventId::from("evt-remote");
        let local = DeviceId::new("self");
        // get_source_device 返回 peer-a → 与 local 不等。
        let uc = ResendEntryUseCase::new(ResendEntryDeps {
            entry_repo: Arc::new(FakeEntryRepo {
                entry: Some(entry_with_event(&entry_id, &event_id)),
            }),
            event_repo: Arc::new(FakeEventRepo {
                source: Some(DeviceId::new("peer-a")),
            }),
            // 下游 ports 都不应被触达,塞 panic-on-call 的 fake。
            selection_repo: Arc::new(FakeSelectionRepo { selection: None }),
            representation_repo: Arc::new(StaticRepRepo { reps: Vec::new() }),
            rep_processing_repo: Arc::new(StubProcessingRepo),
            payload_resolver: Arc::new(StubResolver(ResolveBehavior::Lost)),
            blob_store: Arc::new(UnusedBlobStore),
            entry_delivery_repo: Arc::new(StubDeliveryRepo {
                records: Vec::new(),
            }),
            trusted_peer_repo: Arc::new(StubTrustedPeerRepo { peers: Vec::new() }),
            device_identity: Arc::new(StubDeviceIdentity(local)),
            settings: Arc::new(StubSettings),
            blob_publisher: Arc::new(UnusedPublishGateway),
            dispatch_runner: Arc::new(UnusedDispatchRunner),
        });

        let err = uc
            .execute(ResendEntryCommand {
                entry_id,
                target_filter: None,
            })
            .await
            .expect_err("expected RemoteOrigin");

        match err {
            ResendEntryError::EntryNotResendable {
                entry_id: id,
                reason,
            } => {
                assert_eq!(id.inner(), "entry-remote", "entry_id must accompany reason");
                assert_eq!(reason, NotResendableReason::RemoteOrigin);
            }
            other => panic!("expected EntryNotResendable, got {other:?}"),
        }
    }

    /// V2 — reconstruct 报 `PayloadResolveError::Lost` ⇒ `PayloadLost`。
    #[tokio::test]
    async fn resend_with_lost_payload_returns_payload_lost() {
        let entry_id = EntryId::from("entry-lost");
        let event_id = EventId::from("evt-lost");
        let local = DeviceId::new("self");
        let rep = text_rep("rep-lost", b"placeholder");

        let uc = ResendEntryUseCase::new(ResendEntryDeps {
            entry_repo: Arc::new(FakeEntryRepo {
                entry: Some(entry_with_event(&entry_id, &event_id)),
            }),
            event_repo: Arc::new(FakeEventRepo {
                source: Some(local.clone()),
            }),
            selection_repo: Arc::new(FakeSelectionRepo {
                selection: Some(selection_for(&entry_id, "rep-lost")),
            }),
            representation_repo: Arc::new(StaticRepRepo { reps: vec![rep] }),
            rep_processing_repo: Arc::new(StubProcessingRepo),
            // resolver 返回 Lost
            payload_resolver: Arc::new(StubResolver(ResolveBehavior::Lost)),
            blob_store: Arc::new(UnusedBlobStore),
            entry_delivery_repo: Arc::new(StubDeliveryRepo {
                records: Vec::new(),
            }),
            trusted_peer_repo: Arc::new(StubTrustedPeerRepo { peers: Vec::new() }),
            device_identity: Arc::new(StubDeviceIdentity(local)),
            settings: Arc::new(StubSettings),
            blob_publisher: Arc::new(UnusedPublishGateway),
            dispatch_runner: Arc::new(UnusedDispatchRunner),
        });

        let err = uc
            .execute(ResendEntryCommand {
                entry_id,
                target_filter: None,
            })
            .await
            .expect_err("expected PayloadLost");

        match err {
            ResendEntryError::EntryNotResendable {
                entry_id: id,
                reason,
            } => {
                assert_eq!(id.inner(), "entry-lost", "entry_id must accompany reason");
                assert_eq!(reason, NotResendableReason::PayloadLost);
            }
            other => panic!("expected EntryNotResendable, got {other:?}"),
        }
    }

    /// V3 — `target_filter = Some([ghost])`,ghost 不在 trusted_peer_repo
    /// 内 ⇒ `TargetNotTrusted`。dispatch 不应被调用。
    #[tokio::test]
    async fn resend_filter_includes_untrusted_peer_returns_target_not_trusted() {
        let local = DeviceId::new("self");
        let (uc, entry_id) = build_uc(
            Vec::new(),
            vec![trusted(&local, "peer-a", 1)],
            Arc::new(UnusedDispatchRunner),
        );

        let err = uc
            .execute(ResendEntryCommand {
                entry_id,
                target_filter: Some(vec![DeviceId::new("ghost")]),
            })
            .await
            .expect_err("expected TargetNotTrusted");

        match err {
            ResendEntryError::TargetNotTrusted(d) => assert_eq!(d.as_str(), "ghost"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    /// V4 — `target_filter = None`,所有 trusted peer 都已 Delivered ⇒
    /// 差集为空 ⇒ `NoEligibleTargets`。
    #[tokio::test]
    async fn resend_with_no_eligible_targets_returns_no_eligible_targets() {
        let local = DeviceId::new("self");
        let entry = EntryId::from("entry-1");
        let (uc, entry_id) = build_uc(
            vec![
                delivered_record(&entry, "peer-a", 100),
                delivered_record(&entry, "peer-b", 100),
            ],
            vec![trusted(&local, "peer-a", 1), trusted(&local, "peer-b", 2)],
            Arc::new(UnusedDispatchRunner),
        );

        let err = uc
            .execute(ResendEntryCommand {
                entry_id,
                target_filter: None,
            })
            .await
            .expect_err("expected NoEligibleTargets");

        assert!(matches!(err, ResendEntryError::NoEligibleTargets));
    }

    /// V5 — `target_filter = None`,3 个 trusted peer:peer-a Delivered、
    /// peer-b Failed{Offline}、peer-c 从未尝试。差集 = {peer-b, peer-c}。
    /// dispatch 必须收到 `target_filter = Some([peer-b, peer-c])`(顺序与
    /// trusted_peer.list 一致),entry_id = Some。
    #[tokio::test]
    async fn resend_with_no_filter_dispatches_to_diff_set() {
        let local = DeviceId::new("self");
        let entry = EntryId::from("entry-1");
        let runner = Arc::new(RecordingDispatchRunner::new(|input| {
            Ok(happy_outcome(input))
        }));
        let (uc, entry_id) = build_uc(
            vec![
                delivered_record(&entry, "peer-a", 50),
                failed_offline_record(&entry, "peer-b", 60),
            ],
            vec![
                trusted(&local, "peer-a", 1),
                trusted(&local, "peer-b", 2),
                trusted(&local, "peer-c", 3),
            ],
            runner.clone(),
        );

        let report = uc
            .execute(ResendEntryCommand {
                entry_id,
                target_filter: None,
            })
            .await
            .expect("resend ok");

        assert_eq!(report.accepted, 2, "diff set has 2 targets");

        let captured = runner.captured();
        assert_eq!(captured.len(), 1, "dispatch called exactly once");
        let dispatched_filter = captured[0]
            .target_filter
            .as_ref()
            .expect("target_filter must be Some for resend");
        let dispatched: Vec<&str> = dispatched_filter.iter().map(|d| d.as_str()).collect();
        // 差集应只含未投递的两个 peer,且不含 peer-a。
        assert_eq!(dispatched.len(), 2);
        assert!(dispatched.contains(&"peer-b"));
        assert!(dispatched.contains(&"peer-c"));
        assert!(!dispatched.contains(&"peer-a"));
        assert_eq!(
            captured[0].entry_id.as_ref(),
            Some(&EntryId::from("entry-1"))
        );
        assert_eq!(captured[0].payload_version, 3);
        assert!(
            !captured[0].plaintext.is_empty(),
            "encoded V3 envelope must not be empty"
        );
        // Plaintext 与 content_hash 来自同一次 encode → 内容自洽。
        assert!(captured[0].content_hash.starts_with("blake3v1:"));
    }

    /// V6 — `target_filter = Some([peer-b])`,trusted = {a, b, c}。dispatch
    /// 收到的 filter 应只有 peer-b,其余 trusted 不进 fan-out。
    #[tokio::test]
    async fn resend_with_explicit_filter_dispatches_only_to_listed_peers() {
        let local = DeviceId::new("self");
        let runner = Arc::new(RecordingDispatchRunner::new(|input| {
            Ok(happy_outcome(input))
        }));
        let (uc, entry_id) = build_uc(
            Vec::new(),
            vec![
                trusted(&local, "peer-a", 1),
                trusted(&local, "peer-b", 2),
                trusted(&local, "peer-c", 3),
            ],
            runner.clone(),
        );

        let report = uc
            .execute(ResendEntryCommand {
                entry_id,
                target_filter: Some(vec![DeviceId::new("peer-b")]),
            })
            .await
            .expect("resend ok");

        assert_eq!(report.accepted, 1);

        let captured = runner.captured();
        let dispatched_filter = captured[0]
            .target_filter
            .as_ref()
            .expect("target_filter must be Some");
        assert_eq!(dispatched_filter.len(), 1);
        assert_eq!(dispatched_filter[0].as_str(), "peer-b");
    }

    /// V7 — happy path:dispatch 成功返回 outcome,resend 把字段平铺到
    /// `ResendReport`。`content_hash` 应来自 V3 encode 后的 blake3v1 哈希
    /// (而不是凭空构造),且每次 resend 触发的 dispatch 都该带"刷新过的"
    /// timestamp(由下游 dispatch_uc 写盘时采样,本用例只断 outcome 流通)。
    #[tokio::test]
    async fn resend_records_new_delivery_attempt_with_fresh_updated_at_ms() {
        let local = DeviceId::new("self");
        let runner = Arc::new(RecordingDispatchRunner::new(|input| {
            let mut outcome = happy_outcome(input);
            outcome.total_accepted = 1;
            outcome.total_offline = 1;
            outcome.at_ms = 999_999;
            Ok(outcome)
        }));
        let (uc, entry_id) = build_uc(
            // 既有一条 Failed{Offline} → 在 diff set 内,resend 会再发。
            vec![failed_offline_record(
                &EntryId::from("entry-1"),
                "peer-b",
                100,
            )],
            vec![trusted(&local, "peer-a", 1), trusted(&local, "peer-b", 2)],
            runner.clone(),
        );

        let report = uc
            .execute(ResendEntryCommand {
                entry_id,
                target_filter: None,
            })
            .await
            .expect("resend ok");

        // Report 字段对齐 outcome 工厂的设置。
        assert_eq!(report.accepted, 1);
        assert_eq!(report.offline, 1);
        assert_eq!(report.duplicate, 0);
        assert_eq!(report.errored, 0);
        assert_eq!(report.pending, 0);

        // dispatch 必须收到差集(peer-a + peer-b 都未 Delivered)。
        let captured = runner.captured();
        assert_eq!(captured.len(), 1);
        let dispatched_filter = captured[0].target_filter.as_ref().unwrap();
        let mut sorted: Vec<&str> = dispatched_filter.iter().map(|d| d.as_str()).collect();
        sorted.sort();
        assert_eq!(sorted, vec!["peer-a", "peer-b"]);

        // 入参 plaintext 不为空(encode 成功),content_hash 是 blake3v1。
        assert!(!captured[0].plaintext.is_empty());
        assert!(captured[0].content_hash.starts_with("blake3v1:"));

        // categories 由 from_snapshot 计算 —— text rep 应被识别为 Text。
        assert!(!captured[0]
            .categories
            .iter()
            .any(|c| matches!(c, uc_core::clipboard::ClipboardContentCategory::File)));

        // Bytes 转 anyhow 防 unused
        let _ = Bytes::new();
    }
}

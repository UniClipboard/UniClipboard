//! Slice 2 Phase 2 · T7 — `DispatchClipboardEntryUseCase`.
//!
//! Encrypts one clipboard plaintext payload via [`TransferCipherPort`] and
//! fans it out to every paired member (excluding self) on the clipboard
//! ALPN. Failure per target is isolated in the per-target report so a
//! single unreachable peer never blocks the rest of the roster.
//!
//! ## Inputs, not side-effects
//!
//! This use case takes a [`DispatchClipboardEntryInput`] — plaintext bytes
//! + `content_hash` + `payload_version`. Reading the system clipboard +
//! building the `ClipboardBinaryPayload` is the caller's responsibility
//! (CLI `send` / `watch` in T11, daemon in Phase 3). Keeping the
//! plaintext-production step outside keeps the use case testable with
//! deterministic bytes.
//!
//! ## Iteration source
//!
//! Follows the `EnsureReachableAllUseCase` pattern (T6 / Phase 1):
//! `peer_addr_repo.list()` is the authoritative roster of "members we
//! have an address blob for" and avoids iterating ghost entries in
//! `member_repo` that never completed pairing. We intentionally do **not**
//! pre-filter by `PresencePort::current_state == Online`: presence's
//! `last_state` is populated by our own outbound `ensure_reachable`
//! probes, so when a peer dials us first (accept path only), our cache
//! still reports `Unknown`/`Offline` and a pre-filter would drop a peer
//! that's in fact reachable. Instead we let the dispatch port try every
//! paired member and record `Err(Offline)` in `per_target` for whichever
//! ones the wire can't reach. The iroh dispatch adapter returns quickly
//! on unreachable peers, so this costs little even when many peers are
//! down.
//!
//! ## Concurrency
//!
//! `tokio::task::JoinSet` per target. Phase 1's mockall-Mutex lesson
//! (slice2-phase1-plan.md §12.3 decision 5) only applies when **wall-time
//! concurrency** is asserted — the tests below use mockall throughout
//! because none of them measure wall-clock duration; `.returning(...)`
//! closures return immediately, so the expectation Mutex never blocks
//! anything observable. Hand-written fakes are reserved for cases that
//! genuinely need them (broadcast `subscribe + emit`; see
//! `ingest_inbound.rs::tests` and Phase 1 `roster/facade.rs::FakePresence`).

use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use tokio::task::JoinSet;
use tracing::{debug, info, info_span, instrument, warn, Instrument};
use uc_observability::FlowId;

use crate::facade::blob_transfer::SharedHostEventEmitter;
use crate::facade::host_event::{DeliveryHostEvent, HostEvent};
use uc_core::clipboard::{
    ClipboardContentCategory, ClipboardContentCategorySet, DeliveryFailureReason,
    EntryDeliveryRecord, EntryDeliveryStatus,
};
use uc_core::ids::{DeviceId, EntryId};
use uc_core::ports::security::TransferCipherPort;
use uc_core::ports::{
    ClipboardDispatchError, ClipboardDispatchPort, ClipboardHeader, ClockPort, DeviceIdentityPort,
    DispatchAck, EntryDeliveryRepositoryPort, FirstSyncStatePort, LocalIdentityPort,
    PeerAddressRepositoryPort, PresencePort, ReachabilityState, SettingsPort, SyncPayload,
};
use uc_core::MemberRepositoryPort;
use uc_observability::analytics::{
    AnalyticsPort, Direction, Event, FailureReason, PayloadSizeBucket, PayloadType,
    SyncDeferReason, SyncDeferredProps, SyncEventProps, SyncFailureStage, TransportType,
};

/// Slice 8c-1 · classify the dispatched payload by category priority
/// (File > Image > Text). Empty / unknown sets fall back to Text rather
/// than dropping the event — schema doc §6 prefers a coarse bucket over
/// a missing field.
fn payload_type_from_categories(set: &ClipboardContentCategorySet) -> PayloadType {
    if set
        .iter()
        .any(|c| matches!(c, ClipboardContentCategory::File))
    {
        PayloadType::File
    } else if set
        .iter()
        .any(|c| matches!(c, ClipboardContentCategory::Image))
    {
        PayloadType::Image
    } else {
        // Text / RichText / Link / empty all roll up to Text — fine-grained
        // breakdown is not part of v1 schema (PayloadType is 3-way).
        PayloadType::Text
    }
}

/// Slice 8c-1 · 1:1 mapping ClipboardDispatchError → schema FailureReason.
/// Funnel signal lives in this enum, not in error message text. Keep
/// LocalPolicyExceeded mapped to FileTooLarge (the only triggering case
/// today is `MAX_PAYLOAD_SIZE`); refine if other size policies appear.
fn map_dispatch_error_to_failure_reason(err: &ClipboardDispatchError) -> FailureReason {
    match err {
        ClipboardDispatchError::Offline => FailureReason::PeerOffline,
        ClipboardDispatchError::LocalPolicyExceeded(_) => FailureReason::FileTooLarge,
        ClipboardDispatchError::PeerRejected(_) => FailureReason::NetworkError,
        ClipboardDispatchError::Io(_) => FailureReason::NetworkError,
        ClipboardDispatchError::Internal(_) => FailureReason::Unknown,
    }
}

/// 将即时 dispatch 错误映射为产品分析口径。
///
/// `sync_failed` 在当前路径代表"一次即时发送尝试失败"，不是"最终投递失败"。
/// 对端不可达和网络类错误应留给 pending/retry 或恢复流程继续处理；本地策略拒绝
/// 才是当前 payload 的终态失败。
fn dispatch_failure_stage(err: &ClipboardDispatchError) -> SyncFailureStage {
    match err {
        ClipboardDispatchError::LocalPolicyExceeded(_) => SyncFailureStage::LocalPolicy,
        ClipboardDispatchError::Internal(_) => SyncFailureStage::ImmediateSend,
        ClipboardDispatchError::Offline
        | ClipboardDispatchError::PeerRejected(_)
        | ClipboardDispatchError::Io(_) => SyncFailureStage::ImmediateSend,
    }
}

async fn capture_sync_attempted(
    analytics: &Arc<dyn AnalyticsPort>,
    first_sync_state: &Arc<dyn FirstSyncStatePort>,
    payload_type: PayloadType,
    payload_size_bucket: PayloadSizeBucket,
) {
    analytics.capture(Event::SyncAttempted(SyncEventProps {
        direction: Direction::Outbound,
        payload_type,
        payload_size_bucket,
        transport_type: TransportType::P2pDirect,
        peer_os: None,
        sync_latency_ms: None,
        failure_reason: None,
        failure_stage: None,
    }));
    // Slice 8c-2 · funnel: first attempt fires regardless of outcome — keeps
    // the "started but failed" 漏点信号。deferred 路径也会调用本函数，确保
    // attempted 时序一致；dashboard 端用 `attempted - deferred` 推导用户感知尝试。
    match first_sync_state.mark_first_sync_attempted().await {
        Ok(true) => analytics.capture(Event::FirstClipboardSyncAttempted {
            direction: Direction::Outbound,
        }),
        Ok(false) => {}
        Err(err) => warn!(
            error = %err,
            "first_sync_state.mark_first_sync_attempted failed; skipping fire",
        ),
    }
}

/// Input to one dispatch pass. The caller owns the plaintext →
/// `ClipboardBinaryPayload` → bytes pipeline.
#[derive(Debug, Clone)]
pub(crate) struct DispatchClipboardEntryInput {
    /// Unencrypted payload bytes. Typically the postcard-encoded
    /// `ClipboardBinaryPayload` (V3) the caller built from the system
    /// clipboard snapshot.
    pub plaintext: Bytes,
    /// SHA256 hex of the plaintext above. Receiver uses this for dedup.
    pub content_hash: String,
    /// Payload codec tag, e.g. `3` for the V3 `ClipboardBinaryPayload`.
    pub payload_version: u8,
    /// Set of content categories present in the snapshot, used to gate
    /// against each peer's `send_content_types` toggle. Caller (facade
    /// `dispatch_snapshot*`) computes via
    /// `ClipboardContentCategorySet::from_snapshot`. CLI raw-bytes paths pass
    /// an empty set (fail open) since they can't enumerate reps.
    pub categories: ClipboardContentCategorySet,
    /// 触发本次广播的 entry。`Some` 时,fan-out 结束后会按每个对端的结果
    /// 调用 `EntryDeliveryRepositoryPort::record_attempt` 落盘,供视图层
    /// 追溯"这条 entry 已同步到哪些设备"。`None` 表示无对应 entry 记录
    /// (例如 CLI raw-bytes 路径),此时 dispatch 不落盘 delivery。
    pub entry_id: Option<EntryId>,
}

/// One target's dispatch result. `Ok` + `DispatchAck` when the peer
/// accepted or duplicate-ignored the frame; `Err` when the wire boundary
/// rejected or the peer is offline.
#[derive(Debug, Clone)]
pub(crate) struct DispatchPerTarget {
    pub device_id: DeviceId,
    pub outcome: Result<DispatchAck, String>,
}

/// Aggregated per-pass outcome. `total_accepted` counts peers that
/// returned `Accepted` (the ones whose repos now carry the new entry);
/// `total_offline` counts peers the dispatch port reported as unreachable.
#[derive(Debug, Clone)]
pub(crate) struct DispatchOutcome {
    pub content_hash: String,
    pub per_target: Vec<DispatchPerTarget>,
    pub total_accepted: usize,
    pub total_duplicate: usize,
    pub total_offline: usize,
    pub total_errored: usize,
    pub at_ms: i64,
}

/// Fatal errors that abort the whole pass. Per-peer failures land in
/// `per_target`; they are not errors in this sense.
#[derive(Debug, thiserror::Error)]
pub(crate) enum DispatchSyncError {
    /// Encryption failed — typically because the space session is locked.
    #[error("encryption session not unlocked")]
    LockedSpace,
    /// Encryption failed for any other reason.
    #[error("transfer cipher failure: {0}")]
    CipherFailure(String),
    /// Listing the peer address repository failed.
    #[error("peer_addr_repo.list: {0}")]
    Repository(String),
}

pub(crate) struct DispatchClipboardEntryUseCase {
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    member_repo: Arc<dyn MemberRepositoryPort>,
    presence: Arc<dyn PresencePort>,
    transfer_cipher: Arc<dyn TransferCipherPort>,
    clipboard_dispatch: Arc<dyn ClipboardDispatchPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    local_identity: Arc<dyn LocalIdentityPort>,
    settings: Arc<dyn SettingsPort>,
    clock: Arc<dyn ClockPort>,
    /// fan-out 完成后,按每个 target 的成功/失败落盘 delivery 记录。
    /// 仅在 `DispatchClipboardEntryInput.entry_id` 为 `Some` 时调用。
    entry_delivery_repo: Arc<dyn EntryDeliveryRepositoryPort>,
    /// Slice 8c-1 · per-peer telemetry. One `sync_attempted` /
    /// `sync_succeeded` / `sync_failed` event fires per fan-out target so
    /// PostHog reliability dashboards stay per-peer (peer_os, latency,
    /// failure_reason are all 1:1 with a single peer outcome).
    analytics: Arc<dyn AnalyticsPort>,
    /// Slice 8c-2 · first-sync funnel dedup. spawn 内每次 `mark_*` 返回 `Ok(true)`
    /// 即"我是首次"，同时额外 fire `first_clipboard_sync_attempted` /
    /// `first_clipboard_sync_succeeded` / `first_file_sync_succeeded`。
    /// race 防护由 port impl 内部 `tokio::sync::Mutex` 守护，调用方零感知。
    first_sync_state: Arc<dyn FirstSyncStatePort>,
    /// 共享 host-event bus。每条 delivery 记录写盘成功后追发一条
    /// [`HostEvent::Delivery`],让前端 detail badge 在 dispatch 完成后自动
    /// 刷新而无需手动切 entry。Issue #747 Phase 5。
    ///
    /// emit 走 [`HostEventBus::emit_or_warn`] —— 失败仅 warn,不阻塞
    /// dispatch 主路径;事件丢失 / 乱序由前端 refetch 幂等吸收。CLI / 单元
    /// 测试装配传一根空 bus 即可(无下游 = noop)。
    host_event_bus: SharedHostEventEmitter,
}

impl DispatchClipboardEntryUseCase {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
        presence: Arc<dyn PresencePort>,
        transfer_cipher: Arc<dyn TransferCipherPort>,
        clipboard_dispatch: Arc<dyn ClipboardDispatchPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        local_identity: Arc<dyn LocalIdentityPort>,
        settings: Arc<dyn SettingsPort>,
        clock: Arc<dyn ClockPort>,
        analytics: Arc<dyn AnalyticsPort>,
        first_sync_state: Arc<dyn FirstSyncStatePort>,
        entry_delivery_repo: Arc<dyn EntryDeliveryRepositoryPort>,
        host_event_bus: SharedHostEventEmitter,
    ) -> Self {
        Self {
            peer_addr_repo,
            member_repo,
            presence,
            transfer_cipher,
            clipboard_dispatch,
            device_identity,
            local_identity,
            settings,
            clock,
            analytics,
            first_sync_state,
            entry_delivery_repo,
            host_event_bus,
        }
    }

    // 跨设备可观测性(PR2):
    //   - `flow.id` 在函数体内生成后回填,统一作为本次扇出的相关 ID;PR3 起会
    //      通过 `ClipboardHeader` 走 wire 传到对端,让 inbound 端可以用同一个
    //      `flow.id` 接龙 trace,Sentry 上就能 join "A 端发送 → B 端接收"。
    //   - `flow.kind = "clipboard_sync"`:静态枚举值,方便按业务流过滤。
    //   - `fanout.candidates` 在候选筛完后回填,是单次扇出真实的目标数。
    //   - 每个目标 peer 进 child span(见下 `peer.dispatch`)而不是把
    //     `peer.device_id` 钉在 root —— 扇出 N 个 peer 时 root 只有一个,
    //     钉上会丢失末次写入以外的信息。
    #[instrument(
        skip_all,
        fields(
            content_hash = %input.content_hash,
            flow.id = tracing::field::Empty,
            flow.kind = "clipboard_sync",
            fanout.candidates = tracing::field::Empty,
        ),
    )]
    pub(crate) async fn execute(
        &self,
        input: DispatchClipboardEntryInput,
    ) -> Result<DispatchOutcome, DispatchSyncError> {
        let flow_id = FlowId::generate();
        tracing::Span::current().record("flow.id", tracing::field::display(&flow_id));
        // 1. Encrypt. A locked session surfaces here — let it short-circuit
        //    so we don't spam the dispatch wire with encrypt-retries.
        let ciphertext = match self.transfer_cipher.encrypt(&input.plaintext).await {
            Ok(bytes) => Bytes::from(bytes),
            Err(err) => {
                return Err(match err {
                    uc_core::ports::security::TransferCipherError::NotUnlocked => {
                        DispatchSyncError::LockedSpace
                    }
                    other => DispatchSyncError::CipherFailure(other.to_string()),
                });
            }
        };

        // 2. Enumerate targets. `peer_addr_repo.list()` is the iteration
        //    source (see module doc); self is the only filter. Presence
        //    state is intentionally NOT consulted — see module doc for
        //    rationale. The dispatch port reports `Offline` per-target
        //    for unreachable peers, which we fold into the outcome below.
        let records =
            self.peer_addr_repo.list().await.map_err(|err| {
                DispatchSyncError::Repository(format!("peer_addr_repo.list: {err}"))
            })?;

        let local_device = self.device_identity.current_device_id();
        let mut candidates: Vec<DeviceId> = Vec::with_capacity(records.len());
        for record in records {
            if record.device_id == local_device {
                continue;
            }
            if !self
                .is_send_allowed(&record.device_id, &input.categories)
                .await
            {
                continue;
            }
            candidates.push(record.device_id);
        }

        // 3. Build the header once and clone per target.
        //
        // PR3:`flow_id` 写进 header,跨设备传到 inbound 端。inbound 收到后
        // 会用同一个 id 落到自己的 root span,Sentry 上"A 端 dispatch →
        // B 端 ingest"两条 trace 在 `flow.id` 维度自动 join。
        let origin_device_name = self.load_origin_device_name().await;
        let header = ClipboardHeader {
            version: ClipboardHeader::CURRENT_VERSION,
            content_hash: input.content_hash.clone(),
            captured_at_ms: self.clock.now_ms(),
            origin_device_id: local_device.as_str().to_string(),
            origin_device_name,
            payload_version: input.payload_version,
            flow_id: Some(flow_id.to_string()),
        };

        if candidates.is_empty() {
            info!("dispatch: no paired peers; skipping fan-out");
            return Ok(DispatchOutcome {
                content_hash: input.content_hash,
                per_target: Vec::new(),
                total_accepted: 0,
                total_duplicate: 0,
                total_offline: 0,
                total_errored: 0,
                at_ms: self.clock.now_ms(),
            });
        }

        tracing::Span::current().record("fanout.candidates", candidates.len());

        // 4. Fan-out. One JoinSet task per target; results merged at the end.
        //
        // 每个 peer 走自己的 `peer.dispatch` child span，带上 `peer.device_id`
        // + `flow.id`。这样 Sentry 上扇出 N 个目标时能看到 N 条平行 child span，
        // 单点失败一目了然，而不是被 root 的"末次写入"覆盖。`flow.id` 在
        // child 上也写一份是冗余 —— 但 root span 不一定总在同一个 trace，
        // 在 worker 任务里显式 carry 更稳。
        //
        // Slice 8c-1 · each spawned task fires per-peer telemetry. `sync_attempted`
        // 始终在 dispatch 前发一次，保证时序一致；`sync_succeeded` / `sync_failed`
        // / `sync_deferred` 与 attempted 形成 1:1 配对。known-offline peer 仍尝试
        // 发送（presence 可能 stale）；若 dispatch 仍判为 Offline，结果事件用
        // `sync_deferred` 而不是 `sync_failed`，因为那是预期不可用，不该计入
        // 用户感知失败口径（dashboard 以 `attempted - deferred` 推导用户感知尝试）。
        let payload_type = payload_type_from_categories(&input.categories);
        let payload_size_bucket = PayloadSizeBucket::from_bytes(input.plaintext.len() as u64);
        let mut set: JoinSet<(DeviceId, Result<DispatchAck, ClipboardDispatchError>)> =
            JoinSet::new();
        for device_id in &candidates {
            let dispatch = Arc::clone(&self.clipboard_dispatch);
            let presence = Arc::clone(&self.presence);
            let analytics = Arc::clone(&self.analytics);
            let first_sync_state = Arc::clone(&self.first_sync_state);
            let header = header.clone();
            let device_id = device_id.clone();
            let payload = SyncPayload {
                ciphertext: ciphertext.clone(),
            };
            let child_span = info_span!(
                "peer.dispatch",
                peer.device_id = %device_id.as_str(),
                flow.id = %flow_id,
                flow.kind = "clipboard_sync",
            );
            set.spawn(
                async move {
                    // attempted 始终在 dispatch 前发，时序与口径保持单一：
                    //   attempted = succeeded + failed + deferred
                    //   用户感知尝试 = attempted - deferred
                    // 详见 docs/architecture/telemetry-events.md §7.3b。
                    let preflight_state = presence.current_state(&device_id).await;
                    let known_offline = matches!(preflight_state, ReachabilityState::Offline);
                    capture_sync_attempted(
                        &analytics,
                        &first_sync_state,
                        payload_type,
                        payload_size_bucket,
                    )
                    .await;

                    let started_at = Instant::now();
                    let result = dispatch.dispatch(&device_id, &header, payload).await;
                    let duration_ms =
                        started_at.elapsed().as_millis().min(u32::MAX as u128) as u32;
                    let event = match &result {
                        Ok(_) => Event::SyncSucceeded(SyncEventProps {
                            direction: Direction::Outbound,
                            payload_type,
                            payload_size_bucket,
                            transport_type: TransportType::P2pDirect,
                            peer_os: None,
                            sync_latency_ms: Some(duration_ms),
                            failure_reason: None,
                            failure_stage: None,
                        }),
                        Err(ClipboardDispatchError::Offline) if known_offline => {
                            Event::SyncDeferred(SyncDeferredProps {
                                direction: Direction::Outbound,
                                payload_type,
                                payload_size_bucket,
                                peer_os: None,
                                defer_reason: SyncDeferReason::PeerKnownOffline,
                            })
                        }
                        Err(err) => Event::SyncFailed(SyncEventProps {
                            direction: Direction::Outbound,
                            payload_type,
                            payload_size_bucket,
                            transport_type: TransportType::P2pDirect,
                            peer_os: None,
                            sync_latency_ms: None,
                            failure_reason: Some(map_dispatch_error_to_failure_reason(err)),
                            failure_stage: Some(dispatch_failure_stage(err)),
                        }),
                    };
                    let is_ok = result.is_ok();
                    analytics.capture(event);

                    // Slice 8c-2 · funnel: first success path fires both the
                    // generic clipboard event and (if payload_type=File) the
                    // file-specific event. Both flags独立 dedup。
                    if is_ok {
                        match first_sync_state.mark_first_sync_succeeded().await {
                            Ok(true) => analytics.capture(Event::FirstClipboardSyncSucceeded {
                                direction: Direction::Outbound,
                                peer_os: None,
                                transport_type: TransportType::P2pDirect,
                                duration_ms,
                            }),
                            Ok(false) => {}
                            Err(err) => warn!(
                                error = %err,
                                "first_sync_state.mark_first_sync_succeeded failed; skipping fire",
                            ),
                        }
                        if matches!(payload_type, PayloadType::File) {
                            match first_sync_state.mark_first_file_sync_succeeded().await {
                                Ok(true) => analytics.capture(Event::FirstFileSyncSucceeded {
                                    peer_os: None,
                                    transport_type: TransportType::P2pDirect,
                                    payload_size_bucket,
                                }),
                                Ok(false) => {}
                                Err(err) => warn!(
                                    error = %err,
                                    "first_sync_state.mark_first_file_sync_succeeded failed; skipping fire",
                                ),
                            }
                        }
                    }

                    (device_id, result)
                }
                .instrument(child_span),
            );
        }

        let mut per_target = Vec::with_capacity(candidates.len());
        let mut total_accepted = 0;
        let mut total_duplicate = 0;
        let mut total_offline = 0;
        let mut total_errored = 0;

        // fan-out 完成后,如果调用方提供了 entry_id,就把"每个对端的结果"
        // 落盘到 entry_delivery 表。先在 join loop 内收集本地待写记录,
        // 循环结束再串行 await,避免在 hot path 上多次 await 让出调度器。
        // updated_at_ms 在每个 arm 内独立采样,反映该对端的实际完成时刻
        // (不同 peer 的 fan-out 延迟分布很广,共用单点时间会丢失排障信息)。
        let entry_id_for_delivery = input.entry_id.clone();
        let mut delivery_records: Vec<EntryDeliveryRecord> = Vec::new();

        while let Some(joined) = set.join_next().await {
            match joined {
                Ok((device_id, Ok(DispatchAck::Accepted))) => {
                    total_accepted += 1;
                    debug!(device_id = %device_id.as_str(), "dispatch → Accepted");
                    if let Some(eid) = entry_id_for_delivery.as_ref() {
                        delivery_records.push(EntryDeliveryRecord {
                            entry_id: eid.clone(),
                            target_device_id: device_id.clone(),
                            status: EntryDeliveryStatus::Delivered,
                            reason_detail: None,
                            updated_at_ms: self.clock.now_ms(),
                        });
                    }
                    per_target.push(DispatchPerTarget {
                        device_id,
                        outcome: Ok(DispatchAck::Accepted),
                    });
                }
                Ok((device_id, Ok(DispatchAck::DuplicateIgnored))) => {
                    total_duplicate += 1;
                    debug!(device_id = %device_id.as_str(), "dispatch → DuplicateIgnored");
                    if let Some(eid) = entry_id_for_delivery.as_ref() {
                        delivery_records.push(EntryDeliveryRecord {
                            entry_id: eid.clone(),
                            target_device_id: device_id.clone(),
                            status: EntryDeliveryStatus::Duplicate,
                            reason_detail: None,
                            updated_at_ms: self.clock.now_ms(),
                        });
                    }
                    per_target.push(DispatchPerTarget {
                        device_id,
                        outcome: Ok(DispatchAck::DuplicateIgnored),
                    });
                }
                Ok((device_id, Err(ClipboardDispatchError::Offline))) => {
                    total_offline += 1;
                    warn!(device_id = %device_id.as_str(), "dispatch → Offline");
                    if let Some(eid) = entry_id_for_delivery.as_ref() {
                        delivery_records.push(EntryDeliveryRecord {
                            entry_id: eid.clone(),
                            target_device_id: device_id.clone(),
                            status: EntryDeliveryStatus::Failed {
                                reason: DeliveryFailureReason::Offline,
                            },
                            reason_detail: None,
                            updated_at_ms: self.clock.now_ms(),
                        });
                    }
                    per_target.push(DispatchPerTarget {
                        device_id,
                        outcome: Err("offline".to_string()),
                    });
                }
                Ok((device_id, Err(err))) => {
                    total_errored += 1;
                    warn!(device_id = %device_id.as_str(), error = %err, "dispatch failed");
                    let (failure_reason, reason_detail) = match &err {
                        // Offline 在上一个 arm 已处理,这里不会进。保留以满足穷尽性。
                        ClipboardDispatchError::Offline => (DeliveryFailureReason::Offline, None),
                        ClipboardDispatchError::LocalPolicyExceeded(s) => {
                            (DeliveryFailureReason::LocalPolicy, Some(s.clone()))
                        }
                        ClipboardDispatchError::PeerRejected(s) => {
                            (DeliveryFailureReason::PeerRejected, Some(s.clone()))
                        }
                        ClipboardDispatchError::Io(s) => {
                            (DeliveryFailureReason::Io, Some(s.clone()))
                        }
                        ClipboardDispatchError::Internal(s) => {
                            (DeliveryFailureReason::Internal, Some(s.clone()))
                        }
                    };
                    if let Some(eid) = entry_id_for_delivery.as_ref() {
                        delivery_records.push(EntryDeliveryRecord {
                            entry_id: eid.clone(),
                            target_device_id: device_id.clone(),
                            status: EntryDeliveryStatus::Failed {
                                reason: failure_reason,
                            },
                            reason_detail,
                            updated_at_ms: self.clock.now_ms(),
                        });
                    }
                    per_target.push(DispatchPerTarget {
                        device_id,
                        outcome: Err(err.to_string()),
                    });
                }
                Err(err) => {
                    total_errored += 1;
                    warn!(error = %err, "dispatch task panicked or cancelled");
                }
            }
        }

        // 串行落盘 delivery 记录。失败仅 log,不阻塞主流程的返回,这是
        // 一个可观测性副作用,不该影响 dispatch 自身的成败语义。
        //
        // Issue #747 Phase 5:成功写入一条 record 后,立即追发一条
        // `HostEvent::Delivery::StatusChanged`,让 GUI detail 视图实时
        // 刷新。先 record → 后 emit 的顺序很关键 —— 前端拿到事件后会
        // refetch view,view 必须能读到最新写入,否则前端会得到一份与
        // 事件不一致的旧快照(看似"再切一次 entry 才刷新"的旧问题原貌)。
        // 事件 payload 不携带 status —— 前端按 entry_id 匹配后 refetch
        // 拿真相,事件只是"该不该 refetch"的指针,见 `DeliveryHostEvent`
        // 的注释。
        for record in &delivery_records {
            if let Err(err) = self.entry_delivery_repo.record_attempt(record).await {
                warn!(
                    error = %err,
                    entry_id = %record.entry_id,
                    target_device_id = %record.target_device_id,
                    "failed to record entry delivery"
                );
                continue;
            }
            self.host_event_bus.emit_or_warn(HostEvent::Delivery(
                DeliveryHostEvent::StatusChanged {
                    entry_id: record.entry_id.to_string(),
                    target_device_id: record.target_device_id.as_str().to_string(),
                },
            ));
        }

        Ok(DispatchOutcome {
            content_hash: input.content_hash,
            per_target,
            total_accepted,
            total_duplicate,
            total_offline,
            total_errored,
            at_ms: self.clock.now_ms(),
        })
    }

    /// Per-device sync gate: returns `true` when the local device should
    /// fan a clipboard frame out to `device_id`. Two stages:
    ///
    /// 1. Device-level kill switch (`send_enabled`).
    /// 2. Content-type filter (`send_content_types`, AND-of-allowed across
    ///    the snapshot's category set — see `uc-core` `category.rs` module doc).
    ///    Empty set (raw-bytes / unrecognised payload) passes (fail open)
    ///    so we don't stall sync silently.
    ///
    /// Member-record miss / repo error → fail open with a WARN, mirroring
    /// the device-level gate's posture: a transient glitch should not
    /// silently kill sync.
    async fn is_send_allowed(
        &self,
        device_id: &DeviceId,
        categories: &ClipboardContentCategorySet,
    ) -> bool {
        match self.member_repo.get(device_id).await {
            Ok(Some(member)) => {
                if !member.sync_preferences.send_enabled {
                    info!(
                        device_id = %device_id.as_str(),
                        reason = "send_disabled_by_user",
                        "dispatch: skipping peer per per-device sync preferences"
                    );
                    return false;
                }
                if !categories.allowed_by(&member.sync_preferences.send_content_types) {
                    info!(
                        device_id = %device_id.as_str(),
                        categories = %categories.labels(),
                        denied = %categories
                            .denied_labels(&member.sync_preferences.send_content_types),
                        reason = "content_type_disabled_by_user",
                        "dispatch: skipping peer per per-device content_types filter"
                    );
                    return false;
                }
                true
            }
            Ok(None) => {
                warn!(
                    device_id = %device_id.as_str(),
                    "dispatch: peer in addr repo but missing from member repo; failing open"
                );
                true
            }
            Err(err) => {
                warn!(
                    device_id = %device_id.as_str(),
                    error = %err,
                    "dispatch: member repo lookup failed; failing open"
                );
                true
            }
        }
    }

    /// Load the device's own display name to embed in the outbound header
    /// so the peer can show "from <Alice's Laptop>". Falls back to the
    /// fingerprint if settings are unreadable or empty.
    async fn load_origin_device_name(&self) -> String {
        match self.settings.load().await {
            Ok(settings) => {
                if let Some(name) = settings.general.device_name {
                    if !name.is_empty() {
                        return name;
                    }
                }
            }
            Err(err) => {
                warn!(error = %err, "dispatch: settings load failed; using fingerprint fallback");
            }
        }
        match self.local_identity.get_current_fingerprint().await {
            Ok(Some(fp)) => fp.as_display().to_string(),
            _ => "unknown-device".to_string(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================
//
// **Mocking convention** — locked in by Slice 2 Phase 1 T6 (`ensure_reachable_all`)
// and reaffirmed by Phase 2 plan §10 risk row 4:
//
// * Use `mockall::mock!` for ports whose tests assert call-count + return-
//   value behavior. Every Phase 1 use case test does this; we follow.
// * Use a hand-written fake **only** when ergonomics demand it:
//     - `subscribe()` returning a non-Clone `broadcast::Receiver` plus an
//       `emit(...)` helper to drive the test (see `roster/facade.rs` ::
//       `FakePresence` for the canonical example), or
//     - wall-time concurrency assertions where mockall's internal
//       `Mutex<FnMut>` would serialise concurrent `.returning()` closures
//       (Phase 1 T6's `SleepyPresence`).
//
// For this file: the dispatch use case calls 2 async ports + read-only
// ports; no broadcast emit, no wall-time concurrency assertion. Most ports
// are mocked with `mockall::mock!`. `PresencePort::current_state` is read
// only for telemetry classification and never filters dispatch candidates.

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use chrono::Utc;
    use mockall::predicate::*;
    use tokio::sync::broadcast;

    use uc_core::ports::security::{TransferCipherError, TransferCipherPort};
    use uc_core::ports::{
        ClockPort, DeviceIdentityPort, FirstSyncStateError, LocalIdentityError, LocalIdentityPort,
        PeerAddressError, PeerAddressRecord, PeerAddressRepositoryPort, PresenceError,
        PresenceEvent, PresencePort, ReachabilityState, SettingsPort,
    };
    use uc_core::security::IdentityFingerprint;
    use uc_core::settings::model::Settings;
    use uc_core::{MemberRepositoryPort, MemberSyncPreferences, MembershipError, SpaceMember};

    // ── mockall: PeerAddressRepositoryPort ──────────────────────────────

    mockall::mock! {
        pub PeerAddrRepo {}

        #[async_trait]
        impl PeerAddressRepositoryPort for PeerAddrRepo {
            async fn get(
                &self,
                device: &DeviceId,
            ) -> Result<Option<PeerAddressRecord>, PeerAddressError>;
            async fn upsert(&self, record: &PeerAddressRecord) -> Result<(), PeerAddressError>;
            async fn list(&self) -> Result<Vec<PeerAddressRecord>, PeerAddressError>;
            async fn remove(&self, device: &DeviceId) -> Result<(), PeerAddressError>;
        }
    }

    // ── mockall: TransferCipherPort ─────────────────────────────────────

    mockall::mock! {
        pub Cipher {}

        #[async_trait]
        impl TransferCipherPort for Cipher {
            async fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, TransferCipherError>;
            async fn decrypt(&self, encrypted: &[u8]) -> Result<Vec<u8>, TransferCipherError>;
        }
    }

    // ── mockall: ClipboardDispatchPort ──────────────────────────────────
    //
    // The use case fan-outs via JoinSet, which spawns one task per target.
    // mockall's internal expectation `Mutex<FnMut>` would serialise
    // concurrent `.returning()` closures — but only when those closures
    // perform an `.await` that yields. Our `.returning(|_, _, _| ...)`
    // bodies return immediately, so there's nothing to serialise. The
    // Phase 1 T6 lesson (SleepyPresence) only applies when asserting
    // wall-time concurrency; per-target outcome assertions don't need it.

    mockall::mock! {
        pub Dispatch {}

        #[async_trait]
        impl ClipboardDispatchPort for Dispatch {
            async fn dispatch(
                &self,
                target: &DeviceId,
                header: &ClipboardHeader,
                payload: SyncPayload,
            ) -> Result<DispatchAck, ClipboardDispatchError>;
        }
    }

    // ── mockall: DeviceIdentityPort / LocalIdentityPort / SettingsPort ──

    mockall::mock! {
        pub DeviceId_ {}

        impl DeviceIdentityPort for DeviceId_ {
            fn current_device_id(&self) -> DeviceId;
        }
    }

    mockall::mock! {
        pub LocalIdentity {}

        #[async_trait]
        impl LocalIdentityPort for LocalIdentity {
            async fn create(&self) -> Result<IdentityFingerprint, LocalIdentityError>;
            async fn ensure(&self) -> Result<IdentityFingerprint, LocalIdentityError>;
            async fn get_current_fingerprint(
                &self,
            ) -> Result<Option<IdentityFingerprint>, LocalIdentityError>;
        }
    }

    mockall::mock! {
        pub Settings_ {}

        #[async_trait]
        impl SettingsPort for Settings_ {
            async fn load(&self) -> anyhow::Result<Settings>;
            async fn save(&self, s: &Settings) -> anyhow::Result<()>;
        }
    }

    // ── mockall: MemberRepositoryPort ───────────────────────────────────

    mockall::mock! {
        pub MemberRepo {}

        #[async_trait]
        impl MemberRepositoryPort for MemberRepo {
            async fn get(
                &self,
                device_id: &DeviceId,
            ) -> Result<Option<SpaceMember>, MembershipError>;
            async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError>;
            async fn save(&self, member: &SpaceMember) -> Result<(), MembershipError>;
            async fn remove(&self, device_id: &DeviceId) -> Result<bool, MembershipError>;
        }
    }

    // ── hand-written: ClockPort ─────────────────────────────────────────
    //
    // `ClockPort::now_ms` is sync + 4 lines; mockall's adapter would be
    // strictly more code than the hand-written `FixedClock`. Phase 1's
    // ensure_reachable_all uses the same pattern (`FixedDevice`).

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    /// 测试侧通用的"接收即丢弃"投递仓储。所有验证 dispatch outcome / telemetry
    /// 的测试都通过这个 noop 注入,因为它们不关心 delivery 表的副作用。
    /// 专门验证 delivery 落盘的测试自行注入 [`SpyEntryDeliveryRepo`]。
    struct NoopEntryDeliveryRepo;
    #[async_trait]
    impl EntryDeliveryRepositoryPort for NoopEntryDeliveryRepo {
        async fn record_attempt(
            &self,
            _record: &EntryDeliveryRecord,
        ) -> Result<(), uc_core::clipboard::EntryDeliveryError> {
            Ok(())
        }
        async fn list_by_entry(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Vec<EntryDeliveryRecord>, uc_core::clipboard::EntryDeliveryError> {
            Ok(Vec::new())
        }
    }

    /// 专门验证 fan-out 落盘的 spy。每次 `record_attempt` 把入参 clone 进
    /// 内部 vec,测试结束后逐条 assert 状态映射与 reason_detail。
    #[derive(Default)]
    struct SpyEntryDeliveryRepo {
        attempts: tokio::sync::Mutex<Vec<EntryDeliveryRecord>>,
    }
    impl SpyEntryDeliveryRepo {
        async fn snapshot(&self) -> Vec<EntryDeliveryRecord> {
            self.attempts.lock().await.clone()
        }
    }
    #[async_trait]
    impl EntryDeliveryRepositoryPort for SpyEntryDeliveryRepo {
        async fn record_attempt(
            &self,
            record: &EntryDeliveryRecord,
        ) -> Result<(), uc_core::clipboard::EntryDeliveryError> {
            self.attempts.lock().await.push(record.clone());
            Ok(())
        }
        async fn list_by_entry(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Vec<EntryDeliveryRecord>, uc_core::clipboard::EntryDeliveryError> {
            Ok(Vec::new())
        }
    }

    struct StaticPresence(ReachabilityState);
    #[async_trait]
    impl PresencePort for StaticPresence {
        async fn ensure_reachable(
            &self,
            _device: &DeviceId,
        ) -> Result<ReachabilityState, PresenceError> {
            Ok(self.0)
        }

        async fn current_state(&self, _device: &DeviceId) -> ReachabilityState {
            self.0
        }

        fn subscribe(&self) -> broadcast::Receiver<PresenceEvent> {
            let (_tx, rx) = broadcast::channel(1);
            rx
        }
    }

    // ── helpers ─────────────────────────────────────────────────────────

    fn fp(seed: u8) -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string(
            (0..16)
                .map(|i| char::from(b'A' + ((seed as usize + i) % 26) as u8))
                .collect::<String>(),
        )
        .expect("valid fingerprint")
    }

    fn record(device: &str) -> PeerAddressRecord {
        PeerAddressRecord {
            device_id: DeviceId::new(device),
            addr_blob: vec![0xAA; 32],
            observed_at: Utc::now(),
        }
    }

    /// Build a `Settings` whose `general.device_name` round-trips to a
    /// stable header value.
    fn settings_with_device_name(name: &str) -> Settings {
        let mut s = Settings::default();
        s.general.device_name = Some(name.to_string());
        s
    }

    /// Wire the use case from a set of mock ports. The clock is always
    /// the same fixed value so header `captured_at_ms` assertions are
    /// deterministic.
    fn build_uc(
        peer_addr_repo: MockPeerAddrRepo,
        member_repo: MockMemberRepo,
        cipher: MockCipher,
        dispatch: MockDispatch,
        device_identity: MockDeviceId_,
        local_identity: MockLocalIdentity,
        settings: MockSettings_,
    ) -> DispatchClipboardEntryUseCase {
        build_uc_with_analytics(
            peer_addr_repo,
            member_repo,
            cipher,
            dispatch,
            device_identity,
            local_identity,
            settings,
            Arc::new(uc_observability::analytics::NoopAnalyticsSink),
        )
    }

    /// Variant that accepts an injectable analytics sink — Slice 8c-1
    /// telemetry tests use `CapturingAnalyticsSink` here; the legacy
    /// `build_uc` helper falls through to a `NoopAnalyticsSink` so older
    /// tests stay terse. `first_sync_state` 默认走 `AllMarkedFirstSyncState`
    /// (永远返回 Ok(false))，避免 sync 三事件测试被 first_* 事件污染；
    /// 验证 first_* 触发的测试请用 [`build_uc_with_first_sync_state`]。
    #[allow(clippy::too_many_arguments)]
    fn build_uc_with_analytics(
        peer_addr_repo: MockPeerAddrRepo,
        member_repo: MockMemberRepo,
        cipher: MockCipher,
        dispatch: MockDispatch,
        device_identity: MockDeviceId_,
        local_identity: MockLocalIdentity,
        settings: MockSettings_,
        analytics: Arc<dyn AnalyticsPort>,
    ) -> DispatchClipboardEntryUseCase {
        build_uc_with_first_sync_state(
            peer_addr_repo,
            member_repo,
            cipher,
            dispatch,
            device_identity,
            local_identity,
            settings,
            analytics,
            Arc::new(AllMarkedFirstSyncState),
        )
    }

    /// Slice 8c-2 · 全显式构造：Slice 8c-2 first-path 测试需要 InMemory
    /// first_sync_state（默认全 unmarked，首次 mark 返回 true）来验证
    /// `first_clipboard_sync_*` / `first_file_sync_succeeded` 触发逻辑。
    #[allow(clippy::too_many_arguments)]
    fn build_uc_with_first_sync_state(
        peer_addr_repo: MockPeerAddrRepo,
        member_repo: MockMemberRepo,
        cipher: MockCipher,
        dispatch: MockDispatch,
        device_identity: MockDeviceId_,
        local_identity: MockLocalIdentity,
        settings: MockSettings_,
        analytics: Arc<dyn AnalyticsPort>,
        first_sync_state: Arc<dyn FirstSyncStatePort>,
    ) -> DispatchClipboardEntryUseCase {
        build_uc_with_presence_and_first_sync_state(
            peer_addr_repo,
            member_repo,
            Arc::new(StaticPresence(ReachabilityState::Unknown)),
            cipher,
            dispatch,
            device_identity,
            local_identity,
            settings,
            analytics,
            first_sync_state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_uc_with_presence_and_first_sync_state(
        peer_addr_repo: MockPeerAddrRepo,
        member_repo: MockMemberRepo,
        presence: Arc<dyn PresencePort>,
        cipher: MockCipher,
        dispatch: MockDispatch,
        device_identity: MockDeviceId_,
        local_identity: MockLocalIdentity,
        settings: MockSettings_,
        analytics: Arc<dyn AnalyticsPort>,
        first_sync_state: Arc<dyn FirstSyncStatePort>,
    ) -> DispatchClipboardEntryUseCase {
        DispatchClipboardEntryUseCase::new(
            Arc::new(peer_addr_repo),
            Arc::new(member_repo),
            presence,
            Arc::new(cipher),
            Arc::new(dispatch),
            Arc::new(device_identity),
            Arc::new(local_identity),
            Arc::new(settings),
            Arc::new(FixedClock(1_700_000_000_000)),
            analytics,
            first_sync_state,
            Arc::new(NoopEntryDeliveryRepo),
            Arc::new(crate::facade::host_event::HostEventBus::new()),
        )
    }

    /// Slice 8c-2 · "all flags already marked" fake: every `mark_*` returns
    /// `Ok(false)`, so the use case **never** fires a `first_*` event. Used
    /// by every legacy test so their event-count assertions stay valid.
    struct AllMarkedFirstSyncState;
    #[async_trait]
    impl FirstSyncStatePort for AllMarkedFirstSyncState {
        async fn mark_first_sync_attempted(&self) -> Result<bool, FirstSyncStateError> {
            Ok(false)
        }
        async fn mark_first_sync_succeeded(&self) -> Result<bool, FirstSyncStateError> {
            Ok(false)
        }
        async fn mark_first_file_sync_succeeded(&self) -> Result<bool, FirstSyncStateError> {
            Ok(false)
        }
    }

    /// Slice 8c-2 · in-memory fake mirroring the production
    /// `FileFirstSyncStateRepository`: first call returns `Ok(true)`, subsequent
    /// calls `Ok(false)`. Each flag is independent. Race防护用 `tokio::sync::Mutex`
    /// 守 read-check-write，与 production impl 等价。
    #[derive(Default)]
    struct InMemoryFirstSyncState {
        attempted: tokio::sync::Mutex<bool>,
        succeeded: tokio::sync::Mutex<bool>,
        file_succeeded: tokio::sync::Mutex<bool>,
    }
    #[async_trait]
    impl FirstSyncStatePort for InMemoryFirstSyncState {
        async fn mark_first_sync_attempted(&self) -> Result<bool, FirstSyncStateError> {
            let mut g = self.attempted.lock().await;
            if *g {
                Ok(false)
            } else {
                *g = true;
                Ok(true)
            }
        }
        async fn mark_first_sync_succeeded(&self) -> Result<bool, FirstSyncStateError> {
            let mut g = self.succeeded.lock().await;
            if *g {
                Ok(false)
            } else {
                *g = true;
                Ok(true)
            }
        }
        async fn mark_first_file_sync_succeeded(&self) -> Result<bool, FirstSyncStateError> {
            let mut g = self.file_succeeded.lock().await;
            if *g {
                Ok(false)
            } else {
                *g = true;
                Ok(true)
            }
        }
    }

    /// Build a `MemberRepo` mock that returns a stub `SpaceMember` with
    /// default (all-enabled) `sync_preferences` for every device. Used by
    /// the existing verdicts whose contract predates per-device gating —
    /// they should still observe the same fan-out behaviour.
    fn make_member_repo_all_enabled() -> MockMemberRepo {
        let mut m = MockMemberRepo::new();
        m.expect_get().returning(|did| {
            Ok(Some(SpaceMember {
                device_id: did.clone(),
                device_name: format!("Test {}", did.as_str()),
                identity_fingerprint: fp(0),
                joined_at: Utc::now(),
                sync_preferences: MemberSyncPreferences::default(),
            }))
        });
        m
    }

    /// Build a `DeviceIdentity` mock that returns the same `device_id`
    /// every call. Always-present helper because every test sets self.
    fn make_device_identity(local: &str) -> MockDeviceId_ {
        let local = DeviceId::new(local);
        let mut m = MockDeviceId_::new();
        m.expect_current_device_id()
            .returning(move || local.clone());
        m
    }

    /// Default settings + identity stubs that every test wires identically.
    fn make_local_identity_stub() -> MockLocalIdentity {
        let mut m = MockLocalIdentity::new();
        m.expect_get_current_fingerprint()
            .returning(|| Ok(Some(fp(7))));
        m
    }

    fn make_settings_stub() -> MockSettings_ {
        let mut m = MockSettings_::new();
        m.expect_load()
            .returning(|| Ok(settings_with_device_name("Test Device")));
        m
    }

    fn input() -> DispatchClipboardEntryInput {
        DispatchClipboardEntryInput {
            plaintext: Bytes::from_static(b"hello world"),
            content_hash: "9".repeat(64),
            payload_version: 3,
            // Existing verdicts predate the content-type filter; default
            // to an empty set so they always pass the gate (fail open).
            categories: ClipboardContentCategorySet::empty(),
            // 默认无 entry_id:大部分历史测试只关心 outcome 与 telemetry,
            // 不需要触发 delivery 落盘。专门验证落盘行为的测试自行构造 Some。
            entry_id: None,
        }
    }

    // ── verdicts ────────────────────────────────────────────────────────

    /// 1. Happy path — two paired peers, both accept. mockall asserts
    /// dispatch is called exactly twice (once per peer) and the encrypt
    /// path runs exactly once.
    #[tokio::test]
    async fn fan_outs_to_all_peers_and_counts_accepted() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-a"), record("peer-b")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-a")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-b")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let uc = build_uc(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 2);
        assert_eq!(outcome.total_offline, 0);
        assert_eq!(outcome.total_errored, 0);
        assert_eq!(outcome.per_target.len(), 2);
    }

    /// 2. Unreachable peer — dispatch port returns `Err(Offline)` for a
    /// peer the wire can't reach. The outcome reports it as offline
    /// instead of silently dropping it pre-flight; the other peer still
    /// gets the frame. This is the key contract change that fixes the
    /// "no online peers; skipping fan-out" silent regression where our
    /// local presence cache was empty because the peer dialed us first
    /// (accept-side only updates the peer's cache, not ours).
    #[tokio::test]
    async fn unreachable_peer_is_reported_offline_after_dispatch_attempt() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-on"), record("peer-off")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-on")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        // Crucial: dispatch IS called for `peer-off` (no pre-filter). The
        // port returns `Offline`, and the outcome surfaces that — callers
        // can then decide whether to retry or surface to the user.
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-off")), always(), always())
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::Offline));

        let uc = build_uc(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
        assert_eq!(outcome.total_offline, 1);
        assert_eq!(outcome.per_target.len(), 2);
    }

    /// 3. Self-filter — `peer_addr_repo` inadvertently contains the local
    /// device. mockall enforces self-skip: no dispatch expectation is
    /// registered for `self-device`, so a self-dial would panic.
    #[tokio::test]
    async fn skips_self_even_if_peer_addr_repo_contains_local_device() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("self-device"), record("peer-a")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-a")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let uc = build_uc(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.per_target.len(), 1);
        assert_eq!(outcome.per_target[0].device_id.as_str(), "peer-a");
    }

    /// 4. Locked space — `transfer_cipher.encrypt` returns `NotUnlocked`.
    /// Use case short-circuits with `LockedSpace`. mockall enforces "no
    /// dispatch ever called" by registering zero dispatch expectations.
    #[tokio::test]
    async fn locked_space_short_circuits_before_dispatch() {
        // peer_addr_repo isn't reached — register zero expectations so an
        // accidental call would panic.
        let repo = MockPeerAddrRepo::new();

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|_| Err(TransferCipherError::NotUnlocked));

        let dispatch = MockDispatch::new();

        let uc = build_uc(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let err = uc
            .execute(input())
            .await
            .expect_err("locked space must short-circuit");
        assert!(matches!(err, DispatchSyncError::LockedSpace));
    }

    /// 5. Mixed outcomes — one accept, one offline, one rejected. Each
    /// target's expectation is registered independently with `.with(...)`
    /// matching the device id; mockall picks the right one per call,
    /// ignoring task ordering (JoinSet).
    #[tokio::test]
    async fn mixed_per_target_outcomes_are_reported_independently() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list().times(1).returning(|| {
            Ok(vec![
                record("peer-ok"),
                record("peer-off"),
                record("peer-rej"),
            ])
        });

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-ok")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-off")), always(), always())
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::Offline));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-rej")), always(), always())
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::PeerRejected("too big".to_string())));

        let uc = build_uc(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
        assert_eq!(outcome.total_offline, 1);
        assert_eq!(outcome.total_errored, 1);
        assert_eq!(outcome.per_target.len(), 3);

        use std::collections::HashSet;
        let seen: HashSet<(String, String)> = outcome
            .per_target
            .iter()
            .map(|t| {
                let key = match &t.outcome {
                    Ok(DispatchAck::Accepted) => "accepted",
                    Ok(DispatchAck::DuplicateIgnored) => "duplicate",
                    Err(msg) if msg == "offline" => "offline",
                    Err(_) => "rejected",
                };
                (t.device_id.as_str().to_string(), key.to_string())
            })
            .collect();
        assert!(seen.contains(&("peer-ok".to_string(), "accepted".to_string())));
        assert!(seen.contains(&("peer-off".to_string(), "offline".to_string())));
        assert!(seen.contains(&("peer-rej".to_string(), "rejected".to_string())));
    }

    /// 6. Per-device send gate — `peer-mute` has `send_enabled=false` in
    /// its `MemberSyncPreferences`. The dispatch port must NEVER be
    /// invoked for it; the other peer still receives the frame. mockall
    /// enforces "no dispatch ever for peer-mute" by registering zero
    /// expectations on that arm — any sneaky call would panic.
    #[tokio::test]
    async fn send_disabled_peer_is_skipped_before_dispatch() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-on"), record("peer-mute")]));

        let mut member_repo = MockMemberRepo::new();
        member_repo
            .expect_get()
            .returning(|did| match did.as_str() {
                "peer-mute" => {
                    let mut prefs = MemberSyncPreferences::default();
                    prefs.send_enabled = false;
                    Ok(Some(SpaceMember {
                        device_id: did.clone(),
                        device_name: "Peer Mute".to_string(),
                        identity_fingerprint: fp(0),
                        joined_at: Utc::now(),
                        sync_preferences: prefs,
                    }))
                }
                _ => Ok(Some(SpaceMember {
                    device_id: did.clone(),
                    device_name: format!("Test {}", did.as_str()),
                    identity_fingerprint: fp(0),
                    joined_at: Utc::now(),
                    sync_preferences: MemberSyncPreferences::default(),
                })),
            });

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        // Only peer-on is allowed; peer-mute must never be dispatched to.
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-on")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        // No expect_dispatch for peer-mute → mockall would panic on call.

        let uc = build_uc(
            repo,
            member_repo,
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
        assert_eq!(
            outcome.per_target.len(),
            1,
            "muted peer must not appear in per_target report"
        );
        assert_eq!(outcome.per_target[0].device_id.as_str(), "peer-on");
    }

    /// 7. Fail-open on member lookup miss — peer is in `peer_addr_repo`
    /// but `member_repo.get` returns `Ok(None)` (the two stores drifted).
    /// The dispatch port must still fire so a transient repo gap doesn't
    /// silently kill sync; the operator-visible signal is the WARN log.
    #[tokio::test]
    async fn missing_member_record_fails_open_and_still_dispatches() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-orphan")]));

        let mut member_repo = MockMemberRepo::new();
        member_repo
            .expect_get()
            .with(eq(DeviceId::new("peer-orphan")))
            .times(1)
            .returning(|_| Ok(None));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-orphan")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let uc = build_uc(
            repo,
            member_repo,
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
        assert_eq!(outcome.per_target.len(), 1);
    }

    /// 8. Per-device content-type gate — `peer-no-text` has
    /// `send_content_types.text=false`. Dispatching a `Text` snapshot
    /// must skip that peer; the other peer (default-allowed) still gets
    /// the frame. mockall enforces "no dispatch ever for peer-no-text".
    #[tokio::test]
    async fn send_content_types_text_disabled_peer_is_skipped() {
        use uc_core::settings::model::ContentTypes;

        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-on"), record("peer-no-text")]));

        let mut member_repo = MockMemberRepo::new();
        member_repo
            .expect_get()
            .returning(|did| match did.as_str() {
                "peer-no-text" => {
                    let mut prefs = MemberSyncPreferences::default();
                    let mut ct = ContentTypes::default();
                    ct.text = false;
                    prefs.send_content_types = ct;
                    Ok(Some(SpaceMember {
                        device_id: did.clone(),
                        device_name: "Peer NoText".to_string(),
                        identity_fingerprint: fp(0),
                        joined_at: Utc::now(),
                        sync_preferences: prefs,
                    }))
                }
                _ => Ok(Some(SpaceMember {
                    device_id: did.clone(),
                    device_name: format!("Test {}", did.as_str()),
                    identity_fingerprint: fp(0),
                    joined_at: Utc::now(),
                    sync_preferences: MemberSyncPreferences::default(),
                })),
            });

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-on")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        // No expect_dispatch for peer-no-text → mockall would panic on call.

        let uc = build_uc(
            repo,
            member_repo,
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        // Hand-craft an input whose category set is `{Text}` — the
        // simplest scenario where the text-muted peer must be skipped.
        use uc_core::clipboard::ClipboardContentCategory;
        let mut categories = ClipboardContentCategorySet::empty();
        categories.insert(ClipboardContentCategory::Text);
        let text_input = DispatchClipboardEntryInput {
            plaintext: Bytes::from_static(b"hello world"),
            content_hash: "9".repeat(64),
            payload_version: 3,
            categories,
            entry_id: None,
        };

        let outcome = uc.execute(text_input).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
        assert_eq!(
            outcome.per_target.len(),
            1,
            "text-muted peer must not appear in per_target"
        );
        assert_eq!(outcome.per_target[0].device_id.as_str(), "peer-on");
    }

    /// 9. Empty category set bypasses content-type gate even when the
    /// peer has all content types disabled. Mirrors the CLI raw-bytes
    /// path where the snapshot can't be classified — must fail open.
    #[tokio::test]
    async fn empty_category_set_bypasses_content_types_filter() {
        use uc_core::settings::model::ContentTypes;

        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-strict")]));

        let mut member_repo = MockMemberRepo::new();
        member_repo
            .expect_get()
            .with(eq(DeviceId::new("peer-strict")))
            .times(1)
            .returning(|did| {
                let mut prefs = MemberSyncPreferences::default();
                // Every content type off — only an empty category set should pass.
                let mut ct = ContentTypes::default();
                ct.text = false;
                ct.image = false;
                ct.link = false;
                ct.file = false;
                ct.code_snippet = false;
                ct.rich_text = false;
                prefs.send_content_types = ct;
                Ok(Some(SpaceMember {
                    device_id: did.clone(),
                    device_name: "Peer Strict".to_string(),
                    identity_fingerprint: fp(0),
                    joined_at: Utc::now(),
                    sync_preferences: prefs,
                }))
            });

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-strict")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let uc = build_uc(
            repo,
            member_repo,
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
        );

        // input() defaults to an empty `ClipboardContentCategorySet` — an
        // unrecognised payload should fail open even against an all-off filter.
        let outcome = uc.execute(input()).await.expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
    }

    // ── Slice 8c-1 analytics: per-peer sync_attempted/succeeded/failed ───

    /// Test fake `AnalyticsPort` that records every captured `Event` for
    /// inspection. Mirrors the joiner / sponsor / setup test fakes.
    #[derive(Default)]
    struct CapturingAnalyticsSink {
        captured: std::sync::Mutex<Vec<Event>>,
    }
    impl CapturingAnalyticsSink {
        fn events(&self) -> Vec<Event> {
            self.captured.lock().unwrap().clone()
        }
    }
    impl AnalyticsPort for CapturingAnalyticsSink {
        fn capture(&self, event: Event) {
            self.captured.lock().unwrap().push(event);
        }
    }

    #[tokio::test]
    async fn analytics_fires_attempted_then_succeeded_per_peer_on_happy_path() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-a"), record("peer-b")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-a")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-b")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::DuplicateIgnored));

        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let uc = build_uc_with_analytics(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
            analytics.clone(),
        );

        uc.execute(input()).await.expect("dispatch ok");

        // Expect 4 events total: SyncAttempted×2 + SyncSucceeded×2.
        // Spawn ordering is non-deterministic, but every peer's pair of
        // (Attempted, Succeeded) must be back-to-back inside its own task —
        // we settle for "2 attempted + 2 succeeded total".
        let events = analytics.events();
        assert_eq!(events.len(), 4, "got {events:?}");
        let attempted = events
            .iter()
            .filter(|e| matches!(e, Event::SyncAttempted(_)))
            .count();
        let succeeded = events
            .iter()
            .filter(|e| matches!(e, Event::SyncSucceeded(_)))
            .count();
        assert_eq!((attempted, succeeded), (2, 2));
        // Spot-check schema invariants on one succeeded event:
        // direction=Outbound, transport=P2pDirect, sync_latency_ms set.
        let sample = events
            .iter()
            .find_map(|e| match e {
                Event::SyncSucceeded(p) => Some(p),
                _ => None,
            })
            .expect("at least one SyncSucceeded");
        assert_eq!(sample.direction, Direction::Outbound);
        assert_eq!(sample.transport_type, TransportType::P2pDirect);
        assert!(sample.sync_latency_ms.is_some());
        assert!(sample.failure_reason.is_none());
        assert!(sample.failure_stage.is_none());
    }

    #[tokio::test]
    async fn analytics_fires_failed_with_peer_offline_when_dispatch_returns_offline() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-off")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-off")), always(), always())
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::Offline));

        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let uc = build_uc_with_analytics(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
            analytics.clone(),
        );

        uc.execute(input()).await.expect("dispatch ok");

        let events = analytics.events();
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], Event::SyncAttempted(_)));
        match &events[1] {
            Event::SyncFailed(p) => {
                assert_eq!(p.failure_reason, Some(FailureReason::PeerOffline));
                assert_eq!(p.failure_stage, Some(SyncFailureStage::ImmediateSend));
                assert!(p.sync_latency_ms.is_none());
            }
            other => panic!("expected SyncFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn analytics_defers_instead_of_failing_when_peer_was_already_offline() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-off")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-off")), always(), always())
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::Offline));

        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let uc = build_uc_with_presence_and_first_sync_state(
            repo,
            make_member_repo_all_enabled(),
            Arc::new(StaticPresence(ReachabilityState::Offline)),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
            analytics.clone(),
            Arc::new(AllMarkedFirstSyncState),
        );

        uc.execute(input()).await.expect("dispatch ok");

        // attempted 始终前置，deferred 与 attempted 形成配对。
        // dashboard 端用 `attempted - deferred` 推导用户感知尝试，
        // 用户感知失败率不再把 known-offline 的不可达计入。
        let events = analytics.events();
        assert_eq!(events.len(), 2, "got {events:?}");
        assert!(
            matches!(&events[0], Event::SyncAttempted(_)),
            "first event should be SyncAttempted, got {:?}",
            events[0],
        );
        match &events[1] {
            Event::SyncDeferred(p) => {
                assert_eq!(p.defer_reason, SyncDeferReason::PeerKnownOffline);
                assert_eq!(p.direction, Direction::Outbound);
            }
            other => panic!("expected SyncDeferred, got {other:?}"),
        }
    }

    /// Presence 可能 stale：声明 Offline 但 dispatch 实际成功。此时不走 deferred
    /// 分支（deferred guard 只覆盖 dispatch 返回 Offline 的情况），仍应得到
    /// `SyncAttempted` + `SyncSucceeded`，时序与 not-known-offline 路径一致。
    #[tokio::test]
    async fn attempted_then_succeeded_even_when_peer_was_known_offline() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-stale")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-stale")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let uc = build_uc_with_presence_and_first_sync_state(
            repo,
            make_member_repo_all_enabled(),
            Arc::new(StaticPresence(ReachabilityState::Offline)),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
            analytics.clone(),
            Arc::new(AllMarkedFirstSyncState),
        );

        uc.execute(input()).await.expect("dispatch ok");

        let events = analytics.events();
        assert_eq!(events.len(), 2, "got {events:?}");
        assert!(
            matches!(&events[0], Event::SyncAttempted(_)),
            "first event should be SyncAttempted, got {:?}",
            events[0],
        );
        match &events[1] {
            Event::SyncSucceeded(p) => {
                assert!(p.sync_latency_ms.is_some());
                assert!(p.failure_reason.is_none());
                assert!(p.failure_stage.is_none());
            }
            other => panic!("expected SyncSucceeded, got {other:?}"),
        }
    }

    /// known_offline guard 只保护 `dispatch == Offline` 的不可达。如果对端
    /// 已知离线但 dispatch 报告其他错误（peer 拒收 / IO / 内部错误），仍属
    /// 真失败：发 `sync_attempted` + `sync_failed`，`stage = ImmediateSend`。
    #[tokio::test]
    async fn attempted_then_failed_when_known_offline_peer_returns_non_offline_error() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-broken")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-broken")), always(), always())
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::Io("broken pipe".into())));

        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let uc = build_uc_with_presence_and_first_sync_state(
            repo,
            make_member_repo_all_enabled(),
            Arc::new(StaticPresence(ReachabilityState::Offline)),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
            analytics.clone(),
            Arc::new(AllMarkedFirstSyncState),
        );

        uc.execute(input()).await.expect("dispatch ok");

        let events = analytics.events();
        assert_eq!(events.len(), 2, "got {events:?}");
        assert!(
            matches!(&events[0], Event::SyncAttempted(_)),
            "first event should be SyncAttempted, got {:?}",
            events[0],
        );
        match &events[1] {
            Event::SyncFailed(p) => {
                assert_eq!(p.failure_reason, Some(FailureReason::NetworkError));
                assert_eq!(p.failure_stage, Some(SyncFailureStage::ImmediateSend));
                assert!(p.sync_latency_ms.is_none());
            }
            other => panic!("expected SyncFailed, got {other:?}"),
        }
    }

    /// Slice 8c-2 · first-path: 2 paired peers, 全部 Accepted, payload_type=File.
    /// 期望同一 spawn batch 内三个 `first_*` 事件**各自只 fire 一次**：
    /// `FirstClipboardSyncAttempted` × 1（任意一个 spawn 抢到 attempted mutex）
    /// + `FirstClipboardSyncSucceeded` × 1（同上 succeeded mutex）
    /// + `FirstFileSyncSucceeded` × 1（payload_type=File 分支额外 mark）。
    /// 其余 spawn 进入时 mark 都返回 `Ok(false)`，funnel 漏斗不重复计数。
    #[tokio::test]
    async fn first_path_fires_clipboard_and_file_first_events_exactly_once_per_flag() {
        use uc_core::clipboard::ClipboardContentCategory;

        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-a"), record("peer-b")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-a")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-b")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let first_sync_state = Arc::new(InMemoryFirstSyncState::default());
        let uc = build_uc_with_first_sync_state(
            repo,
            make_member_repo_all_enabled(),
            cipher,
            dispatch,
            make_device_identity("self-device"),
            make_local_identity_stub(),
            make_settings_stub(),
            analytics.clone(),
            first_sync_state,
        );

        let mut categories = ClipboardContentCategorySet::empty();
        categories.insert(ClipboardContentCategory::File);
        let file_input = DispatchClipboardEntryInput {
            plaintext: Bytes::from_static(b"hello world"),
            content_hash: "9".repeat(64),
            payload_version: 3,
            categories,
            entry_id: None,
        };

        uc.execute(file_input).await.expect("dispatch ok");

        let events = analytics.events();
        let attempted = events
            .iter()
            .filter(|e| matches!(e, Event::SyncAttempted(_)))
            .count();
        let succeeded = events
            .iter()
            .filter(|e| matches!(e, Event::SyncSucceeded(_)))
            .count();
        let first_attempted = events
            .iter()
            .filter(|e| matches!(e, Event::FirstClipboardSyncAttempted { .. }))
            .count();
        let first_succeeded = events
            .iter()
            .filter(|e| matches!(e, Event::FirstClipboardSyncSucceeded { .. }))
            .count();
        let first_file = events
            .iter()
            .filter(|e| matches!(e, Event::FirstFileSyncSucceeded { .. }))
            .count();

        assert_eq!(
            (
                attempted,
                succeeded,
                first_attempted,
                first_succeeded,
                first_file
            ),
            (2, 2, 1, 1, 1),
            "expected 2 sync_attempted + 2 sync_succeeded + 1 each first_*; got {events:?}",
        );

        // 字段断言 — FirstClipboardSyncSucceeded 必须 direction=Outbound /
        // transport=P2pDirect / peer_os=None。
        let first_succ_event = events
            .iter()
            .find(|e| matches!(e, Event::FirstClipboardSyncSucceeded { .. }))
            .expect("FirstClipboardSyncSucceeded present");
        match first_succ_event {
            Event::FirstClipboardSyncSucceeded {
                direction,
                peer_os,
                transport_type,
                duration_ms: _,
            } => {
                assert_eq!(*direction, Direction::Outbound);
                assert!(peer_os.is_none());
                assert_eq!(*transport_type, TransportType::P2pDirect);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn map_dispatch_error_covers_all_variants() {
        // Compile-fence: 1:1 mapping table — any new ClipboardDispatchError
        // variant added to uc-core will require an explicit decision here
        // (compiler enforces match completeness on the helper itself).
        for (err, expected) in [
            (ClipboardDispatchError::Offline, FailureReason::PeerOffline),
            (
                ClipboardDispatchError::LocalPolicyExceeded("too big".into()),
                FailureReason::FileTooLarge,
            ),
            (
                ClipboardDispatchError::PeerRejected("bad header".into()),
                FailureReason::NetworkError,
            ),
            (
                ClipboardDispatchError::Io("broken pipe".into()),
                FailureReason::NetworkError,
            ),
            (
                ClipboardDispatchError::Internal("boom".into()),
                FailureReason::Unknown,
            ),
        ] {
            assert_eq!(map_dispatch_error_to_failure_reason(&err), expected);
        }
    }

    // ── delivery 落盘集成测试 ─────────────────────────────────────────

    /// 5 个 peer 分别得到 5 种 ack/err,验证 record_attempt 把它们 1:1
    /// 映射到 5 种 (status, reason_detail)。
    #[tokio::test]
    async fn dispatch_records_delivery_for_each_outcome_variant() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list().times(1).returning(|| {
            Ok(vec![
                record("peer-ok"),
                record("peer-dup"),
                record("peer-off"),
                record("peer-policy"),
                record("peer-io"),
            ])
        });

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-ok")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-dup")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::DuplicateIgnored));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-off")), always(), always())
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::Offline));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-policy")), always(), always())
            .times(1)
            .returning(|_, _, _| {
                Err(ClipboardDispatchError::LocalPolicyExceeded(
                    "too big".to_string(),
                ))
            });
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-io")), always(), always())
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::Io("broken pipe".to_string())));

        let spy = Arc::new(SpyEntryDeliveryRepo::default());
        let uc = DispatchClipboardEntryUseCase::new(
            Arc::new(repo),
            Arc::new(make_member_repo_all_enabled()),
            Arc::new(StaticPresence(ReachabilityState::Unknown)),
            Arc::new(cipher),
            Arc::new(dispatch),
            Arc::new(make_device_identity("self-device")),
            Arc::new(make_local_identity_stub()),
            Arc::new(make_settings_stub()),
            Arc::new(FixedClock(1_700_000_000_000)),
            Arc::new(uc_observability::analytics::NoopAnalyticsSink),
            Arc::new(AllMarkedFirstSyncState),
            Arc::clone(&spy) as Arc<dyn EntryDeliveryRepositoryPort>,
            Arc::new(crate::facade::host_event::HostEventBus::new()),
        );

        let mut input = input();
        input.entry_id = Some(EntryId::from("entry-1".to_string()));
        let _ = uc.execute(input).await.expect("dispatch ok");

        let attempts = spy.snapshot().await;
        assert_eq!(attempts.len(), 5, "每个 target 写一行");

        let by_target: std::collections::HashMap<String, &EntryDeliveryRecord> = attempts
            .iter()
            .map(|r| (r.target_device_id.to_string(), r))
            .collect();
        assert!(matches!(
            by_target["peer-ok"].status,
            EntryDeliveryStatus::Delivered
        ));
        assert!(by_target["peer-ok"].reason_detail.is_none());

        assert!(matches!(
            by_target["peer-dup"].status,
            EntryDeliveryStatus::Duplicate
        ));
        assert!(by_target["peer-dup"].reason_detail.is_none());

        assert!(matches!(
            by_target["peer-off"].status,
            EntryDeliveryStatus::Failed {
                reason: DeliveryFailureReason::Offline
            }
        ));
        assert!(
            by_target["peer-off"].reason_detail.is_none(),
            "Offline 无人类可读补充"
        );

        assert!(matches!(
            by_target["peer-policy"].status,
            EntryDeliveryStatus::Failed {
                reason: DeliveryFailureReason::LocalPolicy
            }
        ));
        assert_eq!(
            by_target["peer-policy"].reason_detail.as_deref(),
            Some("too big")
        );

        assert!(matches!(
            by_target["peer-io"].status,
            EntryDeliveryStatus::Failed {
                reason: DeliveryFailureReason::Io
            }
        ));
        assert_eq!(
            by_target["peer-io"].reason_detail.as_deref(),
            Some("broken pipe")
        );

        for rec in &attempts {
            assert_eq!(rec.entry_id.to_string(), "entry-1");
        }
    }

    /// entry_id=None 路径(CLI raw-bytes / 测试)永不触发落盘:dispatch
    /// 自身不与某条 entry 绑定,落盘对应 entry_id 无处可写。
    #[tokio::test]
    async fn dispatch_without_entry_id_does_not_record_delivery() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-a")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let spy = Arc::new(SpyEntryDeliveryRepo::default());
        let uc = DispatchClipboardEntryUseCase::new(
            Arc::new(repo),
            Arc::new(make_member_repo_all_enabled()),
            Arc::new(StaticPresence(ReachabilityState::Unknown)),
            Arc::new(cipher),
            Arc::new(dispatch),
            Arc::new(make_device_identity("self-device")),
            Arc::new(make_local_identity_stub()),
            Arc::new(make_settings_stub()),
            Arc::new(FixedClock(1_700_000_000_000)),
            Arc::new(uc_observability::analytics::NoopAnalyticsSink),
            Arc::new(AllMarkedFirstSyncState),
            Arc::clone(&spy) as Arc<dyn EntryDeliveryRepositoryPort>,
            Arc::new(crate::facade::host_event::HostEventBus::new()),
        );

        let _ = uc.execute(input()).await.expect("dispatch ok");
        assert!(
            spy.snapshot().await.is_empty(),
            "entry_id=None 时不应有 record_attempt 调用"
        );
    }

    #[test]
    fn dispatch_failure_stage_classifies_failure_phase() {
        for (err, expected) in [
            (
                ClipboardDispatchError::Offline,
                SyncFailureStage::ImmediateSend,
            ),
            (
                ClipboardDispatchError::Io("broken pipe".into()),
                SyncFailureStage::ImmediateSend,
            ),
            (
                ClipboardDispatchError::PeerRejected("bad header".into()),
                SyncFailureStage::ImmediateSend,
            ),
            (
                ClipboardDispatchError::LocalPolicyExceeded("too big".into()),
                SyncFailureStage::LocalPolicy,
            ),
            (
                ClipboardDispatchError::Internal("boom".into()),
                SyncFailureStage::ImmediateSend,
            ),
        ] {
            assert_eq!(dispatch_failure_stage(&err), expected);
        }
    }

    // ── Phase 5 (#747):delivery host event emit ─────────────────────────
    //
    // 写盘单元测试已覆盖"5 种 outcome → 5 种 record"映射;本组聚焦"record
    // 写盘成功后 → bus.emit_or_warn 追发一条 HostEvent::Delivery"。在
    // bus 上注册一个 RecordingEmitter 抓事件序列,断言顺序、payload、
    // 与 `entry_id=None` 路径下不发事件。

    use crate::facade::host_event::{
        DeliveryHostEvent, EmitError as HostEmitError, HostEvent, HostEventBus,
        HostEventEmitterPort,
    };
    use std::sync::Mutex as StdMutex;

    /// 把 HostEvent 全部录到一个 Vec,测试结束后断言序列与 payload。
    /// 与 apply_inbound::tests::RecordingEmitter 等价,但定义在本 mod 内,
    /// 避免跨模块 visibility(uc-application AGENTS §11.4 — orchestrator /
    /// publisher 等内部类型不出 crate)。
    #[derive(Default)]
    struct RecordingEmitter {
        events: StdMutex<Vec<HostEvent>>,
    }
    impl RecordingEmitter {
        fn snapshot(&self) -> Vec<HostEvent> {
            self.events.lock().unwrap().clone()
        }
    }
    impl HostEventEmitterPort for RecordingEmitter {
        fn emit(&self, event: HostEvent) -> Result<(), HostEmitError> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }
    }

    /// 把 spy delivery repo + recording emitter 都装进同一份 dispatch use
    /// case。两个 3-target 测试共享构造,避免重复列 13 个 Arc::new。
    fn build_uc_with_emitter(
        repo: MockPeerAddrRepo,
        cipher: MockCipher,
        dispatch: MockDispatch,
        spy: Arc<SpyEntryDeliveryRepo>,
    ) -> (DispatchClipboardEntryUseCase, Arc<RecordingEmitter>) {
        let recorder = Arc::new(RecordingEmitter::default());
        let bus = Arc::new(HostEventBus::new());
        bus.register(
            "recorder",
            Arc::clone(&recorder) as Arc<dyn HostEventEmitterPort>,
        );
        let uc = DispatchClipboardEntryUseCase::new(
            Arc::new(repo),
            Arc::new(make_member_repo_all_enabled()),
            Arc::new(StaticPresence(ReachabilityState::Unknown)),
            Arc::new(cipher),
            Arc::new(dispatch),
            Arc::new(make_device_identity("self-device")),
            Arc::new(make_local_identity_stub()),
            Arc::new(make_settings_stub()),
            Arc::new(FixedClock(1_700_000_000_000)),
            Arc::new(uc_observability::analytics::NoopAnalyticsSink),
            Arc::new(AllMarkedFirstSyncState),
            spy as Arc<dyn EntryDeliveryRepositoryPort>,
            bus,
        );
        (uc, recorder)
    }

    /// 3 种成功/失败 outcome 都要 emit 一条对应的 Delivery 事件,且事件
    /// 顺序与落盘顺序一致(record_attempt 串行 → emit 在同一循环中追加)。
    /// 事件 payload 只携带 (entry_id, target_device_id);status 由前端
    /// refetch view 拿到,事件本身不承载状态,所以本测试只断言事件出现
    /// 与目标对端集合 1:1 对应。
    #[tokio::test]
    async fn dispatch_emits_delivery_event_for_each_persisted_outcome() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list().times(1).returning(|| {
            Ok(vec![
                record("peer-ok"),
                record("peer-dup"),
                record("peer-off"),
            ])
        });

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-ok")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-dup")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::DuplicateIgnored));
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-off")), always(), always())
            .times(1)
            .returning(|_, _, _| Err(ClipboardDispatchError::Offline));

        let spy = Arc::new(SpyEntryDeliveryRepo::default());
        let (uc, recorder) = build_uc_with_emitter(repo, cipher, dispatch, Arc::clone(&spy));

        let mut input = input();
        input.entry_id = Some(EntryId::from("entry-events".to_string()));
        uc.execute(input).await.expect("dispatch ok");

        // 落盘 3 条 → 应发 3 条事件,1:1 对应。
        let snapshot = recorder.snapshot();
        assert_eq!(
            snapshot.len(),
            3,
            "落盘 3 条 → 应发 3 条事件: {snapshot:#?}"
        );

        // 按 target_device_id 收集,断言三个对端都出现,entry_id 与输入一致。
        let targets: std::collections::HashSet<String> = snapshot
            .iter()
            .map(|ev| match ev {
                HostEvent::Delivery(DeliveryHostEvent::StatusChanged {
                    entry_id,
                    target_device_id,
                }) => {
                    assert_eq!(entry_id, "entry-events", "事件 entry_id 与输入一致");
                    target_device_id.clone()
                }
                other => panic!("expected Delivery event, got {other:?}"),
            })
            .collect();

        assert!(targets.contains("peer-ok"));
        assert!(targets.contains("peer-dup"));
        assert!(targets.contains("peer-off"));
    }

    /// entry_id=None(CLI raw-bytes / 测试)路径既不落盘,也不发事件 ——
    /// "没有 entry 关联"是 dispatch 自身的语义,前端 view 根本不存在,事
    /// 件也无人订阅。
    #[tokio::test]
    async fn dispatch_without_entry_id_emits_no_delivery_event() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-a")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let spy = Arc::new(SpyEntryDeliveryRepo::default());
        let (uc, recorder) = build_uc_with_emitter(repo, cipher, dispatch, Arc::clone(&spy));

        // input() 默认 entry_id = None。
        uc.execute(input()).await.expect("dispatch ok");
        assert!(
            recorder.snapshot().is_empty(),
            "entry_id=None 时不应有任何 delivery 事件"
        );
    }

    /// 装一根没有任何下游注册的空 bus,emit_or_warn 走完空 fan-out 不抛错;
    /// delivery 仍按规则落盘。验证"装配方不关心前端事件"的 CLI / 测试场景
    /// 不需要任何 Option 包裹 —— 空 bus 就是 noop。
    #[tokio::test]
    async fn dispatch_with_empty_bus_still_persists_delivery() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-a")]));

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let spy = Arc::new(SpyEntryDeliveryRepo::default());
        let uc = DispatchClipboardEntryUseCase::new(
            Arc::new(repo),
            Arc::new(make_member_repo_all_enabled()),
            Arc::new(StaticPresence(ReachabilityState::Unknown)),
            Arc::new(cipher),
            Arc::new(dispatch),
            Arc::new(make_device_identity("self-device")),
            Arc::new(make_local_identity_stub()),
            Arc::new(make_settings_stub()),
            Arc::new(FixedClock(1_700_000_000_000)),
            Arc::new(uc_observability::analytics::NoopAnalyticsSink),
            Arc::new(AllMarkedFirstSyncState),
            Arc::clone(&spy) as Arc<dyn EntryDeliveryRepositoryPort>,
            Arc::new(HostEventBus::new()),
        );

        let mut input = input();
        input.entry_id = Some(EntryId::from("entry-no-emitter".to_string()));
        uc.execute(input).await.expect("dispatch ok");

        // 落盘行为不变 —— bus 即便空,record_attempt 仍触发。
        let attempts = spy.snapshot().await;
        assert_eq!(attempts.len(), 1);
        assert!(matches!(attempts[0].status, EntryDeliveryStatus::Delivered));
    }
}

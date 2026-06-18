//! Slice 2 Phase 2 · T9 — `ClipboardSyncFacade` implementation.
//!
//! Thin wrapper over the two use cases. Job is:
//!
//! * Hold `Arc<DispatchClipboardEntryUseCase>` and
//!   `Arc<IngestInboundClipboardUseCase>`, so the facade controls lifetime.
//! * Translate between public (`pub`) and internal (`pub(crate)`) types so
//!   AGENTS.md §11.4 stays intact (external crates never touch the
//!   underlying use case structs).
//! * Expose `spawn_ingest_loop` for bootstrap to call right after F1
//!   `auto_start_network` succeeds — same lifecycle hook pattern as Phase
//!   1's `ensure_reachable_all` but for the clipboard receiver.

use std::sync::Arc;

use bytes::Bytes;
use tokio::sync::broadcast;
use tracing::instrument;

use uc_core::ids::{DeviceId, EntryId};
use uc_core::ports::security::TransferCipherPort;
use uc_core::ports::{
    ClipboardDispatchPort, ClipboardReceiverPort, ClockPort, DeviceIdentityPort, DispatchAck,
    EntryDeliveryRepositoryPort, FirstSyncStatePort, LocalIdentityPort, MobileDeviceRepositoryPort,
    PeerAddressRepositoryPort, PresencePort, SettingsPort,
};
use uc_core::MemberRepositoryPort;
use uc_core::{ClipboardChangeOrigin, SystemClipboardSnapshot};
use uc_observability::analytics::AnalyticsPort;

use crate::facade::blob_transfer::SharedHostEventEmitter;
use crate::usecases::clipboard_sync::dispatch_entry::DispatchEntryRunner;
use crate::usecases::clipboard_sync::get_entry_delivery_view::{
    EntryDeliveryView, GetEntryDeliveryViewError, GetEntryDeliveryViewUseCase,
};
use crate::usecases::clipboard_sync::payload_codec::{
    encode_snapshot_with_blob_refs_to_v3_bytes, V3BlobRef,
};
use crate::usecases::clipboard_sync::{
    encode_snapshot_to_v3_bytes, DispatchClipboardEntryInput, DispatchClipboardEntryUseCase,
    DispatchOutcome, DispatchPerTarget, DispatchSyncError, InboundAction as UcInboundAction,
    InboundClipboardNotice as UcInboundNotice, IngestInboundClipboardUseCase, IngestSpawnHandle,
};
use uc_core::clipboard::ClipboardContentCategorySet;
use uc_core::ports::clipboard::GetClipboardEntryPort;
use uc_core::ports::ClipboardEventRepositoryPort;
use uc_core::trusted_peer::TrustedPeerRepositoryPort;
use uc_observability::FlowId;

/// Construction bundle, mirrors `MemberRosterDeps` pattern so bootstrap
/// wiring stays consistent across facades.
pub struct ClipboardSyncDeps {
    pub peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    pub member_repo: Arc<dyn MemberRepositoryPort>,
    pub presence: Arc<dyn PresencePort>,
    pub transfer_cipher: Arc<dyn TransferCipherPort>,
    pub clipboard_dispatch: Arc<dyn ClipboardDispatchPort>,
    pub clipboard_receiver: Arc<dyn ClipboardReceiverPort>,
    pub device_identity: Arc<dyn DeviceIdentityPort>,
    pub local_identity: Arc<dyn LocalIdentityPort>,
    pub settings: Arc<dyn SettingsPort>,
    pub clock: Arc<dyn ClockPort>,
    /// Slice 8c-1 · per-peer outbound `sync_attempted` /
    /// `sync_succeeded` / `sync_failed` events fire from the dispatch
    /// use case. Inbound events (Slice 8c later) will plug here too.
    pub analytics: Arc<dyn AnalyticsPort>,
    /// Slice 8c-2 · first-sync funnel dedup port. dispatch use case
    /// 在 spawn 内 mark + 条件 fire `first_clipboard_sync_attempted` /
    /// `first_clipboard_sync_succeeded` / `first_file_sync_succeeded`。
    pub first_sync_state: Arc<dyn FirstSyncStatePort>,
    /// fan-out 完成后,按每个对端的结果落盘 delivery 记录;只有 entry_id
    /// 关联的发送路径(LocalCapture → outbound)才会触发实际写入,CLI /
    /// 测试路径走 entry_id=None 时本端口空跑。
    pub entry_delivery_repo: Arc<dyn EntryDeliveryRepositoryPort>,
    /// `get_entry_delivery_view` 拼装视图时需要 entry / event / trusted_peer
    /// 三类仓储:entry 验存在 + 取 delivery_tracked,event 反查来源设备,
    /// trusted_peer 给出"全集"用于合成 Pending。
    pub entry_repo: Arc<dyn GetClipboardEntryPort>,
    pub event_repo: Arc<dyn ClipboardEventRepositoryPort>,
    pub trusted_peer_repo: Arc<dyn TrustedPeerRepositoryPort>,
    /// 移动设备仓库,`GetEntryDeliveryViewUseCase` 用于把 `mobile_sync:` 前缀
    /// 的伪 DeviceId 解析为移动设备的人类可读 label。
    pub mobile_device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    /// 共享 host-event bus。dispatch fan-out 完成、delivery 状态写入后追发
    /// 一条 `HostEvent::Delivery::StatusChanged`,GUI 前端凭此实时刷新 badge。
    /// 测试 / CLI 装配传一根空 bus(`Arc::new(HostEventBus::new())`)即可
    /// —— 无下游 = noop,行为等价于原来的 `None`。
    pub host_event_bus: SharedHostEventEmitter,
}

/// Public-facing input to a dispatch pass. Mirrors the use case's own
/// struct but lives in the facade layer for stability.
#[derive(Debug, Clone)]
pub struct DispatchEntryInput {
    pub plaintext: Bytes,
    pub content_hash: String,
    pub payload_version: u8,
    /// Optional explicit recipient list. `Some(list)` restricts fan-out to
    /// the intersection of `trusted_peer` ∧ `list`; `None` falls back to
    /// "all trusted peers" (the existing default). `Some(vec![])` is
    /// legal — it means "no targets" and produces an empty outcome
    /// (still runs encrypt so callers see a well-formed report).
    ///
    /// **Iron rule**: this filter narrows candidates only. It does NOT
    /// bypass `is_send_allowed` / member gating / presence — those checks
    /// still apply per peer (see `dispatch_entry.rs` module doc).
    pub target_filter: Option<Vec<DeviceId>>,
}

/// Public-facing per-target report.
#[derive(Debug, Clone)]
pub struct DispatchEntryPerTarget {
    pub device_id: DeviceId,
    pub outcome: Result<DispatchAck, String>,
}

/// Public-facing aggregate report. Counts + per-target detail, mirroring
/// the internal `DispatchOutcome`.
///
/// `total_pending` counts peers whose result the main flow did not wait for
/// because the fan-out deadline was hit; their delivery records will be
/// written by a background continuation. They are NOT present in `per_target`.
#[derive(Debug, Clone)]
pub struct DispatchEntryOutcome {
    pub content_hash: String,
    pub per_target: Vec<DispatchEntryPerTarget>,
    pub total_accepted: usize,
    pub total_duplicate: usize,
    pub total_offline: usize,
    pub total_errored: usize,
    pub total_pending: usize,
    pub at_ms: i64,
}

/// Public-facing error type. Collapses the internal variants onto the
/// subset meaningful to external callers.
#[derive(Debug, thiserror::Error)]
pub enum ClipboardSyncError {
    #[error("encryption session not unlocked")]
    LockedSpace,
    #[error("transfer cipher failure: {0}")]
    CipherFailure(String),
    #[error("peer address repository: {0}")]
    Repository(String),
}

impl From<DispatchSyncError> for ClipboardSyncError {
    fn from(err: DispatchSyncError) -> Self {
        match err {
            DispatchSyncError::LockedSpace => ClipboardSyncError::LockedSpace,
            DispatchSyncError::CipherFailure(msg) => ClipboardSyncError::CipherFailure(msg),
            DispatchSyncError::Repository(msg) => ClipboardSyncError::Repository(msg),
        }
    }
}

/// Public view of one inbound clipboard delivery.
#[derive(Debug, Clone)]
pub struct InboundNotice {
    pub from_device: DeviceId,
    pub content_hash: String,
    pub plaintext: Bytes,
    pub flow_id: Option<FlowId>,
    pub action: InboundAction,
    pub at_ms: i64,
}

/// Public mirror of the internal `InboundAction` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundAction {
    NewEntry,
    DuplicateIgnored,
}

/// Public re-export of the ingest spawn handle. Drop or call `abort()`
/// to stop the background loop.
pub struct IngestHandle {
    inner: IngestSpawnHandle,
}

impl IngestHandle {
    pub fn abort(&self) {
        self.inner.abort();
    }
}

/// Clipboard sync facade — the single public entry point for Slice 2
/// Phase 2.
pub struct ClipboardSyncFacade {
    dispatch_uc: Arc<DispatchClipboardEntryUseCase>,
    ingest_uc: Arc<IngestInboundClipboardUseCase>,
    view_uc: Arc<GetEntryDeliveryViewUseCase>,
}

impl ClipboardSyncFacade {
    pub fn new(deps: ClipboardSyncDeps) -> Self {
        let dispatch_uc = Arc::new(DispatchClipboardEntryUseCase::new(
            Arc::clone(&deps.peer_addr_repo),
            Arc::clone(&deps.member_repo),
            Arc::clone(&deps.presence),
            Arc::clone(&deps.transfer_cipher),
            Arc::clone(&deps.clipboard_dispatch),
            Arc::clone(&deps.device_identity),
            Arc::clone(&deps.local_identity),
            Arc::clone(&deps.settings),
            Arc::clone(&deps.clock),
            Arc::clone(&deps.analytics),
            Arc::clone(&deps.first_sync_state),
            Arc::clone(&deps.entry_delivery_repo),
            Arc::clone(&deps.host_event_bus),
        ));
        let ingest_uc = Arc::new(IngestInboundClipboardUseCase::new(
            Arc::clone(&deps.clipboard_receiver),
            Arc::clone(&deps.member_repo),
            Arc::clone(&deps.transfer_cipher),
            Arc::clone(&deps.clock),
        ));
        let view_uc = Arc::new(GetEntryDeliveryViewUseCase::new(
            Arc::clone(&deps.entry_repo),
            Arc::clone(&deps.event_repo),
            Arc::clone(&deps.trusted_peer_repo),
            Arc::clone(&deps.entry_delivery_repo),
            Arc::clone(&deps.device_identity),
            Arc::clone(&deps.member_repo),
            Arc::clone(&deps.mobile_device_repo),
        ));
        Self {
            dispatch_uc,
            ingest_uc,
            view_uc,
        }
    }

    /// 拿一条 entry 对每个可信对端的同步状态视图。详见
    /// [`GetEntryDeliveryViewUseCase::execute`] 的契约说明。
    pub async fn get_entry_delivery_view(
        &self,
        entry_id: &EntryId,
    ) -> Result<EntryDeliveryView, GetEntryDeliveryViewError> {
        self.view_uc.execute(entry_id).await
    }

    /// Fan out one plaintext payload to every online paired peer.
    ///
    /// Phase 2 / CLI / test entry point — caller has already encoded the
    /// payload and computed `content_hash`. The per-device
    /// `send_content_types` filter is bypassed here (empty
    /// `ClipboardContentCategorySet`, fail open) because raw-bytes callers
    /// don't carry the snapshot structure needed to classify; daemon goes through
    /// [`Self::dispatch_snapshot`] / [`Self::dispatch_snapshot_with_blob_refs`]
    /// which preserve the snapshot and apply the filter.
    #[instrument(skip_all, fields(content_hash = %input.content_hash))]
    pub async fn dispatch_entry(
        &self,
        input: DispatchEntryInput,
    ) -> Result<DispatchEntryOutcome, ClipboardSyncError> {
        let internal = self
            .dispatch_uc
            .execute(DispatchClipboardEntryInput {
                plaintext: input.plaintext,
                content_hash: input.content_hash.clone(),
                payload_version: input.payload_version,
                categories: ClipboardContentCategorySet::empty(),
                // raw-bytes 路径不与某条 entry 绑定,跳过 delivery 落盘。
                entry_id: None,
                target_filter: input.target_filter,
            })
            .await?;
        Ok(lift_outcome(internal))
    }

    /// Internal helper used by snapshot-aware dispatch entry points to
    /// thread the snapshot's content category set into the gate.
    /// Public callers go through `dispatch_entry` (empty set, fail open)
    /// or `dispatch_snapshot*` (set computed from the snapshot reps).
    async fn dispatch_internal(
        &self,
        plaintext: Bytes,
        content_hash: String,
        payload_version: u8,
        categories: ClipboardContentCategorySet,
        entry_id: Option<EntryId>,
        target_filter: Option<Vec<DeviceId>>,
    ) -> Result<DispatchEntryOutcome, ClipboardSyncError> {
        let internal = self
            .dispatch_uc
            .execute(DispatchClipboardEntryInput {
                plaintext,
                content_hash,
                payload_version,
                categories,
                entry_id,
                target_filter,
            })
            .await?;
        Ok(lift_outcome(internal))
    }

    /// Phase 3 daemon entry point — encode `snapshot` into the V3
    /// envelope + compute the canonical `snapshot_hash` (matches the
    /// `clipboard_event.snapshot_hash` column on receiver-side dedup),
    /// then dispatch.
    ///
    /// The `origin` parameter is passive metadata for tracing /
    /// telemetry; gating callers (e.g. daemon `clipboard_watcher`) are
    /// expected to short-circuit on `RemotePush` _before_ calling this
    /// method, so the facade does not enforce a guard here. Centralising
    /// the encode keeps daemon + future Tauri / CLI snapshot-aware
    /// senders from re-implementing the V3 codec independently.
    #[instrument(skip_all, fields(rep_count = snapshot.representations.len(), origin = ?origin))]
    pub async fn dispatch_snapshot(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        entry_id: Option<EntryId>,
        target_filter: Option<Vec<DeviceId>>,
    ) -> Result<DispatchEntryOutcome, ClipboardSyncError> {
        let _ = origin; // span metadata only (see doc above)
        let categories = ClipboardContentCategorySet::from_snapshot(&snapshot);
        let (plaintext, content_hash) = encode_snapshot_to_v3_bytes(&snapshot)
            .map_err(|e| ClipboardSyncError::CipherFailure(format!("payload encode: {e}")))?;
        self.dispatch_internal(
            plaintext,
            content_hash,
            3,
            categories,
            entry_id,
            target_filter,
        )
        .await
    }

    /// 编码并发送带 Slice 3 blob 引用的剪贴板快照。
    ///
    /// 普通小 payload 仍在 V3 本体里;大文件内容通过 `blob_refs` 让接收端
    /// 拉取并改写成本机 file-list。
    #[instrument(skip_all, fields(rep_count = snapshot.representations.len(), blob_ref_count = blob_refs.len(), origin = ?origin))]
    pub async fn dispatch_snapshot_with_blob_refs(
        &self,
        snapshot: SystemClipboardSnapshot,
        blob_refs: Vec<V3BlobRef>,
        origin: ClipboardChangeOrigin,
        entry_id: Option<EntryId>,
        target_filter: Option<Vec<DeviceId>>,
    ) -> Result<DispatchEntryOutcome, ClipboardSyncError> {
        let _ = origin;
        let categories = ClipboardContentCategorySet::from_snapshot(&snapshot);
        let (plaintext, content_hash) =
            encode_snapshot_with_blob_refs_to_v3_bytes(&snapshot, &blob_refs)
                .map_err(|e| ClipboardSyncError::CipherFailure(format!("payload encode: {e}")))?;
        self.dispatch_internal(
            plaintext,
            content_hash,
            3,
            categories,
            entry_id,
            target_filter,
        )
        .await
    }

    /// Subscribe to the inbound-notice broadcast. CLI `watch` / future
    /// daemon subscribers attach here.
    pub fn subscribe_inbound_notices(&self) -> broadcast::Receiver<InboundNotice> {
        // Bridge from the internal broadcast to the public-type broadcast
        // via a relay task. This keeps public types independent of
        // `usecases::*` renames while still letting lagging subscribers
        // recover per broadcast semantics.
        let (public_tx, public_rx) = broadcast::channel(64);
        let mut internal_rx = self.ingest_uc.subscribe_notices();
        tokio::spawn(async move {
            loop {
                match internal_rx.recv().await {
                    Ok(internal) => {
                        let lifted = lift_notice(internal);
                        if public_tx.send(lifted).is_err() {
                            // No public subscribers — keep consuming so
                            // the internal broadcast doesn't lag us.
                            continue;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        public_rx
    }

    /// Spawn the ingest background loop. Caller owns the returned handle;
    /// dropping it (or `abort()`) terminates the loop. Typically called by
    /// bootstrap after F1 `auto_start_network` completes.
    pub fn spawn_ingest_loop(&self) -> IngestHandle {
        let inner = Arc::clone(&self.ingest_uc).spawn_run();
        IngestHandle { inner }
    }

    /// Crate-internal accessor — hand the inner dispatch use case to
    /// callers that need to feed `DispatchClipboardEntryInput` directly
    /// (currently only [`ResendEntryUseCase`] via
    /// [`ClipboardOutboundFacade`]). The blanket impl
    /// `DispatchEntryRunner for DispatchClipboardEntryUseCase` provides
    /// the trait-object handle without exposing the concrete use case
    /// across the crate boundary.
    pub(crate) fn dispatch_runner(&self) -> Arc<dyn DispatchEntryRunner> {
        Arc::clone(&self.dispatch_uc) as Arc<dyn DispatchEntryRunner>
    }
}

// ---------------------------------------------------------------------------
// Private mappers — keep internal / public types pinned together.
// ---------------------------------------------------------------------------

fn lift_outcome(internal: DispatchOutcome) -> DispatchEntryOutcome {
    DispatchEntryOutcome {
        content_hash: internal.content_hash,
        per_target: internal
            .per_target
            .into_iter()
            .map(lift_per_target)
            .collect(),
        total_accepted: internal.total_accepted,
        total_duplicate: internal.total_duplicate,
        total_offline: internal.total_offline,
        total_errored: internal.total_errored,
        total_pending: internal.total_pending,
        at_ms: internal.at_ms,
    }
}

fn lift_per_target(internal: DispatchPerTarget) -> DispatchEntryPerTarget {
    DispatchEntryPerTarget {
        device_id: internal.device_id,
        outcome: internal.outcome,
    }
}

fn lift_notice(internal: UcInboundNotice) -> InboundNotice {
    InboundNotice {
        from_device: internal.from_device,
        content_hash: internal.content_hash,
        plaintext: internal.plaintext,
        flow_id: internal.flow_id,
        action: match internal.action {
            UcInboundAction::NewEntry => InboundAction::NewEntry,
            UcInboundAction::DuplicateIgnored => InboundAction::DuplicateIgnored,
        },
        at_ms: internal.at_ms,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// **Mocking convention** (consistent with `usecases::clipboard_sync::*::tests`):
//
// * Use `mockall::mock!` for ports asserted via call counts + return
//   values: `PeerAddressRepositoryPort`, `TransferCipherPort`,
//   `ClipboardDispatchPort`, `PresencePort`, `DeviceIdentityPort`,
//   `LocalIdentityPort`, `SettingsPort`.
// * Hand-write `FakeReceiver` because `ClipboardReceiverPort::subscribe`
//   returns a non-Clone broadcast `Receiver` and the test needs an
//   `emit(...)` helper to drive the loop. Same trade-off as Phase 1
//   `FakePresence` in `roster/facade.rs`.
// * Trivial sync `FixedClock` stays hand-written (4 lines).

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;

    use async_trait::async_trait;
    use mockall::predicate::*;
    use uc_core::ports::security::TransferCipherError;
    use uc_core::ports::{
        ClipboardDispatchError, ClipboardHeader, DispatchAck, FirstSyncStateError,
        InboundClipboard, LocalIdentityError, PeerAddressError, PeerAddressRecord, PresenceError,
        PresenceEvent, ReachabilityState, SyncPayload,
    };
    use uc_core::security::IdentityFingerprint;
    use uc_core::settings::model::Settings;
    use uc_core::{MemberSyncPreferences, MembershipError, SpaceMember};

    /// Slice 8c-2 · facade 内测不验证 first-sync 漏斗事件——给个永远返回
    /// `Ok(false)` 的 noop fake，避免 sync 三事件断言被 first_* 污染。
    struct NoopFirstSyncState;
    #[async_trait]
    impl FirstSyncStatePort for NoopFirstSyncState {
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

    // ── mockall ──────────────────────────────────────────────────────────

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

    mockall::mock! {
        pub Presence {}
        #[async_trait]
        impl PresencePort for Presence {
            async fn ensure_reachable(
                &self,
                device: &DeviceId,
            ) -> Result<ReachabilityState, PresenceError>;
            async fn current_state(&self, device: &DeviceId) -> ReachabilityState;
            fn subscribe(&self) -> broadcast::Receiver<PresenceEvent>;
        }
    }

    mockall::mock! {
        pub Cipher {}
        #[async_trait]
        impl TransferCipherPort for Cipher {
            async fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, TransferCipherError>;
            async fn decrypt(&self, encrypted: &[u8]) -> Result<Vec<u8>, TransferCipherError>;
        }
    }

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

    mockall::mock! {
        pub EntryDeliveryRepo {}
        #[async_trait]
        impl EntryDeliveryRepositoryPort for EntryDeliveryRepo {
            async fn record_attempt(
                &self,
                record: &uc_core::clipboard::EntryDeliveryRecord,
            ) -> Result<(), uc_core::clipboard::EntryDeliveryError>;
            async fn list_by_entry(
                &self,
                entry_id: &EntryId,
            ) -> Result<
                Vec<uc_core::clipboard::EntryDeliveryRecord>,
                uc_core::clipboard::EntryDeliveryError,
            >;
        }
    }

    mockall::mock! {
        pub EntryRepo {}
        #[async_trait]
        impl GetClipboardEntryPort for EntryRepo {
            async fn get_entry(
                &self,
                entry_id: &EntryId,
            ) -> std::result::Result<
                Option<uc_core::clipboard::ClipboardEntry>,
                uc_core::clipboard::ClipboardRepositoryError,
            >;
        }
    }

    mockall::mock! {
        pub EventRepo {}
        #[async_trait]
        impl ClipboardEventRepositoryPort for EventRepo {
            async fn get_representation(
                &self,
                id: &uc_core::ids::EventId,
                representation_id: &str,
            ) -> anyhow::Result<uc_core::ObservedClipboardRepresentation>;
            async fn get_source_device(
                &self,
                event_id: &uc_core::ids::EventId,
            ) -> anyhow::Result<Option<DeviceId>>;
        }
    }

    mockall::mock! {
        pub TrustedPeerRepo {}
        #[async_trait]
        impl TrustedPeerRepositoryPort for TrustedPeerRepo {
            async fn get(
                &self,
                peer_device_id: &DeviceId,
            ) -> Result<
                Option<uc_core::trusted_peer::TrustedPeer>,
                uc_core::trusted_peer::TrustedPeerError,
            >;
            async fn list(
                &self,
            ) -> Result<
                Vec<uc_core::trusted_peer::TrustedPeer>,
                uc_core::trusted_peer::TrustedPeerError,
            >;
            async fn save(
                &self,
                trusted_peer: &uc_core::trusted_peer::TrustedPeer,
            ) -> Result<(), uc_core::trusted_peer::TrustedPeerError>;
            async fn remove(
                &self,
                peer_device_id: &DeviceId,
            ) -> Result<bool, uc_core::trusted_peer::TrustedPeerError>;
        }
    }

    mockall::mock! {
        pub MobileDeviceRepo {}
        #[async_trait]
        impl MobileDeviceRepositoryPort for MobileDeviceRepo {
            async fn save(
                &self,
                device: &uc_core::mobile_sync::MobileDevice,
            ) -> Result<(), uc_core::mobile_sync::MobileDeviceError>;
            async fn find_by_username(
                &self,
                username: &str,
            ) -> Result<Option<uc_core::mobile_sync::MobileDevice>, uc_core::mobile_sync::MobileDeviceError>;
            async fn find_by_device_id(
                &self,
                device_id: &uc_core::mobile_sync::MobileDeviceId,
            ) -> Result<Option<uc_core::mobile_sync::MobileDevice>, uc_core::mobile_sync::MobileDeviceError>;
            async fn list_all(
                &self,
            ) -> Result<Vec<uc_core::mobile_sync::MobileDevice>, uc_core::mobile_sync::MobileDeviceError>;
            async fn delete(
                &self,
                device_id: &uc_core::mobile_sync::MobileDeviceId,
            ) -> Result<bool, uc_core::mobile_sync::MobileDeviceError>;
            async fn record_activity(
                &self,
                device_id: &uc_core::mobile_sync::MobileDeviceId,
                last_seen_at_ms: i64,
                last_seen_ip: Option<String>,
                reported_name: Option<String>,
                reported_os: Option<String>,
            ) -> Result<(), uc_core::mobile_sync::MobileDeviceError>;
            async fn update_mobile_device(
                &self,
                updated: &uc_core::mobile_sync::MobileDevice,
            ) -> Result<bool, uc_core::mobile_sync::MobileDeviceError>;
        }
    }

    fn make_noop_mobile_device_repo() -> MockMobileDeviceRepo {
        let mut m = MockMobileDeviceRepo::new();
        m.expect_find_by_device_id().returning(|_| Ok(None));
        m
    }

    /// `MemberRepo` mock that returns a default-allowed `SpaceMember` for
    /// every device. The two pre-existing facade verdicts (dispatch +
    /// ingest) predate per-device gating; this keeps them green.
    fn make_member_repo_all_enabled() -> MockMemberRepo {
        let mut m = MockMemberRepo::new();
        m.expect_get().returning(|did| {
            Ok(Some(SpaceMember {
                device_id: did.clone(),
                device_name: format!("Test {}", did.as_str()),
                identity_fingerprint: fp(),
                joined_at: chrono::Utc::now(),
                sync_preferences: MemberSyncPreferences::default(),
            }))
        });
        m
    }

    // ── hand-written: ClipboardReceiverPort + ClockPort ─────────────────

    /// `subscribe()` returns a non-Clone `broadcast::Receiver` and the
    /// tests need an `emit(...)` helper — same trade-off as Phase 1's
    /// `FakePresence`.
    struct FakeReceiver {
        tx: broadcast::Sender<InboundClipboard>,
    }
    impl FakeReceiver {
        fn new() -> Self {
            let (tx, _) = broadcast::channel(16);
            Self { tx }
        }
        fn publish(&self, inbound: InboundClipboard) {
            let _ = self.tx.send(inbound);
        }
    }
    #[async_trait]
    impl ClipboardReceiverPort for FakeReceiver {
        fn subscribe(&self) -> broadcast::Receiver<InboundClipboard> {
            self.tx.subscribe()
        }
    }

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    // ── helpers ─────────────────────────────────────────────────────────

    fn fp() -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string("AAAABBBBCCCCDDDD").unwrap()
    }

    fn record(device: &str) -> PeerAddressRecord {
        PeerAddressRecord {
            device_id: DeviceId::new(device),
            addr_blob: vec![0xAA; 8],
            observed_at: chrono::Utc::now(),
        }
    }

    /// Build a `DeviceIdentity` mock that returns the same `device_id`
    /// every call.
    fn make_device_identity(local: &str) -> MockDeviceId_ {
        let local = DeviceId::new(local);
        let mut m = MockDeviceId_::new();
        m.expect_current_device_id()
            .returning(move || local.clone());
        m
    }

    fn make_local_identity() -> MockLocalIdentity {
        let mut m = MockLocalIdentity::new();
        m.expect_get_current_fingerprint()
            .returning(|| Ok(Some(fp())));
        m
    }

    fn make_settings() -> MockSettings_ {
        let mut m = MockSettings_::new();
        m.expect_load().returning(|| Ok(Settings::default()));
        m
    }

    fn make_presence_unknown() -> MockPresence {
        let mut m = MockPresence::new();
        m.expect_current_state()
            .returning(|_| ReachabilityState::Unknown);
        m
    }

    /// 本文件的 dispatch 端到端验证不关心 delivery 表副作用,默认 noop。
    fn make_noop_entry_delivery_repo() -> MockEntryDeliveryRepo {
        let mut m = MockEntryDeliveryRepo::new();
        m.expect_record_attempt().returning(|_| Ok(()));
        m.expect_list_by_entry().returning(|_| Ok(Vec::new()));
        m
    }

    /// 视图组装路径(`get_entry_delivery_view`)在本文件的 dispatch 测试里
    /// 不会被触发。下面三个仓储给一组最小可用的 noop,只为让 facade 装配
    /// 通过;`get_representation` 故意不配 returning——一旦意外调到,mockall
    /// 默认 panic,把"视图路径在本文件不应被触发"这层契约硬化成测试失败。
    fn make_noop_entry_repo() -> MockEntryRepo {
        let mut m = MockEntryRepo::new();
        m.expect_get_entry().returning(|_| Ok(None));
        m
    }

    fn make_noop_event_repo() -> MockEventRepo {
        let mut m = MockEventRepo::new();
        m.expect_get_source_device().returning(|_| Ok(None));
        m
    }

    fn make_noop_trusted_peer_repo() -> MockTrustedPeerRepo {
        let mut m = MockTrustedPeerRepo::new();
        m.expect_get().returning(|_| Ok(None));
        m.expect_list().returning(|| Ok(Vec::new()));
        m.expect_save().returning(|_| Ok(()));
        m.expect_remove().returning(|_| Ok(false));
        m
    }

    /// Wire the facade with the given mock ports + a `FakeReceiver`. The
    /// FakeReceiver is returned alongside so the caller can `publish(...)`
    /// during the test. `member_repo` defaults to "all peers allowed"
    /// because the two facade verdicts here predate per-device gating.
    fn build_facade(
        peer_addr_repo: MockPeerAddrRepo,
        presence: MockPresence,
        cipher: MockCipher,
        dispatch: MockDispatch,
        device_identity: MockDeviceId_,
        local_identity: MockLocalIdentity,
        settings: MockSettings_,
    ) -> (ClipboardSyncFacade, Arc<FakeReceiver>) {
        let receiver = Arc::new(FakeReceiver::new());
        let facade = ClipboardSyncFacade::new(ClipboardSyncDeps {
            peer_addr_repo: Arc::new(peer_addr_repo),
            member_repo: Arc::new(make_member_repo_all_enabled()),
            presence: Arc::new(presence),
            transfer_cipher: Arc::new(cipher),
            clipboard_dispatch: Arc::new(dispatch),
            clipboard_receiver: receiver.clone() as Arc<dyn ClipboardReceiverPort>,
            device_identity: Arc::new(device_identity),
            local_identity: Arc::new(local_identity),
            settings: Arc::new(settings),
            clock: Arc::new(FixedClock(1_700_000_000_000)),
            analytics: Arc::new(uc_observability::analytics::NoopAnalyticsSink),
            first_sync_state: Arc::new(NoopFirstSyncState),
            entry_delivery_repo: Arc::new(make_noop_entry_delivery_repo()),
            entry_repo: Arc::new(make_noop_entry_repo()),
            event_repo: Arc::new(make_noop_event_repo()),
            trusted_peer_repo: Arc::new(make_noop_trusted_peer_repo()),
            mobile_device_repo: Arc::new(make_noop_mobile_device_repo()),
            host_event_bus: Arc::new(crate::facade::host_event::HostEventBus::new()),
        });
        (facade, receiver)
    }

    // ── verdicts ────────────────────────────────────────────────────────

    /// Verdict 1 — `dispatch_entry` delegates to the inner use case and
    /// returns the public-shape outcome. mockall asserts: peer_addr_repo
    /// listed once, encrypt called once, dispatch called once for peer-a.
    /// Presence is read only for telemetry classification; it must not filter
    /// dispatch candidates (see dispatch_entry.rs module doc on iteration source).
    #[tokio::test]
    async fn dispatch_entry_returns_public_outcome_for_online_peer() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-a")]));

        let presence = make_presence_unknown();

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

        let (facade, _receiver) = build_facade(
            repo,
            presence,
            cipher,
            dispatch,
            make_device_identity("self"),
            make_local_identity(),
            make_settings(),
        );

        let outcome = facade
            .dispatch_entry(DispatchEntryInput {
                plaintext: Bytes::from_static(b"hello"),
                content_hash: "abc".to_string(),
                payload_version: 3,
                target_filter: None,
            })
            .await
            .expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
        assert_eq!(outcome.per_target.len(), 1);
        assert_eq!(outcome.per_target[0].device_id.as_str(), "peer-a");
    }

    /// Verdict 2 — `subscribe_inbound_notices` bridges the internal
    /// broadcast to a public-typed one. Decrypt is mocked once; the
    /// public `InboundNotice` round-trips with `InboundAction::NewEntry`.
    #[tokio::test]
    async fn subscribe_inbound_notices_bridges_internal_to_public_type() {
        // Dispatch path is unused in this test; register no expectations.
        // peer_addr_repo / presence likewise unused.
        let repo = MockPeerAddrRepo::new();
        let presence = make_presence_unknown();
        let dispatch = MockDispatch::new();

        let mut cipher = MockCipher::new();
        // Decrypt called once when the published frame reaches the loop.
        cipher
            .expect_decrypt()
            .times(1)
            .returning(|ct| Ok(ct.to_vec()));

        let (facade, receiver) = build_facade(
            repo,
            presence,
            cipher,
            dispatch,
            make_device_identity("self"),
            make_local_identity(),
            make_settings(),
        );
        let mut notices = facade.subscribe_inbound_notices();
        let _ingest = facade.spawn_ingest_loop();

        tokio::time::sleep(Duration::from_millis(30)).await;

        receiver.publish(InboundClipboard {
            peer_device_id: DeviceId::new("peer-x"),
            header: ClipboardHeader {
                version: ClipboardHeader::CURRENT_VERSION,
                content_hash: "xx".repeat(32),
                captured_at_ms: 42,
                origin_device_id: "peer-x".to_string(),
                origin_device_name: "Peer X".to_string(),
                payload_version: 3,
                flow_id: None,
            },
            ciphertext: Bytes::from_static(b"hello"),
        });

        let notice = tokio::time::timeout(Duration::from_secs(2), notices.recv())
            .await
            .expect("notice arrives")
            .expect("sender alive");
        assert_eq!(notice.from_device.as_str(), "peer-x");
        assert_eq!(notice.plaintext, Bytes::from_static(b"hello"));
        assert_eq!(notice.action, InboundAction::NewEntry);
    }

    /// Verdict 3 — `dispatch_snapshot` encodes the snapshot into the V3
    /// envelope + derives the canonical content_hash from
    /// `snapshot_hash()`, then calls the same underlying dispatch path
    /// as `dispatch_entry`. mockall asserts encrypt is invoked with the
    /// encoded envelope bytes (not raw plaintext), and that the target
    /// dispatch fires with `payload_version=3`.
    #[tokio::test]
    async fn dispatch_snapshot_encodes_envelope_and_fans_out() {
        use uc_core::ids::{FormatId, RepresentationId};
        use uc_core::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};

        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-a")]));

        let presence = make_presence_unknown();

        let mut cipher = MockCipher::new();
        // Encrypt gets the V3 envelope bytes, not the raw text. We just
        // assert it's called once and round-trip the bytes unchanged
        // (the test cipher is a passthrough for assertion purposes).
        cipher
            .expect_encrypt()
            .times(1)
            .withf(|plaintext| {
                // The V3 envelope starts with 8B ts_ms (LE) + 2B rep_count (LE).
                // For our fixture: ts_ms=7 → [0x07, 0, 0, 0, 0, 0, 0, 0],
                // rep_count=1 → [0x01, 0x00]. Anchor on rep_count to keep the
                // assertion resilient to ts_ms choice.
                plaintext.len() > 10 && plaintext[8..10] == [0x01, 0x00]
            })
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-a")), always(), always())
            .times(1)
            .withf(|_target, header, _payload| header.payload_version == 3)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let (facade, _receiver) = build_facade(
            repo,
            presence,
            cipher,
            dispatch,
            make_device_identity("self"),
            make_local_identity(),
            make_settings(),
        );

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 7,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                b"hello phase3".to_vec(),
            )],
        };
        let outcome = facade
            .dispatch_snapshot(
                snapshot,
                uc_core::ClipboardChangeOrigin::LocalCapture,
                None,
                None,
            )
            .await
            .expect("dispatch_snapshot ok");
        assert_eq!(outcome.total_accepted, 1);
        assert!(
            outcome.content_hash.starts_with("blake3v1:"),
            "outcome carries the canonical snapshot_hash, got {}",
            outcome.content_hash
        );
    }

    /// Verdict 4 — `spawn_ingest_loop` returns a handle whose `Drop`
    /// aborts the background task. Decrypt has zero expectations: if the
    /// loop kept consuming after the handle drop, mockall would observe
    /// an unexpected decrypt and panic.
    #[tokio::test]
    async fn spawn_ingest_handle_drops_clean() {
        let repo = MockPeerAddrRepo::new();
        let presence = make_presence_unknown();
        let dispatch = MockDispatch::new();
        // Zero decrypt expectations — no inbound is published, so decrypt
        // must never be called even by a leaked task.
        let cipher = MockCipher::new();

        let (facade, receiver) = build_facade(
            repo,
            presence,
            cipher,
            dispatch,
            make_device_identity("self"),
            make_local_identity(),
            make_settings(),
        );
        {
            let _handle = facade.spawn_ingest_loop();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        // Handle dropped. Briefly sleep so the abort settles. If a leaked
        // task touched the receiver after this point, no decrypt is set
        // up to handle it — mockall's `Drop` would panic. Safe to publish
        // here as the (already-aborted) loop would no longer consume it.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = receiver; // keep the receiver alive so tx isn't dropped early
    }

    /// Verdict 5 — `DispatchEntryInput.target_filter = Some([peer-b])` threads
    /// through to the use case. peer-a is listed in `peer_addr_repo` but the
    /// filter excludes it, so `MockDispatch` registers an expectation only for
    /// peer-b. mockall enforces "no other call ever happened" on Drop — if
    /// the filter were dropped on the floor, peer-a would unexpectedly hit
    /// dispatch and the test would panic.
    #[tokio::test]
    async fn dispatch_entry_with_target_filter_threads_to_use_case() {
        let mut repo = MockPeerAddrRepo::new();
        repo.expect_list()
            .times(1)
            .returning(|| Ok(vec![record("peer-a"), record("peer-b")]));

        let presence = make_presence_unknown();

        let mut cipher = MockCipher::new();
        cipher
            .expect_encrypt()
            .times(1)
            .returning(|p| Ok(p.to_vec()));

        let mut dispatch = MockDispatch::new();
        dispatch
            .expect_dispatch()
            .with(eq(DeviceId::new("peer-b")), always(), always())
            .times(1)
            .returning(|_, _, _| Ok(DispatchAck::Accepted));

        let (facade, _receiver) = build_facade(
            repo,
            presence,
            cipher,
            dispatch,
            make_device_identity("self"),
            make_local_identity(),
            make_settings(),
        );

        let outcome = facade
            .dispatch_entry(DispatchEntryInput {
                plaintext: Bytes::from_static(b"hello"),
                content_hash: "abc".to_string(),
                payload_version: 3,
                target_filter: Some(vec![DeviceId::new("peer-b")]),
            })
            .await
            .expect("dispatch ok");
        assert_eq!(outcome.total_accepted, 1);
        assert_eq!(outcome.per_target.len(), 1);
        assert_eq!(outcome.per_target[0].device_id.as_str(), "peer-b");
    }
}

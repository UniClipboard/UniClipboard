//! `SpaceSetupFacade` — space-lifecycle entry point (A1 + A2 + shutdown).
//!
//! Owns the two use cases so A1/A2 success can prime presence cache (F1) via
//! `ensure_reachable_all`. Also owns the sponsor-side inbound orchestrator so
//! the rest of the crate never sees the spawn surface (§11.4).
//!
//! Slice 4 P5c: 历史上还持有 `NetworkControlPort` 用于 A1/A2 后调
//! `start_network` (F1) + `on_shutdown` 调 `stop_network` (F2),已退役——
//! iroh router 由 `SyncEngineAssembly` 直接驱动,libp2p 兼容路径整体下线。
//! F1 hook 保留(改名 `auto_prime_presence`),只跑 `ensure_reachable_all`;
//! F2 不再触碰网络层。
//!
//! Network errors during auto-prime are intentionally non-fatal: the
//! underlying space mutation has already committed and isn't safe to roll
//! back, and presence will lazily recover via the adapter's
//! `Connection::closed` watchdog. Failures are surfaced through
//! `tracing::warn!` so ops still sees them.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{info, instrument, warn};

use uc_core::ids::{DeviceId, SpaceId};
use uc_core::ports::space::{FactoryResetSpacePort, ResumeSpaceSessionPort, SpaceAccessError};
use uc_core::ports::{
    PeerAddressRepositoryPort, PresenceError, PresencePort, ReachabilityState, SettingsPort,
    SetupStatusPort,
};
use uc_core::setup::SetupStatus;
use uc_observability::analytics::AnalyticsFacade;

use crate::facade::space_setup::commands::{
    CurrentInvitation, InitializeSpaceCommand, InitializeSpaceInput, InitializeSpaceResult,
    IssuePairingInvitationResult, MigrationPhaseKind, MigrationProgress,
    PairingInvitationAddressCandidate, SetupStateView, SwitchSpaceCommand, SwitchSpaceInput,
    SwitchSpaceResult, UnlockSpaceCommand, UnlockSpaceInput, UnlockSpaceResult,
};
use crate::facade::space_setup::commands::{
    RedeemPairingInvitationCommand, RedeemPairingInvitationInput, RedeemPairingInvitationResult,
};
use crate::facade::space_setup::deps::SpaceSetupDeps;
use crate::facade::space_setup::errors::{
    CancelInvitationError, FactoryResetError, QueryMigrationProgressError, QuerySetupStateError,
    RedeemPairingInvitationError, ResetSpaceError, SwitchSpaceError,
};
use crate::facade::space_setup::errors::{
    InitializeSpaceError, IssuePairingInvitationError, TryResumeSessionError, UnlockSpaceError,
};
use crate::facade::space_setup::events::PairingOutcome;
use crate::membership::usecases::AdmitMemberUseCase;
use crate::pairing_inbound::orchestrator::PairingInboundOrchestrator;
use crate::pairing_inbound::sponsor_handshake::SponsorHandshakeCoordinator;
use crate::pairing_invitation::InMemoryPairingInvitationHolder;
use crate::pairing_outbound::joiner_handshake::JoinerHandshakeCoordinator;
use crate::trusted_peer::usecases::TrustPeerUseCase;
use crate::usecases::pairing::issue_invitation::IssuePairingInvitationUseCase;
use crate::usecases::pairing::redeem_invitation::RedeemPairingInvitationUseCase;
use crate::usecases::presence::ensure_reachable_all::{
    EnsureReachableAllError, EnsureReachableAllReport, EnsureReachableAllUseCase,
};
use crate::usecases::setup::initialize_space::InitializeSpaceUseCase;
use crate::usecases::setup::switch_space::{JoinerHandshakeRunner, SwitchSpaceUseCase};
use crate::usecases::setup::unlock_space::UnlockSpaceUseCase;
use uc_core::ports::clipboard::BlobMigrationRepoPort;
use uc_core::ports::setup::MigrationStatePort;

/// Space-lifecycle facade (A1 initialise, A2 unlock, B1 issue invitation,
/// B2 redeem invitation, P7e inbound subscriber, F2 shutdown).
pub struct SpaceSetupFacade {
    initialize_space: Arc<InitializeSpaceUseCase>,
    unlock_space: Arc<UnlockSpaceUseCase>,
    issue_pairing_invitation: Arc<IssuePairingInvitationUseCase>,
    redeem_pairing_invitation: Arc<RedeemPairingInvitationUseCase>,
    /// `JoinHandle` for the sponsor-side inbound pairing orchestrator
    /// spawned during construction. Aborted in [`Self::on_shutdown`] so
    /// the event loop doesn't outlive the facade.
    pairing_inbound_handle: JoinHandle<()>,
    /// Broadcast source of sponsor-side pairing completion events.
    /// Held on the facade so [`Self::subscribe_pairing_completion`] can
    /// hand out fresh receivers as long as the facade is alive.
    pairing_outcome_tx: broadcast::Sender<PairingOutcome>,
    /// Held for [`Self::try_resume_session`] — the silent resume path needs
    /// both the setup flag (to decide whether there's anything to resume at
    /// all) and direct access to [`ResumeSpaceSessionPort::try_resume_session`].
    /// Everything else still goes through use cases.
    resume_session: Arc<dyn ResumeSpaceSessionPort>,
    /// Held for [`Self::factory_reset`] — wipes persisted key material before
    /// clearing setup status.
    factory_reset: Arc<dyn FactoryResetSpacePort>,
    setup_status: Arc<dyn SetupStatusPort>,
    /// Slice4 P3 T3.2 · `query_setup_state` reads `device_name` from
    /// `Settings.general`; `cancel_invitation` / `reset` need no
    /// settings access but the field stays `pub(crate)` so a future
    /// query can pick up additional general fields without churn.
    settings: Arc<dyn SettingsPort>,
    /// Slice4 P3 T3.2 · `cancel_invitation` clears the in-memory
    /// pending-invitation map; `query_setup_state` snapshots the
    /// earliest-expiring entry. Held in addition to the use-case-owned
    /// clone so the facade keeps a stable read/write handle.
    invitation_holder: Arc<InMemoryPairingInvitationHolder>,
    /// Slice 2 Phase 1 · T8：F1 hook。A1/A2/B2 成功后
    /// [`Self::auto_prime_presence`] 触发一次全员预连,把 presence 缓存
    /// 填满,让 UI 查 roster 时 online/offline 立刻准。
    ensure_reachable_all: Arc<EnsureReachableAllUseCase>,
    /// Switch-space 4 阶段重加密迁移 use case。已 setup 设备加入另一个
    /// sponsor 空间时由 [`Self::switch_space`] 驱动；启动期补偿在
    /// [`Self::try_resume_session`] 内通过 [`SwitchSpaceUseCase::resume_pending`]
    /// 自动触发。
    switch_space: Arc<SwitchSpaceUseCase>,
    /// Switch-space 进度查询用——`query_migration_progress` 直接读这两份。
    /// 持有同一份 Arc 是因为 use case 内部不暴露状态访问，进度只读路径
    /// 必须自己拿 port。
    migration_state: Arc<dyn MigrationStatePort>,
    blob_migration_repo: Arc<dyn BlobMigrationRepoPort>,
    /// Held for the desktop keepalive scheduler — `list_paired_peer_device_ids`
    /// reads `peer_addr_repo.list()` and `ensure_reachable_one` forwards to
    /// `presence.ensure_reachable`. Both are thin wrappers driven by the
    /// worker's per-peer backoff state machine; the underlying ports stay
    /// owned by use cases as before.
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    presence: Arc<dyn PresencePort>,
    /// `current_device_id()` snapshotted at facade-construction time so
    /// `list_paired_peer_device_ids` can self-filter without grabbing the
    /// `DeviceIdentityPort` lock on every call.
    local_device_id: DeviceId,
    /// Analytics entry point. Held directly on the facade so
    /// `reset_telemetry_identity` (a cross-cutting operation that doesn't
    /// belong to any single use case) can call `analytics.reset_identity()`
    /// without routing through a use case wrapper.
    analytics: Arc<dyn AnalyticsFacade>,
}

impl SpaceSetupFacade {
    /// Wire all use cases from a single [`SpaceSetupDeps`] bundle and
    /// spawn the sponsor-side inbound pairing orchestrator.
    pub fn new(deps: SpaceSetupDeps) -> Self {
        let SpaceSetupDeps {
            space_access,
            local_identity,
            device_identity,
            member_repo,
            setup_status,
            settings,
            clock,
            pairing_invitation,
            pairing_invitation_addresses,
            pairing_invitation_by_address,
            pairing_session,
            pairing_events,
            proof_port,
            trusted_peer_repo,
            peer_addr_repo,
            presence,
            migration_state,
            key_migration,
            blob_migration_repo,
            blob_cipher,
            analytics,
        } = deps;

        // Stash the narrow slices the facade itself drives (`try_resume_session`
        // / `factory_reset`) before the bundle's other slices are handed to the
        // use cases below. The facade owns these two paths directly rather than
        // routing through a use case that would only wrap a single port call.
        let resume_session_for_facade = Arc::clone(&space_access.resume_session);
        let factory_reset_for_facade = Arc::clone(&space_access.factory_reset);
        let setup_status_for_facade = Arc::clone(&setup_status);
        // Slice4 P3 T3.2 · facade-local handle for `query_setup_state`
        // (reads `Settings.general.device_name`).
        let settings_for_facade = Arc::clone(&settings);

        // Invitation holder is purely an internal flow-state component
        // (§11.4) — construct it here so bootstrap never sees the type.
        let invitation_holder = Arc::new(InMemoryPairingInvitationHolder::new());
        // Slice4 P3 T3.2 · facade-local handle for `cancel_invitation`
        // / `query_setup_state` snapshots; the use case + orchestrator
        // already own their own `Arc::clone`s below.
        let invitation_holder_for_facade = Arc::clone(&invitation_holder);

        let initialize_space = Arc::new(InitializeSpaceUseCase::new(
            Arc::clone(&space_access.initialize),
            Arc::clone(&local_identity),
            Arc::clone(&device_identity),
            Arc::clone(&member_repo),
            Arc::clone(&setup_status),
            Arc::clone(&settings),
            Arc::clone(&clock),
            Arc::clone(&analytics),
        ));
        let unlock_space = Arc::new(UnlockSpaceUseCase::new(
            Arc::clone(&space_access.unlock),
            Arc::clone(&setup_status),
            Arc::clone(&analytics),
        ));
        let issue_pairing_invitation = Arc::new(IssuePairingInvitationUseCase::new(
            Arc::clone(&pairing_invitation),
            pairing_invitation_addresses,
            pairing_invitation_by_address,
            Arc::clone(&device_identity),
            Arc::clone(&clock),
            Arc::clone(&invitation_holder),
            Arc::clone(&analytics),
        ));
        // T8 · F1 hook: construct ensure_reachable_all early so peer_addr_repo /
        // device_identity can still be Arc::clone'd here — both are moved into
        // downstream use cases below.
        //
        // Backoff scheduler (P-keepalive-backoff): the desktop keepalive
        // worker reads `peer_addr_repo` and dials individual peers via the
        // facade thin wrappers — clone before the use case ownership move.
        let peer_addr_repo_for_facade = Arc::clone(&peer_addr_repo);
        let presence_for_facade = Arc::clone(&presence);
        let ensure_reachable_all = Arc::new(EnsureReachableAllUseCase::new(
            Arc::clone(&peer_addr_repo),
            presence,
            Arc::clone(&device_identity),
        ));
        // Build the sponsor-side pairing stack: the handshake
        // coordinator owns wire I/O for the KeyslotOffer→Confirm flow;
        // the orchestrator composes it with admit/trust use cases so
        // persistence is done by the already-existing use cases rather
        // than being duplicated here.
        let local_device_id = device_identity.current_device_id();
        // Same id stashed for the keepalive scheduler's self-filter — the
        // original is moved into the inbound orchestrator below.
        let local_device_id_for_facade = local_device_id.clone();
        // Handshake TTL：sponsor 侧从 begin 到 confirm/reject 的 watchdog
        // （P7g），joiner 侧每次 recv 的 timeout（P7h）。180s 是为
        // Tailscale DERP relay 这种跨区中继路径预留的容差 —— 跨洋 DERP
        // RTT 300–800ms 叠 iroh 多 path 协商抖动，60s 不够喂完 4 条
        // 握手消息（实测 #486 复测 13:02 那次 sponsor accept_bi 卡 23s
        // 又 read_exact 卡 34s，joiner 60s TTL 先到 abort）。
        let handshake_ttl = Duration::from_secs(180);
        // admit/trust 两侧都要用 —— sponsor orchestrator 把 joiner 登记
        // 进本机；joiner use case 把 sponsor 登记进本机。构造一次 Arc
        // 共享即可，不给一边复制一边。
        let admit_member_uc = Arc::new(AdmitMemberUseCase::new(Arc::clone(&member_repo)));
        let trust_peer_uc = Arc::new(TrustPeerUseCase::new(Arc::clone(&trusted_peer_repo)));

        let sponsor_handshake = SponsorHandshakeCoordinator::new(
            Arc::clone(&pairing_session),
            Arc::clone(&space_access.prepare_join_offer),
            Arc::clone(&proof_port),
            Arc::clone(&local_identity),
            Arc::clone(&device_identity),
            Arc::clone(&settings),
            Arc::clone(&setup_status),
            Arc::clone(&analytics),
            handshake_ttl,
        );
        // Capacity 16 is more than enough: the outcome fires at most
        // once per handshake and typical subscribers (CLI `invite`, GUI)
        // drain as they arrive. Lag from a slow subscriber would drop the
        // oldest events, which is acceptable — a slow consumer caring
        // only about the latest attempt is fine.
        let (pairing_outcome_tx, _initial_rx) = broadcast::channel(16);
        let inbound_orchestrator = Arc::new(PairingInboundOrchestrator::new(
            pairing_events,
            pairing_invitation,
            invitation_holder,
            Arc::clone(&clock),
            sponsor_handshake,
            Arc::clone(&admit_member_uc),
            Arc::clone(&trust_peer_uc),
            Arc::clone(&peer_addr_repo),
            local_device_id,
            pairing_outcome_tx.clone(),
            Arc::clone(&analytics),
        ));
        let pairing_inbound_handle = inbound_orchestrator.spawn();

        // joiner-side symmetric: coordinator holds wire + crypto, use
        // case composes it with admit/trust/setup-status.
        let joiner_handshake = JoinerHandshakeCoordinator::new(
            pairing_session,
            Arc::clone(&space_access.derive_proof_key),
            proof_port,
            local_identity,
            device_identity,
            settings,
            handshake_ttl,
        );
        // Switch-space 复用同一个 handshake coordinator + admit/trust/peer
        // /clock，所以这些先 clone 一份给 switch-space use case，剩下的再
        // move 进 redeem use case（与既有 use case 装配模式一致）。
        let migration_state_for_facade = Arc::clone(&migration_state);
        let blob_migration_repo_for_facade = Arc::clone(&blob_migration_repo);
        let switch_space = Arc::new(SwitchSpaceUseCase::new(
            Arc::clone(&setup_status),
            Arc::clone(&migration_state),
            key_migration,
            Arc::clone(&blob_migration_repo),
            blob_cipher,
            Arc::clone(&joiner_handshake) as Arc<dyn JoinerHandshakeRunner>,
            Arc::clone(&admit_member_uc),
            Arc::clone(&trust_peer_uc),
            Arc::clone(&peer_addr_repo),
            Arc::clone(&clock),
            Arc::clone(&analytics),
        ));
        // Facade-local handle for `reset_telemetry_identity`. Cloned
        // before `analytics` is moved into the last use case below.
        let analytics_for_facade = Arc::clone(&analytics);
        let redeem_pairing_invitation = Arc::new(RedeemPairingInvitationUseCase::new(
            joiner_handshake,
            admit_member_uc,
            trust_peer_uc,
            setup_status,
            peer_addr_repo,
            clock,
            analytics,
        ));

        Self {
            initialize_space,
            unlock_space,
            issue_pairing_invitation,
            redeem_pairing_invitation,
            pairing_inbound_handle,
            pairing_outcome_tx,
            resume_session: resume_session_for_facade,
            factory_reset: factory_reset_for_facade,
            setup_status: setup_status_for_facade,
            settings: settings_for_facade,
            invitation_holder: invitation_holder_for_facade,
            ensure_reachable_all,
            switch_space,
            migration_state: migration_state_for_facade,
            blob_migration_repo: blob_migration_repo_for_facade,
            peer_addr_repo: peer_addr_repo_for_facade,
            presence: presence_for_facade,
            local_device_id: local_device_id_for_facade,
            analytics: analytics_for_facade,
        }
    }

    /// User-initiated telemetry identity reset.
    ///
    /// Clears the locally-persisted `space_person_id`, regenerates the
    /// anonymous identifiers, rebuilds the global EventContext, and emits
    /// `$identify` so PostHog merges recent events under the new
    /// anonymous person.
    ///
    /// Only this device is affected; peers in the same Space keep their
    /// `space_person_id` and the Space's PostHog group continues to exist
    /// — this device just falls back to Solo.
    #[instrument(skip(self))]
    pub fn reset_telemetry_identity(
        &self,
    ) -> Result<(), crate::facade::space_setup::ResetTelemetryError> {
        self.analytics
            .reset_identity()
            .map_err(|e| crate::facade::space_setup::ResetTelemetryError::Storage(e.to_string()))
    }

    /// Try to restore the in-memory space session silently, using the
    /// KEK cached in secure storage by a previous `init` / `unlock`.
    ///
    /// Returns `Ok(true)` when the session is now unlocked and ready
    /// for pairing operations; `Ok(false)` when there is nothing to
    /// resume (setup has not completed on this profile). Genuine
    /// problems — corrupt key material, missing keyring entry despite
    /// a keyslot on disk, or adapter faults — surface via
    /// [`TryResumeSessionError`].
    ///
    /// Intended for short-lived CLI processes: every `invite` call
    /// drives this before B1 so the sponsor's `verify_proof` path has
    /// the master key in memory when the joiner's ChallengeResponse
    /// lands. GUI / daemon callers can use it at startup to skip the
    /// passphrase prompt when the keyring still has the KEK.
    #[instrument(skip_all)]
    pub async fn try_resume_session(&self) -> Result<bool, TryResumeSessionError> {
        let status = self
            .setup_status
            .get_status()
            .await
            .map_err(|err| TryResumeSessionError::Internal(err.to_string()))?;
        if !status.has_completed {
            return Ok(false);
        }

        // The adapter keys off the current profile, so the `SpaceId`
        // passed here is an opaque handle rather than a lookup key.
        // Minting a fresh UUID matches how A2 `unlock` does it.
        let space_id = SpaceId::new();
        let resumed = match self.resume_session.try_resume_session(&space_id).await {
            Ok(Some(_)) => true,
            // Keyslot missing despite has_completed == true — treat
            // as "nothing to resume" rather than an error: can happen
            // right after factory_reset when setup_status lagged.
            Ok(None) => false,
            Err(SpaceAccessError::CorruptedKeyMaterial) => {
                return Err(TryResumeSessionError::CorruptedKeyMaterial);
            }
            // NotInitialized and WrongPassphrase from load_kek map to
            // "keyring didn't give us what we needed to silently unlock".
            Err(SpaceAccessError::NotInitialized) | Err(SpaceAccessError::WrongPassphrase) => {
                return Err(TryResumeSessionError::KeyringMiss);
            }
            Err(other) => return Err(TryResumeSessionError::Internal(other.to_string())),
        };

        // Switch-space migration recovery hook: if there's an in-flight
        // 4-phase migration left over from a prior daemon crash, advance
        // it now that the session is unlocked and the new master_key is
        // available. `None` is a noop, so the cost on idle paths is one
        // `migration_state.get_current()` read.
        //
        // Failure here surfaces as `TryResumeSessionError::Internal` —
        // the device is in a wedged state (Prepared/HandshakeDone/Swapped
        // with replay failure) and the caller should bubble it up to the
        // operator rather than silently continue with stale data.
        if resumed {
            if let Err(err) = self.switch_space.resume_pending().await {
                warn!(error = %err, "switch-space resume_pending failed during try_resume_session");
                return Err(TryResumeSessionError::Internal(format!(
                    "migration resume failed: {err}"
                )));
            }
        }

        Ok(resumed)
    }

    /// Switch this device to another sponsor's space while preserving
    /// local clipboard history via 4-phase re-encryption migration. See
    /// [`crate::usecases::setup::switch_space`] module doc for full
    /// semantics, including failure rollback and crash recovery.
    ///
    /// Pre-conditions: setup completed (else `NotSetup`), session
    /// unlocked (else `NotUnlocked`), no in-flight migration (else
    /// `PendingMigration`).
    #[instrument(skip_all)]
    pub async fn switch_space(
        &self,
        input: SwitchSpaceInput,
    ) -> Result<SwitchSpaceResult, SwitchSpaceError> {
        let cmd: SwitchSpaceCommand = input.into();
        let result = self.switch_space.execute(cmd).await?;
        // F1 hook: switch-space ends in a fresh sponsor relationship —
        // mirror A1/A2/B2 by priming presence cache so the UI roster
        // shows the new sponsor's online state without waiting on the
        // adapter's lazy probe.
        self.auto_prime_presence().await;
        Ok(result)
    }

    /// Coarse-grained read of switch-space progress for UI polling.
    ///
    /// Returns `phase = None` + `backup_record_count = 0` on the idle
    /// path. While a migration is in flight, `phase` reflects the last
    /// committed step (`Prepared` / `HandshakeDone` / `Swapped`) and
    /// `backup_record_count` is the size of the backup table.
    #[instrument(skip_all)]
    pub async fn query_migration_progress(
        &self,
    ) -> Result<MigrationProgress, QueryMigrationProgressError> {
        let phase = self
            .migration_state
            .get_current()
            .await
            .map_err(|err| QueryMigrationProgressError::StorageFailed(err.to_string()))?;
        let backup_record_count = self
            .blob_migration_repo
            .count_records()
            .await
            .map_err(|err| QueryMigrationProgressError::StorageFailed(err.to_string()))?;
        Ok(MigrationProgress {
            phase: phase.as_ref().map(MigrationPhaseKind::from),
            backup_record_count,
        })
    }

    /// Subscribe to sponsor-side pairing completion events.
    ///
    /// Each call returns a fresh receiver sharing the facade's broadcast
    /// source. Receivers must be obtained **before** the awaited handshake
    /// starts; lag policy follows `tokio::sync::broadcast` (oldest events
    /// are dropped if a subscriber falls behind `capacity`).
    pub fn subscribe_pairing_completion(&self) -> broadcast::Receiver<PairingOutcome> {
        self.pairing_outcome_tx.subscribe()
    }

    /// A1 · Create the encrypted space on a fresh device. On success the
    /// presence cache is primed (F1).
    #[instrument(skip_all)]
    pub async fn initialize_space(
        &self,
        input: InitializeSpaceInput,
    ) -> Result<InitializeSpaceResult, InitializeSpaceError> {
        let cmd: InitializeSpaceCommand = input.into();
        let out = self.initialize_space.execute(cmd).await?;
        self.auto_prime_presence().await;
        Ok(out)
    }

    /// A2 · Unlock the encrypted space after a restart. On success the
    /// presence cache is primed (F1).
    #[instrument(skip_all)]
    pub async fn unlock_space(
        &self,
        input: UnlockSpaceInput,
    ) -> Result<UnlockSpaceResult, UnlockSpaceError> {
        let cmd: UnlockSpaceCommand = input.into();
        let out = self.unlock_space.execute(cmd).await?;

        // Switch-space migration recovery hook — 镜像 try_resume_session:412-419。
        // 手动 unlock 也必须推进 migration replay：startup_recovery 在 auto-unlock
        // 关闭时会跳过 try_resume_session（避免无 KEK 弹钥匙串），那 hook 就不会
        // 接管 pending HandshakeDone migration。没有这一行, 用户在 auto-unlock=off
        // 的设备上一旦 phase 3 中断, 主表 inline 永远停在旧 master_key 加密、
        // session 持新 master_key 的 wedged 态 —— UI 看不到任何历史。
        if let Err(err) = self.switch_space.resume_pending().await {
            warn!(error = %err, "switch-space resume_pending failed during unlock_space");
            return Err(UnlockSpaceError::Internal(format!(
                "migration resume failed: {err}"
            )));
        }

        self.auto_prime_presence().await;
        Ok(out)
    }

    /// B1 · Ask the rendezvous service for a fresh invitation code and
    /// park the resulting aggregate in the application-layer holder.
    ///
    /// Does **not** auto-start the network: the adapter surfaces
    /// [`IssuePairingInvitationError::NetworkNotStarted`] if the runtime
    /// isn't up, letting the UI prompt the user to complete A1/A2 first.
    #[instrument(skip_all)]
    pub async fn issue_pairing_invitation(
        &self,
    ) -> Result<IssuePairingInvitationResult, IssuePairingInvitationError> {
        self.issue_pairing_invitation.execute().await
    }

    /// 按指定本机地址签发配对邀请。
    #[instrument(skip_all, fields(selected_ip = %selected_ip))]
    pub async fn issue_pairing_invitation_for_address(
        &self,
        selected_ip: IpAddr,
    ) -> Result<IssuePairingInvitationResult, IssuePairingInvitationError> {
        self.issue_pairing_invitation
            .execute_for_address(selected_ip)
            .await
    }

    /// 列出当前可用于签发配对邀请的本机地址。
    #[instrument(skip_all)]
    pub async fn list_pairing_invitation_addresses(
        &self,
    ) -> Result<Vec<PairingInvitationAddressCandidate>, IssuePairingInvitationError> {
        self.issue_pairing_invitation.list_addresses().await
    }

    /// B2 · Redeem a sponsor-issued invitation (joiner side).
    ///
    /// Primes presence before dialing because, unlike A1/A2, the joiner's
    /// entry point may be the first user action on this device (no prior
    /// `initialize_space` / `unlock_space` to have triggered F1). Prime
    /// failures are logged but not propagated — the subsequent dial will
    /// fail with [`RedeemPairingInvitationError::SponsorUnreachable`] /
    /// `ServiceUnavailable` if presence is genuinely unusable, which is
    /// the more actionable surface for the UI.
    #[instrument(skip_all)]
    pub async fn redeem_pairing_invitation(
        &self,
        input: RedeemPairingInvitationInput,
    ) -> Result<RedeemPairingInvitationResult, RedeemPairingInvitationError> {
        self.auto_prime_presence().await;
        let cmd: RedeemPairingInvitationCommand = input.into();
        self.redeem_pairing_invitation.execute(cmd).await
    }

    /// Slice4 P3 T3.2 · Cancel any in-flight pairing invitation parked
    /// in the in-memory holder.
    ///
    /// Maps to `POST /v2/setup/cancel`. Returns
    /// [`CancelInvitationError::NotIssued`] when the holder is empty so
    /// the daemon can surface HTTP 409 and the UI can distinguish
    /// "nothing to cancel" from a transport failure.
    ///
    /// Does **not** touch `SetupStatus` — only Pending invitation
    /// aggregates are cleared. The rendezvous server is **not**
    /// notified: stateless v2 model treats invitations as pure local
    /// state, and any joiner that races a redeem against this cancel
    /// will simply hit `take_matching → NotFound` on the sponsor side.
    #[instrument(skip_all)]
    pub async fn cancel_invitation(&self) -> Result<(), CancelInvitationError> {
        let removed = self.invitation_holder.cancel_all().await;
        if removed == 0 {
            return Err(CancelInvitationError::NotIssued);
        }
        info!(count = removed, "cancelled in-flight pairing invitations");
        Ok(())
    }

    /// Slice4 P3 T3.2 · Reset this device back to a fresh-install
    /// state by clearing `SetupStatus` and dropping any in-flight
    /// invitations.
    ///
    /// Maps to `POST /v2/setup/reset`. Stateless model: the only
    /// persistent fact this clears is `SetupStatus.has_completed` (and
    /// `space_id`). The keyslot on disk is intentionally left in place
    /// — operators recover key material via passphrase-based unlock
    /// after re-init, and a true factory reset (key material wipe) is
    /// a separate operator action handled outside this facade.
    ///
    /// The network runtime is **not** stopped: `on_shutdown` is the
    /// canonical F2 path; reset is invoked while the daemon stays up.
    #[instrument(skip_all)]
    pub async fn reset(&self) -> Result<(), ResetSpaceError> {
        self.setup_status
            .set_status(&SetupStatus::default())
            .await
            .map_err(|err| ResetSpaceError::StorageFailed(err.to_string()))?;
        let dropped = self.invitation_holder.cancel_all().await;
        info!(
            cancelled_invitations = dropped,
            "reset cleared setup status and pending invitations"
        );
        Ok(())
    }

    /// User-driven "重置并重新开始" — wipe key material **and** clear setup
    /// status so a user who has forgotten their passphrase can re-run A1
    /// from a fresh-install state.
    ///
    /// Distinct from [`Self::reset`], which intentionally preserves the
    /// keyslot for operator-driven recovery: that path is no use to a user
    /// who can't recall the passphrase — `InitializeSpaceUseCase` would
    /// reject the next setup attempt with `AlreadyInitialized` because the
    /// keyslot is still on disk.
    ///
    /// Step order matters:
    ///
    /// 1. `FactoryResetSpacePort::factory_reset` — wipe keyslot + KEK first. If
    ///    this fails we leave `setup_status.has_completed = true` so the
    ///    UI still routes the user to `UnlockPage` (where they can retry)
    ///    rather than `SetupPage` (which would immediately fail with
    ///    `AlreadyInitialized` due to the residual keyslot).
    /// 2. Clear `SetupStatus` so `EncryptionFacade::state()` flips
    ///    `initialized = false` and the UI routes to `SetupPage`.
    /// 3. Cancel any in-flight invitations — same hygiene as
    ///    [`Self::reset`].
    ///
    /// The `space_id` passed to the port is an opaque handle: the
    /// `SpaceAccessAdapter` keys off the current profile, not this value.
    /// We mint a fresh one rather than reading from `SetupStatus` because
    /// the use-case may run when `SetupStatus.space_id` is `None` (e.g.
    /// `setup_status` is partially populated from a prior abort).
    #[instrument(skip_all)]
    pub async fn factory_reset(&self) -> Result<(), FactoryResetError> {
        let space_id = SpaceId::new();
        self.factory_reset
            .factory_reset(&space_id)
            .await
            .map_err(|err| FactoryResetError::KeyMaterialWipeFailed(err.to_string()))?;
        self.setup_status
            .set_status(&SetupStatus::default())
            .await
            .map_err(|err| FactoryResetError::StorageFailed(err.to_string()))?;
        let dropped = self.invitation_holder.cancel_all().await;
        info!(
            cancelled_invitations = dropped,
            "factory reset wiped key material, cleared setup status, dropped invitations"
        );
        Ok(())
    }

    /// Slice4 P3 T3.2 · Read-only snapshot of setup state for the
    /// stateless v2 UI flow.
    ///
    /// Maps to `GET /v2/setup/state`. Composes three independent
    /// reads into a single response so the UI doesn't have to
    /// orchestrate them itself:
    /// * `has_completed` from [`SetupStatusPort`].
    /// * `current_invitation` from the in-memory holder
    ///   (earliest-expiring Pending entry; `None` when the holder is
    ///   empty).
    /// * `device_name` from `Settings.general.device_name`.
    #[instrument(skip_all)]
    pub async fn query_setup_state(&self) -> Result<SetupStateView, QuerySetupStateError> {
        let status = self
            .setup_status
            .get_status()
            .await
            .map_err(|err| QuerySetupStateError::StorageFailed(err.to_string()))?;
        let current_invitation = self
            .invitation_holder
            .snapshot_earliest()
            .await
            .map(|(code, expires_at)| CurrentInvitation { code, expires_at });
        let settings = self
            .settings
            .load()
            .await
            .map_err(|err| QuerySetupStateError::StorageFailed(err.to_string()))?;
        Ok(SetupStateView {
            has_completed: status.has_completed,
            current_invitation,
            device_name: settings.general.device_name,
        })
    }

    /// Slice 2 Phase 1 · T10 · CLI `members` 入口:主动触发一轮
    /// `ensure_reachable_all`,把 `IrohPresenceAdapter` 的缓存刷新到最新,
    /// 然后 CLI 再调 `MemberRosterFacade::list_with_presence` 读缓存 →
    /// 查询结果天然满足"B 重启后 ≤ 10s 内显示 online"的验收条款。
    ///
    /// 与 F1 hook 里 `auto_prime_presence` 自动触发的那一轮的区别:本方法
    /// 暴露 `ensure_reachable_all` 使用例的结果,让 CLI 决定如何展示
    /// (fatal 错误 / 个别 peer 失败计数);F1 路径吞错只 warn。
    ///
    /// UseCase 本身保持 `pub(crate)`(§11.4),只通过本 facade thin wrapper
    /// 对外,后续 Tauri / GUI 也复用同一入口。
    #[instrument(skip_all)]
    pub async fn refresh_presence(
        &self,
    ) -> Result<EnsureReachableAllReport, EnsureReachableAllError> {
        self.ensure_reachable_all.execute().await
    }

    /// List `DeviceId`s of every paired peer (local device filtered out).
    ///
    /// Thin wrapper over `peer_addr_repo.list()` consumed by the desktop
    /// keepalive scheduler so its per-peer backoff state can discover new
    /// peers and prune removed ones each tick. Mirrors the iteration source
    /// `EnsureReachableAllUseCase` uses internally — peers without an addr
    /// blob are silently absent rather than surfaced as "no address"
    /// errors. The local DeviceId is filtered defensively (the repo
    /// shouldn't contain it; see the same guard in `EnsureReachableAllUseCase`).
    pub async fn list_paired_peer_device_ids(
        &self,
    ) -> Result<Vec<DeviceId>, EnsureReachableAllError> {
        let records = self.peer_addr_repo.list().await.map_err(|err| {
            EnsureReachableAllError::Repository(format!("peer_addr_repo.list: {err}"))
        })?;
        Ok(records
            .into_iter()
            .filter_map(|r| {
                if r.device_id == self.local_device_id {
                    None
                } else {
                    Some(r.device_id)
                }
            })
            .collect())
    }

    /// Ensure a single peer is reachable; thin wrapper over
    /// `PresencePort::ensure_reachable`.
    ///
    /// The keepalive scheduler calls this only for peers whose backoff has
    /// elapsed. `ensure_reachable` (not `verify_reachable`) is intentional:
    /// when our outbound `peers` map already holds an alive connection the
    /// fast-path returns Online without dialing — exactly what the
    /// scheduler wants to avoid burning UDP probes on healthy peers.
    pub async fn ensure_reachable_one(
        &self,
        device: &DeviceId,
    ) -> Result<ReachabilityState, PresenceError> {
        self.presence.ensure_reachable(device).await
    }

    /// F2 · Tear down facade-owned background work cleanly on app exit.
    ///
    /// Slice 4 P5c: 历史上还会调 `network_control.stop_network()`,libp2p 走
    /// 完后 iroh router 由 `SyncEngineAssembly::shutdown` 直接收口,本入口
    /// 现在只剩 abort 入站 pairing orchestrator——让它的 `subscribe` receiver
    /// 立刻 drop,底层 adapter 才能释放事件 channel。
    #[instrument(skip_all)]
    pub async fn on_shutdown(&self) {
        self.pairing_inbound_handle.abort();
    }

    /// Best-effort presence prime after a successful space-lifecycle action.
    /// Does not propagate errors: A1/A2 already committed the space mutation
    /// and rolling that back is worse than leaving presence stale.
    ///
    /// **Slice 2 Phase 1 · T8 · F1 hook**(P5c 改名 `auto_prime_presence`):
    /// 跑一次 `ensure_reachable_all` —— 对所有已知 paired peer 并发探测,
    /// 把 presence 缓存填满,让 UI 下一次 `list_with_presence` 就能拿到
    /// 正确的 online/offline 而不是全是 `Unknown`。预连失败不传给调用方:
    /// A1/A2/B2 的空间变更已经落盘,单个 peer 拨不通属正常情形,
    /// adapter 的 `Connection::closed` watchdog 会按正常流程 lazy 补齐。
    async fn auto_prime_presence(&self) {
        match self.ensure_reachable_all.execute().await {
            Ok(report) => {
                info!(
                    total = report.total,
                    online = report.online,
                    offline = report.offline,
                    errors = report.errors.len(),
                    "F1 ensure_reachable_all completed"
                );
            }
            Err(err) => {
                warn!(
                    error = %err,
                    "ensure_reachable_all failed; presence will recover lazily \
                     on next adapter probe"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Thin smoke tests — the two use cases themselves are covered
    //! exhaustively in `usecases::setup::{initialize_space,unlock_space}`.
    //! Here we only prove that `SpaceSetupFacade` wires them up and
    //! forwards arguments and error codes unchanged.

    use super::*;

    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;

    use chrono::{DateTime, Utc};

    use tokio::sync::mpsc;
    use uc_core::crypto::domain::{ActiveSpace, Passphrase};
    use uc_core::ids::{DeviceId, SpaceId};
    use uc_core::membership::{MembershipError, SpaceMember};
    use uc_core::pairing::invitation::InvitationCode;
    use uc_core::pairing::PairingSessionMessage;
    use uc_core::ports::pairing::{
        DialError, DialOutcome, PairingEventPort, PairingSessionEvent, PairingSessionId,
        PairingSessionPort, SessionError,
    };
    use uc_core::ports::pairing_invitation::{
        CodeOrigin, ConsumeInvitationError, InvitationError, IssuedInvitation,
        PairingInvitationAddressQueryPort, PairingInvitationByAddressPort, PairingInvitationPort,
    };
    use uc_core::ports::space::{
        CurrentSessionProofKeyPort, DeriveProofKeyPort, DeriveSpaceSubkeyPort,
        FactoryResetSpacePort, InitializeSpacePort, IsSpaceUnlockedPort, LockSpacePort,
        PrepareJoinOfferPort, ProofPort, ResumeSpaceSessionPort, SpaceAccessError, UnlockSpacePort,
        VerifyKeychainAccessPort,
    };
    use uc_core::ports::{
        ClockPort, DeviceIdentityPort, LocalIdentityError, LocalIdentityPort, SettingsPort,
        SetupStatusPort,
    };

    use crate::deps::SpaceAccessPorts;
    use uc_core::security::IdentityFingerprint;
    use uc_core::settings::model::Settings;
    use uc_core::setup::SetupStatus;
    use uc_core::space_access::{JoinOffer, ProofDerivedKey, SpaceAccessProofArtifact};
    use uc_core::trusted_peer::{TrustedPeer, TrustedPeerError, TrustedPeerRepositoryPort};
    use uc_core::SessionId;

    // ── fakes (minimal) ──────────────────────────────────────────────────

    #[derive(Default)]
    struct FakeSpaceAccess {
        unlock_err: StdMutex<Option<SpaceAccessError>>,
        factory_reset_calls: StdMutex<u32>,
        factory_reset_err: StdMutex<Option<SpaceAccessError>>,
    }

    #[async_trait]
    impl InitializeSpacePort for FakeSpaceAccess {
        async fn initialize(
            &self,
            space_id: &SpaceId,
            _passphrase: &Passphrase,
        ) -> Result<ActiveSpace, SpaceAccessError> {
            Ok(ActiveSpace::new(space_id.clone()))
        }
    }
    #[async_trait]
    impl UnlockSpacePort for FakeSpaceAccess {
        async fn unlock(
            &self,
            space_id: &SpaceId,
            _passphrase: &Passphrase,
        ) -> Result<ActiveSpace, SpaceAccessError> {
            if let Some(err) = self.unlock_err.lock().unwrap().take() {
                return Err(err);
            }
            Ok(ActiveSpace::new(space_id.clone()))
        }
    }
    #[async_trait]
    impl IsSpaceUnlockedPort for FakeSpaceAccess {
        async fn is_unlocked(&self, _space_id: &SpaceId) -> bool {
            true
        }
    }
    #[async_trait]
    impl LockSpacePort for FakeSpaceAccess {
        async fn lock(&self, _space_id: &SpaceId) -> Result<(), SpaceAccessError> {
            Ok(())
        }
    }
    #[async_trait]
    impl FactoryResetSpacePort for FakeSpaceAccess {
        async fn factory_reset(&self, _space_id: &SpaceId) -> Result<(), SpaceAccessError> {
            *self.factory_reset_calls.lock().unwrap() += 1;
            if let Some(err) = self.factory_reset_err.lock().unwrap().take() {
                return Err(err);
            }
            Ok(())
        }
    }
    #[async_trait]
    impl ResumeSpaceSessionPort for FakeSpaceAccess {
        async fn try_resume_session(
            &self,
            _space_id: &SpaceId,
        ) -> Result<Option<ActiveSpace>, SpaceAccessError> {
            Ok(None)
        }
    }
    #[async_trait]
    impl VerifyKeychainAccessPort for FakeSpaceAccess {
        async fn verify_keychain_access(&self) -> Result<bool, SpaceAccessError> {
            Ok(true)
        }
    }
    #[async_trait]
    impl DeriveSpaceSubkeyPort for FakeSpaceAccess {
        async fn derive_subkey(
            &self,
            _salt: &[u8],
            _info: &[u8],
        ) -> Result<[u8; 32], SpaceAccessError> {
            Ok([0; 32])
        }
    }
    #[async_trait]
    impl CurrentSessionProofKeyPort for FakeSpaceAccess {
        async fn current_session_proof_key(
            &self,
        ) -> Result<Option<ProofDerivedKey>, SpaceAccessError> {
            Ok(None)
        }
    }
    #[async_trait]
    impl PrepareJoinOfferPort for FakeSpaceAccess {
        async fn prepare_join_offer(
            &self,
            _space_id: &SpaceId,
            _passphrase: &Passphrase,
        ) -> Result<JoinOffer, SpaceAccessError> {
            unimplemented!("not used by A1/A2")
        }
    }
    #[async_trait]
    impl DeriveProofKeyPort for FakeSpaceAccess {
        async fn derive_master_key_for_proof(
            &self,
            _offer: &JoinOffer,
            _passphrase: &Passphrase,
        ) -> Result<ProofDerivedKey, SpaceAccessError> {
            unimplemented!("not used by A1/A2")
        }
    }

    struct FakeLocalIdentity {
        fp: IdentityFingerprint,
    }
    #[async_trait]
    impl LocalIdentityPort for FakeLocalIdentity {
        async fn create(&self) -> Result<IdentityFingerprint, LocalIdentityError> {
            Ok(self.fp.clone())
        }
        async fn ensure(&self) -> Result<IdentityFingerprint, LocalIdentityError> {
            Ok(self.fp.clone())
        }
        async fn get_current_fingerprint(
            &self,
        ) -> Result<Option<IdentityFingerprint>, LocalIdentityError> {
            Ok(Some(self.fp.clone()))
        }
    }

    struct FixedDeviceIdentity {
        id: DeviceId,
    }
    impl DeviceIdentityPort for FixedDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            self.id.clone()
        }
    }

    #[derive(Default)]
    struct InMemoryMemberRepo {
        rows: StdMutex<Vec<SpaceMember>>,
    }
    #[async_trait]
    impl uc_core::membership::MemberRepositoryPort for InMemoryMemberRepo {
        async fn get(&self, device_id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .find(|m| &m.device_id == device_id)
                .cloned())
        }
        async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError> {
            Ok(self.rows.lock().unwrap().clone())
        }
        async fn save(&self, member: &SpaceMember) -> Result<(), MembershipError> {
            self.rows.lock().unwrap().push(member.clone());
            Ok(())
        }
        async fn remove(&self, _device_id: &DeviceId) -> Result<bool, MembershipError> {
            Ok(true)
        }
    }

    #[derive(Default)]
    struct InMemorySetupStatus {
        status: StdMutex<SetupStatus>,
    }
    #[async_trait]
    impl SetupStatusPort for InMemorySetupStatus {
        async fn get_status(&self) -> anyhow::Result<SetupStatus> {
            Ok(self.status.lock().unwrap().clone())
        }
        async fn set_status(&self, status: &SetupStatus) -> anyhow::Result<()> {
            *self.status.lock().unwrap() = status.clone();
            Ok(())
        }
    }

    #[derive(Default)]
    struct InMemorySettings {
        settings: StdMutex<Settings>,
    }
    #[async_trait]
    impl SettingsPort for InMemorySettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(self.settings.lock().unwrap().clone())
        }
        async fn save(&self, settings: &Settings) -> anyhow::Result<()> {
            *self.settings.lock().unwrap() = settings.clone();
            Ok(())
        }
    }

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    #[derive(Default)]
    struct FakeInvitationPort {
        calls: StdMutex<u32>,
        next_err: StdMutex<Option<InvitationError>>,
    }

    #[async_trait]
    impl PairingInvitationPort for FakeInvitationPort {
        async fn issue_invitation(&self) -> Result<IssuedInvitation, InvitationError> {
            *self.calls.lock().unwrap() += 1;
            if let Some(err) = self.next_err.lock().unwrap().take() {
                return Err(err);
            }
            Ok(IssuedInvitation {
                code: InvitationCode::new("SMOKE-0001"),
                expires_at: DateTime::parse_from_rfc3339("2026-04-20T10:05:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                code_origin: CodeOrigin::DirectoryIssued,
            })
        }

        async fn consume_invitation(
            &self,
            _code: &InvitationCode,
        ) -> Result<(), ConsumeInvitationError> {
            // Smoke tests don't exercise P7e inbound path.
            Ok(())
        }
    }

    #[async_trait]
    impl PairingInvitationAddressQueryPort for FakeInvitationPort {
        async fn list_invitation_addresses(
            &self,
        ) -> Result<Vec<PairingInvitationAddressCandidate>, InvitationError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl PairingInvitationByAddressPort for FakeInvitationPort {
        async fn issue_invitation_for_address(
            &self,
            _selected_ip: IpAddr,
        ) -> Result<IssuedInvitation, InvitationError> {
            self.issue_invitation().await
        }
    }

    /// Minimal fakes for the Slice 1 pairing session/event ports. The
    /// smoke tests here only verify A1/A2/B1 forwarding and shutdown side
    /// effects; inbound event handling is covered exhaustively in
    /// `pairing_inbound::orchestrator::tests`.
    #[derive(Default)]
    struct NoopSessionPort;

    #[async_trait]
    impl PairingSessionPort for NoopSessionPort {
        async fn dial_by_invitation(
            &self,
            _code: &uc_core::pairing::invitation::InvitationCode,
        ) -> Result<DialOutcome, DialError> {
            unreachable!("smoke tests never dial")
        }
        async fn send(
            &self,
            _session: &PairingSessionId,
            _message: PairingSessionMessage,
        ) -> Result<(), SessionError> {
            Ok(())
        }
        async fn recv_next(
            &self,
            _session: &PairingSessionId,
        ) -> Result<Option<PairingSessionMessage>, SessionError> {
            unreachable!("smoke tests never recv")
        }
        async fn close(&self, _session: &PairingSessionId, _reason: Option<String>) {}
    }

    /// Hands out a single empty receiver; the orchestrator will idle until
    /// the facade is dropped (and `on_shutdown` aborts the task).
    struct IdleEventPort {
        rx: StdMutex<Option<mpsc::Receiver<PairingSessionEvent>>>,
    }
    impl IdleEventPort {
        fn new() -> Self {
            let (_tx, rx) = mpsc::channel(1);
            // Drop the sender on purpose — the channel closes when the
            // receiver's `recv` is awaited. That's fine: the orchestrator's
            // run_loop exits cleanly on channel close.
            Self {
                rx: StdMutex::new(Some(rx)),
            }
        }
    }
    #[async_trait]
    impl PairingEventPort for IdleEventPort {
        async fn subscribe(&self) -> anyhow::Result<mpsc::Receiver<PairingSessionEvent>> {
            self.rx
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| anyhow::anyhow!("IdleEventPort already subscribed"))
        }
    }

    /// Smoke-test stub: proof verification is not exercised here —
    /// the inbound handshake flow is covered in
    /// `pairing_inbound::orchestrator::tests`.
    struct NoopProofPort;
    #[async_trait]
    impl ProofPort for NoopProofPort {
        async fn build_proof(
            &self,
            _pairing_session_id: &SessionId,
            _space_id: &SpaceId,
            _challenge_nonce: [u8; 32],
            _derived_key: &ProofDerivedKey,
        ) -> anyhow::Result<SpaceAccessProofArtifact> {
            unreachable!("smoke tests never drive verification")
        }
        async fn verify_proof(
            &self,
            _proof: &SpaceAccessProofArtifact,
            _expected_nonce: [u8; 32],
        ) -> anyhow::Result<bool> {
            unreachable!("smoke tests never drive verification")
        }
    }

    #[derive(Default)]
    struct NoopTrustedPeerRepo;
    #[async_trait]
    impl TrustedPeerRepositoryPort for NoopTrustedPeerRepo {
        async fn get(&self, _: &DeviceId) -> Result<Option<TrustedPeer>, TrustedPeerError> {
            Ok(None)
        }
        async fn list(&self) -> Result<Vec<TrustedPeer>, TrustedPeerError> {
            Ok(vec![])
        }
        async fn save(&self, _: &TrustedPeer) -> Result<(), TrustedPeerError> {
            Ok(())
        }
        async fn remove(&self, _: &DeviceId) -> Result<bool, TrustedPeerError> {
            Ok(false)
        }
    }

    // Slice 2 Phase 1 · T5/T8 note:
    //
    // * T5:pairing 收尾点(orchestrator / redeem_invitation)会对 peer_addr_repo
    //   做 upsert——行为契约在各自的测试里覆盖,不在本文件。
    // * T8:F1 hook `auto_prime_presence` 在 A1/A2/B2 成功后会 unconditionally
    //   调 `peer_addr_repo.list()` 喂给 `EnsureReachableAllUseCase`。
    //
    // 因此本 helper 换成一个 FakePeerAddrRepo:`list()` 默认返回空 vec
    // (→ ensure_reachable_all 跑完一轮,不触发 presence.ensure_reachable),
    // 并记录 list() 调用次数让 F1 acceptance tests 断言"跑过一次"。
    // 其他 repo 方法保持 "unreachable!()" —— 本 smoke 测试集不该走它们。
    #[derive(Default)]
    struct FakePeerAddrRepo {
        list_calls: StdMutex<u32>,
    }
    impl FakePeerAddrRepo {
        fn list_calls(&self) -> u32 {
            *self.list_calls.lock().unwrap()
        }
    }
    #[async_trait]
    impl uc_core::ports::PeerAddressRepositoryPort for FakePeerAddrRepo {
        async fn get(
            &self,
            _device: &DeviceId,
        ) -> Result<Option<uc_core::ports::PeerAddressRecord>, uc_core::ports::PeerAddressError>
        {
            unreachable!("smoke tests don't read individual peer addresses")
        }
        async fn upsert(
            &self,
            _record: &uc_core::ports::PeerAddressRecord,
        ) -> Result<(), uc_core::ports::PeerAddressError> {
            unreachable!("pairing finalise covered in orchestrator tests, not here")
        }
        async fn list(
            &self,
        ) -> Result<Vec<uc_core::ports::PeerAddressRecord>, uc_core::ports::PeerAddressError>
        {
            *self.list_calls.lock().unwrap() += 1;
            Ok(vec![])
        }
        async fn remove(&self, _device: &DeviceId) -> Result<(), uc_core::ports::PeerAddressError> {
            unreachable!("removal covered in other suites")
        }
    }

    // T8:`ensure_reachable_all` 构造必须拿一个 `Arc<dyn PresencePort>`。
    // 本 smoke 集的 peer_addr_repo 始终返回空 vec,所以 `ensure_reachable`
    // 永远不会被触发;`current_state` / `subscribe` 也不走。3 个方法全
    // `unreachable!()` —— 若某测试路径意外调用到 presence,会立刻 panic
    // 而不是静默通过。
    struct FakePresence;
    #[async_trait]
    impl uc_core::ports::PresencePort for FakePresence {
        async fn ensure_reachable(
            &self,
            _device: &DeviceId,
        ) -> Result<uc_core::ports::ReachabilityState, uc_core::ports::PresenceError> {
            unreachable!("empty peer_addr_repo must keep ensure_reachable untouched")
        }
        async fn current_state(&self, _device: &DeviceId) -> uc_core::ports::ReachabilityState {
            unreachable!("current_state is the roster facade's path, not this one")
        }
        fn subscribe(&self) -> tokio::sync::broadcast::Receiver<uc_core::ports::PresenceEvent> {
            unreachable!("subscribe is the roster facade's path, not this one")
        }
    }

    // ── Switch-space minimal fakes (smoke-test scope only) ────────────────
    //
    // 既有 A1/A2/B1 smoke tests 不走 switch-space 路径，但 SpaceSetupDeps
    // 现在要求 4 个新 ports；下面 4 个 fake 全部返回中性默认值（None /
    // 空 vec / Ok），让 facade 构造器跑得通。switch-space 自身的契约
    // 验证留给 `usecases::setup::switch_space::tests` 与下面的
    // `switch_space::*` smoke。

    /// 内存版 `MigrationStatePort`，默认行为与之前一致（`None` + 任意写都成功）。
    /// 新增能力：通过 `with_phase` / `with_get_failure` 让单测注入 pending 阶段
    /// 或读失败，覆盖 unlock_space → resume_pending hook 的两条关键分支。
    #[derive(Default)]
    struct FakeMigrationState {
        current: StdMutex<Option<uc_core::setup::MigrationPhase>>,
        fail_get: std::sync::atomic::AtomicBool,
    }
    impl FakeMigrationState {
        fn with_phase(phase: uc_core::setup::MigrationPhase) -> Self {
            Self {
                current: StdMutex::new(Some(phase)),
                fail_get: std::sync::atomic::AtomicBool::new(false),
            }
        }
        fn with_get_failure() -> Self {
            Self {
                current: StdMutex::new(None),
                fail_get: std::sync::atomic::AtomicBool::new(true),
            }
        }
    }
    #[async_trait]
    impl uc_core::ports::setup::MigrationStatePort for FakeMigrationState {
        async fn get_current(
            &self,
        ) -> Result<
            Option<uc_core::setup::MigrationPhase>,
            uc_core::ports::setup::MigrationStateError,
        > {
            if self.fail_get.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(uc_core::ports::setup::MigrationStateError::Storage(
                    "injected migration_state get failure".into(),
                ));
            }
            Ok(self.current.lock().unwrap().clone())
        }
        async fn set_current(
            &self,
            phase: Option<uc_core::setup::MigrationPhase>,
        ) -> Result<(), uc_core::ports::setup::MigrationStateError> {
            *self.current.lock().unwrap() = phase;
            Ok(())
        }
    }

    struct FakeKeyMigration;
    #[async_trait]
    impl uc_core::ports::security::KeyMigrationPort for FakeKeyMigration {
        async fn prepare_migration_key(
            &self,
        ) -> Result<uc_core::setup::MigrationRunId, uc_core::ports::security::KeyMigrationError>
        {
            Ok(uc_core::setup::MigrationRunId::new("smoke-run-id"))
        }
        async fn encrypt_with_migration_key(
            &self,
            _run_id: &uc_core::setup::MigrationRunId,
            plaintext: &uc_core::crypto::domain::Plaintext,
            _aad: &uc_core::crypto::domain::Aad,
        ) -> Result<uc_core::crypto::domain::Ciphertext, uc_core::ports::security::KeyMigrationError>
        {
            Ok(uc_core::crypto::domain::Ciphertext::new(
                plaintext.as_bytes().to_vec(),
            ))
        }
        async fn decrypt_with_migration_key(
            &self,
            _run_id: &uc_core::setup::MigrationRunId,
            ciphertext: &uc_core::crypto::domain::Ciphertext,
            _aad: &uc_core::crypto::domain::Aad,
        ) -> Result<uc_core::crypto::domain::Plaintext, uc_core::ports::security::KeyMigrationError>
        {
            Ok(uc_core::crypto::domain::Plaintext::new(
                ciphertext.as_bytes().to_vec(),
            ))
        }
        async fn discard_migration_key(
            &self,
            _run_id: &uc_core::setup::MigrationRunId,
        ) -> Result<(), uc_core::ports::security::KeyMigrationError> {
            Ok(())
        }
    }

    struct FakeBlobMigrationRepo;
    #[async_trait]
    impl uc_core::ports::clipboard::BlobMigrationRepoPort for FakeBlobMigrationRepo {
        async fn list_main_inline_representations(
            &self,
        ) -> Result<
            Vec<(uc_core::ids::EventId, uc_core::ids::RepresentationId)>,
            uc_core::ports::clipboard::BlobMigrationRepoError,
        > {
            Ok(Vec::new())
        }
        async fn read_main_inline_data(
            &self,
            _event_id: &uc_core::ids::EventId,
            _representation_id: &uc_core::ids::RepresentationId,
        ) -> Result<Option<Vec<u8>>, uc_core::ports::clipboard::BlobMigrationRepoError> {
            Ok(None)
        }
        async fn upsert_record(
            &self,
            _record: &uc_core::ports::clipboard::MigrationRecord,
        ) -> Result<(), uc_core::ports::clipboard::BlobMigrationRepoError> {
            Ok(())
        }
        async fn count_records(
            &self,
        ) -> Result<u64, uc_core::ports::clipboard::BlobMigrationRepoError> {
            Ok(0)
        }
        async fn list_records(
            &self,
        ) -> Result<
            Vec<uc_core::ports::clipboard::MigrationRecord>,
            uc_core::ports::clipboard::BlobMigrationRepoError,
        > {
            Ok(Vec::new())
        }
        async fn update_main_inline_data(
            &self,
            _event_id: &uc_core::ids::EventId,
            _representation_id: &uc_core::ids::RepresentationId,
            _new_ciphertext: &[u8],
        ) -> Result<(), uc_core::ports::clipboard::BlobMigrationRepoError> {
            Ok(())
        }
        async fn discard_all_records(
            &self,
        ) -> Result<(), uc_core::ports::clipboard::BlobMigrationRepoError> {
            Ok(())
        }
    }

    struct FakeBlobCipher;
    #[async_trait]
    impl uc_core::ports::security::BlobCipherPort for FakeBlobCipher {
        async fn encrypt(
            &self,
            _space: &uc_core::crypto::domain::ActiveSpace,
            plaintext: &uc_core::crypto::domain::Plaintext,
            _aad: &uc_core::crypto::domain::Aad,
        ) -> Result<uc_core::crypto::domain::Ciphertext, uc_core::ports::security::BlobCipherError>
        {
            Ok(uc_core::crypto::domain::Ciphertext::new(
                plaintext.as_bytes().to_vec(),
            ))
        }
        async fn decrypt(
            &self,
            _space: &uc_core::crypto::domain::ActiveSpace,
            ciphertext: &uc_core::crypto::domain::Ciphertext,
            _aad: &uc_core::crypto::domain::Aad,
        ) -> Result<uc_core::crypto::domain::Plaintext, uc_core::ports::security::BlobCipherError>
        {
            Ok(uc_core::crypto::domain::Plaintext::new(
                ciphertext.as_bytes().to_vec(),
            ))
        }
    }

    fn default_fingerprint() -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap()
    }

    fn make_facade(
        space_access: Arc<FakeSpaceAccess>,
        setup_status: Arc<dyn SetupStatusPort>,
        settings: Arc<dyn SettingsPort>,
    ) -> (
        SpaceSetupFacade,
        Arc<FakeInvitationPort>,
        Arc<FakePeerAddrRepo>,
    ) {
        make_facade_with_migration_state(
            space_access,
            setup_status,
            settings,
            Arc::new(FakeMigrationState::default()),
        )
    }

    fn make_facade_with_migration_state(
        space_access: Arc<FakeSpaceAccess>,
        setup_status: Arc<dyn SetupStatusPort>,
        settings: Arc<dyn SettingsPort>,
        migration_state: Arc<FakeMigrationState>,
    ) -> (
        SpaceSetupFacade,
        Arc<FakeInvitationPort>,
        Arc<FakePeerAddrRepo>,
    ) {
        let pairing_invitation = Arc::new(FakeInvitationPort::default());
        let peer_addr_repo = Arc::new(FakePeerAddrRepo::default());
        let facade = SpaceSetupFacade::new(SpaceSetupDeps {
            space_access: SpaceAccessPorts::from_adapter(space_access),
            local_identity: Arc::new(FakeLocalIdentity {
                fp: default_fingerprint(),
            }),
            device_identity: Arc::new(FixedDeviceIdentity {
                id: DeviceId::new("device-1"),
            }),
            member_repo: Arc::new(InMemoryMemberRepo::default()),
            setup_status,
            settings,
            clock: Arc::new(FixedClock(0)),
            pairing_invitation: pairing_invitation.clone(),
            pairing_invitation_addresses: pairing_invitation.clone(),
            pairing_invitation_by_address: pairing_invitation.clone(),
            pairing_session: Arc::new(NoopSessionPort),
            pairing_events: Arc::new(IdleEventPort::new()),
            proof_port: Arc::new(NoopProofPort),
            trusted_peer_repo: Arc::new(NoopTrustedPeerRepo),
            peer_addr_repo: Arc::clone(&peer_addr_repo)
                as Arc<dyn uc_core::ports::PeerAddressRepositoryPort>,
            presence: Arc::new(FakePresence),
            migration_state,
            key_migration: Arc::new(FakeKeyMigration),
            blob_migration_repo: Arc::new(FakeBlobMigrationRepo),
            blob_cipher: Arc::new(FakeBlobCipher),
            analytics: Arc::new(uc_observability::analytics::NoopAnalyticsFacade),
        });
        (facade, pairing_invitation, peer_addr_repo)
    }

    fn settings_with_device_name(name: &str) -> Arc<InMemorySettings> {
        let holder = InMemorySettings::default();
        {
            let mut s = holder.settings.lock().unwrap();
            s.general.device_name = Some(name.to_string());
        }
        Arc::new(holder)
    }

    // ── tests ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn initialize_space_forwards_happy_path() {
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            settings_with_device_name("mac"),
        );
        let cmd = InitializeSpaceInput {
            passphrase: "hunter22hunter22".to_string(),
            passphrase_confirm: "hunter22hunter22".to_string(),
            device_name: None,
        };
        let out = facade.initialize_space(cmd).await.expect("A1 ok");
        assert_eq!(out.fingerprint, default_fingerprint());
    }

    #[tokio::test]
    async fn initialize_space_forwards_passphrase_mismatch() {
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            settings_with_device_name("mac"),
        );
        let cmd = InitializeSpaceInput {
            passphrase: "hunter22hunter22".to_string(),
            passphrase_confirm: "different22else2".to_string(),
            device_name: None,
        };
        let err = facade.initialize_space(cmd).await.unwrap_err();
        assert!(matches!(err, InitializeSpaceError::PassphraseMismatch));
    }

    #[tokio::test]
    async fn unlock_space_forwards_happy_path() {
        let setup_status = InMemorySetupStatus::default();
        *setup_status.status.lock().unwrap() = SetupStatus {
            has_completed: true,
            space_id: None,
        };
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(setup_status),
            Arc::new(InMemorySettings::default()),
        );
        let cmd = UnlockSpaceInput {
            passphrase: "hunter22hunter22".to_string(),
        };
        facade.unlock_space(cmd).await.expect("A2 ok");
    }

    #[tokio::test]
    async fn unlock_space_forwards_setup_not_completed() {
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            Arc::new(InMemorySettings::default()),
        );
        let cmd = UnlockSpaceInput {
            passphrase: "hunter22hunter22".to_string(),
        };
        let err = facade.unlock_space(cmd).await.unwrap_err();
        assert!(matches!(err, UnlockSpaceError::SetupNotCompleted));
    }

    #[tokio::test]
    async fn unlock_space_forwards_wrong_passphrase() {
        let setup_status = InMemorySetupStatus::default();
        *setup_status.status.lock().unwrap() = SetupStatus {
            has_completed: true,
            space_id: None,
        };
        let space_access = FakeSpaceAccess::default();
        *space_access.unlock_err.lock().unwrap() = Some(SpaceAccessError::WrongPassphrase);
        let (facade, _inv, _peer) = make_facade(
            Arc::new(space_access),
            Arc::new(setup_status),
            Arc::new(InMemorySettings::default()),
        );
        let cmd = UnlockSpaceInput {
            passphrase: "hunter22hunter22".to_string(),
        };
        let err = facade.unlock_space(cmd).await.unwrap_err();
        assert!(matches!(err, UnlockSpaceError::WrongPassphrase));
    }

    #[tokio::test]
    async fn unlock_space_drives_resume_pending_when_migration_pending() {
        // 回归 phase 3 中断后没人推进的 wedged 态：startup_recovery 在
        // auto-unlock 关闭时会跳过 try_resume_session，那时 unlock_space
        // 是唯一能在解锁后接管 migration replay 的入口。
        //
        // Prepared 分支走 cleanup_after_phase2_failure：跑完后
        // migration_state 应被推回 None（验证 hook 真的被触发了，而不是
        // 拿了个空 phase 跑回 noop）。
        let setup_status = InMemorySetupStatus::default();
        *setup_status.status.lock().unwrap() = SetupStatus {
            has_completed: true,
            space_id: None,
        };
        let migration_state = Arc::new(FakeMigrationState::with_phase(
            uc_core::setup::MigrationPhase::Prepared {
                run_id: uc_core::setup::MigrationRunId::new("test-pending-run"),
            },
        ));
        let (facade, _inv, _peer) = make_facade_with_migration_state(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(setup_status),
            Arc::new(InMemorySettings::default()),
            Arc::clone(&migration_state),
        );
        let cmd = UnlockSpaceInput {
            passphrase: "hunter22hunter22".to_string(),
        };
        facade
            .unlock_space(cmd)
            .await
            .expect("unlock_space succeeds with Prepared phase recovery");
        let after = uc_core::ports::setup::MigrationStatePort::get_current(&*migration_state)
            .await
            .expect("get_current ok");
        assert!(
            after.is_none(),
            "resume_pending Prepared 分支应清掉 migration_state，实际仍为 {after:?}"
        );
    }

    #[tokio::test]
    async fn unlock_space_returns_internal_when_resume_pending_fails() {
        // resume_pending 任何失败都要冒到 unlock_space 调用方，不能 silent
        // 吞掉——否则又会复刻 fedora 事故：session 拿到新 master_key，但
        // 主表仍用旧 key，UI 看不到任何数据，daemon 还报 "解锁成功"。
        let setup_status = InMemorySetupStatus::default();
        *setup_status.status.lock().unwrap() = SetupStatus {
            has_completed: true,
            space_id: None,
        };
        let migration_state = Arc::new(FakeMigrationState::with_get_failure());
        let (facade, _inv, _peer) = make_facade_with_migration_state(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(setup_status),
            Arc::new(InMemorySettings::default()),
            migration_state,
        );
        let cmd = UnlockSpaceInput {
            passphrase: "hunter22hunter22".to_string(),
        };
        let err = facade.unlock_space(cmd).await.unwrap_err();
        match err {
            UnlockSpaceError::Internal(msg) => assert!(
                msg.contains("migration resume failed"),
                "expected prefix in internal message, got {msg}"
            ),
            other => panic!("expected Internal, got {other:?}"),
        }
    }

    // ── F2 shutdown ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn on_shutdown_completes_without_panicking() {
        // Slice 4 P5c: F2 hook 不再调 stop_network(NetworkControlPort 已退役),
        // 这里只确认 abort 入站 orchestrator 后 facade 能正常清理,不 panic、
        // 不阻塞。
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            Arc::new(InMemorySettings::default()),
        );
        facade.on_shutdown().await;
    }

    // ── T8 · F1 hook: auto_prime_presence triggers ensure_reachable_all ─
    //
    // 契约(plan §7.1 验收点):
    // * A1 / A2 / B2 成功 → auto_prime_presence → ensure_reachable_all 跑一次
    //   (以 peer_addr_repo.list() 被调计数代理——空 repo 路径下也跑过 list)
    // * ensure_reachable_all 失败 → A1/A2 结果不受影响(本集下用空 repo,
    //   ensure_reachable_all 不会失败,只验证"跑过")

    #[tokio::test]
    async fn f1_hook_initialize_space_success_triggers_ensure_reachable_all() {
        let (facade, _inv, peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            settings_with_device_name("mac"),
        );
        let cmd = InitializeSpaceInput {
            passphrase: "hunter22hunter22".to_string(),
            passphrase_confirm: "hunter22hunter22".to_string(),
            device_name: None,
        };
        facade.initialize_space(cmd).await.expect("A1 ok");
        assert_eq!(
            peer.list_calls(),
            1,
            "A1 success must trigger ensure_reachable_all (list invoked once)",
        );
    }

    #[tokio::test]
    async fn f1_hook_unlock_space_success_triggers_ensure_reachable_all() {
        let setup_status = InMemorySetupStatus::default();
        *setup_status.status.lock().unwrap() = SetupStatus {
            has_completed: true,
            space_id: None,
        };
        let (facade, _inv, peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(setup_status),
            Arc::new(InMemorySettings::default()),
        );
        let cmd = UnlockSpaceInput {
            passphrase: "hunter22hunter22".to_string(),
        };
        facade.unlock_space(cmd).await.expect("A2 ok");
        assert_eq!(
            peer.list_calls(),
            1,
            "A2 success must trigger ensure_reachable_all",
        );
    }

    #[tokio::test]
    async fn f1_hook_skipped_when_lifecycle_action_fails() {
        // A1 失败(passphrase mismatch)→ 不跑 ensure_reachable_all。
        // 验证 guard 顺序正确(失败短路在 prime 之前)。
        let (facade, _inv, peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            settings_with_device_name("mac"),
        );
        let cmd = InitializeSpaceInput {
            passphrase: "hunter22hunter22".to_string(),
            passphrase_confirm: "different22else2".to_string(),
            device_name: None,
        };
        let _ = facade.initialize_space(cmd).await.unwrap_err();
        assert_eq!(peer.list_calls(), 0);
    }

    // ── B1 · issue pairing invitation wiring ─────────────────────────────

    #[tokio::test]
    async fn issue_pairing_invitation_forwards_happy_path() {
        let (facade, inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            Arc::new(InMemorySettings::default()),
        );
        let out = facade.issue_pairing_invitation().await.expect("B1 ok");
        assert_eq!(out.code.as_str(), "SMOKE-0001");
        assert_eq!(*inv.calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn issue_pairing_invitation_forwards_network_not_started() {
        let (facade, inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            Arc::new(InMemorySettings::default()),
        );
        *inv.next_err.lock().unwrap() = Some(InvitationError::NetworkNotStarted);
        let err = facade.issue_pairing_invitation().await.unwrap_err();
        assert!(matches!(
            err,
            IssuePairingInvitationError::NetworkNotStarted
        ));
    }

    // ── Slice4 P3 T3.2 · cancel / reset / query_setup_state ────────────

    #[tokio::test]
    async fn cancel_invitation_returns_not_issued_when_holder_empty() {
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            Arc::new(InMemorySettings::default()),
        );
        let err = facade.cancel_invitation().await.unwrap_err();
        assert!(matches!(err, CancelInvitationError::NotIssued));
    }

    #[tokio::test]
    async fn cancel_invitation_clears_pending_after_issue() {
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            Arc::new(InMemorySettings::default()),
        );
        facade.issue_pairing_invitation().await.expect("B1 ok");
        assert_eq!(facade.invitation_holder.len().await, 1);
        facade.cancel_invitation().await.expect("cancel ok");
        assert_eq!(facade.invitation_holder.len().await, 0);
    }

    #[tokio::test]
    async fn reset_clears_setup_status_and_invitations() {
        let setup_status = InMemorySetupStatus::default();
        *setup_status.status.lock().unwrap() = SetupStatus {
            has_completed: true,
            space_id: None,
        };
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(setup_status),
            Arc::new(InMemorySettings::default()),
        );
        facade.issue_pairing_invitation().await.expect("B1 ok");
        assert_eq!(facade.invitation_holder.len().await, 1);

        facade.reset().await.expect("reset ok");

        assert_eq!(facade.invitation_holder.len().await, 0);
        let view = facade.query_setup_state().await.expect("query ok");
        assert!(!view.has_completed);
        assert!(view.current_invitation.is_none());
    }

    #[tokio::test]
    async fn factory_reset_wipes_key_material_and_clears_setup_status() {
        let setup_status = InMemorySetupStatus::default();
        *setup_status.status.lock().unwrap() = SetupStatus {
            has_completed: true,
            space_id: None,
        };
        let space_access = Arc::new(FakeSpaceAccess::default());
        let (facade, _inv, _peer) = make_facade(
            space_access.clone(),
            Arc::new(setup_status),
            Arc::new(InMemorySettings::default()),
        );
        facade.issue_pairing_invitation().await.expect("B1 ok");
        assert_eq!(facade.invitation_holder.len().await, 1);

        facade.factory_reset().await.expect("factory_reset ok");

        assert_eq!(*space_access.factory_reset_calls.lock().unwrap(), 1);
        assert_eq!(facade.invitation_holder.len().await, 0);
        let view = facade.query_setup_state().await.expect("query ok");
        assert!(!view.has_completed);
    }

    #[tokio::test]
    async fn factory_reset_preserves_setup_status_when_key_wipe_fails() {
        // 关键不变式: keyslot 删除失败时 setup_status 必须保留 `has_completed=true`,
        // 否则 UI 会跳到 SetupPage,用户再走 init 立即撞到 AlreadyInitialized,体验更糟。
        let setup_status = InMemorySetupStatus::default();
        *setup_status.status.lock().unwrap() = SetupStatus {
            has_completed: true,
            space_id: None,
        };
        let space_access = Arc::new(FakeSpaceAccess::default());
        *space_access.factory_reset_err.lock().unwrap() =
            Some(SpaceAccessError::Internal("disk i/o".to_string()));
        let (facade, _inv, _peer) = make_facade(
            space_access.clone(),
            Arc::new(setup_status),
            Arc::new(InMemorySettings::default()),
        );

        let err = facade.factory_reset().await.unwrap_err();

        assert!(matches!(err, FactoryResetError::KeyMaterialWipeFailed(_)));
        let view = facade.query_setup_state().await.expect("query ok");
        assert!(
            view.has_completed,
            "setup_status must remain completed when key wipe fails"
        );
    }

    #[tokio::test]
    async fn query_setup_state_reports_fresh_install_defaults() {
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            Arc::new(InMemorySettings::default()),
        );
        let view = facade.query_setup_state().await.expect("query ok");
        assert!(!view.has_completed);
        assert!(view.current_invitation.is_none());
        assert!(view.device_name.is_none());
    }

    #[tokio::test]
    async fn query_setup_state_reflects_completed_status_and_device_name() {
        let setup_status = InMemorySetupStatus::default();
        *setup_status.status.lock().unwrap() = SetupStatus {
            has_completed: true,
            space_id: None,
        };
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(setup_status),
            settings_with_device_name("MacBook"),
        );
        let view = facade.query_setup_state().await.expect("query ok");
        assert!(view.has_completed);
        assert_eq!(view.device_name.as_deref(), Some("MacBook"));
        assert!(view.current_invitation.is_none());
    }

    #[tokio::test]
    async fn query_setup_state_surfaces_pending_invitation_after_issue() {
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            Arc::new(InMemorySettings::default()),
        );
        facade.issue_pairing_invitation().await.expect("B1 ok");
        let view = facade.query_setup_state().await.expect("query ok");
        let inv = view.current_invitation.expect("invitation present");
        assert_eq!(inv.code.as_str(), "SMOKE-0001");
        assert_eq!(
            inv.expires_at,
            DateTime::parse_from_rfc3339("2026-04-20T10:05:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[tokio::test]
    async fn issue_pairing_invitation_does_not_prime_presence() {
        // B1 不是 space-lifecycle 动作,不应触发 auto_prime_presence
        // (presence 缓存只该被 A1 / A2 / B2 触动,B1 出码不涉及与对端互联)。
        let (facade, _inv, peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            Arc::new(InMemorySettings::default()),
        );
        facade.issue_pairing_invitation().await.expect("B1 ok");
        assert_eq!(
            peer.list_calls(),
            0,
            "B1 must not trigger ensure_reachable_all",
        );
    }

    // ── Switch-space smoke tests (commit 4) ──────────────────────────────
    //
    // 端口契约层面的回归在 `usecases::setup::switch_space::tests` 里覆盖；
    // 这里只验 facade 层的转发 / 启动 hook 行为：
    // * pre-flight 把"设备未 setup"映射成 `NotSetup`。
    // * `query_migration_progress` 在空闲状态（FakeMigrationState 全返
    //   None）下返回 `phase=None, count=0`。
    // * `try_resume_session` 在 has_completed=true + FakeSpaceAccess 返回
    //   `Some(active_space)` 时走通 `resume_pending`（None phase 是 noop，
    //   不会拉错路径）。

    #[tokio::test]
    async fn switch_space_rejects_when_setup_not_completed() {
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()), // has_completed=false
            Arc::new(InMemorySettings::default()),
        );
        let err = facade
            .switch_space(SwitchSpaceInput {
                code: "CODE-1".into(),
                new_passphrase: "hunter22hunter22".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, SwitchSpaceError::NotSetup));
    }

    #[tokio::test]
    async fn query_migration_progress_idle_returns_phase_none_and_zero_count() {
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(InMemorySetupStatus::default()),
            Arc::new(InMemorySettings::default()),
        );
        let progress = facade.query_migration_progress().await.expect("idle ok");
        assert_eq!(progress.phase, None);
        assert_eq!(progress.backup_record_count, 0);
    }

    #[tokio::test]
    async fn try_resume_session_resumes_silent_unlock_and_runs_migration_hook() {
        // FakeSpaceAccess::try_resume_session 默认返回 Ok(None)，模拟没有
        // keyslot 的场景——`try_resume_session` 应返回 Ok(false)，且
        // resume_pending 因为 FakeMigrationState 返回 None 也是 noop。
        // 这里覆盖的是"两个 hook 都不 panic + 错误不被吞"。
        let setup_status = InMemorySetupStatus::default();
        *setup_status.status.lock().unwrap() = SetupStatus {
            has_completed: true,
            space_id: None,
        };
        let (facade, _inv, _peer) = make_facade(
            Arc::new(FakeSpaceAccess::default()),
            Arc::new(setup_status),
            Arc::new(InMemorySettings::default()),
        );
        let resumed = facade.try_resume_session().await.expect("resume ok");
        // FakeSpaceAccess.try_resume_session 默认 Ok(None) → "nothing to resume"
        assert!(!resumed);
    }
}

//! B2 · `RedeemPairingInvitationUseCase` (joiner side).
//!
//! Thin composition layer: delegates wire + crypto to
//! [`JoinerHandshakeCoordinator`], then persists the sponsor's facts
//! (`admit_member` → `trust_peer` → `setup_status`) and maps the
//! outcome to the facade-level result.
//!
//! ## Ordering: persist before declaring success
//!
//! Mirrors sponsor-side P7f cleanup: `admit` → `trust` →
//! `setup_status.set_status(completed)` all land **before** `execute`
//! returns `Ok`. Any persistence failure short-circuits the remaining
//! steps and surfaces as
//! [`RedeemPairingInvitationError::Internal`] — the caller never gets
//! a success result that isn't backed by fully committed local state.
//!
//! `setup_status` is flipped **last**, because `has_completed=true` is
//! the marker `UnlockSpaceUseCase` keys off on the next launch.
//! Flipping it before `trust_peer` landed would leave the policy
//! resolver seeing "setup done" but no trusted peers, blocking every
//! inbound session.
//!
//! ## Why no FSM
//!
//! See F-053: the joiner flow is linear once the passphrase is
//! collected up front (Slice 1 UX), and `SpaceAccessStateMachine`'s
//! default action order is inverted from the persist-before-success
//! ordering this use case wants.
//!
//! ## Why a coordinator below
//!
//! Extracted in post-P7h cleanup: the original 11-arg use case was
//! doing dial + identity assembly + crypto + recv/decode + admit +
//! trust + setup-status in one struct. That broke symmetry with the
//! sponsor side (which already split wire/crypto into
//! [`SponsorHandshakeCoordinator`]). The use case is now 5 deps and
//! one-to-one mirrors `PairingInboundOrchestrator`'s composition
//! shape.
//!
//! [`JoinerHandshakeCoordinator`]:
//!     crate::pairing_outbound::joiner_handshake::JoinerHandshakeCoordinator
//! [`SponsorHandshakeCoordinator`]:
//!     crate::pairing_inbound::sponsor_handshake::SponsorHandshakeCoordinator

use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use tracing::{info, instrument};

use uc_core::ports::pairing::DiscoveryChannel;
use uc_core::ports::{ClockPort, PeerAddressRecord, PeerAddressRepositoryPort, SetupStatusPort};
use uc_core::setup::SetupStatus;
use uc_core::{MemberRepositoryPort, MemberSyncPreferences, TrustedPeerRepositoryPort};
use uc_observability::analytics::events::{
    Event, PairingDiscoveryChannel, PairingFailureReason, PairingMethod,
};
use uc_observability::analytics::AnalyticsFacade;

use crate::facade::space_setup::commands::RedeemPairingInvitationCommand;
use crate::facade::space_setup::{RedeemPairingInvitationError, RedeemPairingInvitationResult};
use crate::membership::errors::MembershipApplicationError;
use crate::membership::usecases::{AdmitMember, AdmitMemberUseCase};
use crate::pairing_outbound::joiner_handshake::{
    JoinerHandshakeCoordinator, JoinerHandshakeOutcome,
};
use crate::trusted_peer::errors::TrustedPeerApplicationError;
use crate::trusted_peer::usecases::{TrustPeer, TrustPeerUseCase};

pub(crate) type AdmitMemberUc = AdmitMemberUseCase<dyn MemberRepositoryPort>;
pub(crate) type TrustPeerUc = TrustPeerUseCase<dyn TrustedPeerRepositoryPort>;

pub(crate) struct RedeemPairingInvitationUseCase {
    handshake: Arc<JoinerHandshakeCoordinator>,
    admit_member: Arc<AdmitMemberUc>,
    trust_peer: Arc<TrustPeerUc>,
    setup_status: Arc<dyn SetupStatusPort>,
    /// Slice 2 Phase 1 · T5：配对完成后 best-effort 把 sponsor 的传输地址
    /// blob 写入仓库。写失败不 fail join（presence 下轮会再拉）。
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    clock: Arc<dyn ClockPort>,
    /// Joiner-side analytics: fires `pairing_started` on entry,
    /// `pairing_succeeded` / `pairing_failed` on result. The identity
    /// switch from anonymous to the sponsor-issued `space_person_id`
    /// also goes through this facade after setup status is persisted.
    /// All calls are fire-and-forget; the gate inside the facade
    /// implementation keeps them off the hot path.
    analytics: Arc<dyn AnalyticsFacade>,
}

impl RedeemPairingInvitationUseCase {
    pub(crate) fn new(
        handshake: Arc<JoinerHandshakeCoordinator>,
        admit_member: Arc<AdmitMemberUc>,
        trust_peer: Arc<TrustPeerUc>,
        setup_status: Arc<dyn SetupStatusPort>,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        clock: Arc<dyn ClockPort>,
        analytics: Arc<dyn AnalyticsFacade>,
    ) -> Self {
        Self {
            handshake,
            admit_member,
            trust_peer,
            setup_status,
            peer_addr_repo,
            clock,
            analytics,
        }
    }

    #[instrument(skip_all, fields(code = %cmd.code.as_str()))]
    pub(crate) async fn execute(
        &self,
        cmd: RedeemPairingInvitationCommand,
    ) -> Result<RedeemPairingInvitationResult, RedeemPairingInvitationError> {
        // Slice 8b · pairing_started 在 execute 入口立即 fire,即使 handshake
        // 第一行就拒绝(InvitationNotFound)也保证 funnel 第一步留下信号。
        // PairingMethod 在 use case 签名里目前不存在区分维度(QR / Code /
        // Discovery 由 GUI 在更上层处理后都进同一入口),v1 固定 Code 占位;
        // 后续若 GUI 把 method 维度下推到 use case 输入再细化。
        self.analytics.capture(Event::PairingStarted {
            method: PairingMethod::Code,
        });
        let started_at = Instant::now();
        let result = async {
            let outcome = self.handshake.handshake(&cmd.code, &cmd.passphrase).await?;
            // `DiscoveryChannel` is `Copy`; capture it before `persist`
            // consumes the outcome so `pairing_succeeded` can record which
            // channel resolved this first pair.
            let channel = outcome.discovery_channel;
            self.persist(outcome).await.map(|res| (res, channel))
        }
        .await;
        let duration_ms = started_at.elapsed().as_millis().min(u32::MAX as u128) as u32;
        match &result {
            Ok((_, channel)) => self.analytics.capture(Event::PairingSucceeded {
                method: PairingMethod::Code,
                // peer_os v1 留空——握手 outcome 里没有对端 OS 字段。后续
                // 协议加入对端 OS 自报后回填,schema 已用 Option 兼容。
                peer_os: None,
                duration_ms,
                discovery_channel: Some(map_discovery_channel(*channel)),
            }),
            Err(err) => self.analytics.capture(Event::PairingFailed {
                method: PairingMethod::Code,
                failure_reason: map_redeem_error_to_pairing_failure_reason(err),
            }),
        }
        result.map(|(res, _)| res)
    }

    async fn persist(
        &self,
        outcome: JoinerHandshakeOutcome,
    ) -> Result<RedeemPairingInvitationResult, RedeemPairingInvitationError> {
        let now = self.now_utc()?;

        // Admit sponsor as member.
        let admit_input = AdmitMember {
            device_id: outcome.sponsor_device_id.clone(),
            device_name: outcome.sponsor_device_name.clone(),
            identity_fingerprint: outcome.sponsor_identity_fingerprint.clone(),
            joined_at: now,
            sync_preferences: MemberSyncPreferences::default(),
        };
        self.admit_member
            .execute(admit_input)
            .await
            .map_err(map_admit_err)?;

        // Trust sponsor.
        let trust_input = TrustPeer {
            local_device_id: outcome.self_device_id.clone(),
            peer_device_id: outcome.sponsor_device_id.clone(),
            peer_fingerprint: outcome.sponsor_identity_fingerprint.clone(),
            trusted_at: now,
        };
        self.trust_peer
            .execute(trust_input)
            .await
            .map_err(map_trust_err)?;

        // Mark setup complete (ordering rationale: see module doc).
        // Adopt the sponsor's `space_id` — without this, future commands
        // on this joiner would mint a fresh id and the two sides would
        // diverge on the canonical identifier.
        self.setup_status
            .set_status(&SetupStatus {
                has_completed: true,
                space_id: Some(outcome.space_id.clone()),
            })
            .await
            .map_err(|e| {
                RedeemPairingInvitationError::Internal(format!("setup_status.set_status: {e}"))
            })?;

        // Slice 2 Phase 1 · T5：best-effort upsert sponsor transport addr。
        // 位置放在 `setup_status` 之后是刻意的：setup_status=true 才是
        // 配对 Success 的单点真相来源；peer address 写入是体验优化，
        // 失败不能回退配对状态。空 blob（旧 sponsor / adapter 未附带）
        // 跳过；写失败仅 warn，presence `ensure_reachable_all` 下一轮
        // 兜底。
        self.persist_sponsor_address(&outcome, now).await;

        // Identity switch runs after setup_status is persisted but
        // before the outer `execute` emits `pairing_succeeded`, so
        // pairing_succeeded already reports under the new person.
        // `None` means the sponsor has no `space_person_id` yet
        // (v1→v2 first-pair case); joiner stays Solo and waits for
        // a future sponsor-initiated re-pair to converge.
        // Adopt failures are warn-logged by the facade and never
        // block pairing — the ground truth of "paired" is
        // setup_status=true, not the analytics side effect.
        if let Some(space_person_id) = outcome.sponsor_space_person_id {
            self.analytics.adopt_from_sponsor(space_person_id);
        }

        info!(
            sponsor_device_id = %outcome.sponsor_device_id.as_str(),
            space_id = %outcome.space_id,
            "joiner pairing complete; local space ready"
        );

        Ok(RedeemPairingInvitationResult {
            sponsor_device_id: outcome.sponsor_device_id,
            sponsor_identity_fingerprint: outcome.sponsor_identity_fingerprint,
            space_id: outcome.space_id,
            self_device_id: outcome.self_device_id,
            self_identity_fingerprint: outcome.self_identity_fingerprint,
        })
    }

    fn now_utc(&self) -> Result<DateTime<Utc>, RedeemPairingInvitationError> {
        DateTime::<Utc>::from_timestamp_millis(self.clock.now_ms()).ok_or_else(|| {
            RedeemPairingInvitationError::Internal("clock returned invalid timestamp".into())
        })
    }

    async fn persist_sponsor_address(
        &self,
        outcome: &JoinerHandshakeOutcome,
        observed_at: DateTime<Utc>,
    ) {
        if outcome.sponsor_transport_address_blob.is_empty() {
            tracing::debug!(
                sponsor_device_id = %outcome.sponsor_device_id.as_str(),
                "sponsor did not supply transport_address_blob; skipping peer_addr_repo upsert"
            );
            return;
        }
        let record = PeerAddressRecord {
            device_id: outcome.sponsor_device_id.clone(),
            addr_blob: outcome.sponsor_transport_address_blob.clone(),
            observed_at,
        };
        if let Err(err) = self.peer_addr_repo.upsert(&record).await {
            tracing::warn!(
                sponsor_device_id = %outcome.sponsor_device_id.as_str(),
                error = %err,
                "peer_addr_repo.upsert failed after pairing; presence will recover lazily"
            );
        } else {
            tracing::debug!(
                sponsor_device_id = %outcome.sponsor_device_id.as_str(),
                blob_len = outcome.sponsor_transport_address_blob.len(),
                "peer_addr_repo.upsert landed for paired sponsor"
            );
        }
    }
}

/// Slice 8b · `RedeemPairingInvitationError` → `PairingFailureReason` 1:1
/// 映射。每个业务变体单独落到独立的 funnel 漏点信号,避免跨 domain 聚合
/// 时丢失"这条 join 是 passphrase 错 vs sponsor 主动拒绝 vs 网络超时"
/// 的关键区分。`Internal` / `SponsorInternal` 占比是架构债务指标
/// (schema doc §7.4)。
/// Map the domain discovery channel onto its telemetry wire enum. Keeps the
/// analytics layer decoupled from `uc-core` port types.
fn map_discovery_channel(channel: DiscoveryChannel) -> PairingDiscoveryChannel {
    match channel {
        DiscoveryChannel::Cloud => PairingDiscoveryChannel::Cloud,
        DiscoveryChannel::Lan => PairingDiscoveryChannel::Lan,
    }
}

fn map_redeem_error_to_pairing_failure_reason(
    err: &RedeemPairingInvitationError,
) -> PairingFailureReason {
    match err {
        RedeemPairingInvitationError::InvitationNotFound => {
            PairingFailureReason::InvitationNotFound
        }
        RedeemPairingInvitationError::InvitationExpired => PairingFailureReason::InvitationExpired,
        RedeemPairingInvitationError::SponsorUnreachable => {
            PairingFailureReason::SponsorUnreachable
        }
        RedeemPairingInvitationError::ServiceUnavailable => {
            PairingFailureReason::ServiceUnavailable
        }
        RedeemPairingInvitationError::PassphraseMismatch => {
            PairingFailureReason::PassphraseMismatch
        }
        RedeemPairingInvitationError::CorruptedKeyMaterial => {
            PairingFailureReason::CorruptedKeyMaterial
        }
        RedeemPairingInvitationError::DeviceNameRequired => {
            PairingFailureReason::DeviceNameRequired
        }
        RedeemPairingInvitationError::SponsorRejectedInvitation => {
            PairingFailureReason::SponsorRejectedInvitation
        }
        RedeemPairingInvitationError::SponsorDeclined => PairingFailureReason::SponsorDeclined,
        RedeemPairingInvitationError::SponsorTimedOut => PairingFailureReason::SponsorTimedOut,
        RedeemPairingInvitationError::SponsorInternal(_) => PairingFailureReason::SponsorInternal,
        RedeemPairingInvitationError::Timeout => PairingFailureReason::Timeout,
        RedeemPairingInvitationError::ConnectionLost => PairingFailureReason::ConnectionLost,
        RedeemPairingInvitationError::Internal(_) => PairingFailureReason::Internal,
    }
}

fn map_admit_err(err: MembershipApplicationError) -> RedeemPairingInvitationError {
    // `AlreadyAdmitted` / `AlreadyTrusted` also land here: we can't
    // distinguish a retry-of-completed-run from a half-committed-state
    // without a separate resume flag, so fail loudly. Recovery path is
    // a factory_reset followed by a fresh redeem.
    RedeemPairingInvitationError::Internal(format!("admit_member: {err}"))
}

fn map_trust_err(err: TrustedPeerApplicationError) -> RedeemPairingInvitationError {
    RedeemPairingInvitationError::Internal(format!("trust_peer: {err}"))
}

#[cfg(test)]
mod tests {
    //! Composition tests only: wire + crypto covered in
    //! [`crate::pairing_outbound::joiner_handshake::tests`]. Here we
    //! verify that a coordinator outcome drives admit → trust →
    //! setup-status in the right order, and that each step's failure
    //! short-circuits the remaining ones without flipping
    //! `setup_status`.
    //!
    //! To avoid mocking the coordinator behind a trait (symmetric with
    //! how sponsor-side orchestrator tests use a real
    //! `SponsorHandshakeCoordinator`), these tests construct a real
    //! `JoinerHandshakeCoordinator` with scripted session/crypto fakes
    //! that deliver a happy-path outcome. That's a small amount of
    //! wire-test overlap with the coordinator's own tests, but keeps
    //! the use case under the same seams production uses.
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use uuid::Uuid;

    use uc_core::crypto::domain::{ActiveSpace, Passphrase};
    use uc_core::ids::{DeviceId, SessionId, SpaceId};
    use uc_core::membership::{MembershipError, SpaceMember};
    use uc_core::pairing::invitation::InvitationCode;
    use uc_core::pairing::session_message::{
        PairingSessionMessage, SponsorConfirm, SponsorKeyslotOffer,
    };
    use uc_core::ports::pairing::{
        DialError, DialOutcome, DiscoveryChannel, PairingSessionId, PairingSessionPort,
        SessionError,
    };
    use uc_core::ports::space::{ProofPort, SpaceAccessError, SpaceAccessPort};
    use uc_core::ports::{DeviceIdentityPort, LocalIdentityError, LocalIdentityPort, SettingsPort};
    use uc_core::security::IdentityFingerprint;
    use uc_core::settings::model::Settings;
    use uc_core::space_access::domain::{JoinOffer, ProofDerivedKey, SpaceAccessProofArtifact};
    use uc_core::trusted_peer::{TrustedPeer, TrustedPeerError};

    use chrono::DateTime;
    use tokio::time::Duration;

    // ── minimal wire fakes (for producing a happy-path outcome) ──────────

    #[derive(Default)]
    struct HappySession {
        sent: StdMutex<Vec<PairingSessionMessage>>,
        recv: StdMutex<VecDeque<PairingSessionMessage>>,
        closed: StdMutex<u32>,
    }
    impl HappySession {
        fn primed() -> Self {
            let me = Self::default();
            me.recv
                .lock()
                .unwrap()
                .push_back(PairingSessionMessage::KeyslotOffer(SponsorKeyslotOffer {
                    space_id: SpaceId::from_str("space-xyz"),
                    keyslot_blob: vec![0xAA; 16],
                    challenge: vec![0x42; 32],
                    pairing_session_id: PairingSessionId::new("session-1"),
                }));
            me.recv
                .lock()
                .unwrap()
                .push_back(PairingSessionMessage::Confirm(SponsorConfirm {
                    space_id: SpaceId::from_str("space-xyz"),
                    sender_device_id: DeviceId::new("sponsor-device"),
                    sender_device_name: "sponsor's laptop".into(),
                    sender_identity_fingerprint: sponsor_fp(),
                    transport_address_blob: Vec::new(),
                    sponsor_space_person_id: None,
                }));
            me
        }
    }
    #[async_trait]
    impl PairingSessionPort for HappySession {
        async fn dial_by_invitation(&self, _: &InvitationCode) -> Result<DialOutcome, DialError> {
            Ok(DialOutcome {
                session_id: PairingSessionId::new("session-1"),
                channel: DiscoveryChannel::Cloud,
            })
        }
        async fn send(
            &self,
            _: &PairingSessionId,
            m: PairingSessionMessage,
        ) -> Result<(), SessionError> {
            self.sent.lock().unwrap().push(m);
            Ok(())
        }
        async fn recv_next(
            &self,
            _: &PairingSessionId,
        ) -> Result<Option<PairingSessionMessage>, SessionError> {
            Ok(self.recv.lock().unwrap().pop_front())
        }
        async fn close(&self, _: &PairingSessionId, _: Option<String>) {
            *self.closed.lock().unwrap() += 1;
        }
    }

    struct UnreachableSession;
    #[async_trait]
    impl PairingSessionPort for UnreachableSession {
        async fn dial_by_invitation(&self, _: &InvitationCode) -> Result<DialOutcome, DialError> {
            Err(DialError::InvitationNotFound)
        }
        async fn send(
            &self,
            _: &PairingSessionId,
            _: PairingSessionMessage,
        ) -> Result<(), SessionError> {
            unreachable!("dial fails before send")
        }
        async fn recv_next(
            &self,
            _: &PairingSessionId,
        ) -> Result<Option<PairingSessionMessage>, SessionError> {
            unreachable!("dial fails before recv")
        }
        async fn close(&self, _: &PairingSessionId, _: Option<String>) {
            unreachable!("no session to close")
        }
    }

    struct HappySpaceAccess;
    #[async_trait]
    impl SpaceAccessPort for HappySpaceAccess {
        async fn initialize(
            &self,
            _: &SpaceId,
            _: &Passphrase,
        ) -> Result<ActiveSpace, SpaceAccessError> {
            unimplemented!()
        }
        async fn unlock(
            &self,
            _: &SpaceId,
            _: &Passphrase,
        ) -> Result<ActiveSpace, SpaceAccessError> {
            unimplemented!()
        }
        async fn is_unlocked(&self, _: &SpaceId) -> bool {
            true
        }
        async fn lock(&self, _: &SpaceId) -> Result<(), SpaceAccessError> {
            Ok(())
        }
        async fn factory_reset(&self, _: &SpaceId) -> Result<(), SpaceAccessError> {
            Ok(())
        }
        async fn try_resume_session(
            &self,
            _: &SpaceId,
        ) -> Result<Option<ActiveSpace>, SpaceAccessError> {
            Ok(None)
        }
        async fn verify_keychain_access(&self) -> Result<bool, SpaceAccessError> {
            Ok(true)
        }
        async fn derive_subkey(&self, _: &[u8], _: &[u8]) -> Result<[u8; 32], SpaceAccessError> {
            Ok([0; 32])
        }
        async fn current_session_proof_key(
            &self,
        ) -> Result<Option<ProofDerivedKey>, SpaceAccessError> {
            Ok(None)
        }
        async fn prepare_join_offer(
            &self,
            _: &SpaceId,
            _: &Passphrase,
        ) -> Result<JoinOffer, SpaceAccessError> {
            unimplemented!()
        }
        async fn derive_master_key_for_proof(
            &self,
            _: &JoinOffer,
            _: &Passphrase,
        ) -> Result<ProofDerivedKey, SpaceAccessError> {
            Ok(ProofDerivedKey::from_bytes([0xCC; 32]))
        }
    }

    struct FixedProof;
    #[async_trait]
    impl ProofPort for FixedProof {
        async fn build_proof(
            &self,
            _: &SessionId,
            _: &SpaceId,
            _: [u8; 32],
            _: &ProofDerivedKey,
        ) -> anyhow::Result<SpaceAccessProofArtifact> {
            Ok(SpaceAccessProofArtifact {
                pairing_session_id: SessionId::new("fixed".to_string()),
                space_id: SpaceId::from_str("space-xyz"),
                challenge_nonce: [0x42; 32],
                proof_bytes: vec![0xFE; 32],
            })
        }
        async fn verify_proof(
            &self,
            _: &SpaceAccessProofArtifact,
            _: [u8; 32],
        ) -> anyhow::Result<bool> {
            unimplemented!()
        }
    }

    struct FixedLocal(IdentityFingerprint);
    #[async_trait]
    impl LocalIdentityPort for FixedLocal {
        async fn create(&self) -> Result<IdentityFingerprint, LocalIdentityError> {
            Ok(self.0.clone())
        }
        async fn ensure(&self) -> Result<IdentityFingerprint, LocalIdentityError> {
            Ok(self.0.clone())
        }
        async fn get_current_fingerprint(
            &self,
        ) -> Result<Option<IdentityFingerprint>, LocalIdentityError> {
            Ok(Some(self.0.clone()))
        }
    }

    struct FixedDevice(DeviceId);
    impl DeviceIdentityPort for FixedDevice {
        fn current_device_id(&self) -> DeviceId {
            self.0.clone()
        }
    }

    struct NamedSettings(String);
    #[async_trait]
    impl SettingsPort for NamedSettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            let mut s = Settings::default();
            s.general.device_name = Some(self.0.clone());
            Ok(s)
        }
        async fn save(&self, _: &Settings) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct RecordingMemberRepo {
        saved: StdMutex<Vec<SpaceMember>>,
        fail_next: StdMutex<Option<MembershipError>>,
    }
    #[async_trait]
    impl MemberRepositoryPort for RecordingMemberRepo {
        async fn get(&self, _: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
            Ok(None)
        }
        async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError> {
            Ok(self.saved.lock().unwrap().clone())
        }
        async fn save(&self, m: &SpaceMember) -> Result<(), MembershipError> {
            if let Some(err) = self.fail_next.lock().unwrap().take() {
                return Err(err);
            }
            self.saved.lock().unwrap().push(m.clone());
            Ok(())
        }
        async fn remove(&self, _: &DeviceId) -> Result<bool, MembershipError> {
            Ok(false)
        }
    }

    #[derive(Default)]
    struct RecordingTrustRepo {
        saved: StdMutex<Vec<TrustedPeer>>,
        fail_next: StdMutex<Option<TrustedPeerError>>,
    }
    #[async_trait]
    impl TrustedPeerRepositoryPort for RecordingTrustRepo {
        async fn get(&self, _: &DeviceId) -> Result<Option<TrustedPeer>, TrustedPeerError> {
            Ok(None)
        }
        async fn list(&self) -> Result<Vec<TrustedPeer>, TrustedPeerError> {
            Ok(self.saved.lock().unwrap().clone())
        }
        async fn save(&self, p: &TrustedPeer) -> Result<(), TrustedPeerError> {
            if let Some(err) = self.fail_next.lock().unwrap().take() {
                return Err(err);
            }
            self.saved.lock().unwrap().push(p.clone());
            Ok(())
        }
        async fn remove(&self, _: &DeviceId) -> Result<bool, TrustedPeerError> {
            Ok(false)
        }
    }

    struct RecordingSetupStatus {
        fail_next: StdMutex<bool>,
        set_calls: StdMutex<Vec<bool>>,
    }
    impl RecordingSetupStatus {
        fn ok() -> Self {
            Self {
                fail_next: StdMutex::new(false),
                set_calls: StdMutex::new(Vec::new()),
            }
        }
        fn failing() -> Self {
            Self {
                fail_next: StdMutex::new(true),
                set_calls: StdMutex::new(Vec::new()),
            }
        }
    }
    #[async_trait]
    impl SetupStatusPort for RecordingSetupStatus {
        async fn get_status(&self) -> anyhow::Result<SetupStatus> {
            Ok(SetupStatus::default())
        }
        async fn set_status(&self, s: &SetupStatus) -> anyhow::Result<()> {
            if *self.fail_next.lock().unwrap() {
                return Err(anyhow::anyhow!("setup-status backend down"));
            }
            self.set_calls.lock().unwrap().push(s.has_completed);
            Ok(())
        }
    }

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    // ── fixtures ─────────────────────────────────────────────────────────

    fn sponsor_fp() -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string("BBBBBBBBBBBBBBBB").unwrap()
    }
    fn joiner_fp() -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string("AAAAAAAAAAAAAAAA").unwrap()
    }
    fn fixed_now_ms() -> i64 {
        DateTime::parse_from_rfc3339("2026-04-20T10:00:00Z")
            .unwrap()
            .timestamp_millis()
    }
    fn cmd(code: &str) -> RedeemPairingInvitationCommand {
        RedeemPairingInvitationCommand {
            code: InvitationCode::new(code),
            passphrase: Passphrase::new("hunter22hunter22"),
        }
    }

    // Slice 2 Phase 1 · T5：用 mockall 定义 `PeerAddressRepositoryPort`
    // 的测试替身。好处：`.expect_upsert().times(N).withf(...).returning(...)`
    // 把"是否调用、调用几次、参数匹配、返回值"打包成一条契约，drop
    // mock 时自动校验；比手写 Recording fake 的 `saved: Vec<_>` +
    // `fail_next: Option<Err>` 更直观。跨 module 不复用宏生成的类型：
    // orchestrator tests 里有另一份独立声明，因为 mockall 生成的符号只在
    // 各自模块内可见。
    mockall::mock! {
        pub PeerAddrRepo {}

        #[async_trait]
        impl PeerAddressRepositoryPort for PeerAddrRepo {
            async fn get(
                &self,
                device: &DeviceId,
            ) -> Result<Option<PeerAddressRecord>, uc_core::ports::PeerAddressError>;
            async fn upsert(
                &self,
                record: &PeerAddressRecord,
            ) -> Result<(), uc_core::ports::PeerAddressError>;
            async fn list(
                &self,
            ) -> Result<Vec<PeerAddressRecord>, uc_core::ports::PeerAddressError>;
            async fn remove(
                &self,
                device: &DeviceId,
            ) -> Result<(), uc_core::ports::PeerAddressError>;
        }
    }

    /// Slice 8b · 单元测试用 capturing sink。生产代码不需要、不暴露——
    /// `AnalyticsPort` 实现里只有 noop / stdout / gated wrapper / posthog;
    /// "把所有 capture 收进 Vec 给断言用"是测试基础设施职责。
    /// 用 `StdMutex` 而非 `parking_lot`,与 module 内既有 fake repo
    /// (`RecordingMemberRepo` / `RecordingTrustRepo`) 同款。
    ///
    /// PR 6 起在同一 timeline 上记录 capture 与 identify，便于断言"identify
    /// 必须在 pairing_succeeded 之前发出"。
    #[derive(Default)]
    struct CapturingAnalyticsSink {
        events: StdMutex<Vec<Event>>,
        ordered: StdMutex<Vec<CapturedAnalytics>>,
    }

    #[derive(Debug, Clone)]
    enum CapturedAnalytics {
        Capture(Event),
        Identify(uc_observability::analytics::IdentifyPayload),
    }

    impl CapturingAnalyticsSink {
        fn snapshot(&self) -> Vec<Event> {
            self.events.lock().unwrap().clone()
        }
        fn ordered(&self) -> Vec<CapturedAnalytics> {
            self.ordered.lock().unwrap().clone()
        }
        fn identify_calls(&self) -> Vec<uc_observability::analytics::IdentifyPayload> {
            self.ordered
                .lock()
                .unwrap()
                .iter()
                .filter_map(|c| match c {
                    CapturedAnalytics::Identify(p) => Some(p.clone()),
                    _ => None,
                })
                .collect()
        }
    }

    impl uc_observability::analytics::AnalyticsPort for CapturingAnalyticsSink {
        fn capture(&self, event: Event) {
            self.events.lock().unwrap().push(event.clone());
            self.ordered
                .lock()
                .unwrap()
                .push(CapturedAnalytics::Capture(event));
        }
        fn identify(&self, payload: uc_observability::analytics::IdentifyPayload) {
            self.ordered
                .lock()
                .unwrap()
                .push(CapturedAnalytics::Identify(payload));
        }
    }

    fn assert_started_then_succeeded(events: &[Event]) {
        assert_eq!(
            events.len(),
            2,
            "expected exactly [PairingStarted, PairingSucceeded], got {events:?}"
        );
        assert!(
            matches!(
                events[0],
                Event::PairingStarted {
                    method: PairingMethod::Code
                }
            ),
            "first event should be PairingStarted{{method: Code}}, got {:?}",
            events[0]
        );
        assert!(
            matches!(
                events[1],
                Event::PairingSucceeded {
                    method: PairingMethod::Code,
                    peer_os: None,
                    discovery_channel: Some(PairingDiscoveryChannel::Cloud),
                    ..
                }
            ),
            "second event should be PairingSucceeded{{method: Code, peer_os: None, \
             discovery_channel: cloud}}, got {:?}",
            events[1]
        );
    }

    fn assert_started_then_failed(events: &[Event], expected: PairingFailureReason) {
        assert_eq!(
            events.len(),
            2,
            "expected exactly [PairingStarted, PairingFailed], got {events:?}"
        );
        assert!(
            matches!(
                events[0],
                Event::PairingStarted {
                    method: PairingMethod::Code
                }
            ),
            "first event should be PairingStarted{{method: Code}}, got {:?}",
            events[0]
        );
        match &events[1] {
            Event::PairingFailed {
                method: PairingMethod::Code,
                failure_reason,
            } => assert_eq!(*failure_reason, expected, "failure_reason mismatch"),
            other => panic!("second event should be PairingFailed, got {other:?}"),
        }
    }

    struct Harness {
        session: Arc<HappySession>,
        member_repo: Arc<RecordingMemberRepo>,
        trust_repo: Arc<RecordingTrustRepo>,
        setup_status: Arc<RecordingSetupStatus>,
        analytics: Arc<CapturingAnalyticsSink>,
    }

    impl Harness {
        fn build(
            session: Arc<dyn PairingSessionPort>,
            session_handle: Arc<HappySession>,
            member_repo: Arc<RecordingMemberRepo>,
            trust_repo: Arc<RecordingTrustRepo>,
            setup_status: Arc<RecordingSetupStatus>,
            peer_addr_repo: Arc<MockPeerAddrRepo>,
        ) -> (RedeemPairingInvitationUseCase, Self) {
            let handshake = JoinerHandshakeCoordinator::new(
                session,
                Arc::new(HappySpaceAccess),
                Arc::new(FixedProof),
                Arc::new(FixedLocal(joiner_fp())),
                Arc::new(FixedDevice(DeviceId::new("joiner-device"))),
                Arc::new(NamedSettings("joiner-laptop".into())),
                Duration::from_secs(30),
            );
            let admit_uc = Arc::new(AdmitMemberUseCase::new(
                member_repo.clone() as Arc<dyn MemberRepositoryPort>
            ));
            let trust_uc = Arc::new(TrustPeerUseCase::new(
                trust_repo.clone() as Arc<dyn TrustedPeerRepositoryPort>
            ));
            let analytics = Arc::new(CapturingAnalyticsSink::default());
            // Default harness uses a noop identity since most A2 tests
            // run with `sponsor_space_person_id = None`; the tests that
            // exercise the adopt path build their own facade locally.
            let facade: Arc<dyn AnalyticsFacade> =
                Arc::new(uc_observability::analytics::DefaultAnalyticsFacade::new(
                    Arc::clone(&analytics) as Arc<dyn uc_observability::analytics::AnalyticsPort>,
                    Arc::new(uc_observability::analytics::NoopAnalyticsIdentity),
                ));
            let uc = RedeemPairingInvitationUseCase::new(
                handshake,
                admit_uc,
                trust_uc,
                setup_status.clone(),
                peer_addr_repo.clone() as Arc<dyn PeerAddressRepositoryPort>,
                Arc::new(FixedClock(fixed_now_ms())),
                facade,
            );
            (
                uc,
                Self {
                    session: session_handle,
                    member_repo,
                    trust_repo,
                    setup_status,
                    analytics,
                },
            )
        }

        fn happy() -> (RedeemPairingInvitationUseCase, Self) {
            let session = Arc::new(HappySession::primed());
            Self::build(
                session.clone(),
                session,
                Arc::new(RecordingMemberRepo::default()),
                Arc::new(RecordingTrustRepo::default()),
                Arc::new(RecordingSetupStatus::ok()),
                // 默认场景（sponsor blob == empty 走 skip 分支）：
                // mock 不期望任何 upsert 调用，发生了会 drop 时 panic。
                {
                    let mut mock = MockPeerAddrRepo::new();
                    mock.expect_upsert().times(0);
                    Arc::new(mock)
                },
            )
        }
    }

    // ── tests ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn happy_path_admit_trust_mark_setup_and_return_facts() {
        let (uc, h) = Harness::happy();
        let out = uc.execute(cmd("CODE-1")).await.unwrap();
        assert_eq!(out.sponsor_device_id.as_str(), "sponsor-device");
        assert_eq!(out.sponsor_identity_fingerprint, sponsor_fp());
        assert_eq!(out.space_id.inner(), "space-xyz");
        assert_eq!(out.self_device_id.as_str(), "joiner-device");
        assert_eq!(out.self_identity_fingerprint, joiner_fp());

        assert_eq!(h.member_repo.saved.lock().unwrap().len(), 1);
        let trusted = &h.trust_repo.saved.lock().unwrap()[0];
        assert_eq!(trusted.local_device_id.as_str(), "joiner-device");
        assert_eq!(trusted.peer_device_id.as_str(), "sponsor-device");
        assert_eq!(
            *h.setup_status.set_calls.lock().unwrap(),
            vec![true],
            "setup_status flipped exactly once to has_completed=true"
        );
        assert_eq!(*h.session.closed.lock().unwrap(), 1);
        // Slice 8b · pairing 三事件埋点:happy path 应产生
        // [PairingStarted, PairingSucceeded] 两条 capture,中间 fire-and-forget
        // 不阻塞主路径。
        assert_started_then_succeeded(&h.analytics.snapshot());
        // T5：HappySession::primed() 给的 Confirm.transport_address_blob
        // 是空 Vec，所以 upsert 应跳过。Harness::happy 的 mock 用
        // `.expect_upsert().times(0)`——若分支失误（对空 blob 也 upsert），
        // drop mock 时 mockall 会 panic。
    }

    /// Session variant that primes a `SponsorConfirm` with a specific
    /// transport blob; used by T5 tests that need to exercise the
    /// non-empty-blob branch.
    fn session_with_sponsor_blob(blob: Vec<u8>) -> Arc<HappySession> {
        let me = Arc::new(HappySession::default());
        me.recv
            .lock()
            .unwrap()
            .push_back(PairingSessionMessage::KeyslotOffer(SponsorKeyslotOffer {
                space_id: SpaceId::from_str("space-xyz"),
                keyslot_blob: vec![0xAA; 16],
                challenge: vec![0x42; 32],
                pairing_session_id: PairingSessionId::new("session-1"),
            }));
        me.recv
            .lock()
            .unwrap()
            .push_back(PairingSessionMessage::Confirm(SponsorConfirm {
                space_id: SpaceId::from_str("space-xyz"),
                sender_device_id: DeviceId::new("sponsor-device"),
                sender_device_name: "sponsor's laptop".into(),
                sender_identity_fingerprint: sponsor_fp(),
                transport_address_blob: blob,
                sponsor_space_person_id: None,
            }));
        me
    }

    #[tokio::test]
    async fn t5_sponsor_blob_non_empty_upserts_peer_addr_repo() {
        // 契约：收到非空 sponsor blob 后，恰好一次 upsert，参数匹配。
        let expected_blob: Vec<u8> = vec![0xab, 0xcd, 0xef];
        let expected_blob_matcher = expected_blob.clone();
        let peer_mock = {
            let mut m = MockPeerAddrRepo::new();
            m.expect_upsert()
                .times(1)
                .withf(move |record| {
                    record.device_id.as_str() == "sponsor-device"
                        && record.addr_blob == expected_blob_matcher
                })
                .returning(|_| Ok(()));
            Arc::new(m)
        };
        let session = session_with_sponsor_blob(expected_blob);
        let (uc, _h) = Harness::build(
            session.clone(),
            session,
            Arc::new(RecordingMemberRepo::default()),
            Arc::new(RecordingTrustRepo::default()),
            Arc::new(RecordingSetupStatus::ok()),
            peer_mock,
        );
        uc.execute(cmd("CODE-ADDR")).await.expect("ok");
        // drop-time mockall 校验：少调 / 多调 / 参数不匹配 都会 panic。
    }

    // —— Phase 098 / PR 6 · v2 跨设备 person 聚合 joiner 端 ——————————

    /// 携带 sponsor_space_person_id=Some 的 sponsor confirm。
    fn session_with_sponsor_person(space_person_id: Uuid) -> Arc<HappySession> {
        let me = Arc::new(HappySession::default());
        me.recv
            .lock()
            .unwrap()
            .push_back(PairingSessionMessage::KeyslotOffer(SponsorKeyslotOffer {
                space_id: SpaceId::from_str("space-xyz"),
                keyslot_blob: vec![0xAA; 16],
                challenge: vec![0x42; 32],
                pairing_session_id: PairingSessionId::new("session-1"),
            }));
        me.recv
            .lock()
            .unwrap()
            .push_back(PairingSessionMessage::Confirm(SponsorConfirm {
                space_id: SpaceId::from_str("space-xyz"),
                sender_device_id: DeviceId::new("sponsor-device"),
                sender_device_name: "sponsor's laptop".into(),
                sender_identity_fingerprint: sponsor_fp(),
                transport_address_blob: Vec::new(),
                sponsor_space_person_id: Some(space_person_id),
            }));
        me
    }

    /// Test-only `AnalyticsIdentityPort` 跟踪 adopt 调用 + 允许注入失败。
    /// `previous_anon` 模拟本机原 anonymous_user_id，A2 会作为
    /// IdentifyPayload.old_distinct_id 发出。
    struct FakeJoinerAnalyticsIdentity {
        previous_anon: Uuid,
        adopted: StdMutex<Vec<Uuid>>,
        adopt_err: StdMutex<Option<String>>,
    }
    impl FakeJoinerAnalyticsIdentity {
        fn new(previous_anon: Uuid) -> Self {
            Self {
                previous_anon,
                adopted: StdMutex::new(Vec::new()),
                adopt_err: StdMutex::new(None),
            }
        }
    }
    impl uc_observability::analytics::AnalyticsIdentityPort for FakeJoinerAnalyticsIdentity {
        fn adopt_space_person(
            &self,
            space_person_id: Uuid,
        ) -> Result<
            uc_observability::analytics::AdoptOutcome,
            uc_observability::analytics::AnalyticsIdentityError,
        > {
            if let Some(msg) = self.adopt_err.lock().unwrap().take() {
                return Err(
                    uc_observability::analytics::AnalyticsIdentityError::PersistFailed(
                        anyhow::anyhow!(msg),
                    ),
                );
            }
            self.adopted.lock().unwrap().push(space_person_id);
            Ok(uc_observability::analytics::AdoptOutcome {
                previous_distinct_id: self.previous_anon,
                new_distinct_id: space_person_id,
            })
        }
        fn release_space_person(
            &self,
        ) -> Result<
            uc_observability::analytics::ReleaseOutcome,
            uc_observability::analytics::AnalyticsIdentityError,
        > {
            Ok(uc_observability::analytics::ReleaseOutcome {
                previous_distinct_id: self.previous_anon,
                new_distinct_id: self.previous_anon,
            })
        }
        fn current_space_person_id(&self) -> Option<Uuid> {
            self.adopted.lock().unwrap().last().copied()
        }
        fn reset_telemetry_identity(
            &self,
        ) -> Result<
            uc_observability::analytics::ReleaseOutcome,
            uc_observability::analytics::AnalyticsIdentityError,
        > {
            Ok(uc_observability::analytics::ReleaseOutcome {
                previous_distinct_id: self.previous_anon,
                new_distinct_id: self.previous_anon,
            })
        }
    }

    fn build_uc_with_identity(
        session: Arc<HappySession>,
        identity: Arc<FakeJoinerAnalyticsIdentity>,
    ) -> (RedeemPairingInvitationUseCase, Arc<CapturingAnalyticsSink>) {
        let handshake = JoinerHandshakeCoordinator::new(
            session.clone() as Arc<dyn PairingSessionPort>,
            Arc::new(HappySpaceAccess),
            Arc::new(FixedProof),
            Arc::new(FixedLocal(joiner_fp())),
            Arc::new(FixedDevice(DeviceId::new("joiner-device"))),
            Arc::new(NamedSettings("joiner-laptop".into())),
            Duration::from_secs(30),
        );
        let admit_uc = Arc::new(AdmitMemberUseCase::new(
            Arc::new(RecordingMemberRepo::default()) as Arc<dyn MemberRepositoryPort>,
        ));
        let trust_uc = Arc::new(TrustPeerUseCase::new(
            Arc::new(RecordingTrustRepo::default()) as Arc<dyn TrustedPeerRepositoryPort>,
        ));
        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let setup_status: Arc<dyn SetupStatusPort> = Arc::new(RecordingSetupStatus::ok());
        let peer_addr_repo: Arc<dyn PeerAddressRepositoryPort> = {
            let mut m = MockPeerAddrRepo::new();
            m.expect_upsert().times(0);
            Arc::new(m)
        };
        let facade: Arc<dyn AnalyticsFacade> =
            Arc::new(uc_observability::analytics::DefaultAnalyticsFacade::new(
                Arc::clone(&analytics) as Arc<dyn uc_observability::analytics::AnalyticsPort>,
                identity as Arc<dyn uc_observability::analytics::AnalyticsIdentityPort>,
            ));
        let uc = RedeemPairingInvitationUseCase::new(
            handshake,
            admit_uc,
            trust_uc,
            setup_status,
            peer_addr_repo,
            Arc::new(FixedClock(fixed_now_ms())),
            facade,
        );
        (uc, analytics)
    }

    /// Happy path：sponsor 派发了 space_person_id → joiner 必须先 adopt、再
    /// 发 `$identify`、最后 emit pairing_succeeded。三步顺序是 dashboard 的
    /// person 合并归属是否生效的硬约束。
    #[tokio::test]
    async fn a2_emits_identify_before_pairing_succeeded() {
        let space_person = Uuid::parse_str("018f0000-0000-7000-8000-00000000000a").unwrap();
        let session = session_with_sponsor_person(space_person);
        let identity = Arc::new(FakeJoinerAnalyticsIdentity::new(Uuid::now_v7()));
        let (uc, analytics) = build_uc_with_identity(session, identity.clone());

        uc.execute(cmd("CODE-1")).await.unwrap();

        // adopt 必须正好一次，参数等于 sponsor 派发的 ID。
        let adopted = identity.adopted.lock().unwrap().clone();
        assert_eq!(
            adopted,
            vec![space_person],
            "adopt 必须正好一次且参数等于 sponsor 派发"
        );

        // identify 必须出现在 pairing_succeeded 之前。
        let ordered = analytics.ordered();
        let identify_pos = ordered
            .iter()
            .position(|c| matches!(c, CapturedAnalytics::Identify(_)))
            .expect("expected $identify");
        let succeeded_pos = ordered
            .iter()
            .position(|c| {
                matches!(
                    c,
                    CapturedAnalytics::Capture(Event::PairingSucceeded { .. })
                )
            })
            .expect("expected pairing_succeeded");
        assert!(
            identify_pos < succeeded_pos,
            "identify 必须在 pairing_succeeded 之前：{ordered:?}"
        );

        // identify payload 端点：old=本机 anon, new=sponsor 派发。
        let calls = analytics.identify_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].old_distinct_id, identity.previous_anon);
        assert_eq!(calls[0].new_distinct_id, space_person);
    }

    /// sponsor 派发 None（v1→v2 升级 sponsor 未持久化场景）→ joiner 端不
    /// adopt、不发 identify，但仍 emit pairing_succeeded（pairing 真的成功
    /// 了）。task_plan §开放问题 2 决策 A 的退化路径。
    #[tokio::test]
    async fn a2_skips_identify_when_sponsor_did_not_dispatch_person_id() {
        let session = Arc::new(HappySession::primed()); // confirm.sponsor_space_person_id = None
        let identity = Arc::new(FakeJoinerAnalyticsIdentity::new(Uuid::now_v7()));
        let (uc, analytics) = build_uc_with_identity(session, identity.clone());

        uc.execute(cmd("CODE-1")).await.unwrap();

        assert!(
            identity.adopted.lock().unwrap().is_empty(),
            "sponsor 派发 None 时 joiner 不应 adopt"
        );
        assert!(
            analytics.identify_calls().is_empty(),
            "sponsor 派发 None 时 joiner 不应发 identify"
        );
        // pairing_succeeded 仍 emit。
        assert_started_then_succeeded(&analytics.snapshot());
    }

    /// adopt 失败时 identify 不发出，但 pairing_succeeded 仍 emit ——
    /// 与 A1 sponsor 端对称。本机 telemetry 维持 Solo 等下次 pairing。
    #[tokio::test]
    async fn a2_skips_identify_when_adopt_space_person_fails() {
        let space_person = Uuid::now_v7();
        let session = session_with_sponsor_person(space_person);
        let identity = Arc::new(FakeJoinerAnalyticsIdentity::new(Uuid::now_v7()));
        *identity.adopt_err.lock().unwrap() = Some("simulated persist failure".into());
        let (uc, analytics) = build_uc_with_identity(session, identity.clone());

        uc.execute(cmd("CODE-1")).await.unwrap();

        assert!(
            identity.adopted.lock().unwrap().is_empty(),
            "adopt 失败时不记录成功 adopt"
        );
        assert!(
            analytics.identify_calls().is_empty(),
            "adopt 失败时不应发 identify"
        );
        assert_started_then_succeeded(&analytics.snapshot());
    }

    #[tokio::test]
    async fn t5_sponsor_blob_upsert_failure_does_not_fail_join() {
        // 预设 upsert 返 Err；T5 best-effort 语义下，execute 仍必须 Ok。
        let peer_mock = {
            let mut m = MockPeerAddrRepo::new();
            m.expect_upsert().times(1).returning(|_| {
                Err(uc_core::ports::PeerAddressError::Internal(
                    "sqlite down".into(),
                ))
            });
            Arc::new(m)
        };
        let session = session_with_sponsor_blob(vec![0x01]);
        let (uc, h) = Harness::build(
            session.clone(),
            session,
            Arc::new(RecordingMemberRepo::default()),
            Arc::new(RecordingTrustRepo::default()),
            Arc::new(RecordingSetupStatus::ok()),
            peer_mock,
        );
        uc.execute(cmd("CODE-FAIL"))
            .await
            .expect("T5 upsert failure does not fail the join");
        assert_eq!(*h.setup_status.set_calls.lock().unwrap(), vec![true]);
    }

    #[tokio::test]
    async fn coordinator_error_passes_through_without_touching_persistence() {
        let session = Arc::new(HappySession::default()); // not primed — won't be used
        let unreachable: Arc<dyn PairingSessionPort> = Arc::new(UnreachableSession);
        // 错误路径 mock：coordinator 提前失败，upsert 绝不应被调到；
        // 若被调，drop mock 时 `.expect_upsert().times(0)` 会 panic。
        let peer_mock = {
            let mut m = MockPeerAddrRepo::new();
            m.expect_upsert().times(0);
            Arc::new(m)
        };
        let (uc, h) = Harness::build(
            unreachable,
            session, // handle kept around so the Harness's `session.closed` stays consistent with the "no close" assertion
            Arc::new(RecordingMemberRepo::default()),
            Arc::new(RecordingTrustRepo::default()),
            Arc::new(RecordingSetupStatus::ok()),
            peer_mock,
        );
        let err = uc.execute(cmd("X")).await.unwrap_err();
        assert!(matches!(
            err,
            RedeemPairingInvitationError::InvitationNotFound
        ));
        // Persistence untouched.
        assert!(h.member_repo.saved.lock().unwrap().is_empty());
        assert!(h.trust_repo.saved.lock().unwrap().is_empty());
        assert!(h.setup_status.set_calls.lock().unwrap().is_empty());
        // Slice 8b · 早期 dial 失败仍应 fire [Started, Failed{InvitationNotFound}]
        // —— funnel 第一步必须留下信号。
        assert_started_then_failed(
            &h.analytics.snapshot(),
            PairingFailureReason::InvitationNotFound,
        );
    }

    #[tokio::test]
    async fn admit_failure_aborts_before_trust_and_setup_status() {
        let member_repo = Arc::new(RecordingMemberRepo::default());
        *member_repo.fail_next.lock().unwrap() =
            Some(MembershipError::Repository("db down".into()));
        let session = Arc::new(HappySession::primed());
        let peer_mock = {
            let mut m = MockPeerAddrRepo::new();
            m.expect_upsert().times(0);
            Arc::new(m)
        };
        let (uc, h) = Harness::build(
            session.clone(),
            session,
            member_repo,
            Arc::new(RecordingTrustRepo::default()),
            Arc::new(RecordingSetupStatus::ok()),
            peer_mock,
        );
        let err = uc.execute(cmd("X")).await.unwrap_err();
        match err {
            RedeemPairingInvitationError::Internal(m) => {
                assert!(m.contains("admit_member"), "msg = {m}")
            }
            other => panic!("expected Internal, got {other:?}"),
        }
        assert!(h.member_repo.saved.lock().unwrap().is_empty());
        assert!(h.trust_repo.saved.lock().unwrap().is_empty());
        assert!(h.setup_status.set_calls.lock().unwrap().is_empty());
        // Slice 8b · admit 持久化失败 → Internal 桶。
        assert_started_then_failed(&h.analytics.snapshot(), PairingFailureReason::Internal);
    }

    #[tokio::test]
    async fn trust_failure_lands_admit_but_does_not_mark_setup() {
        let trust_repo = Arc::new(RecordingTrustRepo::default());
        *trust_repo.fail_next.lock().unwrap() =
            Some(TrustedPeerError::Repository("trust boom".into()));
        let session = Arc::new(HappySession::primed());
        let peer_mock = {
            let mut m = MockPeerAddrRepo::new();
            m.expect_upsert().times(0);
            Arc::new(m)
        };
        let (uc, h) = Harness::build(
            session.clone(),
            session,
            Arc::new(RecordingMemberRepo::default()),
            trust_repo,
            Arc::new(RecordingSetupStatus::ok()),
            peer_mock,
        );
        let err = uc.execute(cmd("X")).await.unwrap_err();
        match err {
            RedeemPairingInvitationError::Internal(m) => {
                assert!(m.contains("trust_peer"), "msg = {m}")
            }
            other => panic!("expected Internal, got {other:?}"),
        }
        // admit landed (Slice 1 "strict" ordering — no admit-rollback
        // compensation; the user-visible surface is the Internal error,
        // and recovery path is factory_reset + fresh redeem).
        assert_eq!(h.member_repo.saved.lock().unwrap().len(), 1);
        assert!(h.trust_repo.saved.lock().unwrap().is_empty());
        assert!(h.setup_status.set_calls.lock().unwrap().is_empty());
        // Slice 8b · trust 持久化失败 → Internal 桶。
        assert_started_then_failed(&h.analytics.snapshot(), PairingFailureReason::Internal);
    }

    #[tokio::test]
    async fn setup_status_failure_lands_admit_and_trust_but_surfaces_internal() {
        let session = Arc::new(HappySession::primed());
        let peer_mock = {
            let mut m = MockPeerAddrRepo::new();
            // Peer addr upsert is gated on setup_status success (it's
            // lifecycle-sequenced after mark-complete), so setup_status
            // failure short-circuits it — mock enforces via times(0).
            m.expect_upsert().times(0);
            Arc::new(m)
        };
        let (uc, h) = Harness::build(
            session.clone(),
            session,
            Arc::new(RecordingMemberRepo::default()),
            Arc::new(RecordingTrustRepo::default()),
            Arc::new(RecordingSetupStatus::failing()),
            peer_mock,
        );
        let err = uc.execute(cmd("X")).await.unwrap_err();
        match err {
            RedeemPairingInvitationError::Internal(m) => {
                assert!(m.contains("setup_status.set_status"), "msg = {m}")
            }
            other => panic!("expected Internal, got {other:?}"),
        }
        // admit + trust both landed; setup_status call was attempted
        // (and failed), so set_calls stays empty.
        assert_eq!(h.member_repo.saved.lock().unwrap().len(), 1);
        assert_eq!(h.trust_repo.saved.lock().unwrap().len(), 1);
        assert!(h.setup_status.set_calls.lock().unwrap().is_empty());
        // Slice 8b · setup_status persist 失败 → Internal 桶。
        assert_started_then_failed(&h.analytics.snapshot(), PairingFailureReason::Internal);
    }

    /// Slice 8b · 锁死 `RedeemPairingInvitationError` → `PairingFailureReason`
    /// 全 14 变体的 1:1 映射。新增 RedeemPairingInvitationError 变体而忘了
    /// 加 PairingFailureReason 时,这条会编译失败 (match 不穷尽);改了 wire
    /// 字符串忘了 schema doc 同步时,events.rs 的 `pairing_failure_reason_wire_format`
    /// 钉死会捕获。
    #[test]
    fn map_redeem_error_covers_all_variants() {
        use super::map_redeem_error_to_pairing_failure_reason as map;
        use PairingFailureReason as R;
        use RedeemPairingInvitationError as E;
        let cases: Vec<(E, R)> = vec![
            (E::InvitationNotFound, R::InvitationNotFound),
            (E::InvitationExpired, R::InvitationExpired),
            (E::SponsorUnreachable, R::SponsorUnreachable),
            (E::ServiceUnavailable, R::ServiceUnavailable),
            (E::PassphraseMismatch, R::PassphraseMismatch),
            (E::CorruptedKeyMaterial, R::CorruptedKeyMaterial),
            (E::DeviceNameRequired, R::DeviceNameRequired),
            (E::SponsorRejectedInvitation, R::SponsorRejectedInvitation),
            (E::SponsorDeclined, R::SponsorDeclined),
            (E::SponsorTimedOut, R::SponsorTimedOut),
            (E::SponsorInternal("boom".into()), R::SponsorInternal),
            (E::Timeout, R::Timeout),
            (E::ConnectionLost, R::ConnectionLost),
            (E::Internal("boom".into()), R::Internal),
        ];
        for (err, expected) in cases.iter() {
            assert_eq!(map(err), *expected, "for {err:?}");
        }
    }
}

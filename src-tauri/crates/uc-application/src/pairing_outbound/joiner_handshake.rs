//! Joiner-side handshake coordinator.
//!
//! Owns the transport + crypto half of the joiner-side pairing flow:
//! dial → send Request → recv KeyslotOffer → derive master key → build
//! proof → send ChallengeResponse → recv Confirm. Returns a structured
//! [`JoinerHandshakeOutcome`] with the sponsor's identity facts. Does
//! **not** touch persistence (`SpaceMember` / `TrustedPeer` /
//! `SetupStatus`) — that's composed in the outer
//! [`RedeemPairingInvitationUseCase`].
//!
//! Symmetric to [`crate::pairing_inbound::sponsor_handshake::
//! SponsorHandshakeCoordinator`]:
//!
//! | concern                 | sponsor                    | joiner                      |
//! |-------------------------|----------------------------|-----------------------------|
//! | coordinator owns        | wire + `verify_proof`      | wire + `derive + build_proof` |
//! | stateful across events  | yes (parked `SessionCtx`)  | no (single-shot `handshake`) |
//! | TTL                     | spawned watchdog (P7g)     | per-`recv` `tokio::timeout` |
//! | close                   | coordinator drives         | coordinator drives          |
//! | persistence             | done by orchestrator       | done by use case            |
//!
//! ## Why this split exists
//!
//! Prior P7h landed everything in one 11-arg use case. The break from
//! sponsor-side shape made the use case a code smell (it did dial +
//! identity assembly + crypto + recv/decode + admit + trust +
//! setup-status). Extracting the coordinator brings joiner back in line
//! with sponsor architecture and drops the use case to 5 deps.
//!
//! ## Error type
//!
//! The coordinator returns [`RedeemPairingInvitationError`] directly
//! rather than a private enum: its variants 1-to-1 map user-facing
//! joiner failures, and the outer use case has no additional variants
//! to introduce at the seam. A private enum + map layer would be
//! duplication with zero signal gain.
//!
//! [`RedeemPairingInvitationError`]:
//!     crate::facade::space_setup::RedeemPairingInvitationError

use std::sync::Arc;

use tokio::time::{timeout, Duration};
use tracing::{debug, info, instrument, warn};

use uc_core::crypto::domain::Passphrase;
use uc_core::ids::{DeviceId, SessionId, SpaceId};
use uc_core::pairing::invitation::InvitationCode;
use uc_core::pairing::session_message::{
    JoinerChallengeResponse, JoinerRequest, PairingRejectReason, PairingSessionMessage,
};
use uc_core::ports::pairing::{DialError, PairingSessionId, PairingSessionPort, SessionError};
use uc_core::ports::space::{ProofPort, SpaceAccessError, SpaceAccessPort};
use uc_core::ports::{DeviceIdentityPort, LocalIdentityPort, SettingsPort};
use uc_core::security::IdentityFingerprint;
use uc_core::space_access::JoinOffer;

use crate::facade::space_setup::RedeemPairingInvitationError;

/// Facts handed to the use case after a successful joiner-side handshake.
///
/// Shaped for the subsequent persistence step (`admit` + `trust`) plus
/// the UI confirmation surface (`self_*` fields) the use case returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinerHandshakeOutcome {
    pub sponsor_device_id: DeviceId,
    pub sponsor_device_name: String,
    pub sponsor_identity_fingerprint: IdentityFingerprint,
    pub space_id: SpaceId,
    pub self_device_id: DeviceId,
    pub self_identity_fingerprint: IdentityFingerprint,
    /// Slice 2 Phase 1 · T5：sponsor 从 `SponsorConfirm.transport_address_blob`
    /// 带来的不透明传输地址字节，由 outer use case best-effort upsert 到
    /// `PeerAddressRepositoryPort`。空 `Vec` 表示 sponsor 未附带地址，
    /// joiner 侧应跳过 upsert。
    pub sponsor_transport_address_blob: Vec<u8>,
    /// Phase 098：sponsor 派发的 telemetry person 标识。
    ///
    /// `Some(uuid)`：joiner 端 use case 在 `pairing_succeeded` 之前调
    /// `analytics_identity.adopt_space_person(uuid)` 接受这个 ID 并发
    /// `$identify`，让本机 telemetry 与 sponsor 聚合为同一 person。
    ///
    /// `None`：sponsor 端尚未确立 telemetry 身份（v1→v2 升级未配对场景）。
    /// joiner 端按 Solo 退化，等待下次 sponsor 自己发新设备 pairing 时再
    /// 通过 sponsor 派发统一切换（task_plan §开放问题 2 决策 A）。
    pub sponsor_space_person_id: Option<uuid::Uuid>,
}

pub(crate) struct JoinerHandshakeCoordinator {
    pairing_session: Arc<dyn PairingSessionPort>,
    space_access: Arc<dyn SpaceAccessPort>,
    proof_port: Arc<dyn ProofPort>,
    local_identity: Arc<dyn LocalIdentityPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    settings: Arc<dyn SettingsPort>,
    /// Per-`recv` TTL — not end-to-end handshake TTL. Independent of
    /// P7g's sponsor-side watchdog: this timer protects against silent
    /// sponsor, the sponsor's protects against silent joiner.
    handshake_ttl: Duration,
}

impl JoinerHandshakeCoordinator {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        pairing_session: Arc<dyn PairingSessionPort>,
        space_access: Arc<dyn SpaceAccessPort>,
        proof_port: Arc<dyn ProofPort>,
        local_identity: Arc<dyn LocalIdentityPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        settings: Arc<dyn SettingsPort>,
        handshake_ttl: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            pairing_session,
            space_access,
            proof_port,
            local_identity,
            device_identity,
            settings,
            handshake_ttl,
        })
    }

    /// Run the full wire + crypto flow. On success the session has
    /// been closed cleanly and the outcome is ready for the outer use
    /// case to persist. On failure the session is also closed (so the
    /// adapter releases its slot) and the error is surfaced.
    #[instrument(skip_all, fields(code = %code.as_str()))]
    pub(crate) async fn handshake(
        &self,
        code: &InvitationCode,
        passphrase: &Passphrase,
    ) -> Result<JoinerHandshakeOutcome, RedeemPairingInvitationError> {
        // Dial first so NotFound / Expired / Unreachable short-circuits
        // without spending any crypto work. A successful dial creates
        // a session in the adapter that we must close on every exit
        // path below (including error paths).
        let session = self
            .pairing_session
            .dial_by_invitation(code)
            .await
            .map_err(map_dial_err)?;
        info!(session = %session, "pairing session dialled");

        match self.drive(&session, code, passphrase).await {
            Ok(outcome) => {
                self.pairing_session
                    .close(&session, Some("handshake completed".into()))
                    .await;
                Ok(outcome)
            }
            Err(err) => {
                self.pairing_session
                    .close(&session, Some(format!("handshake aborted: {err}")))
                    .await;
                Err(err)
            }
        }
    }

    async fn drive(
        &self,
        session: &PairingSessionId,
        code: &InvitationCode,
        passphrase: &Passphrase,
    ) -> Result<JoinerHandshakeOutcome, RedeemPairingInvitationError> {
        // ── 1. Collect local facts ───────────────────────────────────────
        let local_fp = self.local_identity.ensure().await.map_err(|e| {
            RedeemPairingInvitationError::Internal(format!("local_identity.ensure: {e}"))
        })?;
        let local_device_id = self.device_identity.current_device_id();
        let local_device_name = self
            .settings
            .load()
            .await
            .map_err(|e| RedeemPairingInvitationError::Internal(format!("settings.load: {e}")))?
            .general
            .device_name
            .filter(|n| !n.trim().is_empty())
            .ok_or(RedeemPairingInvitationError::DeviceNameRequired)?;
        info!(
            session = %session,
            local_device_id = %local_device_id.as_str(),
            "joiner pairing local facts loaded"
        );

        // ── 2. Send JoinerRequest ────────────────────────────────────────
        // Slice 2 Phase 1 · T5：adapter 暴露本机传输地址 blob；`None`
        // 兜底为空 Vec —— sponsor 收到空 blob 就跳过 peer address upsert，
        // 不阻塞配对本身。
        let transport_address_blob = self
            .pairing_session
            .local_transport_address_blob()
            .await
            .unwrap_or_default();
        let transport_address_blob_len = transport_address_blob.len();
        let request = JoinerRequest {
            invitation_code: code.clone(),
            device_id: local_device_id.clone(),
            device_name: local_device_name,
            identity_fingerprint: local_fp.clone(),
            // 保留字段：Slice 1 sponsor 不消费 transcript nonce，留空占位
            // 即可；加 rand crate 不值当。未来 slice 若把 transcript
            // binding 纳入 HMAC，再在这里填。
            nonce: Vec::new(),
            transport_address_blob,
        };
        self.pairing_session
            .send(session, PairingSessionMessage::Request(request))
            .await
            .map_err(map_session_err)?;
        info!(
            session = %session,
            code = %code.as_str(),
            transport_address_blob_len,
            "JoinerRequest sent; awaiting KeyslotOffer"
        );

        // ── 3. Await KeyslotOffer | Reject ───────────────────────────────
        let offer = match self.recv_with_ttl(session).await? {
            PairingSessionMessage::KeyslotOffer(o) => o,
            PairingSessionMessage::Reject(r) => {
                warn!(
                    session = %session,
                    reason = ?r.reason,
                    "sponsor rejected before KeyslotOffer"
                );
                return Err(map_sponsor_reject(r.reason));
            }
            other => {
                return Err(RedeemPairingInvitationError::Internal(format!(
                    "expected KeyslotOffer, got {}",
                    variant_name(&other),
                )));
            }
        };
        debug!(
            session = %session,
            space_id = %offer.space_id,
            "KeyslotOffer received"
        );

        // ── 4. Derive proof key (side effect: persists local keyslot) ───
        let challenge_nonce = challenge_to_array(&offer.challenge)?;
        let join_offer = JoinOffer {
            space_id: offer.space_id.clone(),
            keyslot_blob: offer.keyslot_blob.clone(),
            challenge_nonce,
        };
        let derived_key = self
            .space_access
            .derive_master_key_for_proof(&join_offer, passphrase)
            .await
            .map_err(map_space_access_err)?;
        debug!(session = %session, "master key derived from sponsor offer");

        // ── 5. Build HMAC proof ──────────────────────────────────────────
        let core_session = SessionId::new(offer.pairing_session_id.as_str().to_string());
        let proof = self
            .proof_port
            .build_proof(
                &core_session,
                &join_offer.space_id,
                challenge_nonce,
                &derived_key,
            )
            .await
            .map_err(|e| RedeemPairingInvitationError::Internal(format!("build_proof: {e}")))?;

        // ── 6. Send ChallengeResponse ────────────────────────────────────
        self.pairing_session
            .send(
                session,
                PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
                    encrypted_challenge: proof.proof_bytes,
                }),
            )
            .await
            .map_err(map_session_err)?;
        info!(session = %session, "ChallengeResponse sent; awaiting Confirm/Reject");

        // ── 7. Await Confirm | Reject ────────────────────────────────────
        let confirm = match self.recv_with_ttl(session).await? {
            PairingSessionMessage::Confirm(c) => c,
            PairingSessionMessage::Reject(r) => {
                warn!(
                    session = %session,
                    reason = ?r.reason,
                    "sponsor rejected before Confirm"
                );
                return Err(map_sponsor_reject(r.reason));
            }
            other => {
                return Err(RedeemPairingInvitationError::Internal(format!(
                    "expected Confirm, got {}",
                    variant_name(&other),
                )));
            }
        };
        info!(
            session = %session,
            sponsor_device_id = %confirm.sender_device_id.as_str(),
            space_id = %confirm.space_id,
            "Confirm received; handshake surface complete"
        );

        Ok(JoinerHandshakeOutcome {
            sponsor_device_id: confirm.sender_device_id,
            sponsor_device_name: confirm.sender_device_name,
            sponsor_identity_fingerprint: confirm.sender_identity_fingerprint,
            space_id: confirm.space_id,
            self_device_id: local_device_id,
            self_identity_fingerprint: local_fp,
            sponsor_transport_address_blob: confirm.transport_address_blob,
            sponsor_space_person_id: confirm.sponsor_space_person_id,
        })
    }

    async fn recv_with_ttl(
        &self,
        session: &PairingSessionId,
    ) -> Result<PairingSessionMessage, RedeemPairingInvitationError> {
        match timeout(self.handshake_ttl, self.pairing_session.recv_next(session)).await {
            Err(_elapsed) => {
                warn!(
                    session = %session,
                    ttl_ms = %self.handshake_ttl.as_millis(),
                    "recv_with_ttl exceeded; aborting handshake"
                );
                Err(RedeemPairingInvitationError::Timeout)
            }
            Ok(Ok(Some(msg))) => {
                info!(
                    session = %session,
                    message_kind = variant_name(&msg),
                    "joiner pairing message received"
                );
                Ok(msg)
            }
            Ok(Ok(None)) => {
                warn!(session = %session, "joiner pairing session closed by sponsor");
                Err(RedeemPairingInvitationError::ConnectionLost)
            }
            Ok(Err(err)) => {
                warn!(
                    session = %session,
                    error = %err,
                    "joiner pairing recv failed"
                );
                Err(map_session_err(err))
            }
        }
    }
}

fn challenge_to_array(bytes: &[u8]) -> Result<[u8; 32], RedeemPairingInvitationError> {
    bytes.try_into().map_err(|_| {
        RedeemPairingInvitationError::Internal(format!(
            "challenge nonce wire length invalid: expected 32 bytes, got {}",
            bytes.len()
        ))
    })
}

fn map_dial_err(err: DialError) -> RedeemPairingInvitationError {
    match err {
        DialError::InvitationNotFound => RedeemPairingInvitationError::InvitationNotFound,
        DialError::InvitationExpired => RedeemPairingInvitationError::InvitationExpired,
        DialError::SponsorUnreachable => RedeemPairingInvitationError::SponsorUnreachable,
        DialError::ServiceUnavailable => RedeemPairingInvitationError::ServiceUnavailable,
        DialError::Internal(m) => RedeemPairingInvitationError::Internal(m),
    }
}

fn map_session_err(err: SessionError) -> RedeemPairingInvitationError {
    match err {
        SessionError::NotFound(_) | SessionError::Closed => {
            RedeemPairingInvitationError::ConnectionLost
        }
        SessionError::Internal(m) => RedeemPairingInvitationError::Internal(m),
    }
}

fn map_space_access_err(err: SpaceAccessError) -> RedeemPairingInvitationError {
    match err {
        SpaceAccessError::WrongPassphrase => RedeemPairingInvitationError::PassphraseMismatch,
        SpaceAccessError::CorruptedKeyMaterial => {
            RedeemPairingInvitationError::CorruptedKeyMaterial
        }
        other => {
            RedeemPairingInvitationError::Internal(format!("derive_master_key_for_proof: {other}"))
        }
    }
}

fn map_sponsor_reject(reason: PairingRejectReason) -> RedeemPairingInvitationError {
    match reason {
        PairingRejectReason::InvitationMismatch => {
            RedeemPairingInvitationError::SponsorRejectedInvitation
        }
        // Sponsor's `verify_proof` failed = wrong passphrase — same
        // user-facing meaning as local `WrongPassphrase`, fold into one
        // variant so UI doesn't need to distinguish "who noticed first".
        PairingRejectReason::PassphraseMismatch => RedeemPairingInvitationError::PassphraseMismatch,
        PairingRejectReason::UserRejected => RedeemPairingInvitationError::SponsorDeclined,
        PairingRejectReason::Timeout => RedeemPairingInvitationError::SponsorTimedOut,
        PairingRejectReason::Internal(m) => RedeemPairingInvitationError::SponsorInternal(m),
    }
}

fn variant_name(message: &PairingSessionMessage) -> &'static str {
    match message {
        PairingSessionMessage::Request(_) => "Request",
        PairingSessionMessage::KeyslotOffer(_) => "KeyslotOffer",
        PairingSessionMessage::ChallengeResponse(_) => "ChallengeResponse",
        PairingSessionMessage::Confirm(_) => "Confirm",
        PairingSessionMessage::Reject(_) => "Reject",
    }
}

#[cfg(test)]
mod tests {
    //! Wire + crypto tests live here. Composition (admit → trust →
    //! setup-status ordering) belongs to
    //! [`crate::usecases::pairing::redeem_invitation::tests`].
    use super::*;

    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use chrono::Duration as ChronoDuration;

    use uc_core::crypto::domain::{ActiveSpace, Passphrase};
    use uc_core::ids::{DeviceId, SpaceId};
    use uc_core::pairing::session_message::{
        JoinerChallengeResponse, JoinerRequest, PairingReject, SponsorConfirm, SponsorKeyslotOffer,
    };
    use uc_core::ports::pairing::{DialError, SessionError};
    use uc_core::ports::space::SpaceAccessError;
    use uc_core::ports::LocalIdentityError;
    use uc_core::security::IdentityFingerprint;
    use uc_core::settings::model::Settings;
    use uc_core::space_access::domain::{ProofDerivedKey, SpaceAccessProofArtifact};

    // ── fakes ────────────────────────────────────────────────────────────

    #[derive(Default)]
    struct ScriptedSession {
        dial_result: StdMutex<Option<Result<PairingSessionId, DialError>>>,
        sent: StdMutex<Vec<(PairingSessionId, PairingSessionMessage)>>,
        closed: StdMutex<Vec<(PairingSessionId, Option<String>)>>,
        recv_script: StdMutex<VecDeque<RecvStep>>,
        send_next_error: StdMutex<Option<SessionError>>,
    }
    enum RecvStep {
        Msg(PairingSessionMessage),
        CleanClose,
        Err(SessionError),
        /// "Never responds" — `std::future::pending().await` lets the
        /// caller's `tokio::time::timeout` wrapper fire under paused
        /// clock.
        Hang,
    }

    impl ScriptedSession {
        fn with_dial_ok(id: &str) -> Self {
            let me = Self::default();
            *me.dial_result.lock().unwrap() = Some(Ok(PairingSessionId::new(id.to_string())));
            me
        }
        fn with_dial_err(err: DialError) -> Self {
            let me = Self::default();
            *me.dial_result.lock().unwrap() = Some(Err(err));
            me
        }
        fn push_recv(&self, step: RecvStep) {
            self.recv_script.lock().unwrap().push_back(step);
        }
        fn sent(&self) -> Vec<(PairingSessionId, PairingSessionMessage)> {
            self.sent.lock().unwrap().clone()
        }
        fn closed(&self) -> Vec<(PairingSessionId, Option<String>)> {
            self.closed.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl PairingSessionPort for ScriptedSession {
        async fn dial_by_invitation(
            &self,
            _code: &InvitationCode,
        ) -> Result<PairingSessionId, DialError> {
            match self.dial_result.lock().unwrap().as_ref() {
                Some(Ok(id)) => Ok(id.clone()),
                Some(Err(err)) => Err(clone_dial_err(err)),
                None => Err(DialError::Internal("test misconfigured".into())),
            }
        }
        async fn send(
            &self,
            session: &PairingSessionId,
            message: PairingSessionMessage,
        ) -> Result<(), SessionError> {
            if let Some(err) = self.send_next_error.lock().unwrap().take() {
                return Err(err);
            }
            self.sent.lock().unwrap().push((session.clone(), message));
            Ok(())
        }
        async fn recv_next(
            &self,
            _session: &PairingSessionId,
        ) -> Result<Option<PairingSessionMessage>, SessionError> {
            let next = self.recv_script.lock().unwrap().pop_front();
            match next {
                Some(RecvStep::Msg(m)) => Ok(Some(m)),
                Some(RecvStep::CleanClose) => Ok(None),
                Some(RecvStep::Err(e)) => Err(e),
                Some(RecvStep::Hang) | None => std::future::pending().await,
            }
        }
        async fn close(&self, session: &PairingSessionId, reason: Option<String>) {
            self.closed.lock().unwrap().push((session.clone(), reason));
        }
    }

    fn clone_dial_err(err: &DialError) -> DialError {
        match err {
            DialError::InvitationNotFound => DialError::InvitationNotFound,
            DialError::InvitationExpired => DialError::InvitationExpired,
            DialError::SponsorUnreachable => DialError::SponsorUnreachable,
            DialError::ServiceUnavailable => DialError::ServiceUnavailable,
            DialError::Internal(m) => DialError::Internal(m.clone()),
        }
    }

    struct ScriptedSpaceAccess {
        fail_next: StdMutex<Option<SpaceAccessError>>,
        derived_key_bytes: [u8; 32],
    }
    impl ScriptedSpaceAccess {
        fn ok() -> Self {
            Self {
                fail_next: StdMutex::new(None),
                derived_key_bytes: [0xCC; 32],
            }
        }
        fn with_err(err: SpaceAccessError) -> Self {
            Self {
                fail_next: StdMutex::new(Some(err)),
                derived_key_bytes: [0xCC; 32],
            }
        }
    }
    #[async_trait]
    impl SpaceAccessPort for ScriptedSpaceAccess {
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
            if let Some(err) = self.fail_next.lock().unwrap().take() {
                return Err(err);
            }
            Ok(ProofDerivedKey::from_bytes(self.derived_key_bytes))
        }
    }

    struct FixedProof(Vec<u8>);
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
                proof_bytes: self.0.clone(),
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

    struct StubSettings(StdMutex<Settings>);
    impl StubSettings {
        fn named(n: &str) -> Self {
            let mut s = Settings::default();
            s.general.device_name = Some(n.into());
            Self(StdMutex::new(s))
        }
        fn blank() -> Self {
            Self(StdMutex::new(Settings::default()))
        }
    }
    #[async_trait]
    impl SettingsPort for StubSettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(self.0.lock().unwrap().clone())
        }
        async fn save(&self, s: &Settings) -> anyhow::Result<()> {
            *self.0.lock().unwrap() = s.clone();
            Ok(())
        }
    }

    // ── fixtures ─────────────────────────────────────────────────────────

    const TEST_TTL: Duration = Duration::from_secs(30);

    fn sponsor_fp() -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string("BBBBBBBBBBBBBBBB").unwrap()
    }
    fn joiner_fp() -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string("AAAAAAAAAAAAAAAA").unwrap()
    }
    fn keyslot_offer() -> SponsorKeyslotOffer {
        SponsorKeyslotOffer {
            space_id: SpaceId::from_str("space-xyz"),
            keyslot_blob: vec![0xAA; 16],
            challenge: vec![0x42; 32],
            pairing_session_id: PairingSessionId::new("session-1"),
        }
    }
    fn sponsor_confirm() -> SponsorConfirm {
        SponsorConfirm {
            space_id: SpaceId::from_str("space-xyz"),
            sender_device_id: DeviceId::new("sponsor-device"),
            sender_device_name: "sponsor's laptop".into(),
            sender_identity_fingerprint: sponsor_fp(),
            transport_address_blob: Vec::new(),
            // Phase 098 默认 None：fixture 共享给多场景测试，绝大多数测试不
            // 关心 person 字段；想验证 Some 路径的测试就近构造一个新 confirm。
            sponsor_space_person_id: None,
        }
    }

    struct Bundle {
        session: Arc<ScriptedSession>,
        space_access: Arc<ScriptedSpaceAccess>,
        settings: Arc<StubSettings>,
    }

    impl Bundle {
        fn happy() -> Self {
            Self {
                session: Arc::new(ScriptedSession::with_dial_ok("session-1")),
                space_access: Arc::new(ScriptedSpaceAccess::ok()),
                settings: Arc::new(StubSettings::named("joiner-laptop")),
            }
        }
        fn with_dial_err(err: DialError) -> Self {
            let mut b = Self::happy();
            b.session = Arc::new(ScriptedSession::with_dial_err(err));
            b
        }

        fn build(&self) -> Arc<JoinerHandshakeCoordinator> {
            JoinerHandshakeCoordinator::new(
                self.session.clone(),
                self.space_access.clone(),
                Arc::new(FixedProof(vec![0xFE; 32])),
                Arc::new(FixedLocal(joiner_fp())),
                Arc::new(FixedDevice(DeviceId::new("joiner-device"))),
                self.settings.clone(),
                TEST_TTL,
            )
        }
    }

    fn code(s: &str) -> InvitationCode {
        InvitationCode::new(s)
    }
    fn passphrase() -> Passphrase {
        Passphrase::new("hunter22hunter22")
    }

    // ── happy path ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn happy_path_outcome_and_wire_sequence() {
        let b = Bundle::happy();
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::KeyslotOffer(
                keyslot_offer(),
            )));
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::Confirm(
                sponsor_confirm(),
            )));
        let coord = b.build();

        let out = coord
            .handshake(&code("CODE-1"), &passphrase())
            .await
            .unwrap();

        assert_eq!(out.sponsor_device_id.as_str(), "sponsor-device");
        assert_eq!(out.sponsor_device_name, "sponsor's laptop");
        assert_eq!(out.sponsor_identity_fingerprint, sponsor_fp());
        assert_eq!(out.space_id.inner(), "space-xyz");
        assert_eq!(out.self_device_id.as_str(), "joiner-device");
        assert_eq!(out.self_identity_fingerprint, joiner_fp());

        let sent = b.session.sent();
        assert_eq!(sent.len(), 2);
        match &sent[0].1 {
            PairingSessionMessage::Request(r) => {
                assert_eq!(r.invitation_code.as_str(), "CODE-1");
                assert_eq!(r.device_id.as_str(), "joiner-device");
                assert_eq!(r.device_name, "joiner-laptop");
                assert_eq!(r.identity_fingerprint, joiner_fp());
            }
            other => panic!("expected Request, got {other:?}"),
        }
        assert!(matches!(
            sent[1].1,
            PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse { .. })
        ));
        assert_eq!(b.session.closed().len(), 1);
    }

    // ── dial errors ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn dial_invitation_not_found_maps_and_no_wire_activity() {
        let b = Bundle::with_dial_err(DialError::InvitationNotFound);
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RedeemPairingInvitationError::InvitationNotFound
        ));
        assert!(b.session.sent().is_empty());
        assert!(b.session.closed().is_empty());
    }

    #[tokio::test]
    async fn dial_invitation_expired_maps() {
        let b = Bundle::with_dial_err(DialError::InvitationExpired);
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RedeemPairingInvitationError::InvitationExpired
        ));
    }

    #[tokio::test]
    async fn dial_sponsor_unreachable_maps() {
        let b = Bundle::with_dial_err(DialError::SponsorUnreachable);
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RedeemPairingInvitationError::SponsorUnreachable
        ));
    }

    #[tokio::test]
    async fn dial_service_unavailable_maps() {
        let b = Bundle::with_dial_err(DialError::ServiceUnavailable);
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RedeemPairingInvitationError::ServiceUnavailable
        ));
    }

    // ── sponsor rejects ──────────────────────────────────────────────────

    #[tokio::test]
    async fn sponsor_reject_invitation_mismatch_after_request() {
        let b = Bundle::happy();
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::Reject(
                PairingReject {
                    reason: PairingRejectReason::InvitationMismatch,
                },
            )));
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RedeemPairingInvitationError::SponsorRejectedInvitation
        ));
        assert_eq!(b.session.sent().len(), 1);
        assert_eq!(b.session.closed().len(), 1);
    }

    #[tokio::test]
    async fn sponsor_reject_passphrase_mismatch_after_challenge() {
        let b = Bundle::happy();
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::KeyslotOffer(
                keyslot_offer(),
            )));
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::Reject(
                PairingReject {
                    reason: PairingRejectReason::PassphraseMismatch,
                },
            )));
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RedeemPairingInvitationError::PassphraseMismatch
        ));
        assert_eq!(b.session.sent().len(), 2);
    }

    #[tokio::test]
    async fn sponsor_reject_timeout_maps_to_sponsor_timed_out() {
        let b = Bundle::happy();
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::Reject(
                PairingReject {
                    reason: PairingRejectReason::Timeout,
                },
            )));
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(err, RedeemPairingInvitationError::SponsorTimedOut));
    }

    #[tokio::test]
    async fn sponsor_reject_user_rejected_maps_to_sponsor_declined() {
        let b = Bundle::happy();
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::Reject(
                PairingReject {
                    reason: PairingRejectReason::UserRejected,
                },
            )));
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(err, RedeemPairingInvitationError::SponsorDeclined));
    }

    #[tokio::test]
    async fn sponsor_reject_internal_carries_message() {
        let b = Bundle::happy();
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::Reject(
                PairingReject {
                    reason: PairingRejectReason::Internal("oops".into()),
                },
            )));
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        match err {
            RedeemPairingInvitationError::SponsorInternal(m) => assert_eq!(m, "oops"),
            other => panic!("expected SponsorInternal, got {other:?}"),
        }
    }

    // ── own TTL ──────────────────────────────────────────────────────────

    #[tokio::test(start_paused = true)]
    async fn ttl_fires_on_first_recv_when_sponsor_silent() {
        let b = Bundle::happy();
        b.session.push_recv(RecvStep::Hang);
        let coord = b.build();
        let handle = tokio::spawn(async move { coord.handshake(&code("X"), &passphrase()).await });
        tokio::time::sleep(TEST_TTL + ChronoDuration::seconds(1).to_std().unwrap()).await;
        let err = handle.await.unwrap().unwrap_err();
        assert!(matches!(err, RedeemPairingInvitationError::Timeout));
    }

    #[tokio::test(start_paused = true)]
    async fn ttl_fires_on_second_recv_when_sponsor_silent_after_offer() {
        let b = Bundle::happy();
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::KeyslotOffer(
                keyslot_offer(),
            )));
        b.session.push_recv(RecvStep::Hang);
        let sent_probe = b.session.clone();
        let closed_probe = b.session.clone();
        let coord = b.build();
        let handle = tokio::spawn(async move { coord.handshake(&code("X"), &passphrase()).await });
        tokio::time::sleep(TEST_TTL + ChronoDuration::seconds(1).to_std().unwrap()).await;
        let err = handle.await.unwrap().unwrap_err();
        assert!(matches!(err, RedeemPairingInvitationError::Timeout));
        assert_eq!(sent_probe.sent().len(), 2);
        assert_eq!(closed_probe.closed().len(), 1);
    }

    // ── local derive failures ────────────────────────────────────────────

    #[tokio::test]
    async fn local_wrong_passphrase_maps_to_passphrase_mismatch() {
        let mut b = Bundle::happy();
        b.space_access = Arc::new(ScriptedSpaceAccess::with_err(
            SpaceAccessError::WrongPassphrase,
        ));
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::KeyslotOffer(
                keyslot_offer(),
            )));
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RedeemPairingInvitationError::PassphraseMismatch
        ));
        // Only Request went out — ChallengeResponse never sent.
        assert_eq!(b.session.sent().len(), 1);
    }

    #[tokio::test]
    async fn local_corrupted_keyslot_maps_to_corrupted() {
        let mut b = Bundle::happy();
        b.space_access = Arc::new(ScriptedSpaceAccess::with_err(
            SpaceAccessError::CorruptedKeyMaterial,
        ));
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::KeyslotOffer(
                keyslot_offer(),
            )));
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RedeemPairingInvitationError::CorruptedKeyMaterial
        ));
    }

    // ── connection / protocol errors ─────────────────────────────────────

    #[tokio::test]
    async fn connection_closed_before_offer_maps_to_connection_lost() {
        let b = Bundle::happy();
        b.session.push_recv(RecvStep::CleanClose);
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(err, RedeemPairingInvitationError::ConnectionLost));
    }

    #[tokio::test]
    async fn session_error_during_recv_maps_to_connection_lost() {
        let b = Bundle::happy();
        b.session.push_recv(RecvStep::Err(SessionError::Closed));
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(err, RedeemPairingInvitationError::ConnectionLost));
    }

    #[tokio::test]
    async fn unexpected_first_frame_surfaces_internal() {
        let b = Bundle::happy();
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::Confirm(
                sponsor_confirm(),
            )));
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        match err {
            RedeemPairingInvitationError::Internal(m) => {
                assert!(m.contains("expected KeyslotOffer"), "msg = {m}")
            }
            other => panic!("expected Internal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unexpected_second_frame_surfaces_internal() {
        let b = Bundle::happy();
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::KeyslotOffer(
                keyslot_offer(),
            )));
        b.session
            .push_recv(RecvStep::Msg(PairingSessionMessage::Request(
                JoinerRequest {
                    invitation_code: InvitationCode::new("X"),
                    device_id: DeviceId::new("x"),
                    device_name: "x".into(),
                    identity_fingerprint: joiner_fp(),
                    nonce: vec![],
                    transport_address_blob: vec![],
                },
            )));
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        match err {
            RedeemPairingInvitationError::Internal(m) => {
                assert!(m.contains("expected Confirm"), "msg = {m}")
            }
            other => panic!("expected Internal, got {other:?}"),
        }
    }

    // ── local facts ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn device_name_missing_short_circuits_before_any_wire_send() {
        let mut b = Bundle::happy();
        b.settings = Arc::new(StubSettings::blank());
        let err = b
            .build()
            .handshake(&code("X"), &passphrase())
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RedeemPairingInvitationError::DeviceNameRequired
        ));
        assert!(b.session.sent().is_empty());
        // Session was dialled so close fires on the error path.
        assert_eq!(b.session.closed().len(), 1);
    }
}

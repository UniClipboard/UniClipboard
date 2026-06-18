//! Sponsor-side inbound pairing orchestrator.
//!
//! Thin coordinator that chains four already-existing pieces into one
//! sponsor-side pairing session:
//!
//! 1. **Pairing invitation** — `InMemoryPairingInvitationHolder::take_matching`
//!    + `PairingInvitationPort::consume_invitation` decide whether this
//!    inbound joiner is expected at all.
//! 2. **Handshake** — [`SponsorHandshakeCoordinator`] prepares the
//!    keyslot offer, parks per-session state, verifies the joiner's
//!    challenge response, and emits `Confirm` / `Reject` on the wire.
//! 3. **Membership admit** — [`AdmitMemberUseCase`] persists the joiner
//!    as a `SpaceMember` (application-level re-pair/replace + error
//!    semantics already encoded there; a stale record from a one-sided
//!    unpair on the joiner does not block re-pairing, #1023).
//! 4. **Trust peer** — [`TrustPeerUseCase`] persists the
//!    `TrustedPeer` row symmetrically.
//!
//! Ordering matters: admit and trust run **before** `confirm` so the
//! sponsor never tells the joiner "you're in" after having failed to
//! record it locally. Any admit / trust error aborts the handshake via
//! `Reject(Internal)` — per Slice 1 project rule, pairing success must
//! not leak ahead of the local state it should have already committed.
//!
//! Per `uc-application/AGENTS.md` §11.4 everything here is `pub(crate)`;
//! the facade constructs the orchestrator during `SpaceSetupFacade::new`
//! and external callers reach pairing exclusively through that facade.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;

use chrono::{TimeZone, Utc};
use tokio::sync::broadcast;
use tokio::sync::mpsc::Receiver;
use tokio::task::JoinHandle;
use tracing::{debug, info, instrument, warn};
use uc_observability::FlowId;

use uc_core::pairing::invitation::InvitationCode;
use uc_core::pairing::session_message::{
    JoinerRequest, PairingRejectReason, PairingSessionMessage,
};
use uc_core::ports::pairing::{PairingEventPort, PairingSessionEvent, PairingSessionId};
use uc_core::ports::{
    ClockPort, ConsumeInvitationError, PairingInvitationPort, PeerAddressRecord,
    PeerAddressRepositoryPort,
};
use uc_core::MemberRepositoryPort;
use uc_core::TrustedPeerRepositoryPort;
use uc_observability::analytics::{AnalyticsFacade, Event, PairingFailureReason, PairingMethod};

use crate::facade::space_setup::PairingOutcome;
use crate::membership::usecases::{AdmitMember, AdmitMemberUseCase};
use crate::pairing_invitation::holder::{InMemoryPairingInvitationHolder, TakeMatchingError};
use crate::trusted_peer::usecases::{TrustPeer, TrustPeerUseCase};

use super::sponsor_handshake::{JoinerFacts, SponsorHandshakeCoordinator, Verdict};

/// Type aliases so the facade can `Arc<...>` the use cases without repeating
/// the dyn-port bound.
pub(crate) type AdmitMemberUc = AdmitMemberUseCase<dyn MemberRepositoryPort>;
pub(crate) type TrustPeerUc = TrustPeerUseCase<dyn TrustedPeerRepositoryPort>;

/// Drives sponsor-side inbound pairing events.
pub(crate) struct PairingInboundOrchestrator {
    pairing_events: Arc<dyn PairingEventPort>,
    pairing_invitation: Arc<dyn PairingInvitationPort>,
    holder: Arc<InMemoryPairingInvitationHolder>,
    clock: Arc<dyn ClockPort>,
    handshake: Arc<SponsorHandshakeCoordinator>,
    admit_member: Arc<AdmitMemberUc>,
    trust_peer: Arc<TrustPeerUc>,
    /// Slice 2 Phase 1 · T5：配对成功后 best-effort 把 joiner 的传输地址
    /// 写入仓库，供后续 `ensure_reachable_all` 直接拨号，避免每次都要走
    /// rendezvous。写失败不 fail 配对（presence 下轮兜底）。
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    local_device_id: uc_core::DeviceId,
    /// Broadcast channel: fires exactly one [`PairingOutcome`] per matched
    /// invitation. `send` is `let _`-ignored because no subscribers is a
    /// legitimate state (e.g., GUI tauri runtime without a live listener);
    /// the CLI `invite` command subscribes before enabling B1.
    outcome_tx: broadcast::Sender<PairingOutcome>,
    /// Outcome telemetry for `pairing_succeeded` / `pairing_failed`.
    /// `pairing_started` is fired upstream by
    /// `IssuePairingInvitationUseCase`; the orchestrator only emits the
    /// outcome events.
    analytics: Arc<dyn AnalyticsFacade>,
    /// Per-session handshake start time, populated when the first valid
    /// `Request` arrives (`on_incoming` after invitation match) and read in
    /// `finalise_verified` to compute `pairing_succeeded.duration_ms`.
    /// Failure paths drop their entry without consulting it. Bounded growth
    /// is guaranteed because every entry is removed at terminal (success
    /// or any post-match failure).
    handshake_started_at: Arc<StdMutex<HashMap<PairingSessionId, Instant>>>,
}

impl PairingInboundOrchestrator {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        pairing_events: Arc<dyn PairingEventPort>,
        pairing_invitation: Arc<dyn PairingInvitationPort>,
        holder: Arc<InMemoryPairingInvitationHolder>,
        clock: Arc<dyn ClockPort>,
        handshake: Arc<SponsorHandshakeCoordinator>,
        admit_member: Arc<AdmitMemberUc>,
        trust_peer: Arc<TrustPeerUc>,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        local_device_id: uc_core::DeviceId,
        outcome_tx: broadcast::Sender<PairingOutcome>,
        analytics: Arc<dyn AnalyticsFacade>,
    ) -> Self {
        Self {
            pairing_events,
            pairing_invitation,
            holder,
            clock,
            handshake,
            admit_member,
            trust_peer,
            peer_addr_repo,
            local_device_id,
            outcome_tx,
            analytics,
            handshake_started_at: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    /// Drop the per-session start time (success or failure terminal).
    fn take_started_at(&self, session: &PairingSessionId) -> Option<Instant> {
        self.handshake_started_at.lock().unwrap().remove(session)
    }

    /// Fire `pairing_failed` with structured reason and broadcast the
    /// `PairingOutcome::Failure` to subscribers in lock-step. Order matters:
    /// telemetry first so a slow / dead subscriber doesn't drop the analytics
    /// signal, then the broadcast for UX-facing consumers.
    fn emit_failure(&self, session: &PairingSessionId, reason: PairingFailureReason) {
        // Drop any started_at entry parked at on_incoming so the map stays
        // bounded even on the failure paths.
        let _ = self.take_started_at(session);
        self.analytics.capture(Event::PairingFailed {
            method: PairingMethod::Code,
            failure_reason: reason,
        });
        let _ = self.outcome_tx.send(PairingOutcome::Failure { reason });
    }

    /// Subscribe to the event port and spawn the drain loop. Returned
    /// `JoinHandle` is owned by the facade so shutdown can `abort()`.
    pub(crate) fn spawn(self: Arc<Self>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let rx = match self.pairing_events.subscribe().await {
                Ok(rx) => rx,
                Err(err) => {
                    warn!(
                        error = %err,
                        "pairing inbound orchestrator failed to subscribe; task exiting"
                    );
                    return;
                }
            };
            self.run_loop(rx).await;
        })
    }

    async fn run_loop(self: Arc<Self>, mut rx: Receiver<PairingSessionEvent>) {
        info!("pairing inbound orchestrator started");
        while let Some(event) = rx.recv().await {
            self.handle_event(event).await;
        }
        info!("pairing inbound orchestrator stopped (event channel closed)");
    }

    // 跨设备可观测性(PR2):
    //   - root span 一开 session 就能拿到 `session.id`,直接做静态字段;
    //   - `flow.id` / `peer.device_id` 在配对入口阶段还不知道(joiner 提交
    //     Request 后才能确定),声明为 `tracing::field::Empty` 占位,在
    //     `match_invitation` / `finalise_verified` 等下游方法里用
    //     `Span::current().record(...)` 回填 —— 因为这些方法都在
    //     `handle_event` 的 instrument 范围内,Span::current() 等价于本 root。
    //   - `flow.kind = "pairing"` 静态枚举值。
    #[instrument(
        skip_all,
        fields(
            event = event_kind(&event),
            session.id = %event_session_id(&event),
            flow.id = tracing::field::Empty,
            flow.kind = "pairing",
            peer.device_id = tracing::field::Empty,
        ),
    )]
    pub(crate) async fn handle_event(&self, event: PairingSessionEvent) {
        let flow_id = FlowId::generate();
        tracing::Span::current().record("flow.id", tracing::field::display(&flow_id));
        match event {
            PairingSessionEvent::Incoming { session, message } => {
                self.on_incoming(session, message).await
            }
            PairingSessionEvent::MessageReceived { session, message } => {
                self.on_message_received(session, message).await
            }
            PairingSessionEvent::Closed { session, reason } => {
                self.handshake
                    .handle_session_closed(&session, reason.as_deref())
                    .await;
            }
        }
    }

    async fn on_incoming(&self, session: PairingSessionId, message: PairingSessionMessage) {
        let incoming_variant = variant_name(&message);
        info!(
            session = %session,
            message_kind = incoming_variant,
            "inbound pairing event received"
        );
        let request = match message {
            PairingSessionMessage::Request(req) => req,
            other => {
                warn!(
                    session = %session,
                    variant = variant_name(&other),
                    "first pairing message was not Request; rejecting session"
                );
                self.handshake
                    .reject(
                        &session,
                        PairingRejectReason::Internal(
                            "expected Request as first pairing message".into(),
                        ),
                    )
                    .await;
                return;
            }
        };
        info!(
            session = %session,
            code = %request.invitation_code.as_str(),
            joiner_device_id = %request.device_id.as_str(),
            transport_address_blob_len = request.transport_address_blob.len(),
            "inbound pairing Request received; matching invitation"
        );

        let Some(invitation_code) = self.match_invitation(&session, &request).await else {
            return;
        };
        self.notify_consume(&invitation_code).await;

        // Slice 8b' · stamp the per-session start time so the verified
        // path can compute `pairing_succeeded.duration_ms`. Idempotent on
        // re-entry: the second insert silently overwrites — this only
        // happens if `Incoming` is replayed for the same session, which
        // would already be a protocol violation upstream.
        self.handshake_started_at
            .lock()
            .unwrap()
            .insert(session.clone(), Instant::now());

        // `begin` sends the KeyslotOffer + parks per-session state; on
        // failure it has already emitted Reject + close internally.
        match self.handshake.begin(&session, request).await {
            Ok(()) => info!(
                session = %session,
                "inbound pairing KeyslotOffer sent; waiting for ChallengeResponse"
            ),
            Err(()) => warn!(
                session = %session,
                "inbound pairing failed while sending KeyslotOffer"
            ),
        }
    }

    /// Returns the matched invitation code on success. On miss / expiry /
    /// holder invariant violation emits `Reject` via the handshake
    /// coordinator and returns `None`.
    async fn match_invitation(
        &self,
        session: &PairingSessionId,
        request: &JoinerRequest,
    ) -> Option<InvitationCode> {
        let now_ms = self.clock.now_ms();
        let now = match Utc.timestamp_millis_opt(now_ms).single() {
            Some(ts) => ts,
            None => {
                warn!(
                    session = %session,
                    now_ms,
                    "ClockPort returned out-of-range timestamp; treating inbound as internal"
                );
                self.handshake
                    .reject(
                        session,
                        PairingRejectReason::Internal("sponsor clock out of range".into()),
                    )
                    .await;
                return None;
            }
        };

        match self
            .holder
            .take_matching(&request.invitation_code, now)
            .await
        {
            Ok(invitation) => {
                // 把 joiner_device_id 提到 root span 的 `peer.device_id`,
                // 后续所有 child span / event 都自动继承,Sentry 上同一
                // pairing flow 的事件可以一键 filter 出来。
                tracing::Span::current().record(
                    "peer.device_id",
                    tracing::field::display(&request.device_id.as_str()),
                );
                info!(
                    session = %session,
                    code = %invitation.code().as_str(),
                    joiner_device_id = %request.device_id.as_str(),
                    "accepted joiner request for pending invitation"
                );
                Some(invitation.code().clone())
            }
            Err(TakeMatchingError::NotFound) => {
                info!(
                    session = %session,
                    code = %request.invitation_code.as_str(),
                    "inbound pairing request for unknown code; rejecting"
                );
                self.handshake
                    .reject(session, PairingRejectReason::InvitationMismatch)
                    .await;
                None
            }
            Err(TakeMatchingError::Expired) => {
                info!(
                    session = %session,
                    code = %request.invitation_code.as_str(),
                    "inbound pairing request after invitation expired; rejecting"
                );
                self.handshake
                    .reject(session, PairingRejectReason::InvitationMismatch)
                    .await;
                // Expired = our invitation; outer caller is done.
                self.emit_failure(session, PairingFailureReason::InvitationExpired);
                None
            }
            Err(TakeMatchingError::Internal(msg)) => {
                warn!(
                    session = %session,
                    code = %request.invitation_code.as_str(),
                    error = %msg,
                    "holder invariant broken on inbound pairing request; rejecting"
                );
                self.handshake
                    .reject(session, PairingRejectReason::Internal(msg))
                    .await;
                self.emit_failure(session, PairingFailureReason::Internal);
                None
            }
        }
    }

    async fn on_message_received(&self, session: PairingSessionId, message: PairingSessionMessage) {
        let message_variant = variant_name(&message);
        info!(
            session = %session,
            message_kind = message_variant,
            "inbound pairing follow-up message received"
        );
        let PairingSessionMessage::ChallengeResponse(response) = message else {
            // Anything else on a mid-handshake session is a joiner-side
            // protocol violation. Log without closing — the session
            // naturally resolves via a later Close or the joiner's own
            // Reject.
            info!(
                session = %session,
                variant = variant_name(&message),
                "unexpected mid-handshake message from joiner"
            );
            return;
        };

        let Some(verdict) = self.handshake.verify_challenge(&session, response).await else {
            debug!(
                session = %session,
                "ChallengeResponse arrived with no parked handshake ctx; ignoring"
            );
            return;
        };

        match verdict {
            Verdict::Verified(facts) => self.finalise_verified(&session, facts).await,
            Verdict::Rejected => {
                info!(session = %session, "joiner proof rejected; sending PassphraseMismatch");
                self.handshake
                    .reject(&session, PairingRejectReason::PassphraseMismatch)
                    .await;
                self.emit_failure(&session, PairingFailureReason::PassphraseMismatch);
            }
        }
    }

    /// Verified branch: admit → trust → confirm. Any persistence error
    /// short-circuits to `Reject(Internal)` so the joiner learns the
    /// sponsor couldn't record them and never sees a false Confirm.
    async fn finalise_verified(&self, session: &PairingSessionId, facts: JoinerFacts) {
        let now = match Utc.timestamp_millis_opt(self.clock.now_ms()).single() {
            Some(ts) => ts,
            None => {
                warn!(session = %session, "clock out of range at finalise; rejecting");
                self.handshake
                    .reject(
                        session,
                        PairingRejectReason::Internal("sponsor clock out of range".into()),
                    )
                    .await;
                self.emit_failure(session, PairingFailureReason::Internal);
                return;
            }
        };

        let admit_input = AdmitMember {
            device_id: facts.device_id.clone(),
            device_name: facts.device_name.clone(),
            identity_fingerprint: facts.identity_fingerprint.clone(),
            joined_at: now,
            sync_preferences: uc_core::MemberSyncPreferences::default(),
        };
        if let Err(err) = self.admit_member.execute(admit_input).await {
            warn!(
                session = %session,
                error = %err,
                "admit_member failed; rejecting with Internal"
            );
            self.handshake
                .reject(
                    session,
                    PairingRejectReason::Internal(format!("admit_member: {err}")),
                )
                .await;
            self.emit_failure(session, PairingFailureReason::Internal);
            return;
        }

        let trust_input = TrustPeer {
            local_device_id: self.local_device_id.clone(),
            peer_device_id: facts.device_id.clone(),
            peer_fingerprint: facts.identity_fingerprint.clone(),
            trusted_at: now,
        };
        if let Err(err) = self.trust_peer.execute(trust_input).await {
            warn!(
                session = %session,
                error = %err,
                "trust_peer failed; rejecting with Internal"
            );
            self.handshake
                .reject(
                    session,
                    PairingRejectReason::Internal(format!("trust_peer: {err}")),
                )
                .await;
            self.emit_failure(session, PairingFailureReason::Internal);
            return;
        }

        if let Err(err) = self.handshake.confirm(session).await {
            warn!(
                session = %session,
                error = %err,
                "Confirm wire send failed after admit+trust committed"
            );
            // Persistence already landed — nothing productive to do
            // beyond the Confirm attempt. `handshake.confirm` has
            // already removed ctx + closed on the happy path; on this
            // Err path the coordinator did not close (it short-circuited
            // on the settings/send failure). We deliberately do not send
            // a Reject here because the joiner's local store may have
            // already advanced; let the natural timeout take care of it.
            self.emit_failure(session, PairingFailureReason::ConnectionLost);
        } else {
            info!(
                session = %session,
                joiner_device_id = %facts.device_id.as_str(),
                "pairing handshake completed"
            );
            // Slice 2 Phase 1 · T5：best-effort 把 joiner 的传输地址 blob
            // 写入 `peer_addr_repo`。空 blob（旧 joiner / adapter 未附带）
            // 跳过 upsert；写失败只 warn 不 fail 配对——presence 下一轮
            // `ensure_reachable_all` 会再拉兜底。
            self.persist_peer_address(&facts, now).await;
            // Slice 8b' · fire `pairing_succeeded` before the broadcast so
            // a slow/dead subscriber can't drop the analytics signal.
            // duration_ms is "actual handshake time" measured from the
            // first valid `Request` (on_incoming) to here — does not
            // include the human-time gap between sponsor issuing the code
            // and the joiner typing it in (which the funnel itself can
            // derive from `pairing_started` → `pairing_succeeded` user
            // timestamps in PostHog).
            let duration_ms = self
                .take_started_at(session)
                .map(|i| i.elapsed().as_millis().min(u32::MAX as u128) as u32)
                .unwrap_or(0);
            self.analytics.capture(Event::PairingSucceeded {
                method: PairingMethod::Code,
                peer_os: None,
                duration_ms,
                // Sponsor side accepts an inbound connection and cannot
                // observe which discovery channel the joiner resolved through.
                discovery_channel: None,
            });
            let _ = self.outcome_tx.send(PairingOutcome::Success {
                peer_device_id: facts.device_id.clone(),
                peer_device_name: facts.device_name.clone(),
                peer_fingerprint: facts.identity_fingerprint.clone(),
            });
        }
    }

    /// Best-effort 写 joiner 传输地址；blob 为空或写失败都只 warn。
    async fn persist_peer_address(&self, facts: &JoinerFacts, observed_at: chrono::DateTime<Utc>) {
        if facts.transport_address_blob.is_empty() {
            debug!(
                device_id = %facts.device_id.as_str(),
                "joiner did not supply transport_address_blob; skipping peer_addr_repo upsert"
            );
            return;
        }
        let record = PeerAddressRecord {
            device_id: facts.device_id.clone(),
            addr_blob: facts.transport_address_blob.clone(),
            observed_at,
        };
        if let Err(err) = self.peer_addr_repo.upsert(&record).await {
            warn!(
                device_id = %facts.device_id.as_str(),
                error = %err,
                "peer_addr_repo.upsert failed after pairing; presence will recover lazily"
            );
        } else {
            debug!(
                device_id = %facts.device_id.as_str(),
                blob_len = facts.transport_address_blob.len(),
                "peer_addr_repo.upsert landed for new joiner"
            );
        }
    }

    async fn notify_consume(&self, code: &InvitationCode) {
        match self.pairing_invitation.consume_invitation(code).await {
            Ok(()) => debug!(code = %code.as_str(), "rendezvous consume acknowledged"),
            Err(ConsumeInvitationError::NotFound | ConsumeInvitationError::Expired) => debug!(
                code = %code.as_str(),
                "rendezvous entry already terminal on consume (benign)"
            ),
            Err(err) => warn!(
                code = %code.as_str(),
                error = %err,
                "rendezvous consume failed; local handshake proceeds regardless"
            ),
        }
    }
}

fn event_kind(event: &PairingSessionEvent) -> &'static str {
    match event {
        PairingSessionEvent::Incoming { .. } => "Incoming",
        PairingSessionEvent::MessageReceived { .. } => "MessageReceived",
        PairingSessionEvent::Closed { .. } => "Closed",
    }
}

/// 抽出当前 pairing 事件所属的 `session_id`。
///
/// 所有变体都自带 session,所以可以无条件返回 `&PairingSessionId`,
/// 让 `handle_event` 的 root span 把 `session.id` 直接做静态字段而不必
/// 用 `Empty` 占位再回填。
fn event_session_id(event: &PairingSessionEvent) -> &PairingSessionId {
    match event {
        PairingSessionEvent::Incoming { session, .. } => session,
        PairingSessionEvent::MessageReceived { session, .. } => session,
        PairingSessionEvent::Closed { session, .. } => session,
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
    //! The orchestrator's own tests verify the ordering contract:
    //! match → consume → handshake.begin → verify → admit → trust →
    //! confirm. The handshake wire adapter is covered in
    //! `sponsor_handshake::tests`; admit/trust are covered in their
    //! respective use-case tests. Here we scope to the composition
    //! glue: which branches call which use cases in which order, and
    //! the persistence-before-confirm ordering guarantee.
    use super::*;

    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use chrono::{DateTime, Duration};
    use tokio::sync::mpsc;

    use uc_core::crypto::domain::Passphrase;
    use uc_core::ids::{DeviceId, SessionId, SpaceId};
    use uc_core::membership::{MembershipError, SpaceMember};
    use uc_core::pairing::invitation::{InvitationCode, PairingInvitation};
    use uc_core::pairing::session_message::{JoinerChallengeResponse, PairingReject};
    use uc_core::ports::pairing::{DialError, DialOutcome, PairingSessionPort, SessionError};
    use uc_core::ports::pairing_invitation::{InvitationError, IssuedInvitation};
    use uc_core::ports::space::{PrepareJoinOfferPort, ProofPort, SpaceAccessError};
    use uc_core::ports::LocalIdentityError;
    use uc_core::ports::{DeviceIdentityPort, LocalIdentityPort, SettingsPort};
    use uc_core::security::IdentityFingerprint;
    use uc_core::settings::model::Settings;
    use uc_core::space_access::domain::{JoinOffer, ProofDerivedKey, SpaceAccessProofArtifact};
    use uc_core::trusted_peer::{TrustedPeer, TrustedPeerError};
    use uc_observability::analytics::{
        AnalyticsPort, DefaultAnalyticsFacade, NoopAnalyticsIdentity,
    };

    // ── fakes ────────────────────────────────────────────────────────────

    /// Test-only `AnalyticsPort` that records every captured `Event` for
    /// later inspection. Mirrors the `CapturingAnalyticsSink` used by the
    /// joiner-side `redeem_invitation` tests (Slice 8b).
    #[derive(Default)]
    struct CapturingAnalyticsSink {
        captured: StdMutex<Vec<Event>>,
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

    struct FakeClock(i64);
    impl ClockPort for FakeClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    #[derive(Default)]
    struct RecordingSessionPort {
        sent: StdMutex<Vec<(PairingSessionId, PairingSessionMessage)>>,
        closed: StdMutex<Vec<(PairingSessionId, Option<String>)>>,
    }
    impl RecordingSessionPort {
        fn sent(&self) -> Vec<(PairingSessionId, PairingSessionMessage)> {
            self.sent.lock().unwrap().clone()
        }
        fn closed(&self) -> Vec<(PairingSessionId, Option<String>)> {
            self.closed.lock().unwrap().clone()
        }
    }
    #[async_trait]
    impl PairingSessionPort for RecordingSessionPort {
        async fn dial_by_invitation(&self, _: &InvitationCode) -> Result<DialOutcome, DialError> {
            unimplemented!()
        }
        async fn send(
            &self,
            session: &PairingSessionId,
            message: PairingSessionMessage,
        ) -> Result<(), SessionError> {
            self.sent.lock().unwrap().push((session.clone(), message));
            Ok(())
        }
        async fn recv_next(
            &self,
            _: &PairingSessionId,
        ) -> Result<Option<PairingSessionMessage>, SessionError> {
            unimplemented!()
        }
        async fn close(&self, session: &PairingSessionId, reason: Option<String>) {
            self.closed.lock().unwrap().push((session.clone(), reason));
        }
    }

    struct ScriptedEventPort(StdMutex<Option<Receiver<PairingSessionEvent>>>);
    #[async_trait]
    impl PairingEventPort for ScriptedEventPort {
        async fn subscribe(&self) -> anyhow::Result<Receiver<PairingSessionEvent>> {
            self.0
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| anyhow::anyhow!("already subscribed"))
        }
    }

    #[derive(Default)]
    struct RecordingInvitationPort {
        consumed: StdMutex<Vec<InvitationCode>>,
    }
    #[async_trait]
    impl PairingInvitationPort for RecordingInvitationPort {
        async fn issue_invitation(&self) -> Result<IssuedInvitation, InvitationError> {
            unimplemented!()
        }
        async fn consume_invitation(
            &self,
            code: &InvitationCode,
        ) -> Result<(), ConsumeInvitationError> {
            self.consumed.lock().unwrap().push(code.clone());
            Ok(())
        }
    }

    struct StubSpaceAccess {
        challenge_nonce: [u8; 32],
    }
    #[async_trait]
    impl PrepareJoinOfferPort for StubSpaceAccess {
        async fn prepare_join_offer(
            &self,
            _: &SpaceId,
            _: &Passphrase,
        ) -> Result<JoinOffer, SpaceAccessError> {
            Ok(JoinOffer {
                space_id: SpaceId::from_str("space-xyz"),
                keyslot_blob: vec![0xAA; 32],
                challenge_nonce: self.challenge_nonce,
            })
        }
    }

    struct ScriptedProof(StdMutex<Vec<bool>>);
    #[async_trait]
    impl ProofPort for ScriptedProof {
        async fn build_proof(
            &self,
            _: &SessionId,
            _: &SpaceId,
            _: [u8; 32],
            _: &ProofDerivedKey,
        ) -> anyhow::Result<SpaceAccessProofArtifact> {
            unimplemented!()
        }
        async fn verify_proof(
            &self,
            _: &SpaceAccessProofArtifact,
            _: [u8; 32],
        ) -> anyhow::Result<bool> {
            let mut q = self.0.lock().unwrap();
            Ok(if q.is_empty() { false } else { q.remove(0) })
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

    /// Minimal SetupStatusPort stub — orchestrator tests don't care
    /// which `space_id` lands in the KeyslotOffer, so a None value
    /// triggers the sponsor coordinator's fresh-UUID fallback. That's
    /// fine because assertions here compare wire intent and use case
    /// ordering, not specific space ids.
    struct OrchestratorStubSetupStatus;
    #[async_trait]
    impl uc_core::ports::SetupStatusPort for OrchestratorStubSetupStatus {
        async fn get_status(&self) -> anyhow::Result<uc_core::setup::SetupStatus> {
            Ok(uc_core::setup::SetupStatus {
                has_completed: true,
                space_id: None,
            })
        }
        async fn set_status(&self, _s: &uc_core::setup::SetupStatus) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// Repo that records every save and can be pre-armed to fail.
    #[derive(Default)]
    struct RecordingMemberRepo {
        saved: StdMutex<Vec<SpaceMember>>,
        fail_next: StdMutex<Option<MembershipError>>,
    }
    #[async_trait]
    impl MemberRepositoryPort for RecordingMemberRepo {
        async fn get(&self, id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
            // Last write wins, mirroring the port's upsert semantics — the
            // re-pair regression test pre-seeds a stale row and expects the
            // use case to observe it.
            Ok(self
                .saved
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|m| &m.device_id == id)
                .cloned())
        }
        async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError> {
            Ok(self.saved.lock().unwrap().clone())
        }
        async fn save(&self, member: &SpaceMember) -> Result<(), MembershipError> {
            if let Some(err) = self.fail_next.lock().unwrap().take() {
                return Err(err);
            }
            self.saved.lock().unwrap().push(member.clone());
            Ok(())
        }
        async fn remove(&self, _: &DeviceId) -> Result<bool, MembershipError> {
            Ok(false)
        }
    }

    #[derive(Default)]
    struct RecordingTrustedPeerRepo {
        saved: StdMutex<Vec<TrustedPeer>>,
        fail_next: StdMutex<Option<TrustedPeerError>>,
    }
    #[async_trait]
    impl TrustedPeerRepositoryPort for RecordingTrustedPeerRepo {
        async fn get(&self, id: &DeviceId) -> Result<Option<TrustedPeer>, TrustedPeerError> {
            // Last write wins, mirroring the port's upsert semantics (see
            // `RecordingMemberRepo::get`).
            Ok(self
                .saved
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|p| &p.peer_device_id == id)
                .cloned())
        }
        async fn list(&self) -> Result<Vec<TrustedPeer>, TrustedPeerError> {
            Ok(self.saved.lock().unwrap().clone())
        }
        async fn save(&self, peer: &TrustedPeer) -> Result<(), TrustedPeerError> {
            if let Some(err) = self.fail_next.lock().unwrap().take() {
                return Err(err);
            }
            self.saved.lock().unwrap().push(peer.clone());
            Ok(())
        }
        async fn remove(&self, _: &DeviceId) -> Result<bool, TrustedPeerError> {
            Ok(false)
        }
    }

    // Slice 2 Phase 1 · T5：用 mockall 生成 `PeerAddressRepositoryPort`
    // 的测试替身。T5 的三条测试断言都是"某次 upsert 是否发生、参数对不对、
    // 失败时外层是否被穿透"——正好是 mockall `.expect_upsert().times(N)
    // .withf(...).returning(...)` 擅长的行为契约。
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

    // ── helpers ──────────────────────────────────────────────────────────

    fn fixed_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-04-20T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }
    fn fixed_now_ms() -> i64 {
        fixed_now().timestamp_millis()
    }
    fn joiner_fp() -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string("AAAAAAAAAAAAAAAA").unwrap()
    }
    fn sponsor_fp() -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string("BBBBBBBBBBBBBBBB").unwrap()
    }
    fn pending(code: &str) -> PairingInvitation {
        let issued = fixed_now();
        let expires = issued + Duration::minutes(5);
        let (inv, _) = PairingInvitation::issue(
            InvitationCode::new(code),
            issued,
            expires,
            DeviceId::new("sponsor-1"),
        );
        inv
    }
    fn joiner_request(code: &str) -> JoinerRequest {
        JoinerRequest {
            invitation_code: InvitationCode::new(code),
            device_id: DeviceId::new("joiner-device"),
            device_name: "joiner's laptop".into(),
            identity_fingerprint: joiner_fp(),
            nonce: vec![1, 2, 3, 4],
            transport_address_blob: vec![],
        }
    }

    /// Build a `MockPeerAddrRepo` that accepts any `upsert` / `get` /
    /// `list` / `remove` call and returns a neutral success. Used by
    /// tests that don't care about T5 peer-address behaviour (the
    /// majority of this module's tests); the T5-specific cases build
    /// their own mocks inline with targeted `.expect_*` setups.
    fn permissive_peer_addr_repo() -> Arc<MockPeerAddrRepo> {
        let mut mock = MockPeerAddrRepo::new();
        mock.expect_upsert().returning(|_| Ok(()));
        mock.expect_get().returning(|_| Ok(None));
        mock.expect_list().returning(|| Ok(vec![]));
        mock.expect_remove().returning(|_| Ok(()));
        Arc::new(mock)
    }

    struct Bundle {
        session_port: Arc<RecordingSessionPort>,
        invitation_port: Arc<RecordingInvitationPort>,
        holder: Arc<InMemoryPairingInvitationHolder>,
        member_repo: Arc<RecordingMemberRepo>,
        trusted_peer_repo: Arc<RecordingTrustedPeerRepo>,
        peer_addr_repo: Arc<MockPeerAddrRepo>,
        proof_verdicts: Vec<bool>,
        clock_ms: i64,
        /// Injectable analytics sink so capture-asserting tests can swap
        /// a `CapturingAnalyticsSink` in. Default is the noop sink for
        /// the legacy tests that don't care about telemetry. Wrapped
        /// into an `AnalyticsFacade` at build time so the orchestrator
        /// sees the same surface the production code does.
        analytics: Arc<dyn AnalyticsPort>,
    }

    impl Bundle {
        fn happy() -> Self {
            Self {
                session_port: Arc::new(RecordingSessionPort::default()),
                invitation_port: Arc::new(RecordingInvitationPort::default()),
                holder: Arc::new(InMemoryPairingInvitationHolder::new()),
                member_repo: Arc::new(RecordingMemberRepo::default()),
                trusted_peer_repo: Arc::new(RecordingTrustedPeerRepo::default()),
                peer_addr_repo: permissive_peer_addr_repo(),
                proof_verdicts: vec![true],
                clock_ms: fixed_now_ms(),
                analytics: Arc::new(uc_observability::analytics::NoopAnalyticsSink),
            }
        }

        fn build(
            self,
            events: Arc<ScriptedEventPort>,
        ) -> (
            Arc<PairingInboundOrchestrator>,
            broadcast::Receiver<PairingOutcome>,
        ) {
            // 大 TTL：orchestrator 这一层的测试不关心 TTL fire，
            // 专门的 TTL 行为测试在 `sponsor_handshake::tests` 里。
            let handshake = SponsorHandshakeCoordinator::new(
                self.session_port.clone() as Arc<dyn PairingSessionPort>,
                Arc::new(StubSpaceAccess {
                    challenge_nonce: [0x42; 32],
                }),
                Arc::new(ScriptedProof(StdMutex::new(self.proof_verdicts))),
                Arc::new(FixedLocal(sponsor_fp())),
                Arc::new(FixedDevice(DeviceId::new("sponsor-device"))),
                Arc::new(NamedSettings("sponsor-mac".into())),
                Arc::new(OrchestratorStubSetupStatus),
                Arc::new(uc_observability::analytics::NoopAnalyticsFacade),
                std::time::Duration::from_secs(3600),
            );
            let (outcome_tx, outcome_rx) = broadcast::channel(16);
            let orch = Arc::new(PairingInboundOrchestrator::new(
                events,
                self.invitation_port.clone(),
                self.holder.clone(),
                Arc::new(FakeClock(self.clock_ms)) as Arc<dyn ClockPort>,
                handshake,
                Arc::new(AdmitMemberUseCase::new(
                    self.member_repo.clone() as Arc<dyn MemberRepositoryPort>
                )),
                Arc::new(TrustPeerUseCase::new(
                    self.trusted_peer_repo.clone() as Arc<dyn TrustedPeerRepositoryPort>
                )),
                self.peer_addr_repo.clone() as Arc<dyn PeerAddressRepositoryPort>,
                DeviceId::new("sponsor-device"),
                outcome_tx,
                Arc::new(DefaultAnalyticsFacade::new(
                    self.analytics.clone(),
                    Arc::new(NoopAnalyticsIdentity),
                )) as Arc<dyn AnalyticsFacade>,
            ));
            (orch, outcome_rx)
        }
    }

    fn drained_events() -> Arc<ScriptedEventPort> {
        let (_tx, rx) = mpsc::channel::<PairingSessionEvent>(1);
        Arc::new(ScriptedEventPort(StdMutex::new(Some(rx))))
    }

    // ── filter branches (invitation mismatch / expired / holder invariant) ─

    #[tokio::test]
    async fn unknown_code_rejects_and_does_not_consume() {
        let b = Bundle::happy();
        b.holder.insert(pending("EXPECTED")).await;
        let sp = b.session_port.clone();
        let inv = b.invitation_port.clone();
        let holder = b.holder.clone();
        let (orch, mut outcomes) = b.build(drained_events());

        orch.handle_event(PairingSessionEvent::Incoming {
            session: PairingSessionId::new("s"),
            message: PairingSessionMessage::Request(joiner_request("WRONG")),
        })
        .await;

        let sent = sp.sent();
        assert_eq!(sent.len(), 1);
        assert!(matches!(
            sent[0].1,
            PairingSessionMessage::Reject(PairingReject {
                reason: PairingRejectReason::InvitationMismatch
            })
        ));
        assert_eq!(sp.closed().len(), 1);
        assert!(inv.consumed.lock().unwrap().is_empty());
        assert_eq!(holder.len().await, 1);
        // Stranger code → not ours; no outcome should fire (the invite
        // command stays listening for the real joiner).
        assert!(outcomes.try_recv().is_err());
    }

    #[tokio::test]
    async fn expired_invitation_rejects_and_drops_slot() {
        let mut b = Bundle::happy();
        b.holder.insert(pending("STALE")).await;
        b.clock_ms = (fixed_now() + Duration::minutes(10)).timestamp_millis();
        let sp = b.session_port.clone();
        let holder = b.holder.clone();
        let (orch, mut outcomes) = b.build(drained_events());

        orch.handle_event(PairingSessionEvent::Incoming {
            session: PairingSessionId::new("s"),
            message: PairingSessionMessage::Request(joiner_request("STALE")),
        })
        .await;

        assert!(matches!(
            sp.sent()[0].1,
            PairingSessionMessage::Reject(PairingReject {
                reason: PairingRejectReason::InvitationMismatch
            })
        ));
        assert_eq!(holder.len().await, 0);
        // Our expired invitation = lifecycle-end; outcome surfaces as
        // Failure so the `invite` command can exit with a useful reason.
        match outcomes.try_recv() {
            Ok(PairingOutcome::Failure { reason }) => {
                assert_eq!(reason, PairingFailureReason::InvitationExpired);
            }
            other => panic!("expected Failure(InvitationExpired), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_request_first_frame_rejects_internal() {
        let b = Bundle::happy();
        let sp = b.session_port.clone();
        let (orch, mut outcomes) = b.build(drained_events());
        orch.handle_event(PairingSessionEvent::Incoming {
            session: PairingSessionId::new("s"),
            message: PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
                encrypted_challenge: vec![],
            }),
        })
        .await;
        match &sp.sent()[0].1 {
            PairingSessionMessage::Reject(r) => match &r.reason {
                PairingRejectReason::Internal(msg) => assert!(msg.contains("Request")),
                o => panic!("expected Internal, got {o:?}"),
            },
            o => panic!("expected Reject, got {o:?}"),
        }
        // Pre-match garbage → can't attribute to any invitation, no outcome.
        assert!(outcomes.try_recv().is_err());
    }

    // ── verified happy path ──────────────────────────────────────────────

    #[tokio::test]
    async fn verified_path_admits_trusts_confirms_in_order() {
        let b = Bundle::happy();
        b.holder.insert(pending("OK")).await;
        let sp = b.session_port.clone();
        let member_repo = b.member_repo.clone();
        let trusted_peer_repo = b.trusted_peer_repo.clone();
        let (orch, mut outcomes) = b.build(drained_events());

        let session = PairingSessionId::new("s-ok");
        orch.handle_event(PairingSessionEvent::Incoming {
            session: session.clone(),
            message: PairingSessionMessage::Request(joiner_request("OK")),
        })
        .await;
        orch.handle_event(PairingSessionEvent::MessageReceived {
            session: session.clone(),
            message: PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
                encrypted_challenge: vec![0x11],
            }),
        })
        .await;

        // Admit + trust both landed.
        let members = member_repo.saved.lock().unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].device_id.as_str(), "joiner-device");
        assert_eq!(members[0].identity_fingerprint, joiner_fp());
        drop(members);

        let trusted = trusted_peer_repo.saved.lock().unwrap();
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].local_device_id.as_str(), "sponsor-device");
        assert_eq!(trusted[0].peer_device_id.as_str(), "joiner-device");
        drop(trusted);

        // Wire: KeyslotOffer + Confirm in order; no Reject.
        let sent = sp.sent();
        assert_eq!(sent.len(), 2);
        assert!(matches!(sent[0].1, PairingSessionMessage::KeyslotOffer(_)));
        assert!(matches!(sent[1].1, PairingSessionMessage::Confirm(_)));
        assert_eq!(sp.closed().len(), 1);
        // Completion outcome fires with joiner facts so the CLI/GUI
        // listener can display "paired with X".
        match outcomes.try_recv() {
            Ok(PairingOutcome::Success {
                peer_device_id,
                peer_device_name,
                peer_fingerprint,
            }) => {
                assert_eq!(peer_device_id.as_str(), "joiner-device");
                assert_eq!(peer_device_name, "joiner's laptop");
                assert_eq!(peer_fingerprint, joiner_fp());
            }
            other => panic!("expected Success, got {other:?}"),
        }
    }

    /// Issue #1023 regression: the joiner unpaired locally (one-sided), so
    /// the sponsor still holds stale member + trust rows for it. A fresh
    /// invite + verified handshake must replace those rows and Confirm —
    /// previously `AlreadyAdmitted` aborted with `Reject(Internal)`.
    #[tokio::test]
    async fn re_pair_with_stale_records_replaces_and_confirms() {
        let b = Bundle::happy();
        b.holder.insert(pending("OK")).await;

        let stale_fp = IdentityFingerprint::from_raw_string("CCCCCCCCCCCCCCCC").unwrap();
        b.member_repo.saved.lock().unwrap().push(SpaceMember {
            device_id: DeviceId::new("joiner-device"),
            device_name: "joiner's old name".into(),
            identity_fingerprint: stale_fp.clone(),
            joined_at: fixed_now() - Duration::days(7),
            sync_preferences: uc_core::MemberSyncPreferences::default(),
        });
        b.trusted_peer_repo.saved.lock().unwrap().push(TrustedPeer {
            local_device_id: DeviceId::new("sponsor-device"),
            peer_device_id: DeviceId::new("joiner-device"),
            peer_fingerprint: stale_fp,
            trusted_at: fixed_now() - Duration::days(7),
        });

        let sp = b.session_port.clone();
        let member_repo = b.member_repo.clone();
        let trusted_peer_repo = b.trusted_peer_repo.clone();
        let (orch, mut outcomes) = b.build(drained_events());

        let session = PairingSessionId::new("s-repair");
        orch.handle_event(PairingSessionEvent::Incoming {
            session: session.clone(),
            message: PairingSessionMessage::Request(joiner_request("OK")),
        })
        .await;
        orch.handle_event(PairingSessionEvent::MessageReceived {
            session,
            message: PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
                encrypted_challenge: vec![0x11],
            }),
        })
        .await;

        // Wire: KeyslotOffer + Confirm, no Reject — pairing succeeded.
        let sent = sp.sent();
        assert_eq!(sent.len(), 2);
        assert!(matches!(sent[0].1, PairingSessionMessage::KeyslotOffer(_)));
        assert!(matches!(sent[1].1, PairingSessionMessage::Confirm(_)));

        // The replacement landed: latest rows carry the fresh handshake facts.
        let members = member_repo.saved.lock().unwrap();
        let latest_member = members.last().unwrap();
        assert_eq!(latest_member.device_name, "joiner's laptop");
        assert_eq!(latest_member.identity_fingerprint, joiner_fp());
        drop(members);

        let trusted = trusted_peer_repo.saved.lock().unwrap();
        let latest_trust = trusted.last().unwrap();
        assert_eq!(latest_trust.peer_fingerprint, joiner_fp());
        drop(trusted);

        assert!(matches!(
            outcomes.try_recv(),
            Ok(PairingOutcome::Success { .. })
        ));
    }

    // ── T5: peer_addr_repo upsert behaviour ──────────────────────────────

    /// Helper: build a joiner_request with an explicit transport blob so
    /// the T5 tests can vary the field without duplicating the whole
    /// struct literal.
    fn joiner_request_with_blob(code: &str, blob: Vec<u8>) -> JoinerRequest {
        JoinerRequest {
            invitation_code: InvitationCode::new(code),
            device_id: DeviceId::new("joiner-device"),
            device_name: "joiner's laptop".into(),
            identity_fingerprint: joiner_fp(),
            nonce: vec![1, 2, 3, 4],
            transport_address_blob: blob,
        }
    }

    #[tokio::test]
    async fn verified_path_with_address_blob_upserts_peer_addr_repo() {
        // mockall 行为契约：期望 upsert 恰好被调一次、参数匹配
        // (device_id == "joiner-device" && addr_blob == <期望字节>)。
        // mockall 在 `drop` MockPeerAddrRepo 时会校验 `.times(1)`，所以
        // 哪怕测试逻辑漏了 assertion，少调或多调也会 panic。
        let expected_blob: Vec<u8> = vec![0xde, 0xad, 0xbe, 0xef];
        let expected_blob_matcher = expected_blob.clone();
        let mut mock = MockPeerAddrRepo::new();
        mock.expect_upsert()
            .times(1)
            .withf(move |record| {
                record.device_id.as_str() == "joiner-device"
                    && record.addr_blob == expected_blob_matcher
            })
            .returning(|_| Ok(()));

        let mut b = Bundle::happy();
        b.peer_addr_repo = Arc::new(mock);
        b.holder.insert(pending("OK")).await;
        let (orch, _outcomes) = b.build(drained_events());

        let session = PairingSessionId::new("s-ok-addr");
        orch.handle_event(PairingSessionEvent::Incoming {
            session: session.clone(),
            message: PairingSessionMessage::Request(joiner_request_with_blob("OK", expected_blob)),
        })
        .await;
        orch.handle_event(PairingSessionEvent::MessageReceived {
            session,
            message: PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
                encrypted_challenge: vec![0x11],
            }),
        })
        .await;
        // 当 orch 持有的最后一个 Arc drop 时，mockall 的析构会校验期望。
    }

    #[tokio::test]
    async fn verified_path_without_address_blob_skips_peer_addr_repo() {
        // 空 blob 场景：mockall `.expect_upsert().times(0)` 在 drop
        // 检查时 fail 以防意外调用。
        let mut mock = MockPeerAddrRepo::new();
        mock.expect_upsert().times(0);

        let mut b = Bundle::happy();
        b.peer_addr_repo = Arc::new(mock);
        b.holder.insert(pending("OK")).await;
        let (orch, mut outcomes) = b.build(drained_events());

        let session = PairingSessionId::new("s-ok-noaddr");
        orch.handle_event(PairingSessionEvent::Incoming {
            session: session.clone(),
            message: PairingSessionMessage::Request(joiner_request_with_blob("OK", Vec::new())),
        })
        .await;
        orch.handle_event(PairingSessionEvent::MessageReceived {
            session,
            message: PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
                encrypted_challenge: vec![0x11],
            }),
        })
        .await;

        // Pairing itself still succeeds end-to-end.
        assert!(matches!(
            outcomes.try_recv(),
            Ok(PairingOutcome::Success { .. })
        ));
    }

    #[tokio::test]
    async fn peer_addr_repo_failure_does_not_fail_pairing() {
        // 预设 upsert 返 Err；T5 best-effort 语义下，配对仍必须成功
        // 广播 `PairingOutcome::Success`。
        let mut mock = MockPeerAddrRepo::new();
        mock.expect_upsert().times(1).returning(|_| {
            Err(uc_core::ports::PeerAddressError::Internal(
                "sqlite down".into(),
            ))
        });

        let mut b = Bundle::happy();
        b.peer_addr_repo = Arc::new(mock);
        b.holder.insert(pending("OK")).await;
        let (orch, mut outcomes) = b.build(drained_events());

        let session = PairingSessionId::new("s-fail-upsert");
        orch.handle_event(PairingSessionEvent::Incoming {
            session: session.clone(),
            message: PairingSessionMessage::Request(joiner_request_with_blob(
                "OK",
                vec![0x01, 0x02],
            )),
        })
        .await;
        orch.handle_event(PairingSessionEvent::MessageReceived {
            session,
            message: PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
                encrypted_challenge: vec![0x11],
            }),
        })
        .await;

        assert!(matches!(
            outcomes.try_recv(),
            Ok(PairingOutcome::Success { .. })
        ));
    }

    // ── unverified → PassphraseMismatch, no persistence ──────────────────

    #[tokio::test]
    async fn unverified_path_rejects_passphrase_mismatch_no_persist() {
        let mut b = Bundle::happy();
        b.holder.insert(pending("BAD")).await;
        b.proof_verdicts = vec![false];
        let sp = b.session_port.clone();
        let member_repo = b.member_repo.clone();
        let trusted_peer_repo = b.trusted_peer_repo.clone();
        let (orch, mut outcomes) = b.build(drained_events());

        let session = PairingSessionId::new("s-bad");
        orch.handle_event(PairingSessionEvent::Incoming {
            session: session.clone(),
            message: PairingSessionMessage::Request(joiner_request("BAD")),
        })
        .await;
        orch.handle_event(PairingSessionEvent::MessageReceived {
            session: session.clone(),
            message: PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
                encrypted_challenge: vec![],
            }),
        })
        .await;

        let sent = sp.sent();
        assert_eq!(sent.len(), 2, "KeyslotOffer + Reject");
        assert!(matches!(
            sent[1].1,
            PairingSessionMessage::Reject(PairingReject {
                reason: PairingRejectReason::PassphraseMismatch
            })
        ));
        assert!(member_repo.saved.lock().unwrap().is_empty());
        assert!(trusted_peer_repo.saved.lock().unwrap().is_empty());
        match outcomes.try_recv() {
            Ok(PairingOutcome::Failure { reason }) => {
                assert_eq!(reason, PairingFailureReason::PassphraseMismatch);
            }
            other => panic!("expected Failure(PassphraseMismatch), got {other:?}"),
        }
    }

    // ── admit failure aborts before trust + Confirm ──────────────────────

    #[tokio::test]
    async fn admit_failure_aborts_before_trust_and_sends_internal_reject() {
        let b = Bundle::happy();
        b.holder.insert(pending("AF")).await;
        *b.member_repo.fail_next.lock().unwrap() =
            Some(MembershipError::Repository("db down".into()));
        let sp = b.session_port.clone();
        let member_repo = b.member_repo.clone();
        let trusted_peer_repo = b.trusted_peer_repo.clone();
        let (orch, mut outcomes) = b.build(drained_events());

        let session = PairingSessionId::new("s-af");
        orch.handle_event(PairingSessionEvent::Incoming {
            session: session.clone(),
            message: PairingSessionMessage::Request(joiner_request("AF")),
        })
        .await;
        orch.handle_event(PairingSessionEvent::MessageReceived {
            session: session.clone(),
            message: PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
                encrypted_challenge: vec![],
            }),
        })
        .await;

        // KeyslotOffer + Reject(Internal admit_member:...) — Confirm
        // never went out.
        let sent = sp.sent();
        assert_eq!(sent.len(), 2);
        match &sent[1].1 {
            PairingSessionMessage::Reject(r) => match &r.reason {
                PairingRejectReason::Internal(msg) => {
                    assert!(msg.contains("admit_member"), "msg = {msg}")
                }
                o => panic!("expected Internal, got {o:?}"),
            },
            o => panic!("expected Reject, got {o:?}"),
        }
        assert!(member_repo.saved.lock().unwrap().is_empty());
        assert!(
            trusted_peer_repo.saved.lock().unwrap().is_empty(),
            "trust must not run when admit failed"
        );
        match outcomes.try_recv() {
            Ok(PairingOutcome::Failure { reason }) => {
                // admit_member persistence failure surfaces as the
                // sponsor-local Internal funnel bucket; finer-grained
                // attribution lives in the tracing log only.
                assert_eq!(reason, PairingFailureReason::Internal);
            }
            other => panic!("expected Failure(Internal), got {other:?}"),
        }
    }

    // ── trust failure aborts after admit already committed ──────────────

    #[tokio::test]
    async fn trust_failure_after_admit_sends_internal_reject() {
        let b = Bundle::happy();
        b.holder.insert(pending("TF")).await;
        *b.trusted_peer_repo.fail_next.lock().unwrap() =
            Some(TrustedPeerError::Repository("trust boom".into()));
        let sp = b.session_port.clone();
        let member_repo = b.member_repo.clone();
        let trusted_peer_repo = b.trusted_peer_repo.clone();
        let (orch, mut outcomes) = b.build(drained_events());

        let session = PairingSessionId::new("s-tf");
        orch.handle_event(PairingSessionEvent::Incoming {
            session: session.clone(),
            message: PairingSessionMessage::Request(joiner_request("TF")),
        })
        .await;
        orch.handle_event(PairingSessionEvent::MessageReceived {
            session: session.clone(),
            message: PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
                encrypted_challenge: vec![],
            }),
        })
        .await;

        // Admit DID land (persistence is committed — Slice 1 has no
        // admit-rollback compensation; that asymmetry is the intended
        // "strict" behaviour the user asked for). Trust did not, and
        // Confirm was not sent.
        assert_eq!(member_repo.saved.lock().unwrap().len(), 1);
        assert!(trusted_peer_repo.saved.lock().unwrap().is_empty());

        let sent = sp.sent();
        assert_eq!(sent.len(), 2);
        match &sent[1].1 {
            PairingSessionMessage::Reject(r) => match &r.reason {
                PairingRejectReason::Internal(msg) => {
                    assert!(msg.contains("trust_peer"), "msg = {msg}")
                }
                o => panic!("expected Internal, got {o:?}"),
            },
            o => panic!("expected Reject, got {o:?}"),
        }
        match outcomes.try_recv() {
            Ok(PairingOutcome::Failure { reason }) => {
                // trust_peer persistence failure also rolls up to Internal
                // — same rationale as the admit failure above.
                assert_eq!(reason, PairingFailureReason::Internal);
            }
            other => panic!("expected Failure(Internal), got {other:?}"),
        }
    }

    // ── Closed event delegates to handshake coordinator ────────────────

    #[tokio::test]
    async fn closed_event_clears_parked_handshake_state() {
        let b = Bundle::happy();
        b.holder.insert(pending("DR")).await;
        let (orch, _outcomes) = b.build(drained_events());

        let session = PairingSessionId::new("s-dr");
        orch.handle_event(PairingSessionEvent::Incoming {
            session: session.clone(),
            message: PairingSessionMessage::Request(joiner_request("DR")),
        })
        .await;
        orch.handle_event(PairingSessionEvent::Closed {
            session,
            reason: Some("peer bailed".into()),
        })
        .await;
        // Follow-up ChallengeResponse on the same id is now a ghost;
        // verify_challenge returns None → orchestrator logs + drops.
        // No panic, no wire side effect beyond the original KeyslotOffer.
    }

    // ── stray follow-up message is logged, not closed ──────────────────

    #[tokio::test]
    async fn stray_non_challenge_followup_is_logged_only() {
        let b = Bundle::happy();
        b.holder.insert(pending("ST")).await;
        let sp = b.session_port.clone();
        let (orch, _outcomes) = b.build(drained_events());
        let session = PairingSessionId::new("s-st");

        orch.handle_event(PairingSessionEvent::Incoming {
            session: session.clone(),
            message: PairingSessionMessage::Request(joiner_request("ST")),
        })
        .await;
        orch.handle_event(PairingSessionEvent::MessageReceived {
            session: session.clone(),
            message: PairingSessionMessage::KeyslotOffer(uc_core::pairing::SponsorKeyslotOffer {
                space_id: SpaceId::from_str("fake"),
                keyslot_blob: vec![],
                challenge: vec![],
                pairing_session_id: session.clone(),
            }),
        })
        .await;

        // Only KeyslotOffer in the sent log; session stays open.
        assert_eq!(sp.sent().len(), 1);
        assert!(sp.closed().is_empty());
    }

    // ── spawn ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn spawn_drains_events_from_subscription() {
        let b = Bundle::happy();
        b.holder.insert(pending("SP")).await;
        let holder = b.holder.clone();
        let invitation_port = b.invitation_port.clone();

        let (tx, rx) = mpsc::channel(16);
        let events = Arc::new(ScriptedEventPort(StdMutex::new(Some(rx))));
        let (orch, mut _outcomes) = b.build(events);

        let handle = Arc::clone(&orch).spawn();
        tx.send(PairingSessionEvent::Incoming {
            session: PairingSessionId::new("sp-1"),
            message: PairingSessionMessage::Request(joiner_request("SP")),
        })
        .await
        .unwrap();
        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("spawn task finishes on channel close")
            .expect("spawn task must not panic");
        assert_eq!(holder.len().await, 0);
        assert_eq!(
            invitation_port.consumed.lock().unwrap().clone(),
            vec![InvitationCode::new("SP")]
        );
    }

    // ── Slice 8b' analytics: pairing_succeeded / pairing_failed wire ─────

    #[tokio::test]
    async fn verified_path_fires_pairing_succeeded_with_method_and_duration() {
        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let mut b = Bundle::happy();
        b.holder.insert(pending("AS")).await;
        b.analytics = analytics.clone();
        let (orch, _outcomes) = b.build(drained_events());

        let session = PairingSessionId::new("s-as");
        orch.handle_event(PairingSessionEvent::Incoming {
            session: session.clone(),
            message: PairingSessionMessage::Request(joiner_request("AS")),
        })
        .await;
        orch.handle_event(PairingSessionEvent::MessageReceived {
            session,
            message: PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
                encrypted_challenge: vec![0x11],
            }),
        })
        .await;

        let events = analytics.events();
        assert_eq!(events.len(), 1, "exactly one event on the success path");
        match &events[0] {
            Event::PairingSucceeded {
                method,
                peer_os,
                duration_ms: _,
                discovery_channel,
            } => {
                assert_eq!(*method, PairingMethod::Code);
                assert_eq!(*peer_os, None, "peer_os not yet propagated by handshake");
                assert_eq!(
                    *discovery_channel, None,
                    "sponsor side cannot observe the joiner's discovery channel"
                );
            }
            other => panic!("expected PairingSucceeded, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unverified_path_fires_pairing_failed_with_passphrase_mismatch() {
        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let mut b = Bundle::happy();
        b.holder.insert(pending("AF")).await;
        b.proof_verdicts = vec![false];
        b.analytics = analytics.clone();
        let (orch, _outcomes) = b.build(drained_events());

        let session = PairingSessionId::new("s-af");
        orch.handle_event(PairingSessionEvent::Incoming {
            session: session.clone(),
            message: PairingSessionMessage::Request(joiner_request("AF")),
        })
        .await;
        orch.handle_event(PairingSessionEvent::MessageReceived {
            session,
            message: PairingSessionMessage::ChallengeResponse(JoinerChallengeResponse {
                encrypted_challenge: vec![],
            }),
        })
        .await;

        let events = analytics.events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            Event::PairingFailed {
                method,
                failure_reason,
            } => {
                assert_eq!(*method, PairingMethod::Code);
                assert_eq!(*failure_reason, PairingFailureReason::PassphraseMismatch);
            }
            other => panic!("expected PairingFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn expired_invitation_fires_pairing_failed_with_invitation_expired() {
        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let mut b = Bundle::happy();
        b.holder.insert(pending("EX")).await;
        b.clock_ms = (fixed_now() + Duration::minutes(10)).timestamp_millis();
        b.analytics = analytics.clone();
        let (orch, _outcomes) = b.build(drained_events());

        orch.handle_event(PairingSessionEvent::Incoming {
            session: PairingSessionId::new("s-ex"),
            message: PairingSessionMessage::Request(joiner_request("EX")),
        })
        .await;

        let events = analytics.events();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            Event::PairingFailed {
                method: PairingMethod::Code,
                failure_reason: PairingFailureReason::InvitationExpired,
            }
        ));
    }

    #[tokio::test]
    async fn spawn_exits_when_subscribe_fails() {
        let b = Bundle::happy();
        let events = drained_events();
        let _ = events.subscribe().await.unwrap();
        let (orch, mut _outcomes) = b.build(events);
        let handle = orch.spawn();
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("task exits on subscribe failure")
            .expect("task must not panic");
    }
}

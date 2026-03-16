//! Space access orchestrator.
//!
//! Coordinates space access state machine and side effects.

use std::sync::Arc;

use chrono::Utc;
use tokio::sync::{mpsc, Mutex};
use tracing::{info_span, Instrument};

use uc_core::ids::SpaceId;
use uc_core::network::SessionId;
use uc_core::security::space_access::action::SpaceAccessAction;
use uc_core::security::space_access::deny_reason_to_code;
use uc_core::security::space_access::event::SpaceAccessEvent;
use uc_core::security::space_access::state::{CancelReason, DenyReason, SpaceAccessState};
use uc_core::security::space_access::state_machine::SpaceAccessStateMachine;
use uc_core::SessionId as CoreSessionId;

use super::context::{SpaceAccessContext, SpaceAccessOffer};
use super::events::{SpaceAccessCompletedEvent, SpaceAccessEventPort};
use super::executor::SpaceAccessExecutor;

/// Errors produced by space access orchestrator.
#[derive(Debug, thiserror::Error)]
pub enum SpaceAccessError {
    #[error("space access action not implemented: {0}")]
    ActionNotImplemented(&'static str),
    #[error("space access missing pairing session id")]
    MissingPairingSessionId,
    #[error("space access missing context: {0}")]
    MissingContext(&'static str),
    #[error("space access crypto failed: {0}")]
    Crypto(#[from] anyhow::Error),
    #[error("space access timer failed: {0}")]
    Timer(#[source] anyhow::Error),
    #[error("space access persistence failed: {0}")]
    Persistence(#[source] anyhow::Error),
}

/// Orchestrator that drives space access state and side effects.
pub struct SpaceAccessOrchestrator {
    context: Arc<Mutex<SpaceAccessContext>>,
    state: Arc<Mutex<SpaceAccessState>>,
    dispatch_lock: Arc<Mutex<()>>,
    event_senders: Arc<Mutex<Vec<mpsc::Sender<SpaceAccessCompletedEvent>>>>,
}

impl SpaceAccessOrchestrator {
    pub fn new() -> Self {
        Self::with_context(SpaceAccessContext::default())
    }

    pub fn with_context(context: SpaceAccessContext) -> Self {
        Self {
            context: Arc::new(Mutex::new(context)),
            state: Arc::new(Mutex::new(SpaceAccessState::Idle)),
            dispatch_lock: Arc::new(Mutex::new(())),
            event_senders: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn start_sponsor_authorization(
        &self,
        executor: &mut SpaceAccessExecutor<'_>,
        pairing_session_id: SessionId,
        space_id: SpaceId,
        ttl_secs: u64,
    ) -> Result<SpaceAccessState, SpaceAccessError> {
        let event = SpaceAccessEvent::SponsorAuthorizationRequested {
            pairing_session_id: pairing_session_id.clone(),
            space_id,
            ttl_secs,
        };
        self.dispatch(executor, event, Some(pairing_session_id))
            .await
    }

    pub async fn get_state(&self) -> SpaceAccessState {
        self.state.lock().await.clone()
    }

    pub fn context(&self) -> Arc<Mutex<SpaceAccessContext>> {
        Arc::clone(&self.context)
    }

    pub async fn dispatch(
        &self,
        executor: &mut SpaceAccessExecutor<'_>,
        event: SpaceAccessEvent,
        pairing_session_id: Option<SessionId>,
    ) -> Result<SpaceAccessState, SpaceAccessError> {
        let _dispatch_guard = self.dispatch_lock.lock().await;

        let span = info_span!("usecase.space_access_orchestrator.dispatch", event = ?event);
        async {
            let current = self.state.lock().await.clone();

            // When re-entering from a terminal state (e.g. sponsor handling a
            // second joiner after the first completed), clear stale context so
            // the new session starts with a clean slate.
            let restarting_from_terminal = matches!(
                current,
                SpaceAccessState::Granted { .. }
                    | SpaceAccessState::Denied { .. }
                    | SpaceAccessState::Cancelled { .. }
            );
            if restarting_from_terminal {
                let mut context = self.context.lock().await;
                context.prepared_offer = None;
                context.joiner_offer = None;
                context.joiner_passphrase = None;
                context.proof_artifact = None;
                context.result_success = None;
                context.result_deny_reason = None;
                // sponsor_peer_id is set by wiring before dispatch — keep it.
            }

            let (next, actions) = SpaceAccessStateMachine::transition(current.clone(), event);
            let is_responder_flow = matches!(
                current,
                SpaceAccessState::WaitingJoinerProof {
                    pairing_session_id: _,
                    space_id: _,
                    expires_at: _,
                }
            );

            {
                let mut context = self.context.lock().await;
                match &next {
                    SpaceAccessState::Granted { .. } => {
                        context.result_success = Some(true);
                        context.result_deny_reason = None;
                    }
                    SpaceAccessState::Denied { reason, .. } => {
                        context.result_success = Some(false);
                        context.result_deny_reason = Some(reason.clone());
                    }
                    _ => {
                        context.result_success = None;
                        context.result_deny_reason = None;
                    }
                }
            }

            let sponsor_persisted = match self
                .execute_actions(executor, pairing_session_id.as_ref(), actions)
                .await
            {
                Ok(persisted) => persisted,
                Err(err) => {
                    if is_responder_flow {
                        self.emit_responder_completion(
                            &next,
                            false,
                            Some(err.to_string()),
                            pairing_session_id.as_ref(),
                        )
                        .await;
                    }
                    return Err(err);
                }
            };

            if is_responder_flow {
                self.emit_responder_completion(
                    &next,
                    sponsor_persisted,
                    None,
                    pairing_session_id.as_ref(),
                )
                .await;
            }

            let mut guard = self.state.lock().await;
            *guard = next.clone();
            Ok(next)
        }
        .instrument(span)
        .await
    }

    async fn emit_responder_completion(
        &self,
        next: &SpaceAccessState,
        sponsor_persisted: bool,
        action_error_reason: Option<String>,
        fallback_session_id: Option<&SessionId>,
    ) {
        let session_id = Self::resolve_session_id(next, fallback_session_id);
        let Some(session_id) = session_id else {
            return;
        };

        if let Some(reason) = action_error_reason {
            self.emit_completion(&session_id, false, Some(reason)).await;
            return;
        }

        match next {
            SpaceAccessState::Granted { .. } => {
                if sponsor_persisted {
                    self.emit_completion(&session_id, true, None).await;
                } else {
                    self.emit_completion(
                        &session_id,
                        false,
                        Some("sponsor_persist_not_executed".to_string()),
                    )
                    .await;
                }
            }
            SpaceAccessState::Denied { reason, .. } => {
                self.emit_completion(&session_id, false, Some(Self::deny_reason_code(reason)))
                    .await;
            }
            SpaceAccessState::Cancelled { reason, .. } => {
                self.emit_completion(&session_id, false, Some(Self::cancel_reason_code(reason)))
                    .await;
            }
            _ => {}
        }
    }

    fn resolve_session_id(
        state: &SpaceAccessState,
        fallback_session_id: Option<&SessionId>,
    ) -> Option<String> {
        match state {
            SpaceAccessState::WaitingOffer {
                pairing_session_id, ..
            }
            | SpaceAccessState::WaitingUserPassphrase {
                pairing_session_id, ..
            }
            | SpaceAccessState::WaitingDecision {
                pairing_session_id, ..
            }
            | SpaceAccessState::WaitingJoinerProof {
                pairing_session_id, ..
            }
            | SpaceAccessState::Granted {
                pairing_session_id, ..
            }
            | SpaceAccessState::Denied {
                pairing_session_id, ..
            }
            | SpaceAccessState::Cancelled {
                pairing_session_id, ..
            } => Some(pairing_session_id.clone()),
            SpaceAccessState::Idle => fallback_session_id.cloned(),
        }
    }

    fn deny_reason_code(reason: &DenyReason) -> String {
        deny_reason_to_code(reason).to_string()
    }

    fn cancel_reason_code(reason: &CancelReason) -> String {
        match reason {
            CancelReason::UserCancelled => "user_cancelled",
            CancelReason::Timeout => "timeout",
            CancelReason::SessionClosed => "session_closed",
        }
        .to_string()
    }

    async fn emit_completion(&self, session_id: &str, success: bool, reason: Option<String>) {
        let peer_id = {
            let context = self.context.lock().await;
            context
                .sponsor_peer_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        };

        let event = SpaceAccessCompletedEvent {
            session_id: session_id.to_string(),
            peer_id,
            success,
            reason,
            ts: Utc::now().timestamp_millis(),
        };

        let senders = { self.event_senders.lock().await.clone() };
        for sender in senders {
            if sender.send(event.clone()).await.is_err() {
                tracing::debug!("space access completion receiver dropped");
            }
        }
    }

    async fn execute_actions(
        &self,
        executor: &mut SpaceAccessExecutor<'_>,
        pairing_session_id: Option<&SessionId>,
        actions: Vec<SpaceAccessAction>,
    ) -> Result<bool, SpaceAccessError> {
        let mut sponsor_persisted = false;
        for action in actions {
            match action {
                SpaceAccessAction::RequestOfferPreparation {
                    pairing_session_id,
                    space_id,
                    expires_at: _,
                } => {
                    let keyslot = executor.crypto.export_keyslot_blob(&space_id).await?;
                    let nonce = executor.crypto.generate_nonce32().await;
                    let offer = SpaceAccessOffer {
                        space_id: space_id.clone(),
                        keyslot,
                        nonce,
                    };
                    let mut context = self.context.lock().await;
                    context.prepared_offer = Some(offer);
                    let _ = pairing_session_id;
                }
                SpaceAccessAction::SendOffer => {
                    let session_id =
                        pairing_session_id.ok_or(SpaceAccessError::MissingPairingSessionId)?;
                    executor.transport.send_offer(session_id).await?;
                }
                SpaceAccessAction::StartTimer { ttl_secs } => {
                    let session_id =
                        pairing_session_id.ok_or(SpaceAccessError::MissingPairingSessionId)?;
                    let session_id = CoreSessionId::from(session_id.as_str());
                    executor
                        .timer
                        .start(&session_id, ttl_secs)
                        .await
                        .map_err(SpaceAccessError::Timer)?;
                }
                SpaceAccessAction::StopTimer => {
                    let session_id =
                        pairing_session_id.ok_or(SpaceAccessError::MissingPairingSessionId)?;
                    let session_id = CoreSessionId::from(session_id.as_str());
                    executor
                        .timer
                        .stop(&session_id)
                        .await
                        .map_err(SpaceAccessError::Timer)?;
                }
                SpaceAccessAction::RequestSpaceKeyDerivation { space_id } => {
                    let session_id =
                        pairing_session_id.ok_or(SpaceAccessError::MissingPairingSessionId)?;
                    let (offer, passphrase) = {
                        let mut context = self.context.lock().await;
                        let offer = context
                            .joiner_offer
                            .as_ref()
                            .ok_or(SpaceAccessError::MissingContext("joiner offer"))?
                            .clone();

                        if offer.space_id != space_id {
                            return Err(SpaceAccessError::MissingContext(
                                "joiner offer space mismatch",
                            ));
                        }

                        let passphrase = context
                            .joiner_passphrase
                            .take()
                            .ok_or(SpaceAccessError::MissingContext("joiner passphrase"))?;

                        (offer, passphrase)
                    };

                    let master_key = executor
                        .crypto
                        .derive_master_key_from_keyslot(&offer.keyslot_blob, passphrase)
                        .await?;

                    let proof = executor
                        .proof
                        .build_proof(
                            &CoreSessionId::from(session_id.as_str()),
                            &space_id,
                            offer.challenge_nonce,
                            &master_key,
                        )
                        .await?;

                    let mut context = self.context.lock().await;
                    context.proof_artifact = Some(proof);
                }
                SpaceAccessAction::SendProof => {
                    let session_id =
                        pairing_session_id.ok_or(SpaceAccessError::MissingPairingSessionId)?;
                    executor.transport.send_proof(session_id).await?;
                }
                SpaceAccessAction::SendResult => {
                    let session_id =
                        pairing_session_id.ok_or(SpaceAccessError::MissingPairingSessionId)?;
                    executor.transport.send_result(session_id).await?;
                }
                SpaceAccessAction::PersistJoinerAccess { space_id } => {
                    let peer_id = {
                        let context = self.context.lock().await;
                        context
                            .sponsor_peer_id
                            .as_ref()
                            .cloned()
                            .ok_or(SpaceAccessError::MissingContext("sponsor peer id"))?
                    };
                    executor
                        .store
                        .persist_joiner_access(&space_id, &peer_id)
                        .await
                        .map_err(SpaceAccessError::Persistence)?;
                }
                SpaceAccessAction::PersistSponsorAccess { space_id } => {
                    let peer_id = {
                        let context = self.context.lock().await;
                        context
                            .sponsor_peer_id
                            .as_ref()
                            .cloned()
                            .ok_or(SpaceAccessError::MissingContext("sponsor peer id"))?
                    };

                    executor
                        .store
                        .persist_sponsor_access(&space_id, &peer_id)
                        .await
                        .map_err(SpaceAccessError::Persistence)?;
                    sponsor_persisted = true;
                }
            }
        }

        Ok(sponsor_persisted)
    }
}

#[async_trait::async_trait]
impl SpaceAccessEventPort for SpaceAccessOrchestrator {
    async fn subscribe(&self) -> anyhow::Result<mpsc::Receiver<SpaceAccessCompletedEvent>> {
        let (event_tx, event_rx) = mpsc::channel(100);
        let mut senders = self.event_senders.lock().await;
        senders.push(event_tx);
        Ok(event_rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::{Duration, Utc};
    use tokio::time::{timeout, Duration as TokioDuration};
    use uc_core::ids::{SessionId as CoreSessionId, SpaceId};
    use uc_core::network::SessionId as NetSessionId;
    use uc_core::ports::space::{CryptoPort, PersistencePort, ProofPort, SpaceAccessTransportPort};
    use uc_core::ports::TimerPort;
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionFormatVersion, KdfParams, KeyScope, KeySlot,
        KeySlotVersion, MasterKey, WrappedMasterKey,
    };
    use uc_core::security::space_access::domain::SpaceAccessProofArtifact;
    use uc_core::security::space_access::event::SpaceAccessEvent;
    use uc_core::security::space_access::state::{DenyReason, SpaceAccessState};
    use uc_core::security::SecretString;

    use crate::usecases::space_access::SpaceAccessEventPort;
    use crate::usecases::space_access::SpaceAccessJoinerOffer;

    type StateAssert = Box<dyn Fn(&SpaceAccessState) + Send + Sync>;

    enum AccessTestStep {
        Dispatch {
            label: &'static str,
            event: SpaceAccessEvent,
            assert: StateAssert,
        },
        PrepareJoinerInput {
            build_offer: Box<dyn Fn() -> SpaceAccessJoinerOffer + Send + Sync>,
            build_passphrase: Box<dyn Fn() -> SecretString + Send + Sync>,
        },
        SetSponsorPeer {
            peer_id: &'static str,
        },
    }

    impl AccessTestStep {
        fn dispatch(label: &'static str, event: SpaceAccessEvent, assert: StateAssert) -> Self {
            Self::Dispatch {
                label,
                event,
                assert,
            }
        }

        fn prepare_joiner_input<FOffer, FPass>(build_offer: FOffer, build_passphrase: FPass) -> Self
        where
            FOffer: Fn() -> SpaceAccessJoinerOffer + Send + Sync + 'static,
            FPass: Fn() -> SecretString + Send + Sync + 'static,
        {
            Self::PrepareJoinerInput {
                build_offer: Box::new(build_offer),
                build_passphrase: Box::new(build_passphrase),
            }
        }

        fn set_sponsor_peer(peer_id: &'static str) -> Self {
            Self::SetSponsorPeer { peer_id }
        }
    }

    #[derive(Default)]
    struct AccessTestHarness {
        crypto: MockCrypto,
        transport: MockTransport,
        proof: MockProof,
        timer: MockTimer,
        store: MockStore,
    }

    impl AccessTestHarness {
        fn executor(&mut self) -> SpaceAccessExecutor<'_> {
            SpaceAccessExecutor {
                crypto: &self.crypto,
                transport: &mut self.transport,
                proof: &self.proof,
                timer: &mut self.timer,
                store: &mut self.store,
            }
        }
    }

    #[tokio::test]
    async fn joiner_side_happy_path() {
        let session_id: NetSessionId = "session-join".into();
        let space_id: SpaceId = "space-join".into();
        let ttl_secs = 30_u64;
        let expires_at = Utc::now() + Duration::seconds(ttl_secs as i64);
        let orchestrator = SpaceAccessOrchestrator::new();
        let mut harness = AccessTestHarness::default();
        let mut executor = harness.executor();

        let steps = vec![
            AccessTestStep::dispatch(
                "join requested",
                SpaceAccessEvent::JoinRequested {
                    pairing_session_id: session_id.clone(),
                    ttl_secs,
                },
                expect_waiting_offer(session_id.clone()),
            ),
            AccessTestStep::dispatch(
                "offer accepted",
                SpaceAccessEvent::OfferAccepted {
                    pairing_session_id: session_id.clone(),
                    space_id: space_id.clone(),
                    expires_at,
                },
                expect_waiting_user_passphrase(session_id.clone(), space_id.clone()),
            ),
            AccessTestStep::prepare_joiner_input(
                {
                    let space_id = space_id.clone();
                    move || build_joiner_offer(&space_id)
                },
                || SecretString::new("join-pass".into()),
            ),
            AccessTestStep::set_sponsor_peer("peer-join"),
            AccessTestStep::dispatch(
                "passphrase submitted",
                SpaceAccessEvent::PassphraseSubmitted,
                expect_waiting_decision(session_id.clone(), space_id.clone()),
            ),
            AccessTestStep::dispatch(
                "access granted",
                SpaceAccessEvent::AccessGranted {
                    pairing_session_id: session_id.clone(),
                    space_id: space_id.clone(),
                },
                expect_granted(session_id.clone(), space_id.clone()),
            ),
        ];

        run_access_steps(&orchestrator, &mut executor, &session_id, &steps).await;

        assert_eq!(harness.transport.proofs(), vec![session_id.clone()]);
        assert!(harness.transport.results().is_empty());
        assert_eq!(harness.timer.start_calls.len(), 2);
        assert_eq!(harness.timer.stop_calls.len(), 2);
        assert_eq!(
            harness.store.joiner_access,
            vec![(space_id.clone(), "peer-join".to_string())]
        );
    }

    #[tokio::test]
    async fn joiner_side_denied() {
        let session_id: NetSessionId = "session-denied".into();
        let space_id: SpaceId = "space-denied".into();
        let ttl_secs = 45_u64;
        let expires_at = Utc::now() + Duration::seconds(ttl_secs as i64);
        let orchestrator = SpaceAccessOrchestrator::new();
        let mut harness = AccessTestHarness::default();
        let mut executor = harness.executor();

        let steps = vec![
            AccessTestStep::dispatch(
                "join requested",
                SpaceAccessEvent::JoinRequested {
                    pairing_session_id: session_id.clone(),
                    ttl_secs,
                },
                expect_waiting_offer(session_id.clone()),
            ),
            AccessTestStep::dispatch(
                "offer accepted",
                SpaceAccessEvent::OfferAccepted {
                    pairing_session_id: session_id.clone(),
                    space_id: space_id.clone(),
                    expires_at,
                },
                expect_waiting_user_passphrase(session_id.clone(), space_id.clone()),
            ),
            AccessTestStep::prepare_joiner_input(
                {
                    let space_id = space_id.clone();
                    move || build_joiner_offer(&space_id)
                },
                || SecretString::new("bad-pass".into()),
            ),
            AccessTestStep::dispatch(
                "passphrase submitted",
                SpaceAccessEvent::PassphraseSubmitted,
                expect_waiting_decision(session_id.clone(), space_id.clone()),
            ),
            AccessTestStep::dispatch(
                "access denied",
                SpaceAccessEvent::AccessDenied {
                    pairing_session_id: session_id.clone(),
                    space_id: space_id.clone(),
                    reason: DenyReason::InvalidProof,
                },
                expect_denied(
                    session_id.clone(),
                    space_id.clone(),
                    DenyReason::InvalidProof,
                ),
            ),
        ];

        run_access_steps(&orchestrator, &mut executor, &session_id, &steps).await;

        assert_eq!(harness.transport.proofs(), vec![session_id.clone()]);
        assert!(harness.transport.results().is_empty());
        assert_eq!(harness.store.joiner_access.len(), 0);
        assert_eq!(harness.timer.stop_calls.len(), 2);
    }

    #[tokio::test]
    async fn sponsor_side_happy_path() {
        let session_id: NetSessionId = "session-sponsor".into();
        let space_id: SpaceId = "space-sponsor".into();
        let ttl_secs = 60_u64;
        let orchestrator = SpaceAccessOrchestrator::new();
        let mut completion_rx = orchestrator
            .subscribe()
            .await
            .expect("subscribe completion");
        let mut harness = AccessTestHarness::default();
        let mut executor = harness.executor();

        let steps = vec![
            AccessTestStep::dispatch(
                "sponsor authorization requested",
                SpaceAccessEvent::SponsorAuthorizationRequested {
                    pairing_session_id: session_id.clone(),
                    space_id: space_id.clone(),
                    ttl_secs,
                },
                expect_waiting_joiner_proof(session_id.clone(), space_id.clone()),
            ),
            AccessTestStep::set_sponsor_peer("peer-sponsor"),
            AccessTestStep::dispatch(
                "proof verified",
                SpaceAccessEvent::ProofVerified {
                    pairing_session_id: session_id.clone(),
                    space_id: space_id.clone(),
                },
                expect_granted(session_id.clone(), space_id.clone()),
            ),
        ];

        run_access_steps(&orchestrator, &mut executor, &session_id, &steps).await;

        assert_eq!(harness.transport.offers(), vec![session_id.clone()]);
        assert_eq!(harness.transport.results(), vec![session_id.clone()]);
        assert_eq!(
            harness.store.sponsor_access,
            vec![(space_id.clone(), "peer-sponsor".into())]
        );
        assert_eq!(harness.timer.start_calls.len(), 1);
        assert_eq!(harness.timer.stop_calls.len(), 1);

        let completion = timeout(TokioDuration::from_secs(1), completion_rx.recv())
            .await
            .expect("completion timeout")
            .expect("completion missing");
        assert_eq!(completion.session_id, session_id);
        assert_eq!(completion.peer_id, "peer-sponsor");
        assert!(completion.success);
        assert!(completion.reason.is_none());
    }

    #[tokio::test]
    async fn sponsor_side_denied() {
        let session_id: NetSessionId = "session-deny".into();
        let space_id: SpaceId = "space-deny".into();
        let ttl_secs = 90_u64;
        let orchestrator = SpaceAccessOrchestrator::new();
        let mut completion_rx = orchestrator
            .subscribe()
            .await
            .expect("subscribe completion");
        let mut harness = AccessTestHarness::default();
        let mut executor = harness.executor();

        let steps = vec![
            AccessTestStep::dispatch(
                "sponsor authorization requested",
                SpaceAccessEvent::SponsorAuthorizationRequested {
                    pairing_session_id: session_id.clone(),
                    space_id: space_id.clone(),
                    ttl_secs,
                },
                expect_waiting_joiner_proof(session_id.clone(), space_id.clone()),
            ),
            AccessTestStep::set_sponsor_peer("peer-deny"),
            AccessTestStep::dispatch(
                "proof rejected",
                SpaceAccessEvent::ProofRejected {
                    pairing_session_id: session_id.clone(),
                    space_id: space_id.clone(),
                    reason: DenyReason::InvalidProof,
                },
                expect_denied(
                    session_id.clone(),
                    space_id.clone(),
                    DenyReason::InvalidProof,
                ),
            ),
        ];

        run_access_steps(&orchestrator, &mut executor, &session_id, &steps).await;

        assert_eq!(harness.transport.offers(), vec![session_id.clone()]);
        assert_eq!(harness.transport.results(), vec![session_id.clone()]);
        assert!(harness.store.sponsor_access.is_empty());
        assert_eq!(harness.timer.stop_calls.len(), 1);

        let completion = timeout(TokioDuration::from_secs(1), completion_rx.recv())
            .await
            .expect("completion timeout")
            .expect("completion missing");
        assert_eq!(completion.session_id, session_id);
        assert_eq!(completion.peer_id, "peer-deny");
        assert!(!completion.success);
        assert_eq!(
            completion.reason.as_deref(),
            Some(uc_core::security::space_access::DENY_REASON_INVALID_PROOF)
        );
    }

    async fn run_access_steps(
        orchestrator: &SpaceAccessOrchestrator,
        executor: &mut SpaceAccessExecutor<'_>,
        pairing_session_id: &NetSessionId,
        steps: &[AccessTestStep],
    ) {
        for step in steps {
            match step {
                AccessTestStep::Dispatch {
                    label,
                    event,
                    assert,
                } => {
                    let next = orchestrator
                        .dispatch(executor, event.clone(), Some(pairing_session_id.clone()))
                        .await
                        .unwrap_or_else(|err| panic!("{} failed: {:?}", label, err));
                    assert(&next);
                }
                AccessTestStep::PrepareJoinerInput {
                    build_offer,
                    build_passphrase,
                    ..
                } => {
                    let mut guard = orchestrator.context.lock().await;
                    guard.joiner_offer = Some(build_offer());
                    guard.joiner_passphrase = Some(build_passphrase());
                }
                AccessTestStep::SetSponsorPeer { peer_id } => {
                    let mut guard = orchestrator.context.lock().await;
                    guard.sponsor_peer_id = Some((*peer_id).to_string());
                }
            }
        }
    }

    fn expect_waiting_offer(expected_session: NetSessionId) -> StateAssert {
        Box::new(move |state| match state {
            SpaceAccessState::WaitingOffer {
                pairing_session_id, ..
            } => {
                assert_eq!(pairing_session_id, &expected_session)
            }
            other => panic!("expected WaitingOffer, got {:?}", other),
        })
    }

    fn expect_waiting_user_passphrase(
        expected_session: NetSessionId,
        expected_space: SpaceId,
    ) -> StateAssert {
        Box::new(move |state| match state {
            SpaceAccessState::WaitingUserPassphrase {
                pairing_session_id,
                space_id,
                ..
            } => {
                assert_eq!(pairing_session_id, &expected_session);
                assert_eq!(space_id, &expected_space);
            }
            other => panic!("expected WaitingUserPassphrase, got {:?}", other),
        })
    }

    fn expect_waiting_decision(
        expected_session: NetSessionId,
        expected_space: SpaceId,
    ) -> StateAssert {
        Box::new(move |state| match state {
            SpaceAccessState::WaitingDecision {
                pairing_session_id,
                space_id,
                ..
            } => {
                assert_eq!(pairing_session_id, &expected_session);
                assert_eq!(space_id, &expected_space);
            }
            other => panic!("expected WaitingDecision, got {:?}", other),
        })
    }

    fn expect_waiting_joiner_proof(
        expected_session: NetSessionId,
        expected_space: SpaceId,
    ) -> StateAssert {
        Box::new(move |state| match state {
            SpaceAccessState::WaitingJoinerProof {
                pairing_session_id,
                space_id,
                ..
            } => {
                assert_eq!(pairing_session_id, &expected_session);
                assert_eq!(space_id, &expected_space);
            }
            other => panic!("expected WaitingJoinerProof, got {:?}", other),
        })
    }

    fn expect_granted(expected_session: NetSessionId, expected_space: SpaceId) -> StateAssert {
        Box::new(move |state| match state {
            SpaceAccessState::Granted {
                pairing_session_id,
                space_id,
            } => {
                assert_eq!(pairing_session_id, &expected_session);
                assert_eq!(space_id, &expected_space);
            }
            other => panic!("expected Granted, got {:?}", other),
        })
    }

    fn expect_denied(
        expected_session: NetSessionId,
        expected_space: SpaceId,
        expected_reason: DenyReason,
    ) -> StateAssert {
        Box::new(move |state| match state {
            SpaceAccessState::Denied {
                pairing_session_id,
                space_id,
                reason,
            } => {
                assert_eq!(pairing_session_id, &expected_session);
                assert_eq!(space_id, &expected_space);
                assert_eq!(reason, &expected_reason);
            }
            other => panic!("expected Denied, got {:?}", other),
        })
    }

    fn build_joiner_offer(space_id: &SpaceId) -> SpaceAccessJoinerOffer {
        SpaceAccessJoinerOffer {
            space_id: space_id.clone(),
            keyslot_blob: vec![1, 2, 3, 4],
            challenge_nonce: [5u8; 32],
        }
    }

    #[derive(Default)]
    struct MockCrypto;

    #[async_trait]
    impl CryptoPort for MockCrypto {
        async fn generate_nonce32(&self) -> [u8; 32] {
            [42u8; 32]
        }

        async fn export_keyslot_blob(&self, _space_id: &SpaceId) -> anyhow::Result<KeySlot> {
            Ok(sample_keyslot())
        }

        async fn derive_master_key_from_keyslot(
            &self,
            _keyslot_blob: &[u8],
            _passphrase: SecretString,
        ) -> anyhow::Result<MasterKey> {
            Ok(MasterKey([7u8; 32]))
        }
    }

    fn sample_keyslot() -> KeySlot {
        KeySlot {
            version: KeySlotVersion::V1,
            scope: KeyScope {
                profile_id: "default".into(),
            },
            kdf: KdfParams::for_initialization(),
            salt: vec![0u8; 16],
            wrapped_master_key: Some(WrappedMasterKey {
                blob: EncryptedBlob {
                    version: EncryptionFormatVersion::V1,
                    aead: EncryptionAlgo::XChaCha20Poly1305,
                    nonce: vec![0u8; 24],
                    ciphertext: vec![0u8; 32],
                    aad_fingerprint: None,
                },
            }),
        }
    }

    #[derive(Default)]
    struct MockProof;

    #[async_trait]
    impl ProofPort for MockProof {
        async fn build_proof(
            &self,
            pairing_session_id: &CoreSessionId,
            space_id: &SpaceId,
            challenge_nonce: [u8; 32],
            _master_key: &MasterKey,
        ) -> anyhow::Result<SpaceAccessProofArtifact> {
            Ok(SpaceAccessProofArtifact {
                pairing_session_id: pairing_session_id.clone(),
                space_id: space_id.clone(),
                challenge_nonce,
                proof_bytes: vec![9, 9, 9],
            })
        }

        async fn verify_proof(
            &self,
            _proof: &SpaceAccessProofArtifact,
            _expected_nonce: [u8; 32],
        ) -> anyhow::Result<bool> {
            Ok(true)
        }
    }

    #[derive(Default)]
    struct MockTransport {
        offers: Vec<String>,
        proofs: Vec<String>,
        results: Vec<String>,
    }

    impl MockTransport {
        fn offers(&self) -> Vec<String> {
            self.offers.clone()
        }

        fn proofs(&self) -> Vec<String> {
            self.proofs.clone()
        }

        fn results(&self) -> Vec<String> {
            self.results.clone()
        }
    }

    #[async_trait]
    impl SpaceAccessTransportPort for MockTransport {
        async fn send_offer(&mut self, session_id: &NetSessionId) -> anyhow::Result<()> {
            self.offers.push(session_id.clone());
            Ok(())
        }

        async fn send_proof(&mut self, session_id: &NetSessionId) -> anyhow::Result<()> {
            self.proofs.push(session_id.clone());
            Ok(())
        }

        async fn send_result(&mut self, session_id: &NetSessionId) -> anyhow::Result<()> {
            self.results.push(session_id.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct MockTimer {
        start_calls: Vec<(String, u64)>,
        stop_calls: Vec<String>,
    }

    #[async_trait]
    impl TimerPort for MockTimer {
        async fn start(&mut self, session_id: &CoreSessionId, ttl_secs: u64) -> anyhow::Result<()> {
            self.start_calls.push((session_id.to_string(), ttl_secs));
            Ok(())
        }

        async fn stop(&mut self, session_id: &CoreSessionId) -> anyhow::Result<()> {
            self.stop_calls.push(session_id.to_string());
            Ok(())
        }
    }

    #[derive(Default)]
    struct MockStore {
        joiner_access: Vec<(SpaceId, String)>,
        sponsor_access: Vec<(SpaceId, String)>,
    }

    #[async_trait]
    impl PersistencePort for MockStore {
        async fn persist_joiner_access(
            &mut self,
            space_id: &SpaceId,
            peer_id: &str,
        ) -> anyhow::Result<()> {
            self.joiner_access
                .push((space_id.clone(), peer_id.to_string()));
            Ok(())
        }

        async fn persist_sponsor_access(
            &mut self,
            space_id: &SpaceId,
            peer_id: &str,
        ) -> anyhow::Result<()> {
            self.sponsor_access
                .push((space_id.clone(), peer_id.to_string()));
            Ok(())
        }
    }
}

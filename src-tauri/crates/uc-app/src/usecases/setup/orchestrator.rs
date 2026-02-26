//! Setup orchestrator.
//!
//! This module coordinates the setup state machine and side effects.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, info_span, warn, Instrument};

use uc_core::{
    ports::space::{PersistencePort, ProofPort, SpaceAccessTransportPort},
    ports::{
        DiscoveryPort, NetworkControlPort, NetworkPort, SetupEventPort, SetupStatusPort, TimerPort,
    },
    security::space_access::{
        event::SpaceAccessEvent,
        state::{DenyReason, SpaceAccessState},
    },
    security::{model::Passphrase, SecretString},
    setup::{
        SetupAction, SetupError as SetupDomainError, SetupEvent, SetupState, SetupStateMachine,
    },
};

use crate::usecases::initialize_encryption::InitializeEncryptionError;
use crate::usecases::pairing::{PairingDomainEvent, PairingEventPort, PairingOrchestrator};
use crate::usecases::setup::context::SetupContext;
use crate::usecases::setup::MarkSetupComplete;
use crate::usecases::space_access::{
    SpaceAccessCryptoFactory, SpaceAccessExecutor, SpaceAccessJoinerOffer, SpaceAccessOrchestrator,
};
use crate::usecases::AppLifecycleCoordinator;
use crate::usecases::InitializeEncryption;

/// Errors produced by the setup orchestrator.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("initialize encryption failed: {0}")]
    InitializeEncryption(#[from] InitializeEncryptionError),
    #[error("mark setup complete failed: {0}")]
    MarkSetupComplete(#[from] anyhow::Error),
    #[error("lifecycle boot failed: {0}")]
    LifecycleFailed(#[source] anyhow::Error),
    #[error("setup action not implemented: {0}")]
    ActionNotImplemented(&'static str),
    #[error("pairing operation failed")]
    PairingFailed,
}

/// Orchestrator that drives setup state and side effects.
pub struct SetupOrchestrator {
    context: Arc<SetupContext>,

    selected_peer_id: Arc<Mutex<Option<String>>>,
    pairing_session_id: Arc<Mutex<Option<String>>>,
    joiner_offer: Arc<Mutex<Option<SpaceAccessJoinerOffer>>>,
    passphrase: Arc<Mutex<Option<Passphrase>>>,
    seeded: AtomicBool,

    // 能力型 use cases (依赖注入)
    initialize_encryption: Arc<InitializeEncryption>,
    mark_setup_complete: Arc<MarkSetupComplete>,
    setup_status: Arc<dyn SetupStatusPort>,
    app_lifecycle: Arc<AppLifecycleCoordinator>,
    pairing_orchestrator: Arc<PairingOrchestrator>,
    setup_event_port: Arc<dyn SetupEventPort>,
    space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    discovery_port: Arc<dyn DiscoveryPort>,
    network_control: Arc<dyn NetworkControlPort>,
    crypto_factory: Arc<dyn SpaceAccessCryptoFactory>,
    network_port: Arc<dyn NetworkPort>,
    transport_port: Arc<Mutex<dyn SpaceAccessTransportPort>>,
    proof_port: Arc<dyn ProofPort>,
    timer_port: Arc<Mutex<dyn TimerPort>>,
    persistence_port: Arc<Mutex<dyn PersistencePort>>,
}

#[cfg(test)]
const JOINER_OFFER_WAIT_TIMEOUT: Duration = Duration::from_millis(300);
#[cfg(not(test))]
const JOINER_OFFER_WAIT_TIMEOUT: Duration = Duration::from_secs(3);
const JOINER_OFFER_POLL_INTERVAL: Duration = Duration::from_millis(20);

impl SetupOrchestrator {
    pub fn new(
        initialize_encryption: Arc<InitializeEncryption>,
        mark_setup_complete: Arc<MarkSetupComplete>,
        setup_status: Arc<dyn SetupStatusPort>,
        app_lifecycle: Arc<AppLifecycleCoordinator>,
        pairing_orchestrator: Arc<PairingOrchestrator>,
        setup_event_port: Arc<dyn SetupEventPort>,
        space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
        discovery_port: Arc<dyn DiscoveryPort>,
        network_control: Arc<dyn NetworkControlPort>,
        crypto_factory: Arc<dyn SpaceAccessCryptoFactory>,
        network_port: Arc<dyn NetworkPort>,
        transport_port: Arc<Mutex<dyn SpaceAccessTransportPort>>,
        proof_port: Arc<dyn ProofPort>,
        timer_port: Arc<Mutex<dyn TimerPort>>,
        persistence_port: Arc<Mutex<dyn PersistencePort>>,
    ) -> Self {
        Self {
            context: SetupContext::default().arc(),
            selected_peer_id: Arc::new(Mutex::new(None)),
            pairing_session_id: Arc::new(Mutex::new(None)),
            joiner_offer: Arc::new(Mutex::new(None)),
            passphrase: Arc::new(Mutex::new(None)),
            seeded: AtomicBool::new(false),
            initialize_encryption,
            mark_setup_complete,
            setup_status,
            app_lifecycle,
            pairing_orchestrator,
            setup_event_port,
            space_access_orchestrator,
            discovery_port,
            network_control,
            crypto_factory,
            network_port,
            transport_port,
            proof_port,
            timer_port,
            persistence_port,
        }
    }

    pub async fn new_space(&self) -> Result<SetupState, SetupError> {
        let event = SetupEvent::StartNewSpace;
        self.dispatch(event).await
    }

    pub async fn join_space(&self) -> Result<SetupState, SetupError> {
        let event = SetupEvent::StartJoinSpace;
        self.dispatch(event).await
    }

    pub async fn select_device(&self, peer_id: String) -> Result<SetupState, SetupError> {
        let event = SetupEvent::ChooseJoinPeer { peer_id };
        self.dispatch(event).await
    }

    pub async fn submit_passphrase(
        &self,
        pass1: String,
        _pass2: String,
    ) -> Result<SetupState, SetupError> {
        let event = SetupEvent::SubmitPassphrase {
            passphrase: SecretString::new(pass1),
        };
        self.dispatch(event).await
    }

    pub async fn verify_passphrase(&self, passphrase: String) -> Result<SetupState, SetupError> {
        let event = SetupEvent::SubmitPassphrase {
            passphrase: SecretString::new(passphrase),
        };
        self.dispatch(event).await
    }

    pub async fn confirm_peer_trust(&self) -> Result<SetupState, SetupError> {
        let event = SetupEvent::ConfirmPeerTrust;
        self.dispatch(event).await
    }

    pub async fn cancel_setup(&self) -> Result<SetupState, SetupError> {
        let event = SetupEvent::CancelSetup;
        self.dispatch(event).await
    }

    pub async fn get_state(&self) -> SetupState {
        self.seed_state_from_status().await;
        self.context.get_state().await
    }

    async fn dispatch(&self, event: SetupEvent) -> Result<SetupState, SetupError> {
        let event = self.capture_context(event).await;
        // Acquire dispatch lock to serialize concurrent dispatch calls.
        // This prevents race conditions where multiple calls read the same state
        // and execute duplicate actions.
        let _dispatch_guard = self.context.acquire_dispatch_lock().await;

        let span = info_span!("usecase.setup_orchestrator.dispatch", event = ?event);
        async {
            let mut current = self.context.get_state().await;
            let mut pending_events = vec![event];

            while let Some(event) = pending_events.pop() {
                let from = current.clone();
                let event_name = format!("{:?}", event);
                let (next, actions) = SetupStateMachine::transition(current, event);
                info!(from = ?from, to = ?next, event = %event_name, "setup state transition");
                let follow_up_events = self.execute_actions(actions).await?;
                self.set_state_and_emit(next.clone(), self.current_pairing_session_id().await)
                    .await;
                current = next;
                pending_events.extend(follow_up_events);
            }

            Ok(current)
        }
        .instrument(span)
        .await
    }

    async fn execute_actions(
        &self,
        actions: Vec<SetupAction>,
    ) -> Result<Vec<SetupEvent>, SetupError> {
        let mut follow_up_events = Vec::new();
        for action in actions {
            debug!(?action, "setup executing action");
            match action {
                SetupAction::CreateEncryptedSpace => {
                    let passphrase = self.take_passphrase().await?;
                    self.initialize_encryption.execute(passphrase).await?;
                    follow_up_events.push(SetupEvent::CreateSpaceSucceeded);
                    debug!("setup action CreateEncryptedSpace completed");
                }
                SetupAction::MarkSetupComplete => {
                    self.app_lifecycle
                        .ensure_ready()
                        .await
                        .map_err(SetupError::LifecycleFailed)?;
                    self.mark_setup_complete.execute().await?;
                    debug!("setup action MarkSetupComplete completed");
                }
                SetupAction::EnsureDiscovery => {
                    info!("setup ensure discovery: requesting network start");
                    self.network_control.start_network().await.map_err(|err| {
                        error!(
                            action = "EnsureDiscovery",
                            step = "start_network",
                            error = %err,
                            "setup ensure discovery failed"
                        );
                        SetupError::PairingFailed
                    })?;

                    let discovered_peers = self
                        .discovery_port
                        .list_discovered_peers()
                        .await
                        .map_err(|err| {
                            error!(
                                action = "EnsureDiscovery",
                                step = "list_discovered_peers",
                                error = %err,
                                "setup ensure discovery failed"
                            );
                            SetupError::PairingFailed
                        })?;

                    info!(
                        discovered_peer_count = discovered_peers.len(),
                        "setup ensure discovery: initial discovered peer snapshot"
                    );

                    debug!("setup action EnsureDiscovery completed");
                }
                SetupAction::EnsurePairing => {
                    self.ensure_pairing_session().await?;
                    debug!("setup action EnsurePairing completed");
                }
                SetupAction::ConfirmPeerTrust => {
                    self.confirm_peer_trust_action().await?;
                    debug!("setup action ConfirmPeerTrust completed");
                }
                SetupAction::AbortPairing => {
                    self.abort_pairing_session().await;
                    debug!("setup action AbortPairing completed");
                }
                SetupAction::StartJoinSpaceAccess => {
                    self.start_join_space_access().await?;
                    debug!("setup action StartJoinSpaceAccess completed");
                }
            }
        }

        Ok(follow_up_events)
    }

    async fn start_join_space_access(&self) -> Result<(), SetupError> {
        let passphrase = {
            let guard = self.passphrase.lock().await;
            guard.as_ref().map(|p| SecretString::new(p.0.clone()))
        }
        .ok_or_else(|| {
            error!("start join space access requested without passphrase");
            SetupError::PairingFailed
        })?;

        let pairing_session_id = {
            let guard = self.pairing_session_id.lock().await;
            guard.clone()
        }
        .ok_or_else(|| {
            error!("start join space access requested without pairing session id");
            SetupError::PairingFailed
        })?;

        let peer_id = {
            let guard = self.selected_peer_id.lock().await;
            guard.clone()
        }
        .ok_or_else(|| {
            error!("start join space access requested without selected peer");
            SetupError::PairingFailed
        })?;

        self.network_port
            .open_pairing_session(peer_id, pairing_session_id.clone())
            .await
            .map_err(|err| {
                error!(
                    error = %err,
                    pairing_session_id = %pairing_session_id,
                    "failed to reopen pairing session for space access"
                );
                SetupError::PairingFailed
            })?;

        self.start_space_access_result_listener(pairing_session_id.clone())
            .await;

        let joiner_offer = self
            .wait_for_joiner_offer(&pairing_session_id)
            .await
            .ok_or_else(|| {
                error!(
                    pairing_session_id = %pairing_session_id,
                    timeout_ms = JOINER_OFFER_WAIT_TIMEOUT.as_millis(),
                    "start join space access requested without received offer"
                );
                SetupError::PairingFailed
            })?;

        {
            let mut guard = self.joiner_offer.lock().await;
            *guard = Some(joiner_offer.clone());
        }

        {
            let context = self.space_access_orchestrator.context();
            let mut guard = context.lock().await;
            guard.joiner_offer = Some(joiner_offer.clone());
            guard.joiner_passphrase = Some(SecretString::new(passphrase.expose().to_string()));
            guard.sponsor_peer_id = self.selected_peer_id.lock().await.clone();
        }

        let crypto = self.crypto_factory.build(passphrase);
        let mut transport = self.transport_port.lock().await;
        let mut timer = self.timer_port.lock().await;
        let mut store = self.persistence_port.lock().await;
        let mut executor = SpaceAccessExecutor {
            crypto: crypto.as_ref(),
            net: self.network_port.as_ref(),
            transport: &mut *transport,
            proof: self.proof_port.as_ref(),
            timer: &mut *timer,
            store: &mut *store,
        };

        let space_id = joiner_offer.space_id.clone();

        self.space_access_orchestrator
            .dispatch(
                &mut executor,
                SpaceAccessEvent::JoinRequested {
                    pairing_session_id: pairing_session_id.clone(),
                    ttl_secs: 60,
                },
                Some(pairing_session_id.clone()),
            )
            .await
            .map_err(|err| {
                error!(
                    error = %err,
                    pairing_session_id = %pairing_session_id,
                    "space access join requested failed during setup"
                );
                SetupError::PairingFailed
            })?;

        self.space_access_orchestrator
            .dispatch(
                &mut executor,
                SpaceAccessEvent::OfferAccepted {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id: space_id.clone(),
                    expires_at: Utc::now() + ChronoDuration::seconds(60),
                },
                Some(pairing_session_id.clone()),
            )
            .await
            .map_err(|err| {
                error!(
                    error = %err,
                    pairing_session_id = %pairing_session_id,
                    "space access offer accepted failed during setup"
                );
                SetupError::PairingFailed
            })?;

        self.space_access_orchestrator
            .dispatch(
                &mut executor,
                SpaceAccessEvent::PassphraseSubmitted,
                Some(pairing_session_id.clone()),
            )
            .await
            .map_err(|err| {
                error!(
                    error = %err,
                    pairing_session_id = %pairing_session_id,
                    "space access passphrase submitted failed during setup"
                );
                SetupError::PairingFailed
            })?;

        Ok(())
    }

    async fn wait_for_joiner_offer(
        &self,
        pairing_session_id: &str,
    ) -> Option<SpaceAccessJoinerOffer> {
        let mut waited = Duration::ZERO;

        loop {
            if let Some(local_offer) = {
                let guard = self.joiner_offer.lock().await;
                guard.clone()
            } {
                return Some(local_offer);
            }

            let context_offer = {
                let context = self.space_access_orchestrator.context();
                let guard = context.lock().await;
                guard.joiner_offer.clone()
            };

            if context_offer.is_some() {
                return context_offer;
            }

            if waited >= JOINER_OFFER_WAIT_TIMEOUT {
                return None;
            }

            if waited == Duration::ZERO {
                warn!(
                    pairing_session_id = %pairing_session_id,
                    timeout_ms = JOINER_OFFER_WAIT_TIMEOUT.as_millis(),
                    "waiting for joiner offer before starting join space access"
                );
            }

            sleep(JOINER_OFFER_POLL_INTERVAL).await;
            waited += JOINER_OFFER_POLL_INTERVAL;
        }
    }

    async fn start_space_access_result_listener(&self, session_id: String) {
        let context = Arc::clone(&self.context);
        let setup_event_port = Arc::clone(&self.setup_event_port);
        let mark_setup_complete = Arc::clone(&self.mark_setup_complete);
        let app_lifecycle = Arc::clone(&self.app_lifecycle);
        let pairing_session_id = Arc::clone(&self.pairing_session_id);
        let space_access_orchestrator = Arc::clone(&self.space_access_orchestrator);

        tokio::spawn(async move {
            loop {
                if !Self::pairing_session_matches(&pairing_session_id, &session_id).await {
                    break;
                }

                let space_access_state = space_access_orchestrator.get_state().await;
                let Some(setup_event) =
                    Self::map_setup_event_from_space_access_state(&space_access_state, &session_id)
                else {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                };

                let _dispatch_guard = context.acquire_dispatch_lock().await;
                let current = context.get_state().await;
                let (next, actions) = SetupStateMachine::transition(current, setup_event);

                for action in actions {
                    if matches!(action, SetupAction::MarkSetupComplete) {
                        if let Err(err) = app_lifecycle.ensure_ready().await {
                            error!(
                                error = %err,
                                session_id = %session_id,
                                "lifecycle ensure_ready failed after space access completion"
                            );
                            let failed_state = SetupState::JoinSpaceInputPassphrase {
                                error: Some(SetupDomainError::PairingFailed),
                            };
                            context.set_state(failed_state.clone()).await;
                            setup_event_port
                                .emit_setup_state_changed(failed_state, Some(session_id.clone()))
                                .await;
                            return;
                        }

                        if let Err(err) = mark_setup_complete.execute().await {
                            error!(
                                error = %err,
                                session_id = %session_id,
                                "mark setup complete failed after space access completion"
                            );
                            let failed_state = SetupState::JoinSpaceInputPassphrase {
                                error: Some(SetupDomainError::PairingFailed),
                            };
                            context.set_state(failed_state.clone()).await;
                            setup_event_port
                                .emit_setup_state_changed(failed_state, Some(session_id.clone()))
                                .await;
                            return;
                        }
                    }
                }

                context.set_state(next.clone()).await;
                setup_event_port
                    .emit_setup_state_changed(next, Some(session_id.clone()))
                    .await;
                break;
            }
        });
    }

    fn map_setup_event_from_space_access_state(
        space_access_state: &SpaceAccessState,
        session_id: &str,
    ) -> Option<SetupEvent> {
        match space_access_state {
            SpaceAccessState::Granted {
                pairing_session_id, ..
            } if pairing_session_id == session_id => Some(SetupEvent::JoinSpaceSucceeded),
            SpaceAccessState::Denied {
                pairing_session_id,
                reason,
                ..
            } if pairing_session_id == session_id => Some(SetupEvent::JoinSpaceFailed {
                error: Self::map_space_access_deny_reason(reason),
            }),
            SpaceAccessState::Cancelled {
                pairing_session_id, ..
            } if pairing_session_id == session_id => Some(SetupEvent::JoinSpaceFailed {
                error: SetupDomainError::PairingFailed,
            }),
            _ => None,
        }
    }

    fn map_space_access_deny_reason(reason: &DenyReason) -> SetupDomainError {
        match reason {
            DenyReason::InvalidProof => SetupDomainError::PassphraseInvalidOrMismatch,
            DenyReason::Expired
            | DenyReason::SpaceMismatch
            | DenyReason::SessionMismatch
            | DenyReason::InternalError => SetupDomainError::PairingFailed,
        }
    }

    async fn capture_context(&self, event: SetupEvent) -> SetupEvent {
        match event {
            SetupEvent::ChooseJoinPeer { peer_id } => {
                *self.selected_peer_id.lock().await = Some(peer_id.clone());
                SetupEvent::ChooseJoinPeer { peer_id }
            }
            SetupEvent::SubmitPassphrase { passphrase } => {
                let (event_passphrase, stored_passphrase) = Self::split_passphrase(passphrase);
                *self.passphrase.lock().await = Some(stored_passphrase);
                SetupEvent::SubmitPassphrase {
                    passphrase: event_passphrase,
                }
            }
            SetupEvent::VerifyPassphrase { passphrase } => {
                let (event_passphrase, stored_passphrase) = Self::split_passphrase(passphrase);
                *self.passphrase.lock().await = Some(stored_passphrase);
                SetupEvent::SubmitPassphrase {
                    passphrase: event_passphrase,
                }
            }
            other => other,
        }
    }

    async fn take_passphrase(&self) -> Result<Passphrase, SetupError> {
        let mut guard = self.passphrase.lock().await;
        guard
            .take()
            .ok_or(SetupError::ActionNotImplemented("CreateEncryptedSpace"))
    }

    fn split_passphrase(passphrase: SecretString) -> (SecretString, Passphrase) {
        let raw = passphrase.into_inner();
        let stored = Passphrase(raw.clone());
        (SecretString::new(raw), stored)
    }

    async fn current_pairing_session_id(&self) -> Option<String> {
        let guard = self.pairing_session_id.lock().await;
        guard.clone()
    }

    async fn set_state_and_emit(&self, state: SetupState, session_id: Option<String>) {
        self.context.set_state(state.clone()).await;
        self.setup_event_port
            .emit_setup_state_changed(state, session_id)
            .await;
    }

    async fn seed_state_from_status(&self) {
        if self.seeded.swap(true, Ordering::SeqCst) {
            return;
        }

        match self.setup_status.get_status().await {
            Ok(status) => {
                if status.has_completed {
                    self.set_state_and_emit(SetupState::Completed, None).await;
                }
            }
            Err(err) => {
                error!(error = %err, "failed to load setup status");
            }
        }
    }

    async fn ensure_pairing_session(&self) -> Result<(), SetupError> {
        let peer_id = {
            let guard = self.selected_peer_id.lock().await;
            guard.clone()
        }
        .ok_or_else(|| {
            error!("ensure pairing requested without selected peer");
            SetupError::PairingFailed
        })?;

        let session_id = self
            .pairing_orchestrator
            .initiate_pairing(peer_id.clone())
            .await
            .map_err(|err| {
                error!(error = %err, peer_id = %peer_id, "failed to initiate pairing session");
                SetupError::PairingFailed
            })?;

        {
            let mut guard = self.pairing_session_id.lock().await;
            *guard = Some(session_id.clone());
        }

        self.start_pairing_verification_listener(session_id).await;
        Ok(())
    }

    async fn confirm_peer_trust_action(&self) -> Result<(), SetupError> {
        let session_id = {
            let guard = self.pairing_session_id.lock().await;
            guard.clone()
        }
        .ok_or_else(|| {
            error!("confirm peer trust requested without active session");
            SetupError::PairingFailed
        })?;

        self.pairing_orchestrator
            .user_accept_pairing(&session_id)
            .await
            .map_err(|err| {
                error!(error = %err, session_id = %session_id, "failed to accept pairing session");
                SetupError::PairingFailed
            })?;

        self.set_state_and_emit(
            SetupState::JoinSpaceInputPassphrase { error: None },
            Some(session_id),
        )
        .await;
        Ok(())
    }

    async fn abort_pairing_session(&self) {
        let session_id = {
            let mut guard = self.pairing_session_id.lock().await;
            guard.take()
        };
        if let Some(session_id) = session_id {
            if let Err(err) = self
                .pairing_orchestrator
                .user_reject_pairing(&session_id)
                .await
            {
                warn!(error = %err, session_id = %session_id, "failed to reject pairing session");
            }
        }

        {
            let mut guard = self.selected_peer_id.lock().await;
            guard.take();
        }
    }

    async fn start_pairing_verification_listener(&self, session_id: String) {
        let mut event_rx = match self.pairing_orchestrator.subscribe().await {
            Ok(rx) => rx,
            Err(err) => {
                error!(
                    error = %err,
                    session_id = %session_id,
                    "failed to subscribe pairing events"
                );
                return;
            }
        };
        let context = Arc::clone(&self.context);
        let pairing_session_id = Arc::clone(&self.pairing_session_id);
        let joiner_offer = Arc::clone(&self.joiner_offer);
        let setup_event_port = Arc::clone(&self.setup_event_port);

        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if !Self::pairing_session_matches(&pairing_session_id, &session_id).await {
                    break;
                }
                match event {
                    PairingDomainEvent::PairingVerificationRequired {
                        session_id: event_session_id,
                        short_code,
                        peer_fingerprint,
                        ..
                    } if event_session_id == session_id => {
                        let next_state = SetupState::JoinSpaceConfirmPeer {
                            short_code,
                            peer_fingerprint: Some(peer_fingerprint),
                            error: None,
                        };
                        context.set_state(next_state.clone()).await;
                        setup_event_port
                            .emit_setup_state_changed(next_state, Some(session_id.clone()))
                            .await;
                    }
                    PairingDomainEvent::PairingFailed {
                        session_id: event_session_id,
                        ..
                    } if event_session_id == session_id => {
                        let next_state = SetupState::JoinSpaceInputPassphrase {
                            error: Some(SetupDomainError::PairingFailed),
                        };
                        context.set_state(next_state.clone()).await;
                        setup_event_port
                            .emit_setup_state_changed(next_state, Some(session_id.clone()))
                            .await;
                        break;
                    }
                    PairingDomainEvent::PairingSucceeded {
                        session_id: event_session_id,
                        ..
                    } if event_session_id == session_id => {
                        continue;
                    }
                    PairingDomainEvent::KeyslotReceived {
                        session_id: event_session_id,
                        keyslot_file,
                        challenge,
                        ..
                    } if event_session_id == session_id => {
                        let challenge_len = challenge.len();
                        let challenge_nonce: [u8; 32] = match challenge.try_into() {
                            Ok(nonce) => nonce,
                            Err(_) => {
                                warn!(
                                    session_id = %session_id,
                                    challenge_len,
                                    "received invalid keyslot challenge length"
                                );
                                continue;
                            }
                        };
                        let keyslot: uc_core::security::model::KeySlot =
                            keyslot_file.clone().into();
                        let keyslot_blob = match serde_json::to_vec(&keyslot) {
                            Ok(blob) => blob,
                            Err(err) => {
                                warn!(
                                    error = %err,
                                    session_id = %session_id,
                                    "failed to serialize keyslot for space access"
                                );
                                continue;
                            }
                        };
                        let offer = SpaceAccessJoinerOffer {
                            space_id: uc_core::ids::SpaceId::from(
                                keyslot_file.scope.profile_id.as_str(),
                            ),
                            keyslot_blob,
                            challenge_nonce,
                        };

                        *joiner_offer.lock().await = Some(offer);
                    }
                    _ => {}
                }
            }
        });
    }

    async fn pairing_session_matches(
        pairing_session: &Arc<Mutex<Option<String>>>,
        target_session: &str,
    ) -> bool {
        let guard = pairing_session.lock().await;
        guard
            .as_ref()
            .map(|current| current == target_session)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecases::pairing::{PairingConfig, PairingOrchestrator};
    use crate::usecases::space_access::SpaceAccessOrchestrator;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::time::{sleep, Duration, Instant};
    use uc_core::network::{DiscoveredPeer, PairedDevice, PairingState};
    use uc_core::ports::network_control::NetworkControlPort;
    use uc_core::ports::security::encryption::EncryptionPort;
    use uc_core::ports::security::encryption_session::EncryptionSessionPort;
    use uc_core::ports::security::encryption_state::EncryptionStatePort;
    use uc_core::ports::security::key_material::KeyMaterialPort;
    use uc_core::ports::security::key_scope::{KeyScopePort, ScopeError};
    use uc_core::ports::space::{CryptoPort, PersistencePort, ProofPort, SpaceAccessTransportPort};
    use uc_core::ports::watcher_control::{WatcherControlError, WatcherControlPort};
    use uc_core::ports::{
        DiscoveryPort, NetworkPort, PairedDeviceRepositoryError, PairedDeviceRepositoryPort,
        SetupEventPort, TimerPort,
    };
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionError, EncryptionFormatVersion, KdfAlgorithm,
        KdfParams, KdfParamsV1, Kek, KeyScope, KeySlot, KeySlotFile, KeySlotVersion, MasterKey,
        Passphrase,
    };
    use uc_core::security::space_access::SpaceAccessProofArtifact;
    use uc_core::security::state::{EncryptionState, EncryptionStateError};
    use uc_core::setup::{SetupError as SetupDomainError, SetupStatus};
    use uc_core::PeerId;

    use crate::usecases::clipboard::ClipboardIntegrationMode;
    use crate::usecases::{
        AppLifecycleCoordinatorDeps, LifecycleEvent, LifecycleEventEmitter, LifecycleState,
        LifecycleStatusPort, SessionReadyEmitter, StartClipboardWatcher, StartNetworkAfterUnlock,
    };

    struct MockSetupStatusPort {
        status: StdMutex<SetupStatus>,
        set_calls: AtomicUsize,
    }

    #[derive(Default)]
    struct MockSetupEventPort {
        emitted: tokio::sync::Mutex<Vec<(SetupState, Option<String>)>>,
    }

    impl MockSetupEventPort {
        async fn snapshot(&self) -> Vec<(SetupState, Option<String>)> {
            self.emitted.lock().await.clone()
        }
    }

    #[async_trait]
    impl SetupEventPort for MockSetupEventPort {
        async fn emit_setup_state_changed(&self, state: SetupState, session_id: Option<String>) {
            self.emitted.lock().await.push((state, session_id));
        }
    }

    impl MockSetupStatusPort {
        fn new(status: SetupStatus) -> Self {
            Self {
                status: StdMutex::new(status),
                set_calls: AtomicUsize::new(0),
            }
        }

        fn set_call_count(&self) -> usize {
            self.set_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl SetupStatusPort for MockSetupStatusPort {
        async fn get_status(&self) -> anyhow::Result<SetupStatus> {
            Ok(self.status.lock().unwrap().clone())
        }

        async fn set_status(&self, status: &SetupStatus) -> anyhow::Result<()> {
            *self.status.lock().unwrap() = status.clone();
            self.set_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct NoopEncryption;

    #[async_trait]
    impl EncryptionPort for NoopEncryption {
        async fn derive_kek(
            &self,
            _passphrase: &Passphrase,
            _salt: &[u8],
            _kdf_params: &uc_core::security::model::KdfParams,
        ) -> Result<Kek, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn wrap_master_key(
            &self,
            _kek: &Kek,
            _master_key: &MasterKey,
            _aead: EncryptionAlgo,
        ) -> Result<EncryptedBlob, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn unwrap_master_key(
            &self,
            _kek: &Kek,
            _blob: &EncryptedBlob,
        ) -> Result<MasterKey, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn encrypt_blob(
            &self,
            _master_key: &MasterKey,
            _plaintext: &[u8],
            _aad: &[u8],
            _algo: EncryptionAlgo,
        ) -> Result<EncryptedBlob, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn decrypt_blob(
            &self,
            _master_key: &MasterKey,
            _blob: &EncryptedBlob,
            _aad: &[u8],
        ) -> Result<Vec<u8>, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }
    }

    struct NoopKeyMaterial;

    #[async_trait]
    impl KeyMaterialPort for NoopKeyMaterial {
        async fn load_keyslot(&self, _scope: &KeyScope) -> Result<KeySlot, EncryptionError> {
            Err(EncryptionError::KeyNotFound)
        }

        async fn store_keyslot(&self, _keyslot: &KeySlot) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn delete_keyslot(&self, _scope: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn load_kek(&self, _scope: &KeyScope) -> Result<Kek, EncryptionError> {
            Err(EncryptionError::KeyNotFound)
        }

        async fn store_kek(&self, _scope: &KeyScope, _kek: &Kek) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn delete_kek(&self, _scope: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    struct NoopKeyScope;

    #[async_trait]
    impl KeyScopePort for NoopKeyScope {
        async fn current_scope(&self) -> Result<KeyScope, ScopeError> {
            Err(ScopeError::FailedToGetCurrentScope)
        }
    }

    struct NoopEncryptionState;

    #[async_trait]
    impl EncryptionStatePort for NoopEncryptionState {
        async fn load_state(&self) -> Result<EncryptionState, EncryptionStateError> {
            Err(EncryptionStateError::LoadError("noop".to_string()))
        }

        async fn persist_initialized(&self) -> Result<(), EncryptionStateError> {
            Ok(())
        }
    }

    struct NoopEncryptionSession;

    #[async_trait]
    impl EncryptionSessionPort for NoopEncryptionSession {
        async fn is_ready(&self) -> bool {
            false
        }

        async fn get_master_key(&self) -> Result<MasterKey, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn set_master_key(&self, _master_key: MasterKey) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn clear(&self) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    struct SucceedEncryption;

    #[async_trait]
    impl EncryptionPort for SucceedEncryption {
        async fn derive_kek(
            &self,
            _passphrase: &Passphrase,
            _salt: &[u8],
            _kdf_params: &uc_core::security::model::KdfParams,
        ) -> Result<Kek, EncryptionError> {
            Ok(Kek([0u8; 32]))
        }

        async fn wrap_master_key(
            &self,
            _kek: &Kek,
            _master_key: &MasterKey,
            _aead: EncryptionAlgo,
        ) -> Result<EncryptedBlob, EncryptionError> {
            Ok(EncryptedBlob {
                version: uc_core::security::model::EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![0u8; 24],
                ciphertext: vec![1u8; 32],
                aad_fingerprint: None,
            })
        }

        async fn unwrap_master_key(
            &self,
            _kek: &Kek,
            _blob: &EncryptedBlob,
        ) -> Result<MasterKey, EncryptionError> {
            Ok(MasterKey([0u8; 32]))
        }

        async fn encrypt_blob(
            &self,
            _master_key: &MasterKey,
            _plaintext: &[u8],
            _aad: &[u8],
            _algo: EncryptionAlgo,
        ) -> Result<EncryptedBlob, EncryptionError> {
            Ok(EncryptedBlob {
                version: uc_core::security::model::EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![0u8; 24],
                ciphertext: vec![1u8; 32],
                aad_fingerprint: None,
            })
        }

        async fn decrypt_blob(
            &self,
            _master_key: &MasterKey,
            _blob: &EncryptedBlob,
            _aad: &[u8],
        ) -> Result<Vec<u8>, EncryptionError> {
            Ok(vec![0u8; 32])
        }
    }

    struct SucceedKeyMaterial;

    #[async_trait]
    impl KeyMaterialPort for SucceedKeyMaterial {
        async fn load_keyslot(&self, _scope: &KeyScope) -> Result<KeySlot, EncryptionError> {
            Err(EncryptionError::KeyNotFound)
        }

        async fn store_keyslot(&self, _keyslot: &KeySlot) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn delete_keyslot(&self, _scope: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn load_kek(&self, _scope: &KeyScope) -> Result<Kek, EncryptionError> {
            Err(EncryptionError::KeyNotFound)
        }

        async fn store_kek(&self, _scope: &KeyScope, _kek: &Kek) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn delete_kek(&self, _scope: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    struct SucceedKeyScope;

    #[async_trait]
    impl KeyScopePort for SucceedKeyScope {
        async fn current_scope(&self) -> Result<KeyScope, ScopeError> {
            Ok(KeyScope {
                profile_id: "default".to_string(),
            })
        }
    }

    struct SucceedEncryptionState;

    #[async_trait]
    impl EncryptionStatePort for SucceedEncryptionState {
        async fn load_state(&self) -> Result<EncryptionState, EncryptionStateError> {
            Ok(EncryptionState::Uninitialized)
        }

        async fn persist_initialized(&self) -> Result<(), EncryptionStateError> {
            Ok(())
        }
    }

    struct SucceedEncryptionSession;

    #[async_trait]
    impl EncryptionSessionPort for SucceedEncryptionSession {
        async fn is_ready(&self) -> bool {
            false
        }

        async fn get_master_key(&self) -> Result<MasterKey, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn set_master_key(&self, _master_key: MasterKey) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn clear(&self) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    // -- Lifecycle mocks -------------------------------------------------------

    struct MockWatcherControl;

    #[async_trait]
    impl WatcherControlPort for MockWatcherControl {
        async fn start_watcher(&self) -> Result<(), WatcherControlError> {
            Ok(())
        }

        async fn stop_watcher(&self) -> Result<(), WatcherControlError> {
            Ok(())
        }
    }

    struct MockNetworkControl;

    #[async_trait]
    impl NetworkControlPort for MockNetworkControl {
        async fn start_network(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct MockSessionReadyEmitter;

    #[async_trait]
    impl SessionReadyEmitter for MockSessionReadyEmitter {
        async fn emit_ready(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct MockLifecycleStatus;

    #[async_trait]
    impl LifecycleStatusPort for MockLifecycleStatus {
        async fn set_state(&self, _state: LifecycleState) -> anyhow::Result<()> {
            Ok(())
        }

        async fn get_state(&self) -> LifecycleState {
            LifecycleState::Idle
        }
    }

    struct MockLifecycleEventEmitter;

    #[async_trait]
    impl LifecycleEventEmitter for MockLifecycleEventEmitter {
        async fn emit_lifecycle_event(&self, _event: LifecycleEvent) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct NoopPairedDeviceRepository;

    #[async_trait]
    impl PairedDeviceRepositoryPort for NoopPairedDeviceRepository {
        async fn get_by_peer_id(
            &self,
            _peer_id: &PeerId,
        ) -> Result<Option<PairedDevice>, PairedDeviceRepositoryError> {
            Ok(None)
        }

        async fn list_all(&self) -> Result<Vec<PairedDevice>, PairedDeviceRepositoryError> {
            Ok(Vec::new())
        }

        async fn upsert(&self, _device: PairedDevice) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn set_state(
            &self,
            _peer_id: &PeerId,
            _state: PairingState,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &PeerId,
            _last_seen_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(&self, _peer_id: &PeerId) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    struct NoopDiscoveryPort;

    #[async_trait]
    impl DiscoveryPort for NoopDiscoveryPort {
        async fn list_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
            Ok(Vec::new())
        }
    }

    struct NoopSpaceAccessCrypto;

    #[async_trait]
    impl CryptoPort for NoopSpaceAccessCrypto {
        async fn generate_nonce32(&self) -> [u8; 32] {
            [0u8; 32]
        }

        async fn export_keyslot_blob(
            &self,
            _space_id: &uc_core::ids::SpaceId,
        ) -> anyhow::Result<KeySlot> {
            Err(anyhow::anyhow!("noop crypto export_keyslot_blob"))
        }

        async fn derive_master_key_from_keyslot(
            &self,
            _keyslot_blob: &[u8],
            _passphrase: SecretString,
        ) -> anyhow::Result<MasterKey> {
            Err(anyhow::anyhow!(
                "noop crypto derive_master_key_from_keyslot"
            ))
        }
    }

    struct NoopSpaceAccessCryptoFactory;

    impl SpaceAccessCryptoFactory for NoopSpaceAccessCryptoFactory {
        fn build(&self, _passphrase: SecretString) -> Box<dyn CryptoPort> {
            Box::new(NoopSpaceAccessCrypto)
        }
    }

    struct SucceedSpaceAccessCrypto;

    #[async_trait]
    impl CryptoPort for SucceedSpaceAccessCrypto {
        async fn generate_nonce32(&self) -> [u8; 32] {
            [1u8; 32]
        }

        async fn export_keyslot_blob(
            &self,
            _space_id: &uc_core::ids::SpaceId,
        ) -> anyhow::Result<KeySlot> {
            Err(anyhow::anyhow!("unused in joiner flow"))
        }

        async fn derive_master_key_from_keyslot(
            &self,
            _keyslot_blob: &[u8],
            _passphrase: SecretString,
        ) -> anyhow::Result<MasterKey> {
            MasterKey::from_bytes(&[7u8; 32]).map_err(|err| anyhow::anyhow!(err.to_string()))
        }
    }

    struct SucceedSpaceAccessCryptoFactory;

    impl SpaceAccessCryptoFactory for SucceedSpaceAccessCryptoFactory {
        fn build(&self, _passphrase: SecretString) -> Box<dyn CryptoPort> {
            Box::new(SucceedSpaceAccessCrypto)
        }
    }

    struct NoopSpaceAccessNetworkPort;

    #[async_trait]
    impl NetworkPort for NoopSpaceAccessNetworkPort {
        async fn send_clipboard(
            &self,
            _peer_id: &str,
            _encrypted_data: Vec<u8>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn broadcast_clipboard(&self, _encrypted_data: Vec<u8>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe_clipboard(
            &self,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<uc_core::network::ClipboardMessage>>
        {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }

        async fn get_discovered_peers(
            &self,
        ) -> anyhow::Result<Vec<uc_core::network::DiscoveredPeer>> {
            Ok(vec![])
        }

        async fn get_connected_peers(
            &self,
        ) -> anyhow::Result<Vec<uc_core::network::ConnectedPeer>> {
            Ok(vec![])
        }

        fn local_peer_id(&self) -> String {
            "local".to_string()
        }

        async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
            Ok(())
        }

        async fn open_pairing_session(
            &self,
            _peer_id: String,
            _session_id: String,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_pairing_on_session(
            &self,
            _session_id: String,
            _message: uc_core::network::PairingMessage,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn close_pairing_session(
            &self,
            _session_id: String,
            _reason: Option<String>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn unpair_device(&self, _peer_id: String) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe_events(
            &self,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<uc_core::network::NetworkEvent>> {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }
    }

    struct NoopSpaceAccessTransportPort;

    #[async_trait]
    impl SpaceAccessTransportPort for NoopSpaceAccessTransportPort {
        async fn send_offer(
            &mut self,
            _session_id: &uc_core::network::SessionId,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_proof(
            &mut self,
            _session_id: &uc_core::network::SessionId,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_result(
            &mut self,
            _session_id: &uc_core::network::SessionId,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct NoopSpaceAccessProofPort;

    #[async_trait]
    impl ProofPort for NoopSpaceAccessProofPort {
        async fn build_proof(
            &self,
            pairing_session_id: &uc_core::SessionId,
            space_id: &uc_core::ids::SpaceId,
            challenge_nonce: [u8; 32],
            _master_key: &MasterKey,
        ) -> anyhow::Result<SpaceAccessProofArtifact> {
            Ok(SpaceAccessProofArtifact {
                pairing_session_id: pairing_session_id.clone(),
                space_id: space_id.clone(),
                challenge_nonce,
                proof_bytes: vec![],
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

    struct NoopSpaceAccessTimerPort;

    #[async_trait]
    impl TimerPort for NoopSpaceAccessTimerPort {
        async fn start(
            &mut self,
            _session_id: &uc_core::SessionId,
            _ttl_secs: u64,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&mut self, _session_id: &uc_core::SessionId) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct NoopSpaceAccessPersistencePort;

    #[async_trait]
    impl PersistencePort for NoopSpaceAccessPersistencePort {
        async fn persist_joiner_access(
            &mut self,
            _space_id: &uc_core::ids::SpaceId,
            _peer_id: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn persist_sponsor_access(
            &mut self,
            _space_id: &uc_core::ids::SpaceId,
            _peer_id: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn build_mock_lifecycle() -> Arc<AppLifecycleCoordinator> {
        Arc::new(AppLifecycleCoordinator::from_deps(
            AppLifecycleCoordinatorDeps {
                watcher: Arc::new(StartClipboardWatcher::new(
                    Arc::new(MockWatcherControl),
                    ClipboardIntegrationMode::Full,
                )),
                network: Arc::new(StartNetworkAfterUnlock::new(Arc::new(MockNetworkControl))),
                announcer: None,
                emitter: Arc::new(MockSessionReadyEmitter),
                status: Arc::new(MockLifecycleStatus),
                lifecycle_emitter: Arc::new(MockLifecycleEventEmitter),
            },
        ))
    }

    fn build_initialize_encryption() -> Arc<InitializeEncryption> {
        Arc::new(InitializeEncryption::from_ports(
            Arc::new(NoopEncryption),
            Arc::new(NoopKeyMaterial),
            Arc::new(NoopKeyScope),
            Arc::new(NoopEncryptionState),
            Arc::new(NoopEncryptionSession),
        ))
    }

    fn build_initialize_encryption_success() -> Arc<InitializeEncryption> {
        Arc::new(InitializeEncryption::from_ports(
            Arc::new(SucceedEncryption),
            Arc::new(SucceedKeyMaterial),
            Arc::new(SucceedKeyScope),
            Arc::new(SucceedEncryptionState),
            Arc::new(SucceedEncryptionSession),
        ))
    }

    fn build_pairing_orchestrator() -> Arc<PairingOrchestrator> {
        let repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            repo,
            "test-device".to_string(),
            "test-device-id".to_string(),
            "test-peer-id".to_string(),
            vec![1; 32],
        );
        Arc::new(orchestrator)
    }

    fn build_pairing_orchestrator_with_actions() -> (
        Arc<PairingOrchestrator>,
        tokio::sync::Mutex<
            tokio::sync::mpsc::Receiver<uc_core::network::pairing_state_machine::PairingAction>,
        >,
    ) {
        let repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            repo,
            "test-device".to_string(),
            "test-device-id".to_string(),
            "test-peer-id".to_string(),
            vec![1; 32],
        );
        (Arc::new(orchestrator), tokio::sync::Mutex::new(rx))
    }

    fn build_space_access_orchestrator() -> Arc<SpaceAccessOrchestrator> {
        Arc::new(SpaceAccessOrchestrator::new())
    }

    fn build_discovery_port() -> Arc<dyn DiscoveryPort> {
        Arc::new(NoopDiscoveryPort)
    }

    fn build_network_control() -> Arc<dyn NetworkControlPort> {
        Arc::new(MockNetworkControl)
    }

    fn build_crypto_factory() -> Arc<dyn SpaceAccessCryptoFactory> {
        Arc::new(NoopSpaceAccessCryptoFactory)
    }

    fn build_success_crypto_factory() -> Arc<dyn SpaceAccessCryptoFactory> {
        Arc::new(SucceedSpaceAccessCryptoFactory)
    }

    fn build_network_port() -> Arc<dyn NetworkPort> {
        Arc::new(NoopSpaceAccessNetworkPort)
    }

    fn build_transport_port() -> Arc<Mutex<dyn SpaceAccessTransportPort>> {
        Arc::new(Mutex::new(NoopSpaceAccessTransportPort))
    }

    fn build_proof_port() -> Arc<dyn ProofPort> {
        Arc::new(NoopSpaceAccessProofPort)
    }

    fn build_timer_port() -> Arc<Mutex<dyn TimerPort>> {
        Arc::new(Mutex::new(NoopSpaceAccessTimerPort))
    }

    fn build_persistence_port() -> Arc<Mutex<dyn PersistencePort>> {
        Arc::new(Mutex::new(NoopSpaceAccessPersistencePort))
    }

    fn build_setup_event_port() -> Arc<dyn SetupEventPort> {
        Arc::new(MockSetupEventPort::default())
    }

    fn build_orchestrator_with_initialize_encryption_and_crypto_factory(
        setup_status: Arc<dyn SetupStatusPort>,
        initialize_encryption: Arc<InitializeEncryption>,
        crypto_factory: Arc<dyn SpaceAccessCryptoFactory>,
    ) -> SetupOrchestrator {
        let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));

        SetupOrchestrator::new(
            initialize_encryption,
            mark_setup_complete,
            setup_status,
            build_mock_lifecycle(),
            build_pairing_orchestrator(),
            build_setup_event_port(),
            build_space_access_orchestrator(),
            build_discovery_port(),
            build_network_control(),
            crypto_factory,
            build_network_port(),
            build_transport_port(),
            build_proof_port(),
            build_timer_port(),
            build_persistence_port(),
        )
    }

    fn build_orchestrator_with_initialize_encryption(
        setup_status: Arc<dyn SetupStatusPort>,
        initialize_encryption: Arc<InitializeEncryption>,
    ) -> SetupOrchestrator {
        build_orchestrator_with_initialize_encryption_and_crypto_factory(
            setup_status,
            initialize_encryption,
            build_crypto_factory(),
        )
    }

    fn build_orchestrator(setup_status: Arc<dyn SetupStatusPort>) -> SetupOrchestrator {
        build_orchestrator_with_initialize_encryption(setup_status, build_initialize_encryption())
    }

    fn sample_keyslot_file(profile_id: &str) -> KeySlotFile {
        KeySlotFile {
            version: KeySlotVersion::V1,
            scope: KeyScope {
                profile_id: profile_id.to_string(),
            },
            kdf: KdfParams {
                alg: KdfAlgorithm::Argon2id,
                params: KdfParamsV1 {
                    mem_kib: 1024,
                    iters: 2,
                    parallelism: 1,
                },
            },
            salt: vec![1, 2, 3, 4],
            wrapped_master_key: EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![9; 24],
                ciphertext: vec![7; 32],
                aad_fingerprint: None,
            },
            created_at: None,
            updated_at: None,
        }
    }

    #[tokio::test]
    async fn get_state_seeds_completed_when_setup_status_completed() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus {
            has_completed: true,
        }));
        let orchestrator = build_orchestrator(setup_status);

        let state = orchestrator.get_state().await;

        assert_eq!(state, SetupState::Completed);
    }

    #[tokio::test]
    async fn join_space_success_marks_setup_complete() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator(setup_status.clone());

        orchestrator
            .context
            .set_state(SetupState::ProcessingJoinSpace { message: None })
            .await;

        orchestrator
            .dispatch(SetupEvent::JoinSpaceSucceeded)
            .await
            .unwrap();

        let status = setup_status.get_status().await.unwrap();

        assert!(status.has_completed);
        assert_eq!(setup_status.set_call_count(), 1);
    }

    #[tokio::test]
    async fn create_space_success_marks_setup_complete() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator_with_initialize_encryption(
            setup_status.clone(),
            build_initialize_encryption_success(),
        );

        orchestrator.new_space().await.unwrap();
        let state = orchestrator
            .submit_passphrase("secret".to_string(), "secret".to_string())
            .await
            .unwrap();

        assert_eq!(state, SetupState::Completed);
        let status = setup_status.get_status().await.unwrap();
        assert!(status.has_completed);
        assert_eq!(setup_status.set_call_count(), 1);
    }

    #[tokio::test]
    async fn select_device_dispatch_emits_processing_join_space_event() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));
        let setup_event_port = Arc::new(MockSetupEventPort::default());
        let (pairing_orchestrator, action_rx) = build_pairing_orchestrator_with_actions();
        let orchestrator = SetupOrchestrator::new(
            build_initialize_encryption(),
            mark_setup_complete,
            setup_status,
            build_mock_lifecycle(),
            pairing_orchestrator,
            setup_event_port.clone(),
            build_space_access_orchestrator(),
            build_discovery_port(),
            build_network_control(),
            build_crypto_factory(),
            build_network_port(),
            build_transport_port(),
            build_proof_port(),
            build_timer_port(),
            build_persistence_port(),
        );

        orchestrator.join_space().await.unwrap();
        let state = orchestrator
            .select_device("peer-event".to_string())
            .await
            .unwrap();

        {
            let mut rx = action_rx.lock().await;
            assert!(
                rx.try_recv().is_ok(),
                "pairing orchestrator should queue initial action"
            );
        }

        assert!(matches!(state, SetupState::ProcessingJoinSpace { .. }));

        let emitted = setup_event_port.snapshot().await;
        assert!(emitted
            .iter()
            .any(|(state, _)| matches!(state, SetupState::ProcessingJoinSpace { .. })));
    }

    #[tokio::test]
    async fn pairing_verification_listener_emits_join_space_confirm_peer_event() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));
        let setup_event_port = Arc::new(MockSetupEventPort::default());
        let (pairing_orchestrator, action_rx) = build_pairing_orchestrator_with_actions();
        let orchestrator = SetupOrchestrator::new(
            build_initialize_encryption(),
            mark_setup_complete,
            setup_status,
            build_mock_lifecycle(),
            pairing_orchestrator,
            setup_event_port.clone(),
            build_space_access_orchestrator(),
            build_discovery_port(),
            build_network_control(),
            build_crypto_factory(),
            build_network_port(),
            build_transport_port(),
            build_proof_port(),
            build_timer_port(),
            build_persistence_port(),
        );

        orchestrator.join_space().await.unwrap();
        orchestrator
            .select_device("peer-verify".to_string())
            .await
            .unwrap();

        {
            let mut rx = action_rx.lock().await;
            assert!(
                rx.try_recv().is_ok(),
                "pairing orchestrator should queue initial action"
            );
        }

        let session_deadline = Instant::now() + Duration::from_secs(1);
        let session_id = loop {
            if let Some(session_id) = orchestrator.pairing_session_id.lock().await.clone() {
                break session_id;
            }
            assert!(
                Instant::now() < session_deadline,
                "pairing session id was not created"
            );
            sleep(Duration::from_millis(10)).await;
        };

        orchestrator
            .pairing_orchestrator
            .handle_challenge(
                &session_id,
                "peer-verify",
                uc_core::network::protocol::PairingChallenge {
                    session_id: session_id.clone(),
                    pin: "654321".to_string(),
                    device_name: "remote-device".to_string(),
                    device_id: "remote-device-id".to_string(),
                    identity_pubkey: vec![9; 32],
                    nonce: vec![2; 32],
                },
            )
            .await
            .unwrap();

        let emit_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let emitted = setup_event_port.snapshot().await;
            let found = emitted.iter().any(|(state, sid)| {
                matches!(state, SetupState::JoinSpaceConfirmPeer { .. })
                    && sid.as_ref() == Some(&session_id)
            });
            if found {
                break;
            }
            assert!(
                Instant::now() < emit_deadline,
                "setup-state-changed JoinSpaceConfirmPeer event timeout"
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn pairing_verification_listener_keeps_listening_for_keyslot_after_verification() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));
        let setup_event_port = Arc::new(MockSetupEventPort::default());
        let (pairing_orchestrator, action_rx) = build_pairing_orchestrator_with_actions();
        let orchestrator = SetupOrchestrator::new(
            build_initialize_encryption(),
            mark_setup_complete,
            setup_status,
            build_mock_lifecycle(),
            pairing_orchestrator,
            setup_event_port.clone(),
            build_space_access_orchestrator(),
            build_discovery_port(),
            build_network_control(),
            build_crypto_factory(),
            build_network_port(),
            build_transport_port(),
            build_proof_port(),
            build_timer_port(),
            build_persistence_port(),
        );

        orchestrator.join_space().await.unwrap();
        orchestrator
            .select_device("peer-verify".to_string())
            .await
            .unwrap();

        {
            let mut rx = action_rx.lock().await;
            assert!(
                rx.try_recv().is_ok(),
                "pairing orchestrator should queue initial action"
            );
        }

        let session_deadline = Instant::now() + Duration::from_secs(1);
        let session_id = loop {
            if let Some(session_id) = orchestrator.pairing_session_id.lock().await.clone() {
                break session_id;
            }
            assert!(
                Instant::now() < session_deadline,
                "pairing session id was not created"
            );
            sleep(Duration::from_millis(10)).await;
        };

        orchestrator
            .pairing_orchestrator
            .handle_challenge(
                &session_id,
                "peer-verify",
                uc_core::network::protocol::PairingChallenge {
                    session_id: session_id.clone(),
                    pin: "654321".to_string(),
                    device_name: "remote-device".to_string(),
                    device_id: "remote-device-id".to_string(),
                    identity_pubkey: vec![9; 32],
                    nonce: vec![2; 32],
                },
            )
            .await
            .unwrap();

        let emit_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let emitted = setup_event_port.snapshot().await;
            let found = emitted.iter().any(|(state, sid)| {
                matches!(state, SetupState::JoinSpaceConfirmPeer { .. })
                    && sid.as_ref() == Some(&session_id)
            });
            if found {
                break;
            }
            assert!(
                Instant::now() < emit_deadline,
                "setup-state-changed JoinSpaceConfirmPeer event timeout"
            );
            sleep(Duration::from_millis(10)).await;
        }

        orchestrator
            .pairing_orchestrator
            .handle_keyslot_offer(
                &session_id,
                "peer-verify",
                uc_core::network::protocol::PairingKeyslotOffer {
                    session_id: session_id.clone(),
                    keyslot_file: Some(sample_keyslot_file("space-listener")),
                    challenge: Some(vec![3; 32]),
                },
            )
            .await
            .unwrap();

        let offer_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if orchestrator.joiner_offer.lock().await.is_some() {
                break;
            }
            assert!(
                Instant::now() < offer_deadline,
                "joiner offer was not captured after verification event"
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn pairing_verification_listener_emits_join_space_failed_event_on_pairing_failure() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));
        let setup_event_port = Arc::new(MockSetupEventPort::default());
        let (pairing_orchestrator, action_rx) = build_pairing_orchestrator_with_actions();
        let orchestrator = SetupOrchestrator::new(
            build_initialize_encryption(),
            mark_setup_complete,
            setup_status,
            build_mock_lifecycle(),
            pairing_orchestrator,
            setup_event_port.clone(),
            build_space_access_orchestrator(),
            build_discovery_port(),
            build_network_control(),
            build_crypto_factory(),
            build_network_port(),
            build_transport_port(),
            build_proof_port(),
            build_timer_port(),
            build_persistence_port(),
        );

        orchestrator.join_space().await.unwrap();
        orchestrator
            .select_device("peer-verify".to_string())
            .await
            .unwrap();

        {
            let mut rx = action_rx.lock().await;
            assert!(
                rx.try_recv().is_ok(),
                "pairing orchestrator should queue initial action"
            );
        }

        let session_deadline = Instant::now() + Duration::from_secs(1);
        let session_id = loop {
            if let Some(session_id) = orchestrator.pairing_session_id.lock().await.clone() {
                break session_id;
            }
            assert!(
                Instant::now() < session_deadline,
                "pairing session id was not created"
            );
            sleep(Duration::from_millis(10)).await;
        };

        orchestrator
            .pairing_orchestrator
            .handle_transport_error(&session_id, "peer-verify", "stream closed".to_string())
            .await
            .unwrap();

        let emit_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let emitted = setup_event_port.snapshot().await;
            let found = emitted.iter().any(|(state, sid)| {
                matches!(
                    state,
                    SetupState::JoinSpaceInputPassphrase {
                        error: Some(SetupDomainError::PairingFailed)
                    }
                ) && sid.as_ref() == Some(&session_id)
            });
            if found {
                break;
            }
            assert!(
                Instant::now() < emit_deadline,
                "setup-state-changed JoinSpaceInputPassphrase error event timeout"
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn capture_context_normalizes_verify_passphrase_events() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator(setup_status);

        let event = orchestrator
            .capture_context(SetupEvent::VerifyPassphrase {
                passphrase: SecretString::new("secret".to_string()),
            })
            .await;

        match event {
            SetupEvent::SubmitPassphrase { .. } => {}
            other => panic!("unexpected event returned: {:?}", other),
        }

        assert!(orchestrator.passphrase.lock().await.is_some());
    }

    #[tokio::test]
    async fn start_join_space_access_maps_space_access_error() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator(setup_status);
        let space_id = uc_core::ids::SpaceId::new();
        let pairing_session_id = "session-join".to_string();

        let crypto = orchestrator
            .crypto_factory
            .build(SecretString::new("seed-pass".to_string()));
        let mut transport = orchestrator.transport_port.lock().await;
        let mut timer = orchestrator.timer_port.lock().await;
        let mut store = orchestrator.persistence_port.lock().await;
        let mut executor = SpaceAccessExecutor {
            crypto: crypto.as_ref(),
            net: orchestrator.network_port.as_ref(),
            transport: &mut *transport,
            proof: orchestrator.proof_port.as_ref(),
            timer: &mut *timer,
            store: &mut *store,
        };

        orchestrator
            .space_access_orchestrator
            .dispatch(
                &mut executor,
                SpaceAccessEvent::JoinRequested {
                    pairing_session_id: pairing_session_id.clone(),
                    ttl_secs: 60,
                },
                Some(pairing_session_id.clone()),
            )
            .await
            .unwrap();
        orchestrator
            .space_access_orchestrator
            .dispatch(
                &mut executor,
                SpaceAccessEvent::OfferAccepted {
                    pairing_session_id: pairing_session_id.clone(),
                    space_id,
                    expires_at: chrono::Utc::now() + chrono::Duration::seconds(60),
                },
                Some(pairing_session_id.clone()),
            )
            .await
            .unwrap();

        drop(executor);
        drop(store);
        drop(timer);
        drop(transport);

        *orchestrator.pairing_session_id.lock().await = Some(pairing_session_id);
        orchestrator
            .context
            .set_state(SetupState::JoinSpaceInputPassphrase { error: None })
            .await;

        let result = orchestrator
            .dispatch(SetupEvent::SubmitPassphrase {
                passphrase: SecretString::new("join-secret".to_string()),
            })
            .await;

        assert!(matches!(result, Err(SetupError::PairingFailed)));
    }

    #[tokio::test]
    async fn start_join_space_access_reads_offer_from_space_access_context() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator(setup_status);

        let offer = SpaceAccessJoinerOffer {
            space_id: uc_core::ids::SpaceId::from("space-from-context"),
            keyslot_blob: vec![1, 2, 3],
            challenge_nonce: [9; 32],
        };

        {
            let context = orchestrator.space_access_orchestrator.context();
            let mut guard = context.lock().await;
            guard.joiner_offer = Some(offer.clone());
        }

        *orchestrator.pairing_session_id.lock().await = Some("session-context".to_string());
        *orchestrator.selected_peer_id.lock().await = Some("peer-context".to_string());
        orchestrator
            .context
            .set_state(SetupState::JoinSpaceInputPassphrase { error: None })
            .await;

        let result = orchestrator
            .dispatch(SetupEvent::SubmitPassphrase {
                passphrase: SecretString::new("join-secret".to_string()),
            })
            .await;

        assert!(matches!(result, Err(SetupError::PairingFailed)));

        let stored_offer = orchestrator
            .joiner_offer
            .lock()
            .await
            .clone()
            .expect("local joiner offer should be hydrated from space access context");
        assert_eq!(stored_offer.space_id.as_ref(), offer.space_id.as_ref());
        assert_eq!(stored_offer.challenge_nonce, offer.challenge_nonce);
    }

    #[tokio::test]
    async fn submit_passphrase_waits_for_late_joiner_offer() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator_with_initialize_encryption_and_crypto_factory(
            setup_status,
            build_initialize_encryption(),
            build_success_crypto_factory(),
        );

        let session_id = "session-late-offer";
        *orchestrator.selected_peer_id.lock().await = Some("peer-late-offer".to_string());
        *orchestrator.pairing_session_id.lock().await = Some(session_id.to_string());
        orchestrator
            .context
            .set_state(SetupState::JoinSpaceInputPassphrase { error: None })
            .await;

        let context = orchestrator.space_access_orchestrator.context();
        tokio::spawn(async move {
            sleep(Duration::from_millis(40)).await;
            let mut guard = context.lock().await;
            guard.joiner_offer = Some(SpaceAccessJoinerOffer {
                space_id: uc_core::ids::SpaceId::from("space-late-offer"),
                keyslot_blob: vec![1, 2, 3, 4],
                challenge_nonce: [7; 32],
            });
        });

        let state = orchestrator
            .dispatch(SetupEvent::SubmitPassphrase {
                passphrase: SecretString::new("join-secret".to_string()),
            })
            .await
            .expect("submit passphrase should wait for late joiner offer");

        assert!(matches!(state, SetupState::ProcessingJoinSpace { .. }));
        assert!(orchestrator.joiner_offer.lock().await.is_some());
    }

    async fn prepare_join_passphrase_submission(
        orchestrator: &SetupOrchestrator,
        session_id: &str,
    ) {
        let offer = SpaceAccessJoinerOffer {
            space_id: uc_core::ids::SpaceId::from("space-join-await"),
            keyslot_blob: vec![1, 2, 3, 4],
            challenge_nonce: [3; 32],
        };

        {
            let context = orchestrator.space_access_orchestrator.context();
            let mut guard = context.lock().await;
            guard.joiner_offer = Some(offer.clone());
        }

        *orchestrator.selected_peer_id.lock().await = Some("peer-join-await".to_string());
        *orchestrator.pairing_session_id.lock().await = Some(session_id.to_string());
        *orchestrator.joiner_offer.lock().await = Some(offer);

        orchestrator
            .context
            .set_state(SetupState::JoinSpaceInputPassphrase { error: None })
            .await;
    }

    #[tokio::test]
    async fn submit_passphrase_does_not_complete_before_space_access_result() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator_with_initialize_encryption_and_crypto_factory(
            setup_status.clone(),
            build_initialize_encryption(),
            build_success_crypto_factory(),
        );

        prepare_join_passphrase_submission(&orchestrator, "session-join-await").await;

        let state = orchestrator
            .dispatch(SetupEvent::SubmitPassphrase {
                passphrase: SecretString::new("join-secret".to_string()),
            })
            .await
            .expect("submit passphrase should start async join flow");

        assert!(matches!(state, SetupState::ProcessingJoinSpace { .. }));
        let status = setup_status.get_status().await.expect("get setup status");
        assert!(!status.has_completed);
    }

    async fn dispatch_space_access_result(
        orchestrator: &SetupOrchestrator,
        event: SpaceAccessEvent,
        session_id: &str,
    ) {
        let crypto = orchestrator
            .crypto_factory
            .build(SecretString::new("join-secret".to_string()));
        let mut transport = orchestrator.transport_port.lock().await;
        let mut timer = orchestrator.timer_port.lock().await;
        let mut store = orchestrator.persistence_port.lock().await;
        let mut executor = SpaceAccessExecutor {
            crypto: crypto.as_ref(),
            net: orchestrator.network_port.as_ref(),
            transport: &mut *transport,
            proof: orchestrator.proof_port.as_ref(),
            timer: &mut *timer,
            store: &mut *store,
        };

        orchestrator
            .space_access_orchestrator
            .dispatch(&mut executor, event, Some(session_id.to_string()))
            .await
            .expect("space access result dispatch should succeed");
    }

    #[tokio::test]
    async fn setup_completes_after_access_granted_result_arrives() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator_with_initialize_encryption_and_crypto_factory(
            setup_status.clone(),
            build_initialize_encryption(),
            build_success_crypto_factory(),
        );
        let session_id = "session-join-granted";

        prepare_join_passphrase_submission(&orchestrator, session_id).await;

        let state = orchestrator
            .dispatch(SetupEvent::SubmitPassphrase {
                passphrase: SecretString::new("join-secret".to_string()),
            })
            .await
            .expect("submit passphrase should enter processing");
        assert!(matches!(state, SetupState::ProcessingJoinSpace { .. }));

        dispatch_space_access_result(
            &orchestrator,
            SpaceAccessEvent::AccessGranted {
                pairing_session_id: session_id.to_string(),
                space_id: uc_core::ids::SpaceId::from("space-join-await"),
            },
            session_id,
        )
        .await;

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if matches!(orchestrator.get_state().await, SetupState::Completed) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "setup did not transition to completed after access granted"
            );
            sleep(Duration::from_millis(10)).await;
        }

        let status = setup_status.get_status().await.expect("get setup status");
        assert!(status.has_completed);
    }

    #[tokio::test]
    async fn setup_returns_to_passphrase_on_access_denied_result() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator_with_initialize_encryption_and_crypto_factory(
            setup_status.clone(),
            build_initialize_encryption(),
            build_success_crypto_factory(),
        );
        let session_id = "session-join-denied";

        prepare_join_passphrase_submission(&orchestrator, session_id).await;

        let state = orchestrator
            .dispatch(SetupEvent::SubmitPassphrase {
                passphrase: SecretString::new("join-secret".to_string()),
            })
            .await
            .expect("submit passphrase should enter processing");
        assert!(matches!(state, SetupState::ProcessingJoinSpace { .. }));

        dispatch_space_access_result(
            &orchestrator,
            SpaceAccessEvent::AccessDenied {
                pairing_session_id: session_id.to_string(),
                space_id: uc_core::ids::SpaceId::from("space-join-await"),
                reason: uc_core::security::space_access::state::DenyReason::InvalidProof,
            },
            session_id,
        )
        .await;

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if matches!(
                orchestrator.get_state().await,
                SetupState::JoinSpaceInputPassphrase {
                    error: Some(SetupDomainError::PassphraseInvalidOrMismatch)
                }
            ) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "setup did not transition back to passphrase input after access denied"
            );
            sleep(Duration::from_millis(10)).await;
        }

        let status = setup_status.get_status().await.expect("get setup status");
        assert!(!status.has_completed);
    }

    enum JoinStepAction {
        Dispatch(Box<dyn Fn() -> SetupEvent + Send + Sync>),
        ForceState(SetupState),
        SimulatePassphrase(&'static str),
        SelectPeer(&'static str),
        SetPairingSession(&'static str),
    }

    struct JoinTestStep {
        label: &'static str,
        action: JoinStepAction,
        expected_state: SetupState,
    }

    impl JoinTestStep {
        fn dispatch<F>(label: &'static str, builder: F, expected_state: SetupState) -> Self
        where
            F: Fn() -> SetupEvent + Send + Sync + 'static,
        {
            Self {
                label,
                action: JoinStepAction::Dispatch(Box::new(builder)),
                expected_state,
            }
        }

        fn force_state(label: &'static str, state: SetupState) -> Self {
            Self {
                label,
                action: JoinStepAction::ForceState(state.clone()),
                expected_state: state,
            }
        }

        fn simulate_passphrase(
            label: &'static str,
            passphrase: &'static str,
            expected_state: SetupState,
        ) -> Self {
            Self {
                label,
                action: JoinStepAction::SimulatePassphrase(passphrase),
                expected_state,
            }
        }

        fn select_peer(
            label: &'static str,
            peer_id: &'static str,
            expected_state: SetupState,
        ) -> Self {
            Self {
                label,
                action: JoinStepAction::SelectPeer(peer_id),
                expected_state,
            }
        }

        fn set_pairing_session(
            label: &'static str,
            session_id: &'static str,
            expected_state: SetupState,
        ) -> Self {
            Self {
                label,
                action: JoinStepAction::SetPairingSession(session_id),
                expected_state,
            }
        }
    }

    async fn simulate_passphrase_submission(orchestrator: &SetupOrchestrator, passphrase: &str) {
        let _ = orchestrator
            .capture_context(SetupEvent::SubmitPassphrase {
                passphrase: SecretString::new(passphrase.to_string()),
            })
            .await;

        orchestrator
            .context
            .set_state(SetupState::ProcessingJoinSpace {
                message: Some("Verifying passphrase…".into()),
            })
            .await;
    }

    async fn run_join_steps(orchestrator: &SetupOrchestrator, steps: &[JoinTestStep]) {
        for step in steps {
            match &step.action {
                JoinStepAction::Dispatch(builder) => {
                    let next = orchestrator
                        .dispatch(builder())
                        .await
                        .unwrap_or_else(|err| panic!("{} failed: {:?}", step.label, err));
                    assert_eq!(next, step.expected_state, "{} state mismatch", step.label);
                }
                JoinStepAction::ForceState(state) => {
                    orchestrator.context.set_state(state.clone()).await;
                    let current = orchestrator.context.get_state().await;
                    assert_eq!(
                        current, step.expected_state,
                        "{} state mismatch",
                        step.label
                    );
                }
                JoinStepAction::SimulatePassphrase(passphrase) => {
                    simulate_passphrase_submission(orchestrator, passphrase).await;
                    let current = orchestrator.context.get_state().await;
                    assert_eq!(
                        current, step.expected_state,
                        "{} state mismatch",
                        step.label
                    );
                }
                JoinStepAction::SelectPeer(peer_id) => {
                    *orchestrator.selected_peer_id.lock().await = Some((*peer_id).to_string());
                    let current = orchestrator.context.get_state().await;
                    assert_eq!(
                        current, step.expected_state,
                        "{} state mismatch",
                        step.label
                    );
                }
                JoinStepAction::SetPairingSession(session_id) => {
                    *orchestrator.pairing_session_id.lock().await = Some((*session_id).to_string());
                    let current = orchestrator.context.get_state().await;
                    assert_eq!(
                        current, step.expected_state,
                        "{} state mismatch",
                        step.label
                    );
                }
            }
        }
    }

    fn join_processing_state(message: &str) -> SetupState {
        SetupState::ProcessingJoinSpace {
            message: Some(message.to_string()),
        }
    }

    #[tokio::test]
    async fn join_space_happy_path() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator(setup_status.clone());

        let steps = vec![
            JoinTestStep::dispatch(
                "start join space",
                || SetupEvent::StartJoinSpace,
                SetupState::JoinSpaceSelectDevice { error: None },
            ),
            JoinTestStep::select_peer(
                "remember peer selection",
                "peer-123",
                SetupState::JoinSpaceSelectDevice { error: None },
            ),
            JoinTestStep::force_state(
                "transition to processing",
                join_processing_state("Connecting to selected device…"),
            ),
            JoinTestStep::force_state(
                "pairing verification delivered",
                SetupState::JoinSpaceConfirmPeer {
                    short_code: "123-456".into(),
                    peer_fingerprint: Some("fp".into()),
                    error: None,
                },
            ),
            JoinTestStep::set_pairing_session(
                "store pairing session",
                "session-1",
                SetupState::JoinSpaceConfirmPeer {
                    short_code: "123-456".into(),
                    peer_fingerprint: Some("fp".into()),
                    error: None,
                },
            ),
            JoinTestStep::force_state(
                "transition to passphrase input",
                SetupState::JoinSpaceInputPassphrase { error: None },
            ),
            JoinTestStep::simulate_passphrase(
                "submit passphrase",
                "join-secret",
                join_processing_state("Verifying passphrase…"),
            ),
            JoinTestStep::dispatch(
                "space access granted",
                || SetupEvent::JoinSpaceSucceeded,
                SetupState::Completed,
            ),
        ];

        run_join_steps(&orchestrator, &steps).await;

        let status = setup_status.get_status().await.unwrap();
        assert!(status.has_completed, "setup status should mark completion");
    }

    #[tokio::test]
    async fn join_space_pairing_fails() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator(setup_status);

        let steps = vec![
            JoinTestStep::dispatch(
                "start join space",
                || SetupEvent::StartJoinSpace,
                SetupState::JoinSpaceSelectDevice { error: None },
            ),
            JoinTestStep::select_peer(
                "remember peer selection",
                "peer-fail",
                SetupState::JoinSpaceSelectDevice { error: None },
            ),
            JoinTestStep::force_state(
                "transition to processing",
                join_processing_state("Connecting to selected device…"),
            ),
            JoinTestStep::set_pairing_session(
                "store pairing session",
                "session-fail",
                join_processing_state("Connecting to selected device…"),
            ),
            JoinTestStep::dispatch(
                "pairing failure propagates",
                || SetupEvent::JoinSpaceFailed {
                    error: SetupDomainError::PairingFailed,
                },
                SetupState::JoinSpaceInputPassphrase {
                    error: Some(SetupDomainError::PairingFailed),
                },
            ),
        ];

        run_join_steps(&orchestrator, &steps).await;
    }

    #[tokio::test]
    async fn join_space_passphrase_wrong() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator(setup_status);

        let steps = vec![
            JoinTestStep::dispatch(
                "start join space",
                || SetupEvent::StartJoinSpace,
                SetupState::JoinSpaceSelectDevice { error: None },
            ),
            JoinTestStep::select_peer(
                "remember peer selection",
                "peer-pass",
                SetupState::JoinSpaceSelectDevice { error: None },
            ),
            JoinTestStep::force_state(
                "transition to processing",
                join_processing_state("Connecting to selected device…"),
            ),
            JoinTestStep::force_state(
                "pairing verification delivered",
                SetupState::JoinSpaceConfirmPeer {
                    short_code: "777-888".into(),
                    peer_fingerprint: None,
                    error: None,
                },
            ),
            JoinTestStep::set_pairing_session(
                "store pairing session",
                "session-pass",
                SetupState::JoinSpaceConfirmPeer {
                    short_code: "777-888".into(),
                    peer_fingerprint: None,
                    error: None,
                },
            ),
            JoinTestStep::force_state(
                "transition to passphrase input",
                SetupState::JoinSpaceInputPassphrase { error: None },
            ),
            JoinTestStep::simulate_passphrase(
                "submit wrong passphrase",
                "wrong-pass",
                join_processing_state("Verifying passphrase…"),
            ),
            JoinTestStep::dispatch(
                "space access denied",
                || SetupEvent::JoinSpaceFailed {
                    error: SetupDomainError::PassphraseInvalidOrMismatch,
                },
                SetupState::JoinSpaceInputPassphrase {
                    error: Some(SetupDomainError::PassphraseInvalidOrMismatch),
                },
            ),
        ];

        run_join_steps(&orchestrator, &steps).await;
    }

    #[tokio::test]
    async fn join_space_cancel_during_pairing() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator(setup_status);

        let steps = vec![
            JoinTestStep::dispatch(
                "start join space",
                || SetupEvent::StartJoinSpace,
                SetupState::JoinSpaceSelectDevice { error: None },
            ),
            JoinTestStep::select_peer(
                "remember peer selection",
                "peer-cancel",
                SetupState::JoinSpaceSelectDevice { error: None },
            ),
            JoinTestStep::force_state(
                "transition to processing",
                join_processing_state("Connecting to selected device…"),
            ),
            JoinTestStep::set_pairing_session(
                "store pairing session",
                "session-cancel",
                join_processing_state("Connecting to selected device…"),
            ),
            JoinTestStep::dispatch(
                "user cancels during pairing",
                || SetupEvent::CancelSetup,
                SetupState::Welcome,
            ),
        ];

        run_join_steps(&orchestrator, &steps).await;

        assert!(orchestrator.selected_peer_id.lock().await.is_none());
        assert!(orchestrator.pairing_session_id.lock().await.is_none());
    }
}

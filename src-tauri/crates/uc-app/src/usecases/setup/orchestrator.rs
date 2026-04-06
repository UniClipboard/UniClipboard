//! Setup orchestrator.
//!
//! This module coordinates the setup state machine transitions and delegates
//! side-effect execution to `SetupActionExecutor`. The orchestrator remains
//! a thin dispatcher that owns session state and the state machine loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rand::RngCore;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, warn, Instrument};

use uc_core::{
    ids::SpaceId,
    ports::space::CryptoPort,
    ports::space::{PersistencePort, ProofPort, SpaceAccessTransportPort},
    ports::{
        DiscoveryPort, NetworkControlPort, PairingTransportPort, SetupEventPort, SetupStatusPort,
        TimerPort,
    },
    security::{model::Passphrase, SecretString},
    security::{
        model::{KeySlot, KeySlotFile},
        space_access::{
            event::SpaceAccessEvent,
            state::{DenyReason, SpaceAccessState},
            SpaceAccessProofArtifact,
        },
    },
    setup::{SetupEvent, SetupState, SetupStateMachine, SetupStatus},
};

use crate::usecases::initialize_encryption::InitializeEncryptionError;
use crate::usecases::setup::action_executor::SetupActionExecutor;
use crate::usecases::setup::context::SetupContext;
use crate::usecases::setup::MarkSetupComplete;
use crate::usecases::space_access::{
    SpaceAccessCryptoFactory, SpaceAccessExecutor, SpaceAccessJoinerOffer, SpaceAccessOrchestrator,
};
use crate::usecases::AppLifecycleCoordinator;
use crate::usecases::InitializeEncryption;
use crate::usecases::SetupPairingFacadePort;

/// Errors produced by the setup orchestrator.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("initialize encryption failed: {0}")]
    InitializeEncryption(#[from] InitializeEncryptionError),
    #[error("mark setup complete failed: {0}")]
    MarkSetupComplete(#[from] anyhow::Error),
    /// Failed to load setup status from persistent storage.
    #[error("load setup status failed: {0}")]
    StatusLoadFailed(#[source] anyhow::Error),
    #[error("setup reset failed: {0}")]
    ResetFailed(#[source] anyhow::Error),
    #[error("lifecycle boot failed: {0}")]
    LifecycleFailed(#[source] anyhow::Error),
    #[error("setup action not implemented: {0}")]
    ActionNotImplemented(&'static str),
    #[error("pairing operation failed")]
    PairingFailed,
}

/// Orchestrator that drives setup state transitions and delegates side effects
/// to `SetupActionExecutor`.
pub struct SetupOrchestrator {
    pub(super) context: Arc<SetupContext>,

    // Session state -- borrowed by action executor via method params
    pub(super) selected_peer_id: Arc<Mutex<Option<String>>>,
    pub(super) pairing_session_id: Arc<Mutex<Option<String>>>,
    pub(super) joiner_offer: Arc<Mutex<Option<SpaceAccessJoinerOffer>>>,
    pub(super) passphrase: Arc<Mutex<Option<Passphrase>>>,
    seeded: AtomicBool,
    seed_lock: Mutex<()>,

    // Retained ports (used only by orchestrator dispatch, not by actions)
    setup_status: Arc<dyn SetupStatusPort>,

    // Action executor handles all side-effect execution
    pub(super) action_executor: Arc<SetupActionExecutor>,
}

struct LoadedKeyslotSpaceAccessCrypto {
    keyslot_file: KeySlotFile,
}

#[async_trait::async_trait]
impl CryptoPort for LoadedKeyslotSpaceAccessCrypto {
    async fn generate_nonce32(&self) -> [u8; 32] {
        let mut nonce = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        nonce
    }

    async fn export_keyslot_blob(&self, _space_id: &SpaceId) -> anyhow::Result<KeySlot> {
        Ok(self.keyslot_file.clone().into())
    }

    async fn derive_master_key_from_keyslot(
        &self,
        _keyslot_blob: &[u8],
        _passphrase: SecretString,
    ) -> anyhow::Result<uc_core::security::MasterKey> {
        Err(anyhow::anyhow!(
            "loaded keyslot crypto cannot derive master key in sponsor flow"
        ))
    }
}

struct NoopRuntimeSpaceAccessCrypto;

#[async_trait::async_trait]
impl CryptoPort for NoopRuntimeSpaceAccessCrypto {
    async fn generate_nonce32(&self) -> [u8; 32] {
        [0u8; 32]
    }

    async fn export_keyslot_blob(&self, _space_id: &SpaceId) -> anyhow::Result<KeySlot> {
        Err(anyhow::anyhow!(
            "noop runtime space access crypto cannot export keyslot"
        ))
    }

    async fn derive_master_key_from_keyslot(
        &self,
        _keyslot_blob: &[u8],
        _passphrase: SecretString,
    ) -> anyhow::Result<uc_core::security::MasterKey> {
        Err(anyhow::anyhow!(
            "noop runtime space access crypto cannot derive master key"
        ))
    }
}

impl SetupOrchestrator {
    pub fn new(
        initialize_encryption: Arc<InitializeEncryption>,
        mark_setup_complete: Arc<MarkSetupComplete>,
        setup_status: Arc<dyn SetupStatusPort>,
        app_lifecycle: Arc<AppLifecycleCoordinator>,
        setup_pairing_facade: Arc<dyn SetupPairingFacadePort>,
        setup_event_port: Arc<dyn SetupEventPort>,
        space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
        discovery_port: Arc<dyn DiscoveryPort>,
        network_control: Arc<dyn NetworkControlPort>,
        crypto_factory: Arc<dyn SpaceAccessCryptoFactory>,
        pairing_transport: Arc<dyn PairingTransportPort>,
        transport_port: Arc<Mutex<dyn SpaceAccessTransportPort>>,
        proof_port: Arc<dyn ProofPort>,
        timer_port: Arc<Mutex<dyn TimerPort>>,
        persistence_port: Arc<Mutex<dyn PersistencePort>>,
    ) -> Self {
        let action_executor = Arc::new(SetupActionExecutor {
            initialize_encryption,
            mark_setup_complete,
            app_lifecycle,
            setup_event_port,
            discovery_port,
            network_control,
            crypto_factory,
            pairing_transport,
            transport_port,
            proof_port,
            timer_port,
            persistence_port,
            setup_pairing_facade,
            space_access_orchestrator,
        });

        Self {
            context: SetupContext::default().arc(),
            selected_peer_id: Arc::new(Mutex::new(None)),
            pairing_session_id: Arc::new(Mutex::new(None)),
            joiner_offer: Arc::new(Mutex::new(None)),
            passphrase: Arc::new(Mutex::new(None)),
            seeded: AtomicBool::new(false),
            seed_lock: Mutex::new(()),
            setup_status,
            action_executor,
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
        let event = SetupEvent::VerifyPassphrase {
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

    /// Complete the join space flow. Called by the frontend when the daemon
    /// emits `setup.spaceAccessCompleted` via the WebSocket bridge.
    pub async fn complete_join_space(&self) -> Result<SetupState, SetupError> {
        let event = SetupEvent::JoinSpaceSucceeded;
        self.dispatch(event).await
    }

    pub async fn reset(&self) -> Result<SetupState, SetupError> {
        let _dispatch_guard = self.context.acquire_dispatch_lock().await;

        self.clear_runtime_session_state().await;
        self.setup_status
            .set_status(&SetupStatus::default())
            .await
            .map_err(SetupError::ResetFailed)?;
        SetupActionExecutor::set_state_and_emit(
            &self.context,
            &self.action_executor.setup_event_port,
            SetupState::Welcome,
            None,
        )
        .await;
        self.seeded.store(false, Ordering::SeqCst);

        Ok(SetupState::Welcome)
    }

    /// Clears in-memory setup session state and any active pairing session.
    ///
    /// Unlike [`reset`](Self::reset), this preserves the device's completion status
    /// stored in persistent storage. Returns the base state derived from
    /// `SetupStatus.has_completed`: [`SetupState::Completed`] if the device has
    /// previously completed setup, or [`SetupState::Welcome`] otherwise.
    pub async fn clear_transient_state(&self) -> Result<SetupState, SetupError> {
        let _dispatch_guard = self.context.acquire_dispatch_lock().await;

        self.clear_runtime_session_state().await;

        let status = self
            .setup_status
            .get_status()
            .await
            .map_err(SetupError::StatusLoadFailed)?;
        let base_state = Self::state_from_status(&status);

        SetupActionExecutor::set_state_and_emit(
            &self.context,
            &self.action_executor.setup_event_port,
            base_state.clone(),
            None,
        )
        .await;
        self.seeded.store(true, Ordering::SeqCst);

        Ok(base_state)
    }

    pub async fn get_state(&self) -> SetupState {
        self.seed_state_from_status().await;
        self.context.get_state().await
    }

    pub async fn start_completed_host_sponsor_authorization(
        &self,
        pairing_session_id: String,
        sponsor_peer_id: String,
        keyslot_file: KeySlotFile,
    ) -> Result<SpaceAccessState, SetupError> {
        let current_state = self.get_state().await;
        if !matches!(current_state, SetupState::Completed) {
            return Err(SetupError::PairingFailed);
        }

        {
            let context = self.action_executor.space_access_orchestrator.context();
            let mut guard = context.lock().await;
            guard.sponsor_peer_id = Some(sponsor_peer_id);
        }

        let space_id = SpaceId::from(keyslot_file.scope.profile_id.as_str());
        self.dispatch_space_access_event_with_crypto(
            LoadedKeyslotSpaceAccessCrypto { keyslot_file },
            SpaceAccessEvent::SponsorAuthorizationRequested {
                pairing_session_id: pairing_session_id.clone(),
                space_id,
                ttl_secs: 60,
            },
            pairing_session_id,
        )
        .await
    }

    pub async fn resolve_host_space_access_proof(
        &self,
        proof: SpaceAccessProofArtifact,
        sponsor_peer_id: Option<String>,
    ) -> Result<SpaceAccessState, SetupError> {
        let current_pairing_session_id = match self
            .action_executor
            .space_access_orchestrator
            .get_state()
            .await
        {
            SpaceAccessState::WaitingJoinerProof {
                pairing_session_id, ..
            } => pairing_session_id,
            _ => return Err(SetupError::PairingFailed),
        };

        let expected = {
            let context = self.action_executor.space_access_orchestrator.context();
            let mut guard = context.lock().await;
            if let Some(peer_id) = sponsor_peer_id {
                guard.sponsor_peer_id = Some(peer_id);
            }
            let Some(offer) = guard.prepared_offer.clone() else {
                return Err(SetupError::PairingFailed);
            };
            offer
        };

        let event = if proof.pairing_session_id.as_str() != current_pairing_session_id {
            SpaceAccessEvent::ProofRejected {
                pairing_session_id: proof.pairing_session_id.as_str().to_string(),
                space_id: proof.space_id.clone(),
                reason: DenyReason::SessionMismatch,
            }
        } else if proof.space_id != expected.space_id {
            SpaceAccessEvent::ProofRejected {
                pairing_session_id: proof.pairing_session_id.as_str().to_string(),
                space_id: proof.space_id.clone(),
                reason: DenyReason::SpaceMismatch,
            }
        } else {
            let verified = self
                .action_executor
                .proof_port
                .verify_proof(&proof, expected.nonce)
                .await
                .map_err(|_| SetupError::PairingFailed)?;

            if verified {
                SpaceAccessEvent::ProofVerified {
                    pairing_session_id: proof.pairing_session_id.as_str().to_string(),
                    space_id: proof.space_id.clone(),
                }
            } else {
                SpaceAccessEvent::ProofRejected {
                    pairing_session_id: proof.pairing_session_id.as_str().to_string(),
                    space_id: proof.space_id.clone(),
                    reason: DenyReason::InvalidProof,
                }
            }
        };

        self.dispatch_space_access_event_with_crypto(
            NoopRuntimeSpaceAccessCrypto,
            event,
            proof.pairing_session_id.as_str().to_string(),
        )
        .await
    }

    pub async fn apply_joiner_space_access_result(
        &self,
        pairing_session_id: String,
        space_id: SpaceId,
        sponsor_peer_id: Option<String>,
        success: bool,
        deny_reason: Option<DenyReason>,
    ) -> Result<SpaceAccessState, SetupError> {
        if let Some(peer_id) = sponsor_peer_id {
            let context = self.action_executor.space_access_orchestrator.context();
            context.lock().await.sponsor_peer_id = Some(peer_id);
        }

        let event = if success {
            SpaceAccessEvent::AccessGranted {
                pairing_session_id: pairing_session_id.clone(),
                space_id,
            }
        } else {
            SpaceAccessEvent::AccessDenied {
                pairing_session_id: pairing_session_id.clone(),
                space_id,
                reason: deny_reason.unwrap_or(DenyReason::InternalError),
            }
        };

        self.dispatch_space_access_event_with_crypto(
            NoopRuntimeSpaceAccessCrypto,
            event,
            pairing_session_id,
        )
        .await
    }

    async fn dispatch(&self, event: SetupEvent) -> Result<SetupState, SetupError> {
        let event = self.capture_context(event).await;
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
                let follow_up_events = self
                    .action_executor
                    .execute_actions(
                        actions,
                        &self.passphrase,
                        &self.selected_peer_id,
                        &self.pairing_session_id,
                        &self.joiner_offer,
                        &self.context,
                    )
                    .await?;
                SetupActionExecutor::set_state_and_emit(
                    &self.context,
                    &self.action_executor.setup_event_port,
                    next.clone(),
                    self.current_pairing_session_id().await,
                )
                .await;
                current = next;
                pending_events.extend(follow_up_events);
            }

            Ok(current)
        }
        .instrument(span)
        .await
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
                SetupEvent::VerifyPassphrase {
                    passphrase: event_passphrase,
                }
            }
            other => other,
        }
    }

    async fn dispatch_space_access_event_with_crypto<C>(
        &self,
        crypto: C,
        event: SpaceAccessEvent,
        pairing_session_id: String,
    ) -> Result<SpaceAccessState, SetupError>
    where
        C: CryptoPort,
    {
        let mut transport = self.action_executor.transport_port.lock().await;
        let mut timer = self.action_executor.timer_port.lock().await;
        let mut store = self.action_executor.persistence_port.lock().await;
        let mut executor = SpaceAccessExecutor {
            crypto: &crypto,
            transport: &mut *transport,
            proof: self.action_executor.proof_port.as_ref(),
            timer: &mut *timer,
            store: &mut *store,
        };

        self.action_executor
            .space_access_orchestrator
            .dispatch(&mut executor, event, Some(pairing_session_id))
            .await
            .map_err(|_| SetupError::PairingFailed)
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

    /// Clears in-memory session state: selected peer, pairing session, joiner offer,
    /// and passphrase. Rejects any active pairing session.
    async fn clear_runtime_session_state(&self) {
        if let Some(session_id) = self.pairing_session_id.lock().await.take() {
            if let Err(error) = self
                .action_executor
                .setup_pairing_facade
                .reject_pairing(&session_id)
                .await
            {
                warn!(
                    error = %error,
                    session_id = %session_id,
                    "failed to reject setup pairing session during setup state clear"
                );
            }
        }

        self.selected_peer_id.lock().await.take();
        self.joiner_offer.lock().await.take();
        self.passphrase.lock().await.take();
        self.action_executor.space_access_orchestrator.reset().await;
    }

    /// Derives the base [`SetupState`] from persisted [`SetupStatus`].
    ///
    /// Returns [`SetupState::Completed`] if `has_completed` is true,
    /// otherwise [`SetupState::Welcome`].
    fn state_from_status(status: &SetupStatus) -> SetupState {
        if status.has_completed {
            SetupState::Completed
        } else {
            SetupState::Welcome
        }
    }

    async fn seed_state_from_status(&self) {
        if self.seeded.load(Ordering::SeqCst) {
            return;
        }

        let _seed_guard = self.seed_lock.lock().await;
        if self.seeded.load(Ordering::SeqCst) {
            return;
        }

        match self.setup_status.get_status().await {
            Ok(status) => {
                let base_state = Self::state_from_status(&status);
                if matches!(base_state, SetupState::Completed) {
                    SetupActionExecutor::set_state_and_emit(
                        &self.context,
                        &self.action_executor.setup_event_port,
                        base_state,
                        None,
                    )
                    .await;
                }
            }
            Err(err) => {
                error!(error = %err, "failed to load setup status");
            }
        }

        self.seeded.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        NoopDiscoveryPort, NoopLifecycleEventEmitter, NoopLifecycleStatus, NoopNetworkControl,
        NoopPairedDeviceRepository, NoopPairingTransport, NoopProofPort, NoopSessionReadyEmitter,
        NoopSpaceAccessPersistence, NoopSpaceAccessTransport, NoopTimerPort,
    };
    use crate::usecases::pairing::{PairingConfig, PairingOrchestrator};
    use crate::usecases::setup::action_executor::SetupActionExecutor;
    use crate::usecases::space_access::{SpaceAccessExecutor, SpaceAccessOrchestrator};
    use crate::usecases::{AppLifecycleCoordinatorDeps, StartNetworkAfterUnlock};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::sync::{Mutex, Notify};
    use tokio::time::{sleep, Duration, Instant};
    use uc_core::network::pairing_state_machine::FailureReason;
    use uc_core::ports::security::encryption::EncryptionPort;
    use uc_core::ports::security::encryption_session::EncryptionSessionPort;
    use uc_core::ports::security::encryption_state::EncryptionStatePort;
    use uc_core::ports::security::key_material::KeyMaterialPort;
    use uc_core::ports::security::key_scope::{KeyScopePort, ScopeError};
    use uc_core::ports::space::CryptoPort;
    use uc_core::ports::SetupEventPort;
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionError, EncryptionFormatVersion, KdfAlgorithm,
        KdfParams, KdfParamsV1, Kek, KeyScope, KeySlot, KeySlotFile, KeySlotVersion, MasterKey,
        Passphrase,
    };
    use uc_core::security::space_access::event::SpaceAccessEvent;
    use uc_core::security::state::{EncryptionState, EncryptionStateError};
    use uc_core::setup::{SetupError as SetupDomainError, SetupStatus};

    struct MockSetupStatusPort {
        status: StdMutex<SetupStatus>,
        set_calls: AtomicUsize,
    }

    struct BlockingSetupStatusPort {
        status: SetupStatus,
        entered_get_status: Notify,
        release_get_status: Notify,
        get_calls: AtomicUsize,
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

    impl BlockingSetupStatusPort {
        fn new(status: SetupStatus) -> Self {
            Self {
                status,
                entered_get_status: Notify::new(),
                release_get_status: Notify::new(),
                get_calls: AtomicUsize::new(0),
            }
        }

        async fn wait_until_get_status_called(&self) {
            self.entered_get_status.notified().await;
        }

        fn release_blocked_get_status(&self) {
            self.release_get_status.notify_waiters();
        }

        fn get_call_count(&self) -> usize {
            self.get_calls.load(Ordering::SeqCst)
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

    #[async_trait]
    impl SetupStatusPort for BlockingSetupStatusPort {
        async fn get_status(&self) -> anyhow::Result<SetupStatus> {
            self.get_calls.fetch_add(1, Ordering::SeqCst);
            self.entered_get_status.notify_one();
            self.release_get_status.notified().await;
            Ok(self.status.clone())
        }

        async fn set_status(&self, _status: &SetupStatus) -> anyhow::Result<()> {
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

        async fn clear_initialized(&self) -> Result<(), EncryptionStateError> {
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

        async fn clear_initialized(&self) -> Result<(), EncryptionStateError> {
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
    // NoopNetworkControl, NoopSessionReadyEmitter, NoopLifecycleStatus,
    // NoopLifecycleEventEmitter, NoopPairedDeviceRepository, NoopDiscoveryPort
    // — imported from crate::testing.
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

    // NoopPairingTransport, NoopSpaceAccessTransport, NoopProofPort,
    // NoopTimerPort, NoopSpaceAccessPersistence — all imported from crate::testing

    fn build_mock_lifecycle() -> Arc<AppLifecycleCoordinator> {
        Arc::new(AppLifecycleCoordinator::from_deps(
            AppLifecycleCoordinatorDeps {
                network: Arc::new(StartNetworkAfterUnlock::new(Arc::new(NoopNetworkControl))),
                announcer: None,
                emitter: Arc::new(NoopSessionReadyEmitter),
                status: Arc::new(NoopLifecycleStatus),
                lifecycle_emitter: Arc::new(NoopLifecycleEventEmitter),
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

    type PairingTestOrchestrator = std::sync::Arc<crate::usecases::pairing::PairingOrchestrator>;

    fn build_pairing_orchestrator() -> PairingTestOrchestrator {
        let repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            repo,
            "test-device".to_string(),
            "test-device-id".to_string(),
            "test-peer-id".to_string(),
            vec![1; 32],
            Arc::new(crate::usecases::StagedPairedDeviceStore::new()),
        );
        Arc::new(orchestrator)
    }

    fn build_pairing_orchestrator_with_actions() -> (
        PairingTestOrchestrator,
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
            Arc::new(crate::usecases::StagedPairedDeviceStore::new()),
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
        Arc::new(NoopNetworkControl)
    }

    fn build_crypto_factory() -> Arc<dyn SpaceAccessCryptoFactory> {
        Arc::new(NoopSpaceAccessCryptoFactory)
    }

    fn build_success_crypto_factory() -> Arc<dyn SpaceAccessCryptoFactory> {
        Arc::new(SucceedSpaceAccessCryptoFactory)
    }

    fn build_pairing_transport() -> Arc<dyn PairingTransportPort> {
        Arc::new(NoopPairingTransport)
    }

    fn build_transport_port() -> Arc<Mutex<dyn SpaceAccessTransportPort>> {
        Arc::new(Mutex::new(NoopSpaceAccessTransport))
    }

    fn build_proof_port() -> Arc<dyn ProofPort> {
        Arc::new(NoopProofPort)
    }

    fn build_timer_port() -> Arc<Mutex<dyn TimerPort>> {
        Arc::new(Mutex::new(NoopTimerPort))
    }

    fn build_persistence_port() -> Arc<Mutex<dyn PersistencePort>> {
        Arc::new(Mutex::new(NoopSpaceAccessPersistence))
    }

    #[derive(Default)]
    struct RecordingSpaceAccessTransportState {
        offers: Vec<String>,
        proofs: Vec<String>,
        results: Vec<String>,
    }

    struct RecordingSpaceAccessTransport {
        state: Arc<std::sync::Mutex<RecordingSpaceAccessTransportState>>,
    }

    impl RecordingSpaceAccessTransport {
        fn new() -> (
            Self,
            Arc<std::sync::Mutex<RecordingSpaceAccessTransportState>>,
        ) {
            let state = Arc::new(std::sync::Mutex::new(
                RecordingSpaceAccessTransportState::default(),
            ));
            (
                Self {
                    state: state.clone(),
                },
                state,
            )
        }
    }

    #[async_trait]
    impl SpaceAccessTransportPort for RecordingSpaceAccessTransport {
        async fn send_offer(
            &mut self,
            session_id: &uc_core::network::SessionId,
        ) -> anyhow::Result<()> {
            self.state
                .lock()
                .expect("lock recording transport")
                .offers
                .push(session_id.clone());
            Ok(())
        }

        async fn send_proof(
            &mut self,
            session_id: &uc_core::network::SessionId,
        ) -> anyhow::Result<()> {
            self.state
                .lock()
                .expect("lock recording transport")
                .proofs
                .push(session_id.clone());
            Ok(())
        }

        async fn send_result(
            &mut self,
            session_id: &uc_core::network::SessionId,
        ) -> anyhow::Result<()> {
            self.state
                .lock()
                .expect("lock recording transport")
                .results
                .push(session_id.clone());
            Ok(())
        }
    }

    struct ConfigurableProofPort {
        verify_ok: bool,
    }

    #[async_trait]
    impl ProofPort for ConfigurableProofPort {
        async fn build_proof(
            &self,
            pairing_session_id: &uc_core::SessionId,
            space_id: &uc_core::ids::SpaceId,
            challenge_nonce: [u8; 32],
            _master_key: &MasterKey,
        ) -> anyhow::Result<uc_core::security::space_access::SpaceAccessProofArtifact> {
            Ok(uc_core::security::space_access::SpaceAccessProofArtifact {
                pairing_session_id: pairing_session_id.clone(),
                space_id: space_id.clone(),
                challenge_nonce,
                proof_bytes: vec![1, 2, 3],
            })
        }

        async fn verify_proof(
            &self,
            _proof: &uc_core::security::space_access::SpaceAccessProofArtifact,
            _expected_nonce: [u8; 32],
        ) -> anyhow::Result<bool> {
            Ok(self.verify_ok)
        }
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
            build_pairing_transport(),
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

    fn build_orchestrator_with_space_access_runtime(
        setup_status: Arc<dyn SetupStatusPort>,
        transport_port: Arc<Mutex<dyn SpaceAccessTransportPort>>,
        proof_port: Arc<dyn ProofPort>,
    ) -> SetupOrchestrator {
        let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));

        SetupOrchestrator::new(
            build_initialize_encryption(),
            mark_setup_complete,
            setup_status,
            build_mock_lifecycle(),
            build_pairing_orchestrator(),
            build_setup_event_port(),
            build_space_access_orchestrator(),
            build_discovery_port(),
            build_network_control(),
            build_crypto_factory(),
            build_pairing_transport(),
            transport_port,
            proof_port,
            build_timer_port(),
            build_persistence_port(),
        )
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
    async fn completed_host_sponsor_authorization_sends_offer_from_loaded_keyslot() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus {
            has_completed: true,
        }));
        let (transport, transport_state) = RecordingSpaceAccessTransport::new();
        let orchestrator = build_orchestrator_with_space_access_runtime(
            setup_status,
            Arc::new(Mutex::new(transport)),
            Arc::new(ConfigurableProofPort { verify_ok: true }),
        );

        let keyslot_file = sample_keyslot_file("space-host-offer");
        let state = orchestrator
            .start_completed_host_sponsor_authorization(
                "session-host-offer".to_string(),
                "peer-host".to_string(),
                keyslot_file,
            )
            .await
            .expect("host sponsor authorization should start");

        assert!(matches!(
            state,
            uc_core::security::space_access::state::SpaceAccessState::WaitingJoinerProof { .. }
        ));

        let guard = transport_state.lock().expect("lock recording transport");
        assert_eq!(guard.offers, vec!["session-host-offer".to_string()]);
        drop(guard);

        let context = orchestrator
            .action_executor
            .space_access_orchestrator
            .context();
        let guard = context.lock().await;
        assert_eq!(guard.sponsor_peer_id.as_deref(), Some("peer-host"));
        let offer = guard
            .prepared_offer
            .as_ref()
            .expect("prepared offer should exist");
        assert_eq!(offer.space_id.as_ref(), "space-host-offer");
    }

    #[tokio::test]
    async fn host_space_access_proof_verification_sends_result() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus {
            has_completed: true,
        }));
        let (transport, transport_state) = RecordingSpaceAccessTransport::new();
        let orchestrator = build_orchestrator_with_space_access_runtime(
            setup_status,
            Arc::new(Mutex::new(transport)),
            Arc::new(ConfigurableProofPort { verify_ok: true }),
        );

        orchestrator
            .start_completed_host_sponsor_authorization(
                "session-host-proof".to_string(),
                "peer-proof".to_string(),
                sample_keyslot_file("space-host-proof"),
            )
            .await
            .expect("host sponsor authorization should start");

        let proof = uc_core::security::space_access::SpaceAccessProofArtifact {
            pairing_session_id: uc_core::ids::SessionId::from("session-host-proof"),
            space_id: uc_core::ids::SpaceId::from("space-host-proof"),
            challenge_nonce: [0u8; 32],
            proof_bytes: vec![7, 7, 7],
        };

        let state = orchestrator
            .resolve_host_space_access_proof(proof, Some("peer-proof".to_string()))
            .await
            .expect("proof verification should succeed");

        assert!(matches!(
            state,
            uc_core::security::space_access::state::SpaceAccessState::Granted { .. }
        ));

        let guard = transport_state.lock().expect("lock recording transport");
        assert_eq!(guard.results, vec!["session-host-proof".to_string()]);
    }

    #[tokio::test]
    async fn joiner_space_access_result_advances_waiting_decision_to_granted() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let (transport, _transport_state) = RecordingSpaceAccessTransport::new();
        let orchestrator = build_orchestrator_with_space_access_runtime(
            setup_status,
            Arc::new(Mutex::new(transport)),
            Arc::new(ConfigurableProofPort { verify_ok: true }),
        );

        let session_id = "session-join-result".to_string();
        let space_id = uc_core::ids::SpaceId::from("space-join-result");

        {
            let context = orchestrator
                .action_executor
                .space_access_orchestrator
                .context();
            let mut guard = context.lock().await;
            guard.joiner_offer = Some(SpaceAccessJoinerOffer {
                space_id: space_id.clone(),
                keyslot_blob: vec![1, 2, 3],
                challenge_nonce: [4u8; 32],
            });
            guard.joiner_passphrase = Some(SecretString::new("join-secret".to_string()));
            guard.sponsor_peer_id = Some("peer-host".to_string());
        }

        let crypto = SucceedSpaceAccessCrypto;
        let mut transport = orchestrator.action_executor.transport_port.lock().await;
        let mut timer = orchestrator.action_executor.timer_port.lock().await;
        let mut store = orchestrator.action_executor.persistence_port.lock().await;
        let mut executor = SpaceAccessExecutor {
            crypto: &crypto,
            transport: &mut *transport,
            proof: orchestrator.action_executor.proof_port.as_ref(),
            timer: &mut *timer,
            store: &mut *store,
        };

        orchestrator
            .action_executor
            .space_access_orchestrator
            .dispatch(
                &mut executor,
                SpaceAccessEvent::JoinRequested {
                    pairing_session_id: session_id.clone(),
                    ttl_secs: 60,
                },
                Some(session_id.clone()),
            )
            .await
            .expect("join requested");
        orchestrator
            .action_executor
            .space_access_orchestrator
            .dispatch(
                &mut executor,
                SpaceAccessEvent::OfferAccepted {
                    pairing_session_id: session_id.clone(),
                    space_id: space_id.clone(),
                    expires_at: chrono::Utc::now() + chrono::Duration::seconds(60),
                },
                Some(session_id.clone()),
            )
            .await
            .expect("offer accepted");
        orchestrator
            .action_executor
            .space_access_orchestrator
            .dispatch(
                &mut executor,
                SpaceAccessEvent::PassphraseSubmitted,
                Some(session_id.clone()),
            )
            .await
            .expect("passphrase submitted");
        drop(executor);
        drop(store);
        drop(timer);
        drop(transport);

        let state = orchestrator
            .apply_joiner_space_access_result(
                session_id,
                space_id,
                Some("peer-host".to_string()),
                true,
                None,
            )
            .await
            .expect("joiner result should apply");

        assert!(matches!(
            state,
            uc_core::security::space_access::state::SpaceAccessState::Granted { .. }
        ));
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
    async fn concurrent_get_state_waits_for_seed_completion() {
        let setup_status = Arc::new(BlockingSetupStatusPort::new(SetupStatus {
            has_completed: true,
        }));
        let orchestrator = Arc::new(build_orchestrator(setup_status.clone()));

        let first_call = {
            let orchestrator = orchestrator.clone();
            tokio::spawn(async move { orchestrator.get_state().await })
        };

        setup_status.wait_until_get_status_called().await;

        let second_call = {
            let orchestrator = orchestrator.clone();
            tokio::spawn(async move { orchestrator.get_state().await })
        };

        setup_status.release_blocked_get_status();

        let first_state = first_call
            .await
            .expect("first get_state task should succeed");
        let second_state = second_call
            .await
            .expect("second get_state task should succeed");

        assert_eq!(first_state, SetupState::Completed);
        assert_eq!(second_state, SetupState::Completed);
        assert_eq!(setup_status.get_call_count(), 1);
    }

    #[tokio::test]
    async fn clear_transient_state_returns_uncompleted_device_to_welcome() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator(setup_status.clone());

        orchestrator
            .context
            .set_state(SetupState::JoinSpaceInputPassphrase { error: None })
            .await;
        *orchestrator.selected_peer_id.lock().await = Some("peer-a".to_string());
        *orchestrator.pairing_session_id.lock().await = Some("session-a".to_string());
        *orchestrator.passphrase.lock().await = Some(Passphrase("secret".to_string()));

        let state = orchestrator
            .clear_transient_state()
            .await
            .expect("clear transient state should succeed");

        assert_eq!(state, SetupState::Welcome);
        assert_eq!(orchestrator.get_state().await, SetupState::Welcome);
        assert!(orchestrator.selected_peer_id.lock().await.is_none());
        assert!(orchestrator.pairing_session_id.lock().await.is_none());
        assert!(orchestrator.passphrase.lock().await.is_none());
        assert!(!setup_status.get_status().await.unwrap().has_completed);
        assert_eq!(setup_status.set_call_count(), 0);
    }

    #[tokio::test]
    async fn clear_transient_state_preserves_completed_base_state() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus {
            has_completed: true,
        }));
        let orchestrator = build_orchestrator(setup_status.clone());

        orchestrator
            .context
            .set_state(SetupState::JoinSpaceConfirmPeer {
                short_code: "123-456".to_string(),
                peer_fingerprint: Some("fingerprint".to_string()),
                error: None,
            })
            .await;
        *orchestrator.selected_peer_id.lock().await = Some("peer-a".to_string());
        *orchestrator.pairing_session_id.lock().await = Some("session-a".to_string());

        let state = orchestrator
            .clear_transient_state()
            .await
            .expect("clear transient state should succeed");

        assert_eq!(state, SetupState::Completed);
        assert_eq!(orchestrator.get_state().await, SetupState::Completed);
        assert!(orchestrator.selected_peer_id.lock().await.is_none());
        assert!(orchestrator.pairing_session_id.lock().await.is_none());
        assert!(setup_status.get_status().await.unwrap().has_completed);
        assert_eq!(setup_status.set_call_count(), 0);
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
            pairing_orchestrator.clone(),
            setup_event_port.clone(),
            build_space_access_orchestrator(),
            build_discovery_port(),
            build_network_control(),
            build_crypto_factory(),
            build_pairing_transport(),
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
            pairing_orchestrator.clone(),
            setup_event_port.clone(),
            build_space_access_orchestrator(),
            build_discovery_port(),
            build_network_control(),
            build_crypto_factory(),
            build_pairing_transport(),
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

        pairing_orchestrator
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
            pairing_orchestrator.clone(),
            setup_event_port.clone(),
            build_space_access_orchestrator(),
            build_discovery_port(),
            build_network_control(),
            build_crypto_factory(),
            build_pairing_transport(),
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

        pairing_orchestrator
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

        pairing_orchestrator
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
            pairing_orchestrator.clone(),
            setup_event_port.clone(),
            build_space_access_orchestrator(),
            build_discovery_port(),
            build_network_control(),
            build_crypto_factory(),
            build_pairing_transport(),
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

        pairing_orchestrator
            .handle_transport_error(&session_id, "peer-verify", "stream closed".to_string())
            .await
            .unwrap();

        let emit_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let emitted = setup_event_port.snapshot().await;
            let found = emitted.iter().any(|(state, sid)| {
                matches!(
                    state,
                    SetupState::JoinSpaceSelectDevice {
                        error: Some(SetupDomainError::PairingFailed)
                    }
                ) && sid.as_ref() == Some(&session_id)
            });
            if found {
                break;
            }
            assert!(
                Instant::now() < emit_deadline,
                "setup-state-changed JoinSpaceSelectDevice error event timeout"
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn capture_context_preserves_verify_passphrase_events() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator(setup_status);

        let event = orchestrator
            .capture_context(SetupEvent::VerifyPassphrase {
                passphrase: SecretString::new("secret".to_string()),
            })
            .await;

        match event {
            SetupEvent::VerifyPassphrase { .. } => {}
            other => panic!("unexpected event returned: {:?}", other),
        }

        assert!(orchestrator.passphrase.lock().await.is_some());
    }

    #[test]
    fn map_pairing_failure_reason_maps_rejected_timeout_and_peer_unavailable() {
        let rejected = SetupActionExecutor::map_pairing_failure_reason(&FailureReason::Other(
            "Peer rejected pairing".to_string(),
        ));
        assert_eq!(rejected, SetupDomainError::PairingRejected);

        let timeout = SetupActionExecutor::map_pairing_failure_reason(&FailureReason::Other(
            "Timeout(WaitingChallenge)".to_string(),
        ));
        assert_eq!(timeout, SetupDomainError::NetworkTimeout);

        let generic = SetupActionExecutor::map_pairing_failure_reason(&FailureReason::Other(
            "stream closed".to_string(),
        ));
        assert_eq!(generic, SetupDomainError::PairingFailed);

        let unavailable = SetupActionExecutor::map_pairing_failure_reason(&FailureReason::Other(
            "no_local_pairing_participant_ready".to_string(),
        ));
        assert_eq!(unavailable, SetupDomainError::PeerUnavailable);

        let busy = SetupActionExecutor::map_pairing_failure_reason(&FailureReason::PeerBusy);
        assert_eq!(busy, SetupDomainError::PeerUnavailable);
    }

    #[tokio::test]
    async fn start_join_space_access_maps_space_access_error() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let orchestrator = build_orchestrator(setup_status);
        let space_id = uc_core::ids::SpaceId::new();
        let pairing_session_id = "session-join".to_string();

        let crypto = orchestrator
            .action_executor
            .crypto_factory
            .build(SecretString::new("seed-pass".to_string()));
        let mut transport = orchestrator.action_executor.transport_port.lock().await;
        let mut timer = orchestrator.action_executor.timer_port.lock().await;
        let mut store = orchestrator.action_executor.persistence_port.lock().await;
        let mut executor = SpaceAccessExecutor {
            crypto: crypto.as_ref(),
            transport: &mut *transport,
            proof: orchestrator.action_executor.proof_port.as_ref(),
            timer: &mut *timer,
            store: &mut *store,
        };

        orchestrator
            .action_executor
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
            .action_executor
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
            let context = orchestrator
                .action_executor
                .space_access_orchestrator
                .context();
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

        let context = orchestrator
            .action_executor
            .space_access_orchestrator
            .context();
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
            let context = orchestrator
                .action_executor
                .space_access_orchestrator
                .context();
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
            .action_executor
            .crypto_factory
            .build(SecretString::new("join-secret".to_string()));
        let mut transport = orchestrator.action_executor.transport_port.lock().await;
        let mut timer = orchestrator.action_executor.timer_port.lock().await;
        let mut store = orchestrator.action_executor.persistence_port.lock().await;
        let mut executor = SpaceAccessExecutor {
            crypto: crypto.as_ref(),
            transport: &mut *transport,
            proof: orchestrator.action_executor.proof_port.as_ref(),
            timer: &mut *timer,
            store: &mut *store,
        };

        orchestrator
            .action_executor
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
                SetupState::JoinSpaceSelectDevice {
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
                SetupState::JoinSpaceSelectDevice { error: None },
            ),
        ];

        run_join_steps(&orchestrator, &steps).await;

        assert!(orchestrator.selected_peer_id.lock().await.is_none());
        assert!(orchestrator.pairing_session_id.lock().await.is_none());
    }

    /// Verify that when peerA rejects the initial pairing request, peerB
    /// (the joiner) transitions back to JoinSpaceSelectDevice with
    /// error=PairingRejected.
    ///
    /// This covers UAT Test 4: "peerA clicks reject → peerB sees an error
    /// instead of staying on the spinning ProcessingJoinSpace screen."
    #[tokio::test]
    async fn join_space_initial_request_rejected_by_peer_returns_pairing_rejected_error() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));
        let setup_event_port = Arc::new(MockSetupEventPort::default());
        let (pairing_orchestrator, action_rx) = build_pairing_orchestrator_with_actions();
        let orchestrator = SetupOrchestrator::new(
            build_initialize_encryption(),
            mark_setup_complete,
            setup_status,
            build_mock_lifecycle(),
            pairing_orchestrator.clone(),
            setup_event_port.clone(),
            build_space_access_orchestrator(),
            build_discovery_port(),
            build_network_control(),
            build_crypto_factory(),
            build_pairing_transport(),
            build_transport_port(),
            build_proof_port(),
            build_timer_port(),
            build_persistence_port(),
        );

        // Start join flow and select device (which also initiates pairing).
        orchestrator.join_space().await.unwrap();
        orchestrator
            .select_device("peer-reject".to_string())
            .await
            .unwrap();

        // Consume the initial Send action queued by the state machine.
        {
            let mut rx = action_rx.lock().await;
            assert!(
                rx.try_recv().is_ok(),
                "pairing orchestrator should queue initial send action"
            );
        }

        // Wait for the session id to be stored by the setup listener.
        let session_deadline = Instant::now() + Duration::from_secs(1);
        let session_id = loop {
            if let Some(sid) = orchestrator.pairing_session_id.lock().await.clone() {
                break sid;
            }
            assert!(
                Instant::now() < session_deadline,
                "pairing session id was not set after select_device"
            );
            sleep(Duration::from_millis(10)).await;
        };

        // Simulate peerA sending a Reject on the initial request.
        // The pairing state machine is in RequestSent state, which accepts RecvReject.
        pairing_orchestrator
            .handle_reject(&session_id, "peer-reject")
            .await
            .unwrap();

        // The setup pairing listener should receive PairingFailed with a
        // "rejected" reason and drive setup to JoinSpaceSelectDevice with
        // error=PairingRejected.
        let emit_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let emitted = setup_event_port.snapshot().await;
            let found = emitted.iter().any(|(state, sid)| {
                matches!(
                    state,
                    SetupState::JoinSpaceSelectDevice {
                        error: Some(SetupDomainError::PairingRejected)
                    }
                ) && sid.as_ref() == Some(&session_id)
            });
            if found {
                break;
            }
            assert!(
                Instant::now() < emit_deadline,
                "expected JoinSpaceSelectDevice(PairingRejected) event within 1s after reject"
            );
            sleep(Duration::from_millis(10)).await;
        }
    }

    /// Verify that a low-latency PairingVerificationRequired event (arriving
    /// immediately after initiate_pairing) is not missed by the setup listener
    /// because of the subscribe-before-initiate ordering fix.
    ///
    /// This covers UAT Test 2: "ProcessingJoinSpace no longer stalls when the
    /// verification event arrives before the listener was subscribed."
    #[tokio::test]
    async fn join_space_low_latency_verification_advances_to_confirm_peer() {
        let setup_status = Arc::new(MockSetupStatusPort::new(SetupStatus::default()));
        let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));
        let setup_event_port = Arc::new(MockSetupEventPort::default());
        let (pairing_orchestrator, action_rx) = build_pairing_orchestrator_with_actions();
        let orchestrator = SetupOrchestrator::new(
            build_initialize_encryption(),
            mark_setup_complete,
            setup_status,
            build_mock_lifecycle(),
            pairing_orchestrator.clone(),
            setup_event_port.clone(),
            build_space_access_orchestrator(),
            build_discovery_port(),
            build_network_control(),
            build_crypto_factory(),
            build_pairing_transport(),
            build_transport_port(),
            build_proof_port(),
            build_timer_port(),
            build_persistence_port(),
        );

        // Start join flow and select device.
        orchestrator.join_space().await.unwrap();
        orchestrator
            .select_device("peer-low-latency".to_string())
            .await
            .unwrap();

        // Consume the initial Send action.
        {
            let mut rx = action_rx.lock().await;
            assert!(
                rx.try_recv().is_ok(),
                "pairing orchestrator should queue initial send action"
            );
        }

        // Wait for the session id to be captured.
        let session_deadline = Instant::now() + Duration::from_secs(1);
        let session_id = loop {
            if let Some(sid) = orchestrator.pairing_session_id.lock().await.clone() {
                break sid;
            }
            assert!(
                Instant::now() < session_deadline,
                "pairing session id was not set after select_device"
            );
            sleep(Duration::from_millis(10)).await;
        };

        // Immediately deliver a PairingChallenge — this is the low-latency
        // path where the remote responds with a challenge before the listener
        // had a chance to subscribe in the old (buggy) ordering.  With the
        // subscribe-before-initiate fix, the listener is already active.
        pairing_orchestrator
            .handle_challenge(
                &session_id,
                "peer-low-latency",
                uc_core::network::protocol::PairingChallenge {
                    session_id: session_id.clone(),
                    pin: "111-222".to_string(),
                    device_name: "remote-ll".to_string(),
                    device_id: "remote-ll-id".to_string(),
                    identity_pubkey: vec![5; 32],
                    nonce: vec![6; 32],
                },
            )
            .await
            .unwrap();

        // Setup state should advance to JoinSpaceConfirmPeer.
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
                "expected JoinSpaceConfirmPeer event within 1s \
                 — low-latency verification event was missed"
            );
            sleep(Duration::from_millis(10)).await;
        }
    }
}

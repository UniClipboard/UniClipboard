use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tempfile::TempDir;
use uc_app::usecases::clipboard::ClipboardIntegrationMode;
use uc_app::usecases::space_access::{SpaceAccessExecutor, SpaceAccessOrchestrator};
use uc_app::usecases::{
    space_access::{
        DefaultSpaceAccessCryptoFactory, HmacProofAdapter, SpaceAccessPersistenceAdapter,
    },
    AppLifecycleCoordinator, AppLifecycleCoordinatorDeps, InitializeEncryption, LifecycleEvent,
    LifecycleEventEmitter, LifecycleState, LifecycleStatusPort, MarkSetupComplete, PairingConfig,
    PairingOrchestrator, SessionReadyEmitter, SetupOrchestrator, StartClipboardWatcher,
    StartNetworkAfterUnlock,
};
use uc_core::network::pairing_state_machine::PairingAction;
use uc_core::network::protocol::{PairingChallenge, PairingMessage};
use uc_core::network::{DiscoveredPeer, PairedDevice, PairingState};
use uc_core::ports::network_control::NetworkControlPort;
use uc_core::ports::security::key_scope::{KeyScopePort, ScopeError};
use uc_core::ports::security::secure_storage::{SecureStorageError, SecureStoragePort};
use uc_core::ports::space::{CryptoPort, PersistencePort, SpaceAccessTransportPort};
use uc_core::ports::watcher_control::{WatcherControlError, WatcherControlPort};
use uc_core::ports::{
    DiscoveryPort, EncryptionSessionPort, PairedDeviceRepositoryError, PairedDeviceRepositoryPort,
    SetupEventPort, SetupStatusPort, TimerPort,
};
use uc_core::security::model::KeyScope;
use uc_core::security::space_access::event::SpaceAccessEvent;
use uc_core::setup::SetupState;
use uc_core::PeerId;
use uc_infra::fs::key_slot_store::JsonKeySlotStore;
use uc_infra::security::{
    DefaultKeyMaterialService, EncryptionRepository, FileEncryptionStateRepository,
    InMemoryEncryptionSession,
};
use uc_infra::setup_status::FileSetupStatusRepository;
use uc_infra::time::Timer;

use tokio::sync::mpsc;
use tokio::time::{sleep, Duration, Instant};

#[derive(Default)]
struct InMemorySecureStorage {
    data: Mutex<HashMap<String, Vec<u8>>>,
}

impl SecureStoragePort for InMemorySecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        Ok(self.data.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        self.data
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
        self.data.lock().unwrap().remove(key);
        Ok(())
    }
}

struct TestKeyScope {
    scope: KeyScope,
}

impl Default for TestKeyScope {
    fn default() -> Self {
        Self {
            scope: KeyScope {
                profile_id: "default".to_string(),
            },
        }
    }
}

#[async_trait::async_trait]
impl KeyScopePort for TestKeyScope {
    async fn current_scope(&self) -> Result<KeyScope, ScopeError> {
        Ok(self.scope.clone())
    }
}

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

struct OrderedNetworkControl {
    calls: Arc<Mutex<Vec<&'static str>>>,
}

#[async_trait]
impl NetworkControlPort for OrderedNetworkControl {
    async fn start_network(&self) -> anyhow::Result<()> {
        self.calls.lock().unwrap().push("network");
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

struct NoopSetupEventPort;

#[async_trait]
impl SetupEventPort for NoopSetupEventPort {
    async fn emit_setup_state_changed(&self, _state: SetupState, _session_id: Option<String>) {}
}

struct OrderedDiscoveryPort {
    calls: Arc<Mutex<Vec<&'static str>>>,
}

#[async_trait]
impl DiscoveryPort for OrderedDiscoveryPort {
    async fn list_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
        self.calls.lock().unwrap().push("discovery");
        Ok(Vec::new())
    }
}

struct NoopSpaceAccessTransport;

#[async_trait]
impl SpaceAccessTransportPort for NoopSpaceAccessTransport {
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

struct NoopSpaceAccessNetworkPort;

#[async_trait]
impl uc_core::ports::NetworkPort for NoopSpaceAccessNetworkPort {
    async fn send_clipboard(&self, _peer_id: &str, _encrypted_data: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn broadcast_clipboard(&self, _encrypted_data: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe_clipboard(
        &self,
    ) -> anyhow::Result<tokio::sync::mpsc::Receiver<uc_core::network::ClipboardMessage>> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(rx)
    }

    async fn get_discovered_peers(&self) -> anyhow::Result<Vec<uc_core::network::DiscoveredPeer>> {
        Ok(vec![])
    }

    async fn get_connected_peers(&self) -> anyhow::Result<Vec<uc_core::network::ConnectedPeer>> {
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

struct DeterministicSpaceAccessCrypto;

#[async_trait]
impl CryptoPort for DeterministicSpaceAccessCrypto {
    async fn generate_nonce32(&self) -> [u8; 32] {
        [1; 32]
    }

    async fn export_keyslot_blob(
        &self,
        _space_id: &uc_core::ids::SpaceId,
    ) -> anyhow::Result<uc_core::security::model::KeySlot> {
        Ok(uc_core::security::model::KeySlot {
            version: uc_core::security::model::KeySlotVersion::V1,
            scope: uc_core::security::model::KeyScope {
                profile_id: "space-access-test".to_string(),
            },
            kdf: uc_core::security::model::KdfParams::for_initialization(),
            salt: vec![2; 16],
            wrapped_master_key: None,
        })
    }

    async fn derive_master_key_from_keyslot(
        &self,
        _keyslot_blob: &[u8],
        _passphrase: uc_core::security::SecretString,
    ) -> anyhow::Result<uc_core::security::model::MasterKey> {
        uc_core::security::model::MasterKey::from_bytes(&[3; 32])
            .map_err(|err| anyhow::anyhow!(err.to_string()))
    }
}

struct DeterministicSpaceAccessPersistence;

#[async_trait]
impl PersistencePort for DeterministicSpaceAccessPersistence {
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

async fn drive_space_access_to_waiting_decision(
    orchestrator: &SpaceAccessOrchestrator,
    pairing_session_id: String,
    space_id: uc_core::ids::SpaceId,
) {
    {
        let context = orchestrator.context();
        let mut guard = context.lock().await;
        guard.joiner_offer = Some(uc_app::usecases::space_access::SpaceAccessJoinerOffer {
            space_id: space_id.clone(),
            keyslot_blob: vec![9, 8, 7],
            challenge_nonce: [4; 32],
        });
        guard.joiner_passphrase = Some(uc_core::security::SecretString::new(
            "join-secret".to_string(),
        ));
        guard.sponsor_peer_id = Some("peer-join".to_string());
    }

    let network_port = NoopSpaceAccessNetworkPort;
    let crypto = DeterministicSpaceAccessCrypto;
    let mut transport = NoopSpaceAccessTransport;
    let proof = HmacProofAdapter::new();
    let mut timer = Timer::new();
    let mut store = DeterministicSpaceAccessPersistence;
    let mut executor = SpaceAccessExecutor {
        crypto: &crypto,
        net: &network_port,
        transport: &mut transport,
        proof: &proof,
        timer: &mut timer,
        store: &mut store,
    };

    orchestrator
        .dispatch(
            &mut executor,
            SpaceAccessEvent::JoinRequested {
                pairing_session_id: pairing_session_id.clone(),
                ttl_secs: 60,
            },
            Some(pairing_session_id.clone()),
        )
        .await
        .expect("join requested");

    orchestrator
        .dispatch(
            &mut executor,
            SpaceAccessEvent::OfferAccepted {
                pairing_session_id: pairing_session_id.clone(),
                space_id: space_id.clone(),
                expires_at: chrono::Utc::now() + chrono::Duration::seconds(60),
            },
            Some(pairing_session_id.clone()),
        )
        .await
        .expect("offer accepted");

    let state = orchestrator
        .dispatch(
            &mut executor,
            SpaceAccessEvent::PassphraseSubmitted,
            Some(pairing_session_id),
        )
        .await
        .expect("passphrase submitted");

    assert!(matches!(
        state,
        uc_core::security::space_access::state::SpaceAccessState::WaitingDecision { .. }
    ));
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

fn build_ordered_mock_lifecycle(
    calls: Arc<Mutex<Vec<&'static str>>>,
) -> Arc<AppLifecycleCoordinator> {
    Arc::new(AppLifecycleCoordinator::from_deps(
        AppLifecycleCoordinatorDeps {
            watcher: Arc::new(StartClipboardWatcher::new(
                Arc::new(MockWatcherControl),
                ClipboardIntegrationMode::Full,
            )),
            network: Arc::new(StartNetworkAfterUnlock::new(Arc::new(
                OrderedNetworkControl { calls },
            ))),
            announcer: None,
            emitter: Arc::new(MockSessionReadyEmitter),
            status: Arc::new(MockLifecycleStatus),
            lifecycle_emitter: Arc::new(MockLifecycleEventEmitter),
        },
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
    tokio::sync::Mutex<mpsc::Receiver<PairingAction>>,
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

#[tokio::test]
async fn create_space_flow_marks_setup_complete_and_persists_state() {
    let temp_dir = TempDir::new().expect("temp dir");
    let vault_dir = temp_dir.path().join("vault");
    std::fs::create_dir_all(&vault_dir).expect("create vault dir");

    let keyslot_store = Arc::new(JsonKeySlotStore::new(vault_dir.clone()));
    let secure_storage = Arc::new(InMemorySecureStorage::default());
    let key_material = Arc::new(DefaultKeyMaterialService::new(
        secure_storage,
        keyslot_store,
    ));

    let encryption = Arc::new(EncryptionRepository);
    let key_scope = Arc::new(TestKeyScope::default());
    let encryption_state = Arc::new(FileEncryptionStateRepository::new(vault_dir.clone()));
    let encryption_session = Arc::new(InMemoryEncryptionSession::new());

    let initialize_encryption = Arc::new(InitializeEncryption::from_ports(
        encryption.clone(),
        key_material.clone(),
        key_scope.clone(),
        encryption_state.clone(),
        encryption_session.clone(),
    ));

    let setup_status = Arc::new(FileSetupStatusRepository::with_defaults(vault_dir.clone()));
    let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));
    let crypto_factory: Arc<dyn uc_app::usecases::space_access::SpaceAccessCryptoFactory> =
        Arc::new(DefaultSpaceAccessCryptoFactory::new(
            encryption,
            key_material,
            key_scope,
            encryption_state.clone(),
            encryption_session.clone(),
        ));
    let transport_port: Arc<tokio::sync::Mutex<dyn SpaceAccessTransportPort>> =
        Arc::new(tokio::sync::Mutex::new(NoopSpaceAccessTransport));
    let proof_port: Arc<dyn uc_core::ports::space::ProofPort> = Arc::new(HmacProofAdapter::new());
    let timer_port: Arc<tokio::sync::Mutex<dyn TimerPort>> =
        Arc::new(tokio::sync::Mutex::new(Timer::new()));
    let persistence_port: Arc<tokio::sync::Mutex<dyn uc_core::ports::space::PersistencePort>> =
        Arc::new(tokio::sync::Mutex::new(SpaceAccessPersistenceAdapter::new(
            encryption_state,
            Arc::new(NoopPairedDeviceRepository),
        )));
    let orchestrator = SetupOrchestrator::new(
        initialize_encryption,
        mark_setup_complete,
        setup_status.clone(),
        build_mock_lifecycle(),
        build_pairing_orchestrator(),
        Arc::new(NoopSetupEventPort),
        build_space_access_orchestrator(),
        build_discovery_port(),
        Arc::new(MockNetworkControl),
        crypto_factory,
        Arc::new(NoopSpaceAccessNetworkPort),
        transport_port,
        proof_port,
        timer_port,
        persistence_port,
    );

    orchestrator.new_space().await.expect("new space");
    let state = orchestrator
        .submit_passphrase("secret".to_string(), "secret".to_string())
        .await
        .expect("submit passphrase");

    assert_eq!(state, SetupState::Completed);

    let status = setup_status.get_status().await.expect("get status");
    assert!(status.has_completed);
    assert!(encryption_session.is_ready().await);
    assert!(vault_dir.join(".initialized_encryption").exists());
}

async fn drive_to_join_passphrase_state(
    orchestrator: &SetupOrchestrator,
    pairing_orchestrator: &PairingOrchestrator,
    action_rx: &tokio::sync::Mutex<mpsc::Receiver<PairingAction>>,
) -> String {
    let state = orchestrator.join_space().await.expect("join space");
    assert_eq!(state, SetupState::JoinSpaceSelectDevice { error: None });

    let state = orchestrator
        .select_device("peer-join".to_string())
        .await
        .expect("select device");
    assert!(matches!(state, SetupState::ProcessingJoinSpace { .. }));

    let session_id = {
        let mut rx = action_rx.lock().await;
        let action = rx.recv().await.expect("pairing action");
        match action {
            PairingAction::Send {
                message: PairingMessage::Request(request),
                ..
            } => request.session_id,
            other => panic!("unexpected pairing action: {:?}", other),
        }
    };

    pairing_orchestrator
        .handle_challenge(
            &session_id,
            "peer-join",
            PairingChallenge {
                session_id: session_id.clone(),
                pin: "123456".to_string(),
                device_name: "remote-device".to_string(),
                device_id: "remote-device-id".to_string(),
                identity_pubkey: vec![7; 32],
                nonce: vec![1; 32],
            },
        )
        .await
        .expect("handle challenge");

    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if matches!(
            orchestrator.get_state().await,
            SetupState::JoinSpaceConfirmPeer { .. }
        ) {
            break;
        }
        assert!(Instant::now() < deadline, "join confirm peer state timeout");
        sleep(Duration::from_millis(10)).await;
    }

    let state = orchestrator
        .confirm_peer_trust()
        .await
        .expect("confirm peer trust");
    assert_eq!(state, SetupState::JoinSpaceInputPassphrase { error: None });

    session_id
}

#[tokio::test]
async fn ensure_discovery_starts_network_before_listing_peers() {
    let temp_dir = TempDir::new().expect("temp dir");
    let vault_dir = temp_dir.path().join("vault");
    std::fs::create_dir_all(&vault_dir).expect("create vault dir");

    let keyslot_store = Arc::new(JsonKeySlotStore::new(vault_dir.clone()));
    let secure_storage = Arc::new(InMemorySecureStorage::default());
    let key_material = Arc::new(DefaultKeyMaterialService::new(
        secure_storage,
        keyslot_store,
    ));
    let encryption = Arc::new(EncryptionRepository);
    let key_scope = Arc::new(TestKeyScope::default());
    let encryption_state = Arc::new(FileEncryptionStateRepository::new(vault_dir.clone()));
    let encryption_session = Arc::new(InMemoryEncryptionSession::new());

    let initialize_encryption = Arc::new(InitializeEncryption::from_ports(
        encryption.clone(),
        key_material.clone(),
        key_scope.clone(),
        encryption_state.clone(),
        encryption_session.clone(),
    ));

    let setup_status = Arc::new(FileSetupStatusRepository::with_defaults(vault_dir.clone()));
    let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));
    let crypto_factory: Arc<dyn uc_app::usecases::space_access::SpaceAccessCryptoFactory> =
        Arc::new(DefaultSpaceAccessCryptoFactory::new(
            encryption,
            key_material,
            key_scope,
            encryption_state.clone(),
            encryption_session,
        ));
    let transport_port: Arc<tokio::sync::Mutex<dyn SpaceAccessTransportPort>> =
        Arc::new(tokio::sync::Mutex::new(NoopSpaceAccessTransport));
    let proof_port: Arc<dyn uc_core::ports::space::ProofPort> = Arc::new(HmacProofAdapter::new());
    let timer_port: Arc<tokio::sync::Mutex<dyn TimerPort>> =
        Arc::new(tokio::sync::Mutex::new(Timer::new()));
    let persistence_port: Arc<tokio::sync::Mutex<dyn uc_core::ports::space::PersistencePort>> =
        Arc::new(tokio::sync::Mutex::new(SpaceAccessPersistenceAdapter::new(
            encryption_state,
            Arc::new(NoopPairedDeviceRepository),
        )));
    let calls = Arc::new(Mutex::new(Vec::new()));

    let orchestrator = SetupOrchestrator::new(
        initialize_encryption,
        mark_setup_complete,
        setup_status,
        build_ordered_mock_lifecycle(calls.clone()),
        build_pairing_orchestrator(),
        Arc::new(NoopSetupEventPort),
        build_space_access_orchestrator(),
        Arc::new(OrderedDiscoveryPort {
            calls: calls.clone(),
        }),
        Arc::new(OrderedNetworkControl {
            calls: calls.clone(),
        }),
        crypto_factory,
        Arc::new(NoopSpaceAccessNetworkPort),
        transport_port,
        proof_port,
        timer_port,
        persistence_port,
    );

    let state = orchestrator.join_space().await.expect("join space");
    assert_eq!(state, SetupState::JoinSpaceSelectDevice { error: None });
    assert_eq!(calls.lock().unwrap().as_slice(), ["network", "discovery"]);
}

#[tokio::test]
async fn join_space_access_invokes_space_access_orchestrator() {
    let temp_dir = TempDir::new().expect("temp dir");
    let vault_dir = temp_dir.path().join("vault");
    std::fs::create_dir_all(&vault_dir).expect("create vault dir");

    let keyslot_store = Arc::new(JsonKeySlotStore::new(vault_dir.clone()));
    let secure_storage = Arc::new(InMemorySecureStorage::default());
    let key_material = Arc::new(DefaultKeyMaterialService::new(
        secure_storage,
        keyslot_store,
    ));

    let encryption = Arc::new(EncryptionRepository);
    let key_scope = Arc::new(TestKeyScope::default());
    let encryption_state = Arc::new(FileEncryptionStateRepository::new(vault_dir.clone()));
    let encryption_session = Arc::new(InMemoryEncryptionSession::new());

    let initialize_encryption = Arc::new(InitializeEncryption::from_ports(
        encryption.clone(),
        key_material.clone(),
        key_scope.clone(),
        encryption_state.clone(),
        encryption_session.clone(),
    ));

    let setup_status = Arc::new(FileSetupStatusRepository::with_defaults(vault_dir.clone()));
    let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));
    let crypto_factory: Arc<dyn uc_app::usecases::space_access::SpaceAccessCryptoFactory> =
        Arc::new(DefaultSpaceAccessCryptoFactory::new(
            encryption,
            key_material,
            key_scope,
            encryption_state.clone(),
            encryption_session,
        ));
    let transport_port: Arc<tokio::sync::Mutex<dyn SpaceAccessTransportPort>> =
        Arc::new(tokio::sync::Mutex::new(NoopSpaceAccessTransport));
    let proof_port: Arc<dyn uc_core::ports::space::ProofPort> = Arc::new(HmacProofAdapter::new());
    let timer_port: Arc<tokio::sync::Mutex<dyn TimerPort>> =
        Arc::new(tokio::sync::Mutex::new(Timer::new()));
    let persistence_port: Arc<tokio::sync::Mutex<dyn uc_core::ports::space::PersistencePort>> =
        Arc::new(tokio::sync::Mutex::new(SpaceAccessPersistenceAdapter::new(
            encryption_state,
            Arc::new(NoopPairedDeviceRepository),
        )));
    let (pairing_orchestrator, action_rx) = build_pairing_orchestrator_with_actions();
    let space_access_orchestrator = build_space_access_orchestrator();
    let orchestrator = SetupOrchestrator::new(
        initialize_encryption,
        mark_setup_complete,
        setup_status,
        build_mock_lifecycle(),
        pairing_orchestrator.clone(),
        Arc::new(NoopSetupEventPort),
        space_access_orchestrator.clone(),
        build_discovery_port(),
        Arc::new(MockNetworkControl),
        crypto_factory,
        Arc::new(NoopSpaceAccessNetworkPort),
        transport_port,
        proof_port,
        timer_port,
        persistence_port,
    );

    let _pairing_session_id =
        drive_to_join_passphrase_state(&orchestrator, &pairing_orchestrator, &action_rx).await;

    let result = orchestrator
        .submit_passphrase("join-secret".to_string(), "join-secret".to_string())
        .await;

    assert!(matches!(
        result,
        Err(uc_app::usecases::setup::SetupError::PairingFailed)
    ));
}

#[tokio::test]
async fn join_space_access_propagates_space_access_error() {
    let temp_dir = TempDir::new().expect("temp dir");
    let vault_dir = temp_dir.path().join("vault");
    std::fs::create_dir_all(&vault_dir).expect("create vault dir");

    let keyslot_store = Arc::new(JsonKeySlotStore::new(vault_dir.clone()));
    let secure_storage = Arc::new(InMemorySecureStorage::default());
    let key_material = Arc::new(DefaultKeyMaterialService::new(
        secure_storage,
        keyslot_store,
    ));

    let encryption = Arc::new(EncryptionRepository);
    let key_scope = Arc::new(TestKeyScope::default());
    let encryption_state = Arc::new(FileEncryptionStateRepository::new(vault_dir.clone()));
    let encryption_session = Arc::new(InMemoryEncryptionSession::new());

    let initialize_encryption = Arc::new(InitializeEncryption::from_ports(
        encryption.clone(),
        key_material.clone(),
        key_scope.clone(),
        encryption_state.clone(),
        encryption_session.clone(),
    ));

    let setup_status = Arc::new(FileSetupStatusRepository::with_defaults(vault_dir.clone()));
    let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));
    let crypto_factory: Arc<dyn uc_app::usecases::space_access::SpaceAccessCryptoFactory> =
        Arc::new(DefaultSpaceAccessCryptoFactory::new(
            encryption,
            key_material,
            key_scope,
            encryption_state.clone(),
            encryption_session,
        ));
    let transport_port: Arc<tokio::sync::Mutex<dyn SpaceAccessTransportPort>> =
        Arc::new(tokio::sync::Mutex::new(NoopSpaceAccessTransport));
    let proof_port: Arc<dyn uc_core::ports::space::ProofPort> = Arc::new(HmacProofAdapter::new());
    let timer_port: Arc<tokio::sync::Mutex<dyn TimerPort>> =
        Arc::new(tokio::sync::Mutex::new(Timer::new()));
    let persistence_port: Arc<tokio::sync::Mutex<dyn uc_core::ports::space::PersistencePort>> =
        Arc::new(tokio::sync::Mutex::new(SpaceAccessPersistenceAdapter::new(
            encryption_state,
            Arc::new(NoopPairedDeviceRepository),
        )));
    let (pairing_orchestrator, action_rx) = build_pairing_orchestrator_with_actions();
    let space_access_orchestrator = build_space_access_orchestrator();
    let orchestrator = SetupOrchestrator::new(
        initialize_encryption,
        mark_setup_complete,
        setup_status,
        build_mock_lifecycle(),
        pairing_orchestrator.clone(),
        Arc::new(NoopSetupEventPort),
        space_access_orchestrator.clone(),
        build_discovery_port(),
        Arc::new(MockNetworkControl),
        crypto_factory.clone(),
        Arc::new(NoopSpaceAccessNetworkPort),
        transport_port.clone(),
        proof_port,
        timer_port.clone(),
        persistence_port.clone(),
    );

    let pairing_session_id =
        drive_to_join_passphrase_state(&orchestrator, &pairing_orchestrator, &action_rx).await;

    let space_id = uc_core::ids::SpaceId::new();
    let crypto = crypto_factory.build(uc_core::security::SecretString::new(
        "seed-pass".to_string(),
    ));
    let network_port = NoopSpaceAccessNetworkPort;
    let proof_adapter = HmacProofAdapter::new();
    let mut transport = transport_port.lock().await;
    let mut timer = timer_port.lock().await;
    let mut store = persistence_port.lock().await;
    let mut executor = SpaceAccessExecutor {
        crypto: crypto.as_ref(),
        net: &network_port,
        transport: &mut *transport,
        proof: &proof_adapter,
        timer: &mut *timer,
        store: &mut *store,
    };
    space_access_orchestrator
        .dispatch(
            &mut executor,
            SpaceAccessEvent::JoinRequested {
                pairing_session_id: pairing_session_id.clone(),
                ttl_secs: 60,
            },
            Some(pairing_session_id.clone()),
        )
        .await
        .expect("join requested");
    space_access_orchestrator
        .dispatch(
            &mut executor,
            SpaceAccessEvent::OfferAccepted {
                pairing_session_id: pairing_session_id.clone(),
                space_id,
                expires_at: chrono::Utc::now() + chrono::Duration::seconds(60),
            },
            Some(pairing_session_id),
        )
        .await
        .expect("offer accepted");
    drop(executor);
    drop(store);
    drop(timer);
    drop(transport);

    let result = orchestrator
        .submit_passphrase("join-secret".to_string(), "join-secret".to_string())
        .await;

    assert!(matches!(
        result,
        Err(uc_app::usecases::setup::SetupError::PairingFailed)
    ));
}

#[tokio::test]
async fn join_space_flow_converges_to_granted_on_access_granted_result() {
    let space_access_orchestrator = build_space_access_orchestrator();
    let pairing_session_id = "session-granted".to_string();
    let space_id = uc_core::ids::SpaceId::new();

    drive_space_access_to_waiting_decision(
        space_access_orchestrator.as_ref(),
        pairing_session_id.clone(),
        space_id.clone(),
    )
    .await;

    let network_port = NoopSpaceAccessNetworkPort;
    let crypto = DeterministicSpaceAccessCrypto;
    let mut transport = NoopSpaceAccessTransport;
    let proof = HmacProofAdapter::new();
    let mut timer = Timer::new();
    let mut store = DeterministicSpaceAccessPersistence;
    let mut executor = SpaceAccessExecutor {
        crypto: &crypto,
        net: &network_port,
        transport: &mut transport,
        proof: &proof,
        timer: &mut timer,
        store: &mut store,
    };

    let state = space_access_orchestrator
        .dispatch(
            &mut executor,
            SpaceAccessEvent::AccessGranted {
                pairing_session_id: pairing_session_id.clone(),
                space_id: space_id.clone(),
            },
            Some(pairing_session_id),
        )
        .await
        .expect("access granted convergence");

    assert!(matches!(
        state,
        uc_core::security::space_access::state::SpaceAccessState::Granted { .. }
    ));
}

#[tokio::test]
async fn join_space_flow_converges_to_denied_on_access_denied_result() {
    let space_access_orchestrator = build_space_access_orchestrator();
    let pairing_session_id = "session-denied".to_string();
    let space_id = uc_core::ids::SpaceId::new();

    drive_space_access_to_waiting_decision(
        space_access_orchestrator.as_ref(),
        pairing_session_id.clone(),
        space_id.clone(),
    )
    .await;

    let network_port = NoopSpaceAccessNetworkPort;
    let crypto = DeterministicSpaceAccessCrypto;
    let mut transport = NoopSpaceAccessTransport;
    let proof = HmacProofAdapter::new();
    let mut timer = Timer::new();
    let mut store = DeterministicSpaceAccessPersistence;
    let mut executor = SpaceAccessExecutor {
        crypto: &crypto,
        net: &network_port,
        transport: &mut transport,
        proof: &proof,
        timer: &mut timer,
        store: &mut store,
    };

    let state = space_access_orchestrator
        .dispatch(
            &mut executor,
            SpaceAccessEvent::AccessDenied {
                pairing_session_id: pairing_session_id.clone(),
                space_id,
                reason: uc_core::security::space_access::state::DenyReason::InvalidProof,
            },
            Some(pairing_session_id),
        )
        .await
        .expect("access denied convergence");

    assert!(matches!(
        state,
        uc_core::security::space_access::state::SpaceAccessState::Denied {
            reason: uc_core::security::space_access::state::DenyReason::InvalidProof,
            ..
        }
    ));
}

#[tokio::test]
async fn join_space_flow_times_out_when_result_does_not_arrive() {
    let space_access_orchestrator = build_space_access_orchestrator();
    let pairing_session_id = "session-timeout".to_string();
    let space_id = uc_core::ids::SpaceId::new();

    drive_space_access_to_waiting_decision(
        space_access_orchestrator.as_ref(),
        pairing_session_id.clone(),
        space_id,
    )
    .await;

    let network_port = NoopSpaceAccessNetworkPort;
    let crypto = DeterministicSpaceAccessCrypto;
    let mut transport = NoopSpaceAccessTransport;
    let proof = HmacProofAdapter::new();
    let mut timer = Timer::new();
    let mut store = DeterministicSpaceAccessPersistence;
    let mut executor = SpaceAccessExecutor {
        crypto: &crypto,
        net: &network_port,
        transport: &mut transport,
        proof: &proof,
        timer: &mut timer,
        store: &mut store,
    };

    let state = space_access_orchestrator
        .dispatch(
            &mut executor,
            SpaceAccessEvent::Timeout,
            Some(pairing_session_id),
        )
        .await
        .expect("timeout convergence");

    assert!(matches!(
        state,
        uc_core::security::space_access::state::SpaceAccessState::Cancelled {
            reason: uc_core::security::space_access::state::CancelReason::Timeout,
            ..
        }
    ));
}

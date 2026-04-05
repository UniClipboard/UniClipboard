use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tempfile::TempDir;
use uc_app::testing::{
    NoopDiscoveryPort, NoopLifecycleEventEmitter, NoopLifecycleStatus, NoopNetworkControl,
    NoopPairedDeviceRepository, NoopPairingTransport, NoopSessionReadyEmitter, NoopSetupEventPort,
    NoopSpaceAccessPersistence, NoopSpaceAccessTransport,
};
use uc_app::usecases::space_access::{SpaceAccessExecutor, SpaceAccessOrchestrator};
use uc_app::usecases::{
    space_access::{
        DefaultSpaceAccessCryptoFactory, HmacProofAdapter, SpaceAccessPersistenceAdapter,
    },
    AppLifecycleCoordinator, AppLifecycleCoordinatorDeps, InitializeEncryption, MarkSetupComplete,
    PairingConfig, PairingOrchestrator, SetupOrchestrator, StartNetworkAfterUnlock,
};
use uc_core::network::pairing_state_machine::PairingAction;
use uc_core::network::protocol::{PairingChallenge, PairingMessage};
use uc_core::network::DiscoveredPeer;
use uc_core::ports::network_control::NetworkControlPort;
use uc_core::ports::security::key_scope::{KeyScopePort, ScopeError};
use uc_core::ports::security::secure_storage::{SecureStorageError, SecureStoragePort};
use uc_core::ports::space::{CryptoPort, SpaceAccessTransportPort};
use uc_core::ports::{DiscoveryPort, EncryptionSessionPort, SetupStatusPort, TimerPort};
use uc_core::security::model::KeyScope;
use uc_core::security::space_access::event::SpaceAccessEvent;
use uc_core::setup::SetupState;
use uc_infra::fs::key_slot_store::JsonKeySlotStore;
use uc_infra::security::{
    DefaultKeyMaterialService, EncryptionRepository, FileEncryptionStateRepository,
};
use uc_infra::setup_status::FileSetupStatusRepository;
use uc_infra::time::Timer;
use uc_platform::adapters::InMemoryEncryptionSessionPort;

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

// NoopNetworkControl, NoopSessionReadyEmitter, NoopLifecycleStatus,
// NoopLifecycleEventEmitter, NoopPairedDeviceRepository, NoopDiscoveryPort,
// NoopSetupEventPort, NoopSpaceAccessTransport, NoopPairingTransport
// — imported from uc_app::testing.
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

// NoopSpaceAccessPersistence replaced by NoopSpaceAccessPersistence from uc_app::testing

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

    let crypto = DeterministicSpaceAccessCrypto;
    let mut transport = NoopSpaceAccessTransport;
    let proof = HmacProofAdapter::new();
    let mut timer = Timer::new();
    let mut store = NoopSpaceAccessPersistence;
    let mut executor = SpaceAccessExecutor {
        crypto: &crypto,
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
            network: Arc::new(StartNetworkAfterUnlock::new(Arc::new(NoopNetworkControl))),
            announcer: None,
            emitter: Arc::new(NoopSessionReadyEmitter),
            status: Arc::new(NoopLifecycleStatus),
            lifecycle_emitter: Arc::new(NoopLifecycleEventEmitter),
        },
    ))
}

fn build_ordered_mock_lifecycle(
    calls: Arc<Mutex<Vec<&'static str>>>,
) -> Arc<AppLifecycleCoordinator> {
    Arc::new(AppLifecycleCoordinator::from_deps(
        AppLifecycleCoordinatorDeps {
            network: Arc::new(StartNetworkAfterUnlock::new(Arc::new(
                OrderedNetworkControl { calls },
            ))),
            announcer: None,
            emitter: Arc::new(NoopSessionReadyEmitter),
            status: Arc::new(NoopLifecycleStatus),
            lifecycle_emitter: Arc::new(NoopLifecycleEventEmitter),
        },
    ))
}

fn build_pairing_orchestrator() -> Arc<PairingOrchestrator> {
    let repo = Arc::new(NoopPairedDeviceRepository);
    let staged_store = Arc::new(uc_app::usecases::StagedPairedDeviceStore::new());
    let (orchestrator, _rx) = PairingOrchestrator::new(
        PairingConfig::default(),
        repo,
        "test-device".to_string(),
        "test-device-id".to_string(),
        "test-peer-id".to_string(),
        vec![1; 32],
        staged_store,
    );
    Arc::new(orchestrator)
}

fn build_pairing_orchestrator_with_actions() -> (
    Arc<PairingOrchestrator>,
    tokio::sync::Mutex<mpsc::Receiver<PairingAction>>,
) {
    let repo = Arc::new(NoopPairedDeviceRepository);
    let staged_store = Arc::new(uc_app::usecases::StagedPairedDeviceStore::new());
    let (orchestrator, rx) = PairingOrchestrator::new(
        PairingConfig::default(),
        repo,
        "test-device".to_string(),
        "test-device-id".to_string(),
        "test-peer-id".to_string(),
        vec![1; 32],
        staged_store,
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
    let encryption_session = Arc::new(InMemoryEncryptionSessionPort::new());

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
            Arc::new(uc_app::usecases::StagedPairedDeviceStore::new()),
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
        Arc::new(NoopNetworkControl),
        crypto_factory,
        Arc::new(NoopPairingTransport),
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
    let encryption_session = Arc::new(InMemoryEncryptionSessionPort::new());

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
            Arc::new(uc_app::usecases::StagedPairedDeviceStore::new()),
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
        Arc::new(NoopPairingTransport),
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
    let encryption_session = Arc::new(InMemoryEncryptionSessionPort::new());

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
            Arc::new(uc_app::usecases::StagedPairedDeviceStore::new()),
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
        Arc::new(NoopNetworkControl),
        crypto_factory,
        Arc::new(NoopPairingTransport),
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
    let encryption_session = Arc::new(InMemoryEncryptionSessionPort::new());

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
            Arc::new(uc_app::usecases::StagedPairedDeviceStore::new()),
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
        Arc::new(NoopNetworkControl),
        crypto_factory.clone(),
        Arc::new(NoopPairingTransport),
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
    let proof_adapter = HmacProofAdapter::new();
    let mut transport = transport_port.lock().await;
    let mut timer = timer_port.lock().await;
    let mut store = persistence_port.lock().await;
    let mut executor = SpaceAccessExecutor {
        crypto: crypto.as_ref(),
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

    let crypto = DeterministicSpaceAccessCrypto;
    let mut transport = NoopSpaceAccessTransport;
    let proof = HmacProofAdapter::new();
    let mut timer = Timer::new();
    let mut store = NoopSpaceAccessPersistence;
    let mut executor = SpaceAccessExecutor {
        crypto: &crypto,
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

    let crypto = DeterministicSpaceAccessCrypto;
    let mut transport = NoopSpaceAccessTransport;
    let proof = HmacProofAdapter::new();
    let mut timer = Timer::new();
    let mut store = NoopSpaceAccessPersistence;
    let mut executor = SpaceAccessExecutor {
        crypto: &crypto,
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

    let crypto = DeterministicSpaceAccessCrypto;
    let mut transport = NoopSpaceAccessTransport;
    let proof = HmacProofAdapter::new();
    let mut timer = Timer::new();
    let mut store = NoopSpaceAccessPersistence;
    let mut executor = SpaceAccessExecutor {
        crypto: &crypto,
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

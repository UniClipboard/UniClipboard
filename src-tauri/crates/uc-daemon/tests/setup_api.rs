use std::sync::Arc;
use std::sync::{Mutex as StdMutex, OnceLock};

use anyhow::Result;
use async_trait::async_trait;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{sleep, Duration, Instant};
use tower::ServiceExt;
use uc_app::runtime::CoreRuntime;
use uc_app::testing::{
    NoopDiscoveryPort, NoopLifecycleEventEmitter, NoopLifecycleStatus, NoopNetworkControl,
    NoopPairingTransport, NoopProofPort, NoopSessionReadyEmitter, NoopSetupEventPort,
    NoopSpaceAccessPersistence, NoopSpaceAccessTransport, NoopTimerPort,
};
use uc_app::usecases::app_lifecycle::{AppLifecycleCoordinator, AppLifecycleCoordinatorDeps};
use uc_app::usecases::pairing::PairingDomainEvent;
use uc_app::usecases::setup::{MarkSetupComplete, SetupPairingFacadePort};
use uc_app::usecases::space_access::{SpaceAccessCryptoFactory, SpaceAccessOrchestrator};
use uc_app::usecases::{CoreUseCases, InitializeEncryption, SetupOrchestrator};
use uc_bootstrap::assembly::SetupAssemblyPorts;
use uc_bootstrap::build_cli_runtime;
use uc_bootstrap::{build_non_gui_runtime_with_setup, builders::build_daemon_app};
use uc_core::network::PairingRequest;
use uc_core::ports::space::CryptoPort;
use uc_core::ports::SetupStatusPort;
use uc_core::security::model::{
    EncryptedBlob, EncryptionAlgo, EncryptionFormatVersion, KdfAlgorithm, KdfParams, KdfParamsV1,
    KeyScope, KeySlot, KeySlotFile, KeySlotVersion, MasterKey,
};
use uc_core::setup::SetupStatus;
use uc_daemon::api::auth::load_or_create_auth_token;
use uc_daemon::api::query::DaemonQueryService;
use uc_daemon::api::server::{build_router, DaemonApiState};
use uc_daemon::api::types::DaemonWsEvent;
use uc_daemon::pairing::host::DaemonPairingHost;
use uc_daemon::security::SecurityState;
use uc_daemon::state::RuntimeState;

fn build_runtime() -> Arc<CoreRuntime> {
    static RUNTIME: OnceLock<Arc<CoreRuntime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| Arc::new(build_cli_runtime(None).expect("build cli runtime")))
        .clone()
}

async fn build_setup_router() -> (axum::Router, String) {
    static TEST_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
    let _guard = TEST_LOCK
        .get_or_init(|| StdMutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let runtime = build_runtime();
    let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
    let query_service = Arc::new(DaemonQueryService::new(runtime.clone(), state));
    let tempdir = tempfile::tempdir().unwrap();
    let token_path = tempdir.path().join("daemon.token");
    let token = load_or_create_auth_token(&token_path).unwrap();
    let token_value = std::fs::read_to_string(token_path).unwrap();
    let setup_orchestrator = build_setup_orchestrator(runtime);
    let pid = std::process::id();
    let security = Arc::new(SecurityState::new_with_pid(pid));
    let session_token = security.make_session_token_for_pid(pid);
    let api_state = DaemonApiState::new(query_service, token, None, security).with_setup(setup_orchestrator);
    (build_router(api_state), session_token)
}

fn with_profile_env<T>(
    profile: &str,
    xdg_runtime_dir: &std::path::Path,
    f: impl FnOnce() -> T,
) -> T {
    static ENV_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
    let _guard = ENV_LOCK
        .get_or_init(|| StdMutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let previous_profile = std::env::var("UC_PROFILE").ok();
    let previous_xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok();

    std::env::set_var("UC_PROFILE", profile);
    std::env::set_var("XDG_RUNTIME_DIR", xdg_runtime_dir);

    let result = f();

    match previous_profile {
        Some(value) => std::env::set_var("UC_PROFILE", value),
        None => std::env::remove_var("UC_PROFILE"),
    }
    match previous_xdg_runtime_dir {
        Some(value) => std::env::set_var("XDG_RUNTIME_DIR", value),
        None => std::env::remove_var("XDG_RUNTIME_DIR"),
    }

    result
}

fn build_reset_router() -> (axum::Router, String) {
    static RUNTIME_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
    let _guard = RUNTIME_LOCK
        .get_or_init(|| StdMutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let tempdir = tempfile::tempdir().expect("tempdir");
    let profile = format!(
        "setup-api-reset-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos()
    );

    with_profile_env(&profile, tempdir.path(), || {
        let ctx = build_daemon_app().expect("build daemon app");
        let setup_ports = SetupAssemblyPorts::from_network(
            ctx.pairing_orchestrator.clone(),
            ctx.space_access_orchestrator.clone(),
            ctx.deps.network_ports.peers.clone(),
            None,
            Arc::new(uc_app::usecases::LoggingLifecycleEventEmitter),
        );
        let runtime = Arc::new(
            build_non_gui_runtime_with_setup(
                ctx.deps,
                ctx.storage_paths.clone(),
                setup_ports,
            )
            .expect("build non-gui runtime with setup"),
        );
        let setup_orchestrator = runtime.setup_orchestrator().clone();
        let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
        let query_service = Arc::new(DaemonQueryService::new(runtime.clone(), state.clone()));
        let token_dir = tempfile::tempdir().expect("token tempdir");
        let token_path = token_dir.path().join("daemon.token");
        let token = load_or_create_auth_token(&token_path).expect("load auth token");
        let token_value = std::fs::read_to_string(&token_path).expect("read auth token");
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel::<DaemonWsEvent>(128);
        let pairing_host = Arc::new(DaemonPairingHost::new(
            runtime.clone(),
            ctx.pairing_orchestrator,
            ctx.pairing_action_rx,
            state,
            ctx.space_access_orchestrator,
            ctx.key_slot_store,
            event_tx,
        ));
        let pid = std::process::id();
        let security = Arc::new(SecurityState::new_with_pid(pid));
        let session_token = security.make_session_token_for_pid(pid);
        let api_state = DaemonApiState::new(query_service, token, Some(runtime), security)
            .with_setup(setup_orchestrator)
            .with_pairing_host(pairing_host);
        (build_router(api_state), session_token)
    })
}

async fn build_reset_router_async() -> (axum::Router, String) {
    tokio::task::spawn_blocking(build_reset_router)
        .await
        .expect("setup reset fixture join failed")
}

fn build_setup_orchestrator(runtime: Arc<CoreRuntime>) -> Arc<SetupOrchestrator> {
    build_setup_orchestrator_with_overrides(
        runtime,
        Arc::new(FakeSetupPairingFacade::default()),
        Arc::new(NoopSpaceAccessCryptoFactory),
    )
}

fn build_setup_orchestrator_with_overrides(
    runtime: Arc<CoreRuntime>,
    setup_pairing_facade: Arc<dyn SetupPairingFacadePort>,
    crypto_factory: Arc<dyn SpaceAccessCryptoFactory>,
) -> Arc<SetupOrchestrator> {
    let wiring = runtime.wiring_deps();
    let initialize_encryption = Arc::new(InitializeEncryption::from_ports(
        wiring.security.encryption.clone(),
        wiring.security.key_material.clone(),
        wiring.security.key_scope.clone(),
        wiring.security.encryption_state.clone(),
        wiring.security.encryption_session.clone(),
    ));
    let setup_status = Arc::new(InMemorySetupStatus::default());
    let mark_setup_complete = Arc::new(MarkSetupComplete::new(setup_status.clone()));

    Arc::new(SetupOrchestrator::new(
        initialize_encryption,
        mark_setup_complete,
        setup_status,
        build_mock_lifecycle(),
        setup_pairing_facade,
        Arc::new(NoopSetupEventPort),
        Arc::new(SpaceAccessOrchestrator::new()),
        Arc::new(NoopDiscoveryPort),
        Arc::new(NoopNetworkControl),
        crypto_factory,
        Arc::new(NoopPairingTransport),
        Arc::new(Mutex::new(NoopSpaceAccessTransport)),
        Arc::new(NoopProofPort),
        Arc::new(Mutex::new(NoopTimerPort)),
        Arc::new(Mutex::new(NoopSpaceAccessPersistence)),
    ))
}

fn build_mock_lifecycle() -> Arc<AppLifecycleCoordinator> {
    Arc::new(AppLifecycleCoordinator::from_deps(
        AppLifecycleCoordinatorDeps {
            network: Arc::new(uc_app::usecases::StartNetworkAfterUnlock::new(Arc::new(
                NoopNetworkControl,
            ))),
            announcer: None,
            emitter: Arc::new(NoopSessionReadyEmitter),
            status: Arc::new(NoopLifecycleStatus),
            lifecycle_emitter: Arc::new(NoopLifecycleEventEmitter),
        },
    ))
}

fn authed_request(
    method: &str,
    uri: &str,
    token: &str,
    body: Body,
    content_type: Option<&str>,
) -> Request<Body> {
    // token is a JWT session token (pre-obtained via SecurityState::make_session_token_for_pid)
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Session {}", token.trim()));
    if let Some(content_type) = content_type {
        builder = builder.header("Content-Type", content_type);
    }
    builder.body(body).unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn get_setup_state(app: &axum::Router, token: &str) -> Value {
    let response = app
        .clone()
        .oneshot(authed_request(
            "GET",
            "/setup/state",
            token,
            Body::empty(),
            None,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn reset_setup(app: &axum::Router, token: &str) -> axum::response::Response {
    app.clone()
        .oneshot(authed_request(
            "POST",
            "/setup/reset",
            token,
            Body::empty(),
            None,
        ))
        .await
        .unwrap()
}

fn assert_setup_state_metadata_shape(body: &Value) {
    assert!(body.get("sessionId").is_some());
    assert!(body.get("nextStepHint").is_some());
    assert!(body.get("clipboardMode").is_some());
    assert!(body.get("deviceName").is_some());
    assert!(body.get("peerId").is_some());
    assert!(body.get("session_id").is_none());
    assert!(body.get("next_step_hint").is_none());
    assert!(body.get("clipboard_mode").is_none());
    assert!(body.get("device_name").is_none());
    assert!(body.get("peer_id").is_none());

    let hint = body["nextStepHint"].as_str().expect("nextStepHint string");
    assert!(matches!(
        hint,
        "create-space-passphrase"
            | "join-select-peer"
            | "join-waiting-for-host"
            | "join-enter-passphrase"
            | "host-confirm-peer"
            | "completed"
            | "idle"
    ));
}

#[derive(Default)]
struct InMemorySetupStatus {
    status: Mutex<SetupStatus>,
}

#[async_trait]
impl SetupStatusPort for InMemorySetupStatus {
    async fn get_status(&self) -> Result<SetupStatus> {
        Ok(self.status.lock().await.clone())
    }

    async fn set_status(&self, status: &SetupStatus) -> Result<()> {
        *self.status.lock().await = status.clone();
        Ok(())
    }
}

#[derive(Default)]
struct FakeSetupPairingFacade;

#[async_trait]
impl SetupPairingFacadePort for FakeSetupPairingFacade {
    async fn subscribe(&self) -> Result<mpsc::Receiver<PairingDomainEvent>> {
        let (_tx, rx) = mpsc::channel(8);
        Ok(rx)
    }

    async fn initiate_pairing(&self, _peer_id: String) -> Result<String> {
        Ok("session-test".to_string())
    }

    async fn accept_pairing(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }

    async fn reject_pairing(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }

    async fn cancel_pairing(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }

    async fn verify_pairing(&self, _session_id: &str, _pin_matches: bool) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
struct RecordingSetupPairingFacade {
    session_id: String,
    tx: mpsc::Sender<PairingDomainEvent>,
    rx: Arc<Mutex<Option<mpsc::Receiver<PairingDomainEvent>>>>,
    accepted_sessions: Arc<StdMutex<Vec<String>>>,
}

impl RecordingSetupPairingFacade {
    fn new(session_id: &str) -> Self {
        let (tx, rx) = mpsc::channel(8);
        Self {
            session_id: session_id.to_string(),
            tx,
            rx: Arc::new(Mutex::new(Some(rx))),
            accepted_sessions: Arc::new(StdMutex::new(Vec::new())),
        }
    }

    async fn emit(&self, event: PairingDomainEvent) {
        self.tx
            .send(event)
            .await
            .expect("pairing event should send to setup subscriber");
    }

    fn accepted_sessions(&self) -> Vec<String> {
        self.accepted_sessions
            .lock()
            .expect("accepted sessions lock")
            .clone()
    }
}

#[async_trait]
impl SetupPairingFacadePort for RecordingSetupPairingFacade {
    async fn subscribe(&self) -> Result<mpsc::Receiver<PairingDomainEvent>> {
        self.rx
            .lock()
            .await
            .take()
            .ok_or_else(|| anyhow::anyhow!("pairing event receiver already taken"))
    }

    async fn initiate_pairing(&self, _peer_id: String) -> Result<String> {
        Ok(self.session_id.clone())
    }

    async fn accept_pairing(&self, session_id: &str) -> Result<()> {
        self.accepted_sessions
            .lock()
            .expect("accepted sessions lock")
            .push(session_id.to_string());
        Ok(())
    }

    async fn reject_pairing(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }

    async fn cancel_pairing(&self, _session_id: &str) -> Result<()> {
        Ok(())
    }

    async fn verify_pairing(&self, _session_id: &str, _pin_matches: bool) -> Result<()> {
        Ok(())
    }
}

struct WorkingSpaceAccessCryptoFactory;

impl SpaceAccessCryptoFactory for WorkingSpaceAccessCryptoFactory {
    fn build(&self, _passphrase: uc_core::security::SecretString) -> Box<dyn CryptoPort> {
        Box::new(WorkingSpaceAccessCrypto)
    }
}

struct WorkingSpaceAccessCrypto;

#[async_trait]
impl CryptoPort for WorkingSpaceAccessCrypto {
    async fn generate_nonce32(&self) -> [u8; 32] {
        [7u8; 32]
    }

    async fn export_keyslot_blob(&self, _space_id: &uc_core::ids::SpaceId) -> Result<KeySlot> {
        Err(anyhow::anyhow!(
            "working test crypto does not export keyslots"
        ))
    }

    async fn derive_master_key_from_keyslot(
        &self,
        _keyslot_blob: &[u8],
        _passphrase: uc_core::security::SecretString,
    ) -> Result<MasterKey> {
        MasterKey::from_bytes(&[5u8; 32]).map_err(anyhow::Error::from)
    }
}

struct JoinSetupFixture {
    app: axum::Router,
    token: String,
    facade: Arc<RecordingSetupPairingFacade>,
}

struct HostSetupFixture {
    app: axum::Router,
    token: String,
    runtime: Arc<CoreRuntime>,
    pairing_host: Arc<DaemonPairingHost>,
    state: Arc<RwLock<RuntimeState>>,
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

fn inbound_request(session_id: &str, local_peer_id: &str) -> PairingRequest {
    PairingRequest {
        session_id: session_id.to_string(),
        device_name: "Remote Device".to_string(),
        device_id: "remote-device-id".to_string(),
        peer_id: local_peer_id.to_string(),
        identity_pubkey: vec![1, 2, 3],
        nonce: vec![7; 32],
    }
}

fn build_join_setup_fixture() -> JoinSetupFixture {
    static TEST_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
    let _guard = TEST_LOCK
        .get_or_init(|| StdMutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let runtime = build_runtime();
    let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
    let query_service = Arc::new(DaemonQueryService::new(runtime.clone(), state));
    let tempdir = tempfile::tempdir().expect("tempdir");
    let token_path = tempdir.path().join("daemon.token");
    let token = load_or_create_auth_token(&token_path).expect("load auth token");
    let token_value = std::fs::read_to_string(token_path).expect("read auth token");
    let facade = Arc::new(RecordingSetupPairingFacade::new("session-test"));
    let setup_orchestrator = build_setup_orchestrator_with_overrides(
        runtime,
        facade.clone(),
        Arc::new(WorkingSpaceAccessCryptoFactory),
    );
    let pid = std::process::id();
    let security = Arc::new(SecurityState::new_with_pid(pid));
    let session_token = security.make_session_token_for_pid(pid);
    let api_state = DaemonApiState::new(query_service, token, None, security).with_setup(setup_orchestrator);

    JoinSetupFixture {
        app: build_router(api_state),
        token: session_token,
        facade,
    }
}

async fn build_join_setup_fixture_async() -> JoinSetupFixture {
    tokio::task::spawn_blocking(build_join_setup_fixture)
        .await
        .expect("join setup fixture join failed")
}

fn build_host_setup_fixture() -> HostSetupFixture {
    static RUNTIME_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
    let _guard = RUNTIME_LOCK
        .get_or_init(|| StdMutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let tempdir = tempfile::tempdir().expect("tempdir");
    let profile = format!(
        "setup-api-host-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos()
    );

    with_profile_env(&profile, tempdir.path(), || {
        let ctx = build_daemon_app().expect("build daemon app");
        let setup_ports = SetupAssemblyPorts::from_network(
            ctx.pairing_orchestrator.clone(),
            ctx.space_access_orchestrator.clone(),
            ctx.deps.network_ports.peers.clone(),
            None,
            Arc::new(uc_app::usecases::LoggingLifecycleEventEmitter),
        );
        let runtime = Arc::new(
            build_non_gui_runtime_with_setup(
                ctx.deps,
                ctx.storage_paths.clone(),
                setup_ports,
            )
            .expect("build non-gui runtime with setup"),
        );
        let setup_orchestrator = runtime.setup_orchestrator().clone();
        let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
        let query_service = Arc::new(DaemonQueryService::new(runtime.clone(), state.clone()));
        let token_dir = tempfile::tempdir().expect("token tempdir");
        let token_path = token_dir.path().join("daemon.token");
        let token = load_or_create_auth_token(&token_path).expect("load auth token");
        let token_value = std::fs::read_to_string(&token_path).expect("read auth token");
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel::<DaemonWsEvent>(128);
        let pairing_host = Arc::new(DaemonPairingHost::new(
            runtime.clone(),
            ctx.pairing_orchestrator,
            ctx.pairing_action_rx,
            state.clone(),
            ctx.space_access_orchestrator,
            ctx.key_slot_store,
            event_tx,
        ));
        let pid = std::process::id();
        let security = Arc::new(SecurityState::new_with_pid(pid));
        let session_token = security.make_session_token_for_pid(pid);
        let api_state = DaemonApiState::new(query_service, token, Some(runtime.clone()), security)
            .with_setup(setup_orchestrator)
            .with_pairing_host(pairing_host.clone());

        HostSetupFixture {
            app: build_router(api_state),
            token: session_token,
            runtime,
            pairing_host,
            state,
        }
    })
}

async fn build_host_setup_fixture_async() -> HostSetupFixture {
    tokio::task::spawn_blocking(build_host_setup_fixture)
        .await
        .expect("host setup fixture join failed")
}

struct NoopSpaceAccessCryptoFactory;

impl SpaceAccessCryptoFactory for NoopSpaceAccessCryptoFactory {
    fn build(&self, _passphrase: uc_core::security::SecretString) -> Box<dyn CryptoPort> {
        Box::new(NoopSpaceAccessCrypto)
    }
}

struct NoopSpaceAccessCrypto;

#[async_trait]
impl CryptoPort for NoopSpaceAccessCrypto {
    async fn generate_nonce32(&self) -> [u8; 32] {
        [0u8; 32]
    }

    async fn export_keyslot_blob(&self, _space_id: &uc_core::ids::SpaceId) -> Result<KeySlot> {
        Err(anyhow::anyhow!("noop export_keyslot_blob"))
    }

    async fn derive_master_key_from_keyslot(
        &self,
        _keyslot_blob: &[u8],
        _passphrase: uc_core::security::SecretString,
    ) -> Result<MasterKey> {
        Err(anyhow::anyhow!("noop derive_master_key_from_keyslot"))
    }
}

async fn wait_for_setup_state(
    app: &axum::Router,
    token: &str,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let state = get_setup_state(app, token).await;
        if predicate(&state["state"]) {
            return state;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for expected setup state, last state: {state}"
        );
        sleep(Duration::from_millis(10)).await;
    }
}

async fn wait_for_setup_response(
    app: &axum::Router,
    token: &str,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let response = get_setup_state(app, token).await;
        if predicate(&response) {
            return response;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for expected setup response, last response: {response}"
        );
        sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn setup_state_requires_authentication() {
    let (app, _) = build_setup_router().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/setup/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn setup_host_route_starts_new_space_and_returns_setup_state() {
    let (app, token) = build_setup_router().await;

    let response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/host",
            &token,
            Body::empty(),
            None,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(body["state"]["CreateSpaceInputPassphrase"]["error"].is_null());
    assert_eq!(body["nextStepHint"], "create-space-passphrase");
    assert!(body.get("sessionId").is_some());
    assert!(body.get("nextStepHint").is_some());
    assert!(body.get("session_id").is_none());
    assert!(body.get("next_step_hint").is_none());
}

#[tokio::test]
async fn setup_join_route_returns_join_select_device_state() {
    let (app, token) = build_setup_router().await;

    let response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/join",
            &token,
            Body::empty(),
            None,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(body["state"]["JoinSpaceSelectDevice"]["error"].is_null());
    assert_eq!(body["nextStepHint"], "join-select-peer");

    let state = get_setup_state(&app, &token).await;
    assert!(state["state"]["JoinSpaceSelectDevice"]["error"].is_null());
    assert_setup_state_metadata_shape(&state);
}

#[tokio::test]
async fn setup_select_peer_route_returns_processing_join_state() {
    let (app, token) = build_setup_router().await;

    let join_response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/join",
            &token,
            Body::empty(),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(join_response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/select-peer",
            &token,
            Body::from(json!({ "peerId": "peer-remote" }).to_string()),
            Some("application/json"),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(body["state"]["ProcessingJoinSpace"]["message"].is_string());
    assert_eq!(body["nextStepHint"], "join-waiting-for-host");
}

#[tokio::test]
async fn setup_confirm_peer_route_rejects_when_no_pending_confirmation_exists() {
    let (app, token) = build_setup_router().await;

    let response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/confirm-peer",
            &token,
            Body::empty(),
            None,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = json_body(response).await;
    assert_eq!(body["code"], "invalid_setup_transition");
}

#[tokio::test]
async fn setup_submit_passphrase_route_rejects_malformed_payload() {
    let (app, token) = build_setup_router().await;

    let response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/submit-passphrase",
            &token,
            Body::from(json!({ "pass": "secret" }).to_string()),
            Some("application/json"),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(response).await;
    assert_eq!(body["code"], "bad_request");
}

#[tokio::test]
async fn setup_confirm_peer_routes_host_confirmation_through_daemon_pairing_host() {
    let fixture = build_host_setup_fixture_async().await;

    let host_response = fixture
        .app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/host",
            &fixture.token,
            Body::empty(),
            None,
        ))
        .await
        .expect("setup host request should succeed");
    assert_eq!(host_response.status(), StatusCode::OK);

    let passphrase_response = fixture
        .app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/submit-passphrase",
            &fixture.token,
            Body::from(json!({ "passphrase": "secret-passphrase" }).to_string()),
            Some("application/json"),
        ))
        .await
        .expect("setup submit passphrase should succeed for host flow");
    assert_eq!(passphrase_response.status(), StatusCode::OK);

    let local_peer_id = CoreUseCases::new(fixture.runtime.as_ref())
        .get_local_device_info()
        .execute()
        .await
        .expect("local device info should load")
        .peer_id;
    fixture
        .pairing_host
        .set_discoverability("gui".to_string(), true, Some(60_000))
        .await;
    fixture
        .pairing_host
        .set_participant_ready("gui".to_string(), true, Some(60_000))
        .await;
    fixture
        .pairing_host
        .handle_incoming_request(
            "peer-remote".to_string(),
            inbound_request("session-host-confirm", &local_peer_id),
        )
        .await
        .expect("daemon pairing host should accept inbound request fixture");

    let pending_state = wait_for_setup_response(&fixture.app, &fixture.token, |response| {
        response["nextStepHint"] == Value::String("host-confirm-peer".to_string())
            && response["sessionId"] == Value::String("session-host-confirm".to_string())
    })
    .await;
    assert_eq!(pending_state["nextStepHint"], "host-confirm-peer");

    let confirm_response = fixture
        .app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/confirm-peer",
            &fixture.token,
            Body::empty(),
            None,
        ))
        .await
        .expect("setup confirm peer request should succeed");

    assert_eq!(confirm_response.status(), StatusCode::OK);
    let body = json_body(confirm_response).await;
    assert_eq!(body["nextStepHint"], "completed");

    let guard = fixture.state.read().await;
    let snapshot = guard
        .pairing_session("session-host-confirm")
        .expect("pairing snapshot should remain available");
    assert_eq!(snapshot.state, "verifying");
}

#[tokio::test]
async fn setup_submit_passphrase_routes_join_passphrase_through_verify_passphrase() {
    let fixture = build_join_setup_fixture_async().await;

    let join_response = fixture
        .app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/join",
            &fixture.token,
            Body::empty(),
            None,
        ))
        .await
        .expect("setup join request should succeed");
    assert_eq!(join_response.status(), StatusCode::OK);

    let select_response = fixture
        .app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/select-peer",
            &fixture.token,
            Body::from(json!({ "peerId": "peer-remote" }).to_string()),
            Some("application/json"),
        ))
        .await
        .expect("setup select peer request should succeed");
    assert_eq!(select_response.status(), StatusCode::OK);

    fixture
        .facade
        .emit(PairingDomainEvent::PairingVerificationRequired {
            session_id: "session-test".to_string(),
            peer_id: "peer-remote".to_string(),
            short_code: "123456".to_string(),
            local_fingerprint: "local-fingerprint".to_string(),
            peer_fingerprint: "peer-fingerprint".to_string(),
        })
        .await;

    wait_for_setup_state(&fixture.app, &fixture.token, |state| {
        state.get("JoinSpaceConfirmPeer").is_some()
    })
    .await;

    let confirm_response = fixture
        .app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/confirm-peer",
            &fixture.token,
            Body::empty(),
            None,
        ))
        .await
        .expect("join confirm peer should succeed");
    assert_eq!(confirm_response.status(), StatusCode::OK);
    let confirm_body = json_body(confirm_response).await;
    assert!(confirm_body["state"]["JoinSpaceInputPassphrase"]["error"].is_null());
    assert_eq!(
        fixture.facade.accepted_sessions(),
        vec!["session-test".to_string()]
    );

    fixture
        .facade
        .emit(PairingDomainEvent::KeyslotReceived {
            session_id: "session-test".to_string(),
            peer_id: "peer-remote".to_string(),
            keyslot_file: sample_keyslot_file("join-space"),
            challenge: vec![3; 32],
        })
        .await;

    let submit_response = fixture
        .app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/submit-passphrase",
            &fixture.token,
            Body::from(json!({ "passphrase": "secret-passphrase" }).to_string()),
            Some("application/json"),
        ))
        .await
        .expect("join submit passphrase should succeed");

    assert_eq!(submit_response.status(), StatusCode::OK);
    let submit_body = json_body(submit_response).await;
    assert!(submit_body["state"]["ProcessingJoinSpace"]["message"].is_string());
    assert_eq!(submit_body["nextStepHint"], "join-waiting-for-host");
}

#[tokio::test]
async fn setup_cancel_route_returns_idle_or_select_state_without_500() {
    let (app, token) = build_setup_router().await;

    let join_response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/join",
            &token,
            Body::empty(),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(join_response.status(), StatusCode::OK);

    let select_response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/select-peer",
            &token,
            Body::from(json!({ "peerId": "peer-remote" }).to_string()),
            Some("application/json"),
        ))
        .await
        .unwrap();
    assert_eq!(select_response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/cancel",
            &token,
            Body::empty(),
            None,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(
        body["state"]["JoinSpaceSelectDevice"].is_object()
            || body["state"] == Value::String("Welcome".to_string())
    );
    assert!(
        body["nextStepHint"] == Value::String("join-select-peer".to_string())
            || body["nextStepHint"] == Value::String("idle".to_string())
    );
}

#[tokio::test]
async fn setup_reset_clears_active_setup_state() {
    let (app, token) = build_reset_router_async().await;

    let host_response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/host",
            &token,
            Body::empty(),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(host_response.status(), StatusCode::OK);

    let before_reset = get_setup_state(&app, &token).await;
    assert_eq!(before_reset["nextStepHint"], "create-space-passphrase");

    let reset_response = reset_setup(&app, &token).await;
    assert_eq!(reset_response.status(), StatusCode::OK);
    let reset_body = json_body(reset_response).await;
    assert!(reset_body["profile"].as_str().is_some());
    assert_eq!(reset_body["daemonKeptRunning"], Value::Bool(true));

    let after_reset = get_setup_state(&app, &token).await;
    assert_eq!(after_reset["state"], Value::String("Welcome".to_string()));
    assert_eq!(after_reset["nextStepHint"], "idle");
}

#[tokio::test]
async fn setup_reset_releases_pairing_host_leases() {
    let (app, token) = build_reset_router_async().await;

    let discoverability_response = app
        .clone()
        .oneshot(authed_request(
            "PUT",
            "/pairing/discoverability/current",
            &token,
            Body::from(
                json!({
                    "clientKind": "setup-cli",
                    "discoverable": true,
                    "leaseTtlMs": 60_000
                })
                .to_string(),
            ),
            Some("application/json"),
        ))
        .await
        .unwrap();
    assert_eq!(discoverability_response.status(), StatusCode::ACCEPTED);

    let participant_response = app
        .clone()
        .oneshot(authed_request(
            "PUT",
            "/pairing/participants/current",
            &token,
            Body::from(
                json!({
                    "clientKind": "setup-cli",
                    "ready": true,
                    "leaseTtlMs": 60_000
                })
                .to_string(),
            ),
            Some("application/json"),
        ))
        .await
        .unwrap();
    assert_eq!(participant_response.status(), StatusCode::ACCEPTED);

    let reset_response = reset_setup(&app, &token).await;
    assert_eq!(reset_response.status(), StatusCode::OK);

    let initiate_response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/pairing/sessions",
            &token,
            Body::from(json!({ "peerId": "peer-after-reset" }).to_string()),
            Some("application/json"),
        ))
        .await
        .unwrap();
    assert_eq!(initiate_response.status(), StatusCode::BAD_REQUEST);
    let body = json_body(initiate_response).await;
    assert!(
        body["error"] == Value::String("host_not_discoverable".to_string())
            || body["code"] == Value::String("host_not_discoverable".to_string())
    );
}

#[tokio::test]
async fn setup_reset_allows_second_host_start_without_manual_cleanup() {
    let (app, token) = build_reset_router_async().await;

    let first_host = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/host",
            &token,
            Body::empty(),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(first_host.status(), StatusCode::OK);

    let first_passphrase = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/submit-passphrase",
            &token,
            Body::from(json!({ "passphrase": "secret-passphrase" }).to_string()),
            Some("application/json"),
        ))
        .await
        .unwrap();
    assert_eq!(first_passphrase.status(), StatusCode::OK);

    let reset_response = reset_setup(&app, &token).await;
    assert_eq!(reset_response.status(), StatusCode::OK);

    let second_host = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/setup/host",
            &token,
            Body::empty(),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(second_host.status(), StatusCode::OK);
    let second_body = json_body(second_host).await;
    assert_eq!(second_body["nextStepHint"], "create-space-passphrase");
}

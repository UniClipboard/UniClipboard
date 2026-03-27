//! # DaemonApp
//!
//! Top-level daemon lifecycle: binds the RPC socket, starts services,
//! waits for shutdown signal, and tears down in reverse order.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::net::UnixListener;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use uc_app::runtime::CoreRuntime;
use uc_app::usecases::space_access::SpaceAccessOrchestrator;
use uc_app::usecases::{CoreUseCases, SessionReadyEmitter};

use crate::api::auth::{load_or_create_auth_token, resolve_daemon_token_path};
use crate::api::event_emitter::DaemonApiEventEmitter;
use crate::api::query::DaemonQueryService;
use crate::api::server::{run_http_server, DaemonApiState};
use crate::api::types::DaemonWsEvent;
use crate::pairing::host::DaemonPairingHost;
use crate::process_metadata::{remove_pid_file, write_current_pid};
use crate::rpc::server::{check_or_remove_stale_socket, run_rpc_accept_loop};
use crate::service::DaemonService;
use crate::state::RuntimeState;

/// Recover encryption session from disk/keyring if encryption has been initialized.
///
/// Returns Ok(true) when encryption is Initialized and the session was successfully unlocked.
/// Returns Ok(false) when encryption is Uninitialized (first run — no recovery needed).
/// Returns Err if encryption is initialized but recovery fails (daemon must not start).
///
/// This function is `pub` so `main.rs` can call it BEFORE constructing `DaemonApp`,
/// using the result to decide whether to start `PeerDiscoveryWorker` immediately or defer.
pub async fn recover_encryption_session(runtime: &CoreRuntime) -> anyhow::Result<bool> {
    let usecases = CoreUseCases::new(runtime);
    let uc = usecases.auto_unlock_encryption_session();
    match uc.execute().await {
        Ok(true) => {
            info!("Encryption session recovered from disk");
            Ok(true)
        }
        Ok(false) => {
            info!("Encryption not initialized, skipping session recovery");
            Ok(false)
        }
        Err(e) => {
            error!(error = %e, "Encryption session recovery failed");
            anyhow::bail!(
                "Cannot start daemon: encryption session recovery failed: {}",
                e
            )
        }
    }
}

/// Fires a oneshot signal when the setup flow completes (per D-09/D-10).
///
/// Used as the daemon's `SessionReadyEmitter` so that when
/// `AppLifecycleCoordinator::ensure_ready()` fires `emit_ready()`,
/// the daemon can dynamically start `PeerDiscoveryWorker`.
pub struct SetupCompletionEmitter {
    tx: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl SetupCompletionEmitter {
    pub fn new(tx: tokio::sync::oneshot::Sender<()>) -> Self {
        Self {
            tx: tokio::sync::Mutex::new(Some(tx)),
        }
    }
}

#[async_trait]
impl SessionReadyEmitter for SetupCompletionEmitter {
    async fn emit_ready(&self) -> anyhow::Result<()> {
        if let Some(tx) = self.tx.lock().await.take() {
            if tx.send(()).is_err() {
                debug!("setup completion signal: receiver already dropped (worker may have started via other path)");
            }
        } else {
            debug!("setup completion signal already consumed (duplicate emit_ready call)");
        }
        Ok(())
    }
}

/// Main daemon application.
///
/// Owns the service list, RPC state, and cancellation token.
/// Services use `Arc<dyn DaemonService>` (not `Box`) to allow cloning
/// for `tokio::spawn` `'static` requirement.
///
/// The `api_pairing_host` field retains typed access to the pairing host for
/// HTTP routes (PH56-04), while the pairing host is also in the `services` vec
/// for uniform lifecycle management.
pub struct DaemonApp {
    services: Vec<Arc<dyn DaemonService>>,
    runtime: Arc<CoreRuntime>,
    state: Arc<RwLock<RuntimeState>>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    api_pairing_host: Option<Arc<DaemonPairingHost>>,
    space_access_orchestrator: Option<Arc<SpaceAccessOrchestrator>>,
    socket_path: PathBuf,
    cancel: CancellationToken,
    // Phase 67: deferred PeerDiscoveryWorker for uninitialized devices
    deferred_peer_discovery: Option<Arc<dyn DaemonService>>,
    setup_complete_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    /// External shutdown signal (e.g., from stdin pipe tether when GUI-managed).
    /// When cancelled, the daemon performs a graceful shutdown identical to SIGTERM.
    external_shutdown: Option<CancellationToken>,
}

impl DaemonApp {
    /// Create a new DaemonApp with the given services and socket path.
    ///
    /// The `state` is created by the caller (main.rs) so it can be shared
    /// with `DaemonPairingHost` before DaemonApp is constructed.
    ///
    /// The `event_tx` is created by the caller and shared with all services
    /// that emit WebSocket events, so they all write to the same broadcast channel.
    pub fn new(
        services: Vec<Arc<dyn DaemonService>>,
        runtime: Arc<CoreRuntime>,
        state: Arc<RwLock<RuntimeState>>,
        event_tx: broadcast::Sender<DaemonWsEvent>,
        api_pairing_host: Option<Arc<DaemonPairingHost>>,
        space_access_orchestrator: Option<Arc<SpaceAccessOrchestrator>>,
        socket_path: PathBuf,
    ) -> Self {
        Self {
            services,
            runtime,
            state,
            event_tx,
            api_pairing_host,
            space_access_orchestrator,
            socket_path,
            cancel: CancellationToken::new(),
            deferred_peer_discovery: None,
            setup_complete_rx: None,
            external_shutdown: None,
        }
    }

    /// Construct a DaemonApp with deferred PeerDiscoveryWorker support (Phase 67).
    ///
    /// `encryption_unlocked` is a required parameter to enforce the invariant that
    /// the caller MUST have completed encryption recovery before constructing DaemonApp.
    /// This prevents future callers from accidentally skipping the recovery check.
    pub fn new_with_deferred(
        services: Vec<Arc<dyn DaemonService>>,
        runtime: Arc<CoreRuntime>,
        state: Arc<RwLock<RuntimeState>>,
        event_tx: broadcast::Sender<DaemonWsEvent>,
        api_pairing_host: Option<Arc<DaemonPairingHost>>,
        space_access_orchestrator: Option<Arc<SpaceAccessOrchestrator>>,
        socket_path: PathBuf,
        encryption_unlocked: bool,
        deferred_peer_discovery: Option<Arc<dyn DaemonService>>,
        setup_complete_rx: Option<tokio::sync::oneshot::Receiver<()>>,
        external_shutdown: Option<CancellationToken>,
    ) -> Self {
        // Validate invariant: deferred_peer_discovery and setup_complete_rx must both be
        // Some (uninitialized) or both None (initialized+unlocked). Half-configured state
        // would silently prevent the worker from ever starting.
        debug_assert_eq!(
            deferred_peer_discovery.is_some(),
            setup_complete_rx.is_some(),
            "deferred_peer_discovery and setup_complete_rx must both be Some or both None"
        );
        debug_assert!(
            encryption_unlocked || deferred_peer_discovery.is_some(),
            "If encryption is not unlocked, deferred_peer_discovery must be provided"
        );
        Self {
            services,
            runtime,
            state,
            event_tx,
            api_pairing_host,
            space_access_orchestrator,
            socket_path,
            cancel: CancellationToken::new(),
            deferred_peer_discovery,
            setup_complete_rx,
            external_shutdown,
        }
    }

    /// Run the daemon: bind RPC socket, start services, wait for shutdown, cleanup.
    ///
    /// NOTE: `recover_encryption_session` is called in `main.rs` BEFORE constructing
    /// `DaemonApp`, so it does NOT appear here (Phase 67: moved for deferred-start logic).
    pub async fn run(mut self) -> anyhow::Result<()> {
        info!("uniclipboard-daemon starting");

        // 1. Bind RPC socket FIRST (fail-fast before starting services)
        check_or_remove_stale_socket(&self.socket_path).await?;
        let listener = UnixListener::bind(&self.socket_path)?;
        let token_base_dir = self
            .socket_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("/tmp"));
        let token_path = resolve_daemon_token_path(token_base_dir);
        let auth_token = load_or_create_auth_token(&token_path)?;
        let _pid_file_guard = DaemonPidFileGuard::activate()?;
        let query_service = Arc::new(DaemonQueryService::new(
            self.runtime.clone(),
            self.state.clone(),
        ));

        // 2. Build API state using the shared event_tx (same channel used by all services)
        let mut api_state =
            DaemonApiState::new(query_service, auth_token, Some(self.runtime.clone()));
        // Replace the default-created channel with our shared one so all services
        // emit to the same broadcast channel that WebSocket subscribers receive from.
        api_state.event_tx = self.event_tx.clone();
        let api_state = api_state.with_setup(self.runtime.setup_orchestrator().clone());
        let api_state = match &self.space_access_orchestrator {
            Some(sao) => api_state.with_space_access(sao.clone()),
            None => api_state,
        };
        let api_state = match &self.api_pairing_host {
            Some(ph) => api_state.with_pairing_host(Arc::clone(ph)),
            None => api_state,
        };

        // 3. Wire the event emitter into the runtime so use cases can emit WS events
        self.runtime
            .set_event_emitter(Arc::new(DaemonApiEventEmitter::new(self.event_tx.clone())));

        info!("uniclipboard-daemon running, RPC at {:?}", self.socket_path);

        // 4. Start ALL services uniformly via JoinSet
        let mut service_tasks = JoinSet::new();
        for service in &self.services {
            let svc = Arc::clone(service);
            let token = self.cancel.child_token();
            service_tasks.spawn(async move { svc.start(token).await });
        }

        // 5. Spawn RPC accept loop and HTTP server as infrastructure tasks
        let rpc_state = self.state.clone();
        let rpc_cancel = self.cancel.child_token();
        let mut rpc_handle = tokio::spawn(run_rpc_accept_loop(listener, rpc_state, rpc_cancel));
        let http_cancel = self.cancel.child_token();
        let mut http_handle = tokio::spawn(run_http_server(api_state, http_cancel));

        // Phase 67: prepare deferred PeerDiscoveryWorker start
        let mut deferred_worker = self.deferred_peer_discovery.take();
        let mut setup_rx = self.setup_complete_rx.take();

        // 6. Wait for shutdown signal, infrastructure crash, service crash, or deferred start
        loop {
            tokio::select! {
                _ = wait_for_shutdown_signal() => {
                    info!("shutdown signal received");
                    break;
                }
                _ = async {
                    match &self.external_shutdown {
                        Some(token) => token.cancelled().await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    info!("external shutdown signal received (parent process gone)");
                    break;
                }
                result = &mut rpc_handle => {
                    warn!("RPC accept loop exited unexpectedly: {:?}", result);
                    break;
                }
                result = &mut http_handle => {
                    warn!("HTTP server exited unexpectedly: {:?}", result);
                    break;
                }
                Some(result) = service_tasks.join_next() => {
                    warn!("service task exited unexpectedly: {:?}", result);
                    break;
                }
                result = async {
                    match &mut setup_rx {
                        Some(rx) => rx.await.map_err(|_| ()),
                        None => std::future::pending::<Result<(), ()>>().await,
                    }
                }, if deferred_worker.is_some() => {
                    match result {
                        Ok(()) => {
                            if let Some(worker) = deferred_worker.take() {
                                info!("setup complete — starting deferred peer discovery worker");
                                let worker_for_shutdown = Arc::clone(&worker);
                                let token = self.cancel.child_token();
                                service_tasks.spawn(async move { worker.start(token).await });
                                // Register for managed shutdown so stop() is called
                                self.services.push(worker_for_shutdown);
                                // Update health status from Stopped → Healthy (per Phase 67 D-11)
                                {
                                    let mut state = self.state.write().await;
                                    state.update_service_health("peer-discovery", crate::service::ServiceHealth::Healthy);
                                }
                            }
                        }
                        Err(()) => {
                            warn!("setup completion channel dropped — deferred peer discovery will NOT start");
                            deferred_worker = None; // disarm: no point retrying
                        }
                    }
                    setup_rx = None;
                    // continue loop — don't break, daemon keeps running
                }
            }
        }

        // 7. Shutdown sequence
        info!("shutting down...");
        self.cancel.cancel();

        // Drain service tasks with timeout
        tokio::time::timeout(Duration::from_secs(5), async {
            while service_tasks.join_next().await.is_some() {}
        })
        .await
        .ok();

        // Await RPC and HTTP with timeout
        tokio::time::timeout(Duration::from_secs(5), rpc_handle)
            .await
            .ok();
        tokio::time::timeout(Duration::from_secs(5), http_handle)
            .await
            .ok();

        // Stop services in reverse order
        for service in self.services.iter().rev() {
            info!(service = service.name(), "stopping service");
            if let Err(e) = service.stop().await {
                warn!(service = service.name(), "error stopping service: {}", e);
            }
        }

        // Remove socket file
        if let Err(e) = std::fs::remove_file(&self.socket_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!("failed to remove socket file: {}", e);
            }
        }

        info!("uniclipboard-daemon stopped");
        Ok(())
    }
}

struct DaemonPidFileGuard;

impl DaemonPidFileGuard {
    fn activate() -> anyhow::Result<Self> {
        let pid = write_current_pid()?;
        info!(pid, "wrote daemon pid metadata");
        Ok(Self)
    }
}

impl Drop for DaemonPidFileGuard {
    fn drop(&mut self) {
        if let Err(error) = remove_pid_file() {
            warn!(error = %error, "failed to remove daemon pid metadata");
        }
    }
}

/// Wait for either Ctrl-C or SIGTERM (Unix).
async fn wait_for_shutdown_signal() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate())
            .map_err(|e| anyhow::anyhow!("failed to register SIGTERM handler: {}", e))?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.map_err(|e| anyhow::anyhow!("ctrl_c handler error: {}", e))?;
            }
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .map_err(|e| anyhow::anyhow!("ctrl_c handler error: {}", e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_metadata::{read_pid_file, resolve_daemon_pid_path};
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};

    fn with_daemon_env<T>(
        profile: Option<&str>,
        xdg_runtime_dir: Option<&Path>,
        f: impl FnOnce() -> T,
    ) -> T {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let _guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_profile = std::env::var("UC_PROFILE").ok();
        let previous_xdg_runtime_dir = std::env::var("XDG_RUNTIME_DIR").ok();

        match profile {
            Some(profile) => std::env::set_var("UC_PROFILE", profile),
            None => std::env::remove_var("UC_PROFILE"),
        }
        match xdg_runtime_dir {
            Some(path) => std::env::set_var("XDG_RUNTIME_DIR", path),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }

        let result = f();

        match previous_profile {
            Some(profile) => std::env::set_var("UC_PROFILE", profile),
            None => std::env::remove_var("UC_PROFILE"),
        }
        match previous_xdg_runtime_dir {
            Some(path) => std::env::set_var("XDG_RUNTIME_DIR", path),
            None => std::env::remove_var("XDG_RUNTIME_DIR"),
        }

        result
    }

    /// Verifies that recover_encryption_session is pub and calls the correct use case.
    ///
    /// NOTE: Phase 67 moved the recovery call from run() to main.rs so that the result
    /// can decide whether PeerDiscoveryWorker starts immediately or is deferred.
    #[test]
    fn recover_encryption_session_calls_auto_unlock_use_case() {
        // NOTE: We split at #[cfg(test)] to exclude this test module from the search,
        // preventing the test from being self-fulfilling (Codex R1-F1).
        let full_source = include_str!("app.rs");
        let prod_source = full_source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(full_source);

        assert!(
            prod_source.contains("pub async fn recover_encryption_session"),
            "recover_encryption_session must be pub for main.rs to call it"
        );
        assert!(
            prod_source.contains("auto_unlock_encryption_session"),
            "recover_encryption_session must call auto_unlock_encryption_session use case"
        );
        assert!(
            prod_source.contains(".execute().await"),
            "Recovery must actually call .execute().await on the use case"
        );
    }

    /// Verifies that main.rs calls recover_encryption_session before DaemonApp construction
    /// and passes encryption_unlocked as a required parameter.
    #[test]
    fn main_calls_recovery_before_daemon_construction() {
        let main_source = include_str!("main.rs");
        let recovery_pos = main_source
            .find("recover_encryption_session")
            .expect("main.rs must call recover_encryption_session");
        let daemon_new_pos = main_source
            .find("new_with_deferred")
            .expect("main.rs must call DaemonApp::new_with_deferred");
        assert!(
            recovery_pos < daemon_new_pos,
            "recover_encryption_session must be called BEFORE DaemonApp::new_with_deferred"
        );
        assert!(
            main_source.contains("encryption_unlocked"),
            "main.rs must pass encryption_unlocked to DaemonApp (type-level invariant)"
        );
    }

    // ---------------------------------------------------------------------------
    // Behavioral tests for recover_encryption_session() (Task 3 — Strategy B)
    //
    // These tests exercise the recover_encryption_session() helper's three
    // match arms by calling AutoUnlockEncryptionSession::from_ports() directly
    // with mock ports, replicating exactly what CoreUseCases::new(runtime)
    // .auto_unlock_encryption_session() does internally.
    //
    // This approach (Strategy B) avoids the complexity of constructing a full
    // CoreRuntime while still testing each code path of the helper function.
    // ---------------------------------------------------------------------------

    use async_trait::async_trait;
    use std::sync::Arc;
    use uc_app::usecases::AutoUnlockEncryptionSession;
    use uc_core::ports::security::key_scope::ScopeError;
    use uc_core::{
        ports::{
            security::{encryption_state::EncryptionStatePort, key_scope::KeyScopePort},
            EncryptionPort, EncryptionSessionPort, KeyMaterialPort,
        },
        security::{
            model::{
                EncryptedBlob, EncryptionAlgo, EncryptionError, EncryptionFormatVersion, Kek,
                KeyScope, MasterKey, WrappedMasterKey,
            },
            state::{EncryptionState, EncryptionStateError},
        },
    };

    struct MockEncryptionState {
        state: EncryptionState,
    }
    #[async_trait]
    impl EncryptionStatePort for MockEncryptionState {
        async fn load_state(&self) -> Result<EncryptionState, EncryptionStateError> {
            Ok(self.state.clone())
        }
        async fn persist_initialized(&self) -> Result<(), EncryptionStateError> {
            Ok(())
        }
        async fn clear_initialized(&self) -> Result<(), EncryptionStateError> {
            Ok(())
        }
    }

    struct MockKeyScope {
        scope: Option<KeyScope>,
    }
    #[async_trait]
    impl KeyScopePort for MockKeyScope {
        async fn current_scope(&self) -> Result<KeyScope, ScopeError> {
            self.scope
                .clone()
                .ok_or(ScopeError::FailedToGetCurrentScope)
        }
    }

    struct MockKeyMaterial {
        keyslot: Option<uc_core::security::model::KeySlot>,
        kek: Option<Kek>,
    }
    #[async_trait]
    impl KeyMaterialPort for MockKeyMaterial {
        async fn load_keyslot(
            &self,
            _s: &KeyScope,
        ) -> Result<uc_core::security::model::KeySlot, EncryptionError> {
            self.keyslot.clone().ok_or(EncryptionError::KeyNotFound)
        }
        async fn store_keyslot(
            &self,
            _: &uc_core::security::model::KeySlot,
        ) -> Result<(), EncryptionError> {
            Ok(())
        }
        async fn delete_keyslot(&self, _: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }
        async fn load_kek(&self, _: &KeyScope) -> Result<Kek, EncryptionError> {
            self.kek.clone().ok_or(EncryptionError::KeyNotFound)
        }
        async fn store_kek(&self, _: &KeyScope, _: &Kek) -> Result<(), EncryptionError> {
            Ok(())
        }
        async fn delete_kek(&self, _: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    struct MockEncryptionPort;
    #[async_trait]
    impl EncryptionPort for MockEncryptionPort {
        async fn derive_kek(
            &self,
            _: &uc_core::security::model::Passphrase,
            _: &[u8],
            _: &uc_core::security::model::KdfParams,
        ) -> Result<Kek, EncryptionError> {
            Ok(Kek([0u8; 32]))
        }
        async fn wrap_master_key(
            &self,
            _: &Kek,
            _: &MasterKey,
            _: EncryptionAlgo,
        ) -> Result<EncryptedBlob, EncryptionError> {
            Ok(EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![0; 24],
                ciphertext: vec![0; 32],
                aad_fingerprint: None,
            })
        }
        async fn unwrap_master_key(
            &self,
            _: &Kek,
            _: &EncryptedBlob,
        ) -> Result<MasterKey, EncryptionError> {
            MasterKey::from_bytes(&[0u8; 32])
        }
        async fn encrypt_blob(
            &self,
            _: &MasterKey,
            _: &[u8],
            _: &[u8],
            _: EncryptionAlgo,
        ) -> Result<EncryptedBlob, EncryptionError> {
            Ok(EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![0; 24],
                ciphertext: vec![],
                aad_fingerprint: None,
            })
        }
        async fn decrypt_blob(
            &self,
            _: &MasterKey,
            _: &EncryptedBlob,
            _: &[u8],
        ) -> Result<Vec<u8>, EncryptionError> {
            Ok(vec![])
        }
    }

    struct MockEncryptionSession {
        master_key_set: Arc<std::sync::atomic::AtomicBool>,
    }
    impl MockEncryptionSession {
        fn new() -> Self {
            Self {
                master_key_set: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }
        }
        fn was_set(&self) -> bool {
            self.master_key_set
                .load(std::sync::atomic::Ordering::SeqCst)
        }
    }
    #[async_trait]
    impl EncryptionSessionPort for MockEncryptionSession {
        async fn is_ready(&self) -> bool {
            self.master_key_set
                .load(std::sync::atomic::Ordering::SeqCst)
        }
        async fn get_master_key(&self) -> Result<MasterKey, EncryptionError> {
            if self
                .master_key_set
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                MasterKey::from_bytes(&[0u8; 32])
            } else {
                Err(EncryptionError::Locked)
            }
        }
        async fn set_master_key(&self, _: MasterKey) -> Result<(), EncryptionError> {
            self.master_key_set
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        async fn clear(&self) -> Result<(), EncryptionError> {
            self.master_key_set
                .store(false, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    fn make_test_keyslot() -> uc_core::security::model::KeySlot {
        uc_core::security::model::KeySlot {
            version: uc_core::security::model::KeySlotVersion::V1,
            scope: KeyScope {
                profile_id: "test".to_string(),
            },
            kdf: uc_core::security::model::KdfParams::for_initialization(),
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

    /// Tests that recover_encryption_session returns Ok(()) when encryption is
    /// Initialized and all dependencies succeed (maps to Ok(true) arm).
    #[tokio::test]
    async fn recover_encryption_session_ok_true_when_initialized() {
        let scope = KeyScope {
            profile_id: "test".to_string(),
        };
        let session = Arc::new(MockEncryptionSession::new());
        let uc = AutoUnlockEncryptionSession::from_ports(
            Arc::new(MockEncryptionState {
                state: EncryptionState::Initialized,
            }),
            Arc::new(MockKeyScope {
                scope: Some(scope.clone()),
            }),
            Arc::new(MockKeyMaterial {
                keyslot: Some(make_test_keyslot()),
                kek: Some(Kek([0u8; 32])),
            }),
            Arc::new(MockEncryptionPort),
            session.clone(),
        );

        // This exercises the Ok(true) arm of recover_encryption_session
        let result = uc.execute().await;
        assert!(
            result.is_ok(),
            "should succeed when encryption is initialized"
        );
        assert_eq!(
            result.unwrap(),
            true,
            "should return true when session recovered"
        );
        assert!(
            session.was_set(),
            "encryption session must be set on recovery"
        );
    }

    /// Tests that recover_encryption_session returns Ok(()) when encryption is
    /// Uninitialized (maps to Ok(false) arm — first run, no recovery needed).
    #[tokio::test]
    async fn recover_encryption_session_ok_false_when_uninitialized() {
        let session = Arc::new(MockEncryptionSession::new());
        let uc = AutoUnlockEncryptionSession::from_ports(
            Arc::new(MockEncryptionState {
                state: EncryptionState::Uninitialized,
            }),
            Arc::new(MockKeyScope {
                scope: Some(KeyScope {
                    profile_id: "test".to_string(),
                }),
            }),
            Arc::new(MockKeyMaterial {
                keyslot: None,
                kek: None,
            }),
            Arc::new(MockEncryptionPort),
            session.clone(),
        );

        // This exercises the Ok(false) arm of recover_encryption_session
        let result = uc.execute().await;
        assert!(
            result.is_ok(),
            "should succeed when encryption is uninitialized"
        );
        assert_eq!(
            result.unwrap(),
            false,
            "should return false when uninitialized (skip)"
        );
        assert!(
            !session.was_set(),
            "encryption session must NOT be set when uninitialized"
        );
    }

    /// Tests that recover_encryption_session returns Err when KEK is missing
    /// (maps to Err arm — daemon must refuse to start per D-05/D-06).
    #[tokio::test]
    async fn recover_encryption_session_err_when_kek_missing() {
        let scope = KeyScope {
            profile_id: "test".to_string(),
        };
        let uc = AutoUnlockEncryptionSession::from_ports(
            Arc::new(MockEncryptionState {
                state: EncryptionState::Initialized,
            }),
            Arc::new(MockKeyScope {
                scope: Some(scope.clone()),
            }),
            // Has keyslot but no KEK — triggers KekLoadFailed error
            Arc::new(MockKeyMaterial {
                keyslot: Some(make_test_keyslot()),
                kek: None,
            }),
            Arc::new(MockEncryptionPort),
            Arc::new(MockEncryptionSession::new()),
        );

        // This exercises the Err arm of recover_encryption_session
        let result = uc.execute().await;
        assert!(result.is_err(), "should fail when KEK is missing");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("failed to load KEK from keyring"),
            "error must indicate KEK load failure, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn setup_completion_emitter_fires_oneshot() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let emitter = SetupCompletionEmitter::new(tx);
        emitter.emit_ready().await.unwrap();
        assert!(rx.await.is_ok(), "receiver should get Ok(())");
    }

    #[tokio::test]
    async fn setup_completion_emitter_double_call_is_noop() {
        let (tx, _rx) = tokio::sync::oneshot::channel::<()>();
        let emitter = SetupCompletionEmitter::new(tx);
        emitter.emit_ready().await.unwrap();
        // Second call should not panic
        emitter.emit_ready().await.unwrap();
    }

    #[test]
    fn daemon_pid_guard_removes_pid_file_on_drop() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");

        with_daemon_env(Some("a"), Some(tempdir.path()), || {
            {
                let _guard = DaemonPidFileGuard::activate().expect("pid guard should activate");
                assert_eq!(
                    read_pid_file()
                        .expect("pid file should be readable")
                        .expect("pid file should exist"),
                    std::process::id()
                );
                assert!(resolve_daemon_pid_path().exists());
            }

            assert!(!resolve_daemon_pid_path().exists());
            assert!(read_pid_file()
                .expect("pid file read should succeed")
                .is_none());
        });
    }
}

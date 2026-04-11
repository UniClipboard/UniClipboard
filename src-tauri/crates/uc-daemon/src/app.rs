//! # DaemonApp
//!
//! Top-level daemon lifecycle: starts the HTTP API server and services,
//! waits for shutdown signal, and tears down in reverse order.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use uc_app::runtime::CoreRuntime;
use uc_app::usecases::space_access::SpaceAccessOrchestrator;
use uc_app::usecases::{CoreUseCases, SessionReadyEmitter};

use crate::api::auth::load_or_create_auth_token;
use crate::api::event_emitter::DaemonApiEventEmitter;
use crate::api::query::DaemonQueryService;
use crate::api::server::{run_http_server, DaemonApiState};
use crate::api::types::DaemonWsEvent;
use crate::pairing::host::DaemonPairingHost;
use crate::process_metadata::DaemonPidManager;
use crate::search::coordinator::SearchCoordinator;
use crate::security::{cleanup_rate_limiter_task, SecurityState};
use crate::service::DaemonService;
use crate::state::RuntimeState;

/// Recover encryption session from disk/keyring if encryption has been initialized.
///
/// # Parameters
///
/// - `runtime`: The core runtime
/// - `auto_unlock_enabled`: Whether to attempt automatic unlock via keyring
///
/// # Returns
///
/// - `Ok(true)`: Session was successfully unlocked (encryption initialized + unlock succeeded)
/// - `Ok(false)`: Session was NOT unlocked — either encryption is uninitialized, or
///   `auto_unlock_enabled` is false while encryption is initialized (requires manual unlock)
/// - `Err`: Unlock failed (daemon must not start in this case)
///
/// This function is `pub` so `main.rs` can call it BEFORE constructing `DaemonApp`,
/// using the result to decide whether to start `PeerDiscoveryWorker` immediately or defer.
pub async fn recover_encryption_session(
    runtime: &CoreRuntime,
    auto_unlock_enabled: bool,
) -> anyhow::Result<bool> {
    // Check encryption state first — needed to determine behavior when auto_unlock is disabled.
    let encryption_state = runtime
        .wiring_deps()
        .security
        .encryption_state
        .load_state()
        .await
        .map_err(|e| anyhow::anyhow!("failed to load encryption state: {}", e))?;

    // When auto-unlock is disabled, skip the unlock attempt entirely.
    // If encryption is already initialized, return false so the GUI can prompt for manual unlock.
    // If encryption is uninitialized, return false (no unlock needed — setup flow handles it).
    if !auto_unlock_enabled {
        if encryption_state == uc_core::security::state::EncryptionState::Initialized {
            info!("Auto-unlock disabled via settings — skipping encryption session recovery");
        } else {
            info!("Encryption not initialized, skipping session recovery");
        }
        return Ok(false);
    }

    // Auto-unlock enabled: attempt to recover the session from keyring.
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

/// Fires a notification when the setup/unlock flow completes.
///
/// Used as the daemon's `SessionReadyEmitter` so that when
/// `AppLifecycleCoordinator::ensure_ready()` fires `emit_ready()`,
/// the daemon can dynamically start deferred services (clipboard-watcher,
/// inbound-clipboard-sync, peer-discovery).
///
/// Uses `Notify` instead of a oneshot so that multiple sources can trigger it:
/// - Setup flow completion (uninitialized encryption)
/// - `/lifecycle/ready` API endpoint (GUI unlock)
pub struct SetupCompletionEmitter {
    notify: Arc<tokio::sync::Notify>,
}

impl SetupCompletionEmitter {
    pub fn new(notify: Arc<tokio::sync::Notify>) -> Self {
        Self { notify }
    }
}

#[async_trait]
impl SessionReadyEmitter for SetupCompletionEmitter {
    async fn emit_ready(&self) -> anyhow::Result<()> {
        self.notify.notify_one();
        debug!("setup completion signal sent via Notify");
        Ok(())
    }
}

/// Main daemon application.
///
/// Owns the service list and cancellation token.
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
    cancel: CancellationToken,
    // Deferred services: clipboard-watcher, inbound-clipboard-sync, and peer-discovery
    // are deferred until the GUI signals ready (--gui-managed) or setup completes (uninitialized).
    deferred_services: Vec<Arc<dyn DaemonService>>,
    /// Notify triggered by either SetupCompletionEmitter or /lifecycle/ready API.
    deferred_ready_notify: Option<Arc<tokio::sync::Notify>>,
    /// External shutdown signal (e.g., from stdin pipe tether when GUI-managed).
    /// When cancelled, the daemon performs a graceful shutdown identical to SIGTERM.
    external_shutdown: Option<CancellationToken>,
    /// Gate that controls clipboard capture. Passed to DaemonApiState so the
    /// `/lifecycle/ready` endpoint can open it when the GUI signals readiness.
    clipboard_capture_gate: Option<Arc<AtomicBool>>,
    /// Search coordinator — wired into DaemonApiState for HTTP route access.
    search_coordinator: Option<Arc<SearchCoordinator>>,
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
    ) -> Self {
        Self {
            services,
            runtime,
            state,
            event_tx,
            api_pairing_host,
            space_access_orchestrator,
            cancel: CancellationToken::new(),
            deferred_services: Vec::new(),
            deferred_ready_notify: None,
            external_shutdown: None,
            clipboard_capture_gate: None,
            search_coordinator: None,
        }
    }

    /// Construct a DaemonApp with deferred services support.
    ///
    /// Services in `deferred_services` will only start when `setup_complete_rx` fires.
    /// This is used for:
    /// - `--gui-managed` mode: clipboard services are deferred until the GUI signals unlock
    /// - Uninitialized encryption: peer-discovery is deferred until setup completes
    ///
    /// `encryption_unlocked` is a required parameter to enforce the invariant that
    /// the caller MUST have completed encryption recovery before constructing DaemonApp.
    pub fn new_with_deferred(
        services: Vec<Arc<dyn DaemonService>>,
        runtime: Arc<CoreRuntime>,
        state: Arc<RwLock<RuntimeState>>,
        event_tx: broadcast::Sender<DaemonWsEvent>,
        api_pairing_host: Option<Arc<DaemonPairingHost>>,
        space_access_orchestrator: Option<Arc<SpaceAccessOrchestrator>>,
        _encryption_unlocked: bool,
        deferred_services: Vec<Arc<dyn DaemonService>>,
        deferred_ready_notify: Option<Arc<tokio::sync::Notify>>,
        external_shutdown: Option<CancellationToken>,
        clipboard_capture_gate: Option<Arc<AtomicBool>>,
        search_coordinator: Option<Arc<SearchCoordinator>>,
    ) -> Self {
        // Validate invariant: deferred_services and deferred_ready_notify must be
        // consistent. If there are deferred services, there must be a Notify to trigger them.
        debug_assert!(
            deferred_services.is_empty() || deferred_ready_notify.is_some(),
            "deferred_services is non-empty but deferred_ready_notify is None — services would never start"
        );
        Self {
            services,
            runtime,
            state,
            event_tx,
            api_pairing_host,
            space_access_orchestrator,
            cancel: CancellationToken::new(),
            deferred_services,
            deferred_ready_notify,
            external_shutdown,
            clipboard_capture_gate,
            search_coordinator,
        }
    }

    /// Run the daemon: start the HTTP API server and services, wait for shutdown, cleanup.
    ///
    /// NOTE: `recover_encryption_session` is called in `main.rs` BEFORE constructing
    /// `DaemonApp`, so it does NOT appear here (Phase 67: moved for deferred-start logic).
    pub async fn run(mut self) -> anyhow::Result<()> {
        info!("uniclipboard-daemon starting");

        // 1. Load or create auth token (stored alongside PID metadata)
        let storage_paths = self.runtime.storage_paths();
        let token_path = storage_paths.daemon_token_path();
        debug!(
            token_path = %token_path.display(),
            "loading daemon auth token"
        );
        let auth_token = load_or_create_auth_token(&token_path)?;
        let pid_manager = DaemonPidManager::new(storage_paths.clone());
        let _pid_file_guard = DaemonPidFileGuard::activate(pid_manager.clone())?;
        let pid = pid_manager.write_current_pid()?;
        info!(pid, "wrote daemon pid metadata");
        let query_service = Arc::new(DaemonQueryService::new(
            self.runtime.clone(),
            self.state.clone(),
        ));

        // 2. Build security state and register daemon's own PID
        let security = Arc::new(SecurityState::new());
        security.register_pid(pid).await;

        // 3. Build API state using the shared event_tx (same channel used by all services)
        let mut api_state = DaemonApiState::new(
            query_service,
            auth_token,
            Some(self.runtime.clone()),
            security,
        );
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
        let api_state = match &self.clipboard_capture_gate {
            Some(gate) => api_state.with_clipboard_gate(Arc::clone(gate)),
            None => api_state,
        };
        let api_state = match &self.deferred_ready_notify {
            Some(notify) => api_state.with_deferred_ready_notify(Arc::clone(notify)),
            None => api_state,
        };
        let api_state = match &self.search_coordinator {
            Some(coordinator) => api_state.with_search_coordinator(Arc::clone(coordinator)),
            None => api_state,
        };

        // 3. Wire the event emitter into the runtime so use cases can emit WS events
        self.runtime
            .set_event_emitter(Arc::new(DaemonApiEventEmitter::new(self.event_tx.clone())));

        info!("uniclipboard-daemon running");

        // 4. Start ALL services uniformly via JoinSet
        let mut service_tasks = JoinSet::new();
        for service in &self.services {
            let svc = Arc::clone(service);
            let token = self.cancel.child_token();
            service_tasks.spawn(async move { svc.start(token).await });
        }

        // 5. Spawn HTTP server and rate limiter cleanup task
        // Clone security and cancel BEFORE moving api_state into the HTTP server
        let security_for_cleanup = api_state.security.clone();
        let cleanup_cancel = self.cancel.child_token();
        let http_cancel = self.cancel.child_token();
        let mut http_handle = tokio::spawn(run_http_server(api_state, http_cancel));

        // Rate limiter cleanup: runs every 5 minutes, respects cleanup_cancel
        let _cleanup_handle = cleanup_rate_limiter_task(security_for_cleanup, cleanup_cancel);

        // Prepare deferred services start
        let mut deferred = std::mem::take(&mut self.deferred_services);
        let ready_notify = self.deferred_ready_notify.take();

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
                result = &mut http_handle => {
                    warn!("HTTP server exited unexpectedly: {:?}", result);
                    break;
                }
                Some(result) = service_tasks.join_next() => {
                    warn!("service task exited unexpectedly: {:?}", result);
                    break;
                }
                _ = async {
                    match &ready_notify {
                        Some(n) => n.notified().await,
                        None => std::future::pending::<()>().await,
                    }
                }, if !deferred.is_empty() => {
                    info!(
                        count = deferred.len(),
                        "ready signal received — starting deferred services"
                    );
                    for worker in deferred.drain(..) {
                        let name = worker.name().to_string();
                        info!(service = %name, "starting deferred service");
                        let worker_for_shutdown: Arc<dyn DaemonService> = Arc::clone(&worker);
                        let token = self.cancel.child_token();
                        service_tasks.spawn(async move { worker.start(token).await });
                        self.services.push(worker_for_shutdown);
                        {
                            let mut state = self.state.write().await;
                            state.update_service_health(&name, crate::service::ServiceHealth::Healthy);
                        }
                    }
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

        // Await HTTP server with timeout
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

        info!("uniclipboard-daemon stopped");
        Ok(())
    }
}

struct DaemonPidFileGuard {
    manager: DaemonPidManager,
}

impl DaemonPidFileGuard {
    fn activate(manager: DaemonPidManager) -> anyhow::Result<Self> {
        let pid = manager.write_current_pid()?;
        info!(pid, "wrote daemon pid metadata");
        Ok(Self { manager })
    }
}

impl Drop for DaemonPidFileGuard {
    fn drop(&mut self) {
        if let Err(error) = self.manager.remove_pid_file() {
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
    use crate::process_metadata::DaemonPidManager;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};
    use uc_app::app_paths::AppPaths;

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

    /// Verifies that entrypoint.rs calls recover_encryption_session before DaemonApp construction.
    #[test]
    fn main_calls_recovery_before_daemon_construction() {
        let main_source = include_str!("entrypoint.rs");
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
    async fn setup_completion_emitter_fires_notify() {
        let notify = Arc::new(tokio::sync::Notify::new());
        let emitter = SetupCompletionEmitter::new(notify.clone());
        emitter.emit_ready().await.unwrap();
        // After emit_ready, the next notified() should return immediately
        tokio::time::timeout(std::time::Duration::from_millis(100), notify.notified())
            .await
            .expect("notified() should return immediately after emit_ready");
    }

    #[tokio::test]
    async fn setup_completion_emitter_double_call_is_safe() {
        let notify = Arc::new(tokio::sync::Notify::new());
        let emitter = SetupCompletionEmitter::new(notify);
        emitter.emit_ready().await.unwrap();
        // Second call should not panic
        emitter.emit_ready().await.unwrap();
    }

    #[test]
    fn daemon_pid_guard_removes_pid_file_on_drop() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");

        with_daemon_env(Some("a"), Some(tempdir.path()), || {
            let app_paths = AppPaths::with_base_data_local_dir(tempdir.path().into());
            let mgr = DaemonPidManager::new(app_paths);
            let pid_path = mgr.pid_path_for_testing();

            {
                let _guard =
                    DaemonPidFileGuard::activate(mgr.clone()).expect("pid guard should activate");
                assert_eq!(
                    mgr.read_pid_file()
                        .expect("pid file should be readable")
                        .expect("pid file should exist"),
                    std::process::id()
                );
                assert!(pid_path.exists());
            }

            assert!(!pid_path.exists());
            assert!(mgr
                .read_pid_file()
                .expect("pid file read should succeed")
                .is_none());
        });
    }
}

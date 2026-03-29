//! # Use Cases Accessor
//!
//! This module provides the `UseCases` accessor which is attached to `AppRuntime`
//! to provide convenient access to all use cases with their dependencies pre-wired.
//!
//! ## Architecture
//!
//! - **uc-app/usecases**: Pure use cases with `new()` constructors taking ports
//! - **uc-tauri/bootstrap**: This module wires `Arc<dyn Port>` from AppDeps into use cases
//! - **Commands**: Call `runtime.usecases().xxx()` to get use case instances
//!
//! ## Usage
//!
//! ```rust,no_run
//! use uc_tauri::bootstrap::AppRuntime;
//! use tauri::State;
//!
//! #[tauri::command]
//! async fn my_command(runtime: State<'_, AppRuntime>) -> Result<(), String> {
//!     let uc = runtime.usecases().list_clipboard_entries();
//!     uc.execute(50, 0).await.map_err(|e| e.to_string())?;
//!     Ok(())
//! }
//! ```
//!
//! ## Adding New Use Cases
//!
//! 1. Ensure use case has a `new()` constructor taking its required ports
//! 2. Add a method to `UseCases` that calls `new()` with deps
//! 3. Commands can now call `runtime.usecases().your_use_case()`

use std::sync::{Arc, RwLock};

use uc_app::task_registry::TaskRegistry;
use uc_app::{runtime::CoreRuntime, App, AppDeps};
use uc_core::config::AppConfig;
use uc_core::ports::SettingsPort;
use uc_core::security::state::EncryptionState;

use uc_core::ports::host_event_emitter::HostEventEmitterPort;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DaemonBootstrapOwnershipSnapshot {
    pub replacement_attempt: u8,
    pub spawned_child_pid: Option<u32>,
    pub last_incompatible_reason: Option<String>,
}

#[derive(Clone, Default)]
pub struct DaemonBootstrapOwnershipState(Arc<RwLock<DaemonBootstrapOwnershipSnapshot>>);

impl DaemonBootstrapOwnershipState {
    pub fn snapshot(&self) -> DaemonBootstrapOwnershipSnapshot {
        match self.0.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in DaemonBootstrapOwnershipState::snapshot, recovering from poisoned state"
                );
                poisoned.into_inner().clone()
            }
        }
    }

    pub fn record_spawned_child(&self, pid: Option<u32>) {
        match self.0.write() {
            Ok(mut guard) => {
                guard.spawned_child_pid = pid;
            }
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in DaemonBootstrapOwnershipState::record_spawned_child, recovering from poisoned state"
                );
                let mut guard = poisoned.into_inner();
                guard.spawned_child_pid = pid;
            }
        }
    }

    pub fn clear_spawned_child(&self) {
        self.record_spawned_child(None);
    }

    pub fn record_replacement_attempt(&self, reason: String) {
        match self.0.write() {
            Ok(mut guard) => {
                guard.replacement_attempt = guard.replacement_attempt.saturating_add(1);
                guard.last_incompatible_reason = Some(reason);
            }
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in DaemonBootstrapOwnershipState::record_replacement_attempt, recovering from poisoned state"
                );
                let mut guard = poisoned.into_inner();
                guard.replacement_attempt = guard.replacement_attempt.saturating_add(1);
                guard.last_incompatible_reason = Some(reason);
            }
        }
    }
}

/// Application runtime with dependencies.
///
/// This struct holds all application dependencies and provides
/// access to use cases through the `usecases()` method.
///
/// Approved access pattern for command modules:
/// - Use `runtime.usecases()` for business operations
/// - Use `runtime.device_id()`, `runtime.is_encryption_ready()`, and
///   `runtime.settings_port()` for simple read-only state access
/// - Direct `runtime.deps.*` access is not allowed in command modules
///
/// ## Architecture / 架构
///
/// The `AppRuntime` serves as the central point for accessing all application
/// dependencies and use cases. It wraps `AppDeps` and provides a `usecases()`
/// method that returns a `UseCases` accessor.
///
/// `AppRuntime` 是访问所有应用依赖和用例的中心点。它包装 `AppDeps` 并提供
/// 返回 `UseCases` 访问器的 `usecases()` 方法。
///
/// ## Usage Example / 使用示例
///
/// ```rust,no_run
/// use uc_tauri::bootstrap::AppRuntime;
/// use tauri::State;
///
/// #[tauri::command]
/// async fn get_entries(runtime: State<'_, AppRuntime>) -> Result<(), String> {
///     let uc = runtime.usecases().list_clipboard_entries();
///     let entries = uc.execute(50, 0).await.map_err(|e| e.to_string())?;
///     Ok(())
/// }
/// ```
///
/// 包含所有应用依赖的运行时。
///
/// 此结构体保存所有应用依赖，并通过 `usecases()` 方法提供用例访问。
pub struct AppRuntime {
    /// Tauri-free core runtime with all domain state.
    core: Arc<CoreRuntime>,
    /// Tauri AppHandle for event emission (optional, set after Tauri setup).
    /// Uses RwLock for interior mutability since Arc<AppRuntime> is shared.
    app_handle: Arc<std::sync::RwLock<Option<tauri::AppHandle>>>,
}

impl AppRuntime {
    /// Create a new AppRuntime from dependencies.
    /// 从依赖创建新的 AppRuntime。
    pub fn new(deps: AppDeps, storage_paths: uc_app::app_paths::AppPaths) -> Self {
        let setup_ports = uc_bootstrap::assembly::SetupAssemblyPorts::placeholder(&deps);
        let event_emitter: Arc<dyn HostEventEmitterPort> =
            Arc::new(crate::adapters::host_event_emitter::LoggingEventEmitter);
        Self::with_setup(deps, setup_ports, storage_paths, event_emitter)
    }

    /// Create a new AppRuntime with explicit setup orchestrator dependencies.
    pub fn with_setup(
        deps: AppDeps,
        setup_ports: uc_bootstrap::assembly::SetupAssemblyPorts,
        storage_paths: uc_app::app_paths::AppPaths,
        event_emitter: Arc<dyn HostEventEmitterPort>,
    ) -> Self {
        let lifecycle_status: Arc<dyn uc_app::usecases::LifecycleStatusPort> =
            Arc::new(uc_app::usecases::InMemoryLifecycleStatus::new());
        let app_handle = Arc::new(std::sync::RwLock::new(None));
        // Clipboard integration mode is resolved from the UC_CLIPBOARD_MODE env var.
        // Defaults to Full (standalone GUI watches clipboard directly).
        // Set UC_CLIPBOARD_MODE=passive when a daemon is running and handling
        // clipboard capture + broadcast via DaemonWsBridge.
        let clipboard_integration_mode = uc_bootstrap::resolve_clipboard_integration_mode();
        let task_registry = Arc::new(TaskRegistry::new());

        // Create the shared emitter cell BEFORE both consumers.
        // This cell is shared between CoreRuntime and build_setup_orchestrator
        // so that HostEventSetupPort reads the current emitter after swap.
        let emitter_cell = Arc::new(std::sync::RwLock::new(event_emitter));

        // Build session_ready_emitter from app_handle BEFORE build_setup_orchestrator.
        let session_ready_emitter: Arc<dyn uc_app::usecases::SessionReadyEmitter> = Arc::new(
            crate::adapters::lifecycle::TauriSessionReadyEmitter::new(app_handle.clone()),
        );

        // Pass shared state + adapters to build_setup_orchestrator as SEPARATE params.
        let setup_orchestrator = uc_bootstrap::assembly::build_setup_orchestrator(
            &deps,
            setup_ports,
            lifecycle_status.clone(), // same instance goes to CoreRuntime below
            emitter_cell.clone(),     // same instance goes to CoreRuntime below
            session_ready_emitter,    // constructed from app_handle above
        );

        // Pass the SAME cell to CoreRuntime — no re-wrapping.
        let core = Arc::new(CoreRuntime::new(
            deps,
            emitter_cell,
            lifecycle_status,
            setup_orchestrator,
            clipboard_integration_mode,
            task_registry,
            storage_paths,
        ));

        Self { core, app_handle }
    }

    /// Wire a `ClipboardWriteCoordinator` into the inner `CoreRuntime`.
    ///
    /// Must be called BEFORE `Arc::new(runtime)` (i.e. while the runtime is still
    /// uniquely owned). GUI bootstrap calls this with
    /// `background.clipboard_write_coordinator.clone()` after `with_setup()`.
    pub fn with_clipboard_write_coordinator(
        mut self,
        coordinator: Arc<uc_app::usecases::ClipboardWriteCoordinator>,
    ) -> Self {
        // Arc::get_mut succeeds here because the caller has not yet shared the Arc.
        if let Some(core) = Arc::get_mut(&mut self.core) {
            core.set_clipboard_write_coordinator(coordinator);
        } else {
            tracing::warn!(
                "with_clipboard_write_coordinator called after Arc was shared — coordinator not set"
            );
        }
        self
    }

    /// Set the Tauri AppHandle for event emission.
    /// This must be called after Tauri setup completes.
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        match self.app_handle.write() {
            Ok(mut guard) => {
                *guard = Some(handle);
            }
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in set_app_handle, recovering from poisoned state"
                );
                let mut guard = poisoned.into_inner();
                *guard = Some(handle);
            }
        }
    }

    /// Get a reference to the AppHandle, if available.
    pub fn app_handle(&self) -> std::sync::RwLockReadGuard<'_, Option<tauri::AppHandle>> {
        self.app_handle.read().unwrap_or_else(|poisoned| {
            tracing::error!("RwLock poisoned in app_handle, recovering from poisoned state");
            poisoned.into_inner()
        })
    }

    /// Returns a clone of the shared app_handle cell.
    /// Used by consumers (like TauriSessionReadyEmitter) that need to hold onto the handle.
    pub fn app_handle_cell(&self) -> Arc<std::sync::RwLock<Option<tauri::AppHandle>>> {
        self.app_handle.clone()
    }

    /// Get the current event emitter (clones the inner Arc).
    ///
    /// Returns the active [`HostEventEmitterPort`] implementation. During early bootstrap,
    /// this is a [`LoggingEventEmitter`]; after setup, a `TauriEventEmitter`.
    pub fn event_emitter(&self) -> Arc<dyn HostEventEmitterPort> {
        self.core.event_emitter()
    }

    /// Swap the event emitter. Called from setup callback to replace the
    /// initial [`LoggingEventEmitter`] with a [`TauriEventEmitter`] once the
    /// Tauri `AppHandle` is available.
    pub fn set_event_emitter(&self, emitter: Arc<dyn HostEventEmitterPort>) {
        self.core.set_event_emitter(emitter);
    }

    /// Returns a reference to the CoreRuntime for consumers that need it.
    pub fn core(&self) -> &Arc<CoreRuntime> {
        &self.core
    }

    /// Get use cases accessor.
    /// 获取用例访问器。
    pub fn usecases(&self) -> AppUseCases<'_> {
        AppUseCases::new(self)
    }

    /// Returns the current device ID for tracing spans and session context.
    /// For business operations involving device identity, use `self.usecases()`.
    pub fn device_id(&self) -> String {
        self.core.device_id()
    }

    /// Check if the encryption session is ready.
    pub async fn is_encryption_ready(&self) -> bool {
        self.core.is_encryption_ready().await
    }

    /// Returns the persisted encryption state used by readiness checks.
    pub async fn encryption_state(&self) -> Result<EncryptionState, String> {
        self.core.encryption_state().await
    }

    /// Returns a clone of the settings port for resolve_pairing_device_name.
    /// This is a thin accessor; for settings business operations, use usecases().
    pub fn settings_port(&self) -> Arc<dyn SettingsPort> {
        self.core.settings_port()
    }

    /// Returns a reference to the underlying AppDeps for wiring/bootstrap code only.
    ///
    /// **IMPORTANT**: This method is intended exclusively for bootstrap wiring code
    /// (e.g., `start_background_tasks` in `main.rs`). Command handlers MUST NOT use
    /// this method — use `runtime.usecases()` or specific facade methods instead.
    pub fn wiring_deps(&self) -> &AppDeps {
        self.core.wiring_deps()
    }

    pub fn clipboard_integration_mode(&self) -> uc_core::clipboard::ClipboardIntegrationMode {
        self.core.clipboard_integration_mode()
    }

    /// Returns a reference to the task registry for lifecycle management.
    ///
    /// Used by bootstrap code to spawn tracked background tasks and by the
    /// app exit hook to trigger graceful shutdown.
    pub fn task_registry(&self) -> &Arc<TaskRegistry> {
        self.core.task_registry()
    }
}

/// Tauri-aware use case accessors wrapping CoreUseCases.
///
/// Provides transparent access to all CoreUseCases methods (via Deref) plus
/// 3 non-core accessors that cannot live in uc-app:
/// - apply_autostart (needs AppHandle)
/// - app_lifecycle_coordinator (needs TauriSessionReadyEmitter)
/// - sync_outbound_clipboard (needs uc_infra TransferPayloadEncryptorAdapter)
pub struct AppUseCases<'a> {
    app_runtime: &'a AppRuntime,
    core: uc_app::usecases::CoreUseCases<'a>,
}

impl<'a> AppUseCases<'a> {
    pub fn new(app_runtime: &'a AppRuntime) -> Self {
        let core = uc_app::usecases::CoreUseCases::new(&app_runtime.core);
        Self { app_runtime, core }
    }

    /// Apply OS-level autostart setting.
    ///
    /// Requires AppHandle to be set (returns None during early bootstrap).
    pub fn apply_autostart(
        &self,
    ) -> Option<
        uc_platform::usecases::ApplyAutostartSetting<crate::adapters::autostart::TauriAutostart>,
    > {
        let guard = self.app_runtime.app_handle();
        let handle = guard.as_ref()?;
        let adapter = Arc::new(crate::adapters::autostart::TauriAutostart::new(
            handle.clone(),
        ));
        Some(uc_platform::usecases::ApplyAutostartSetting::new(adapter))
    }

    /// Get the AppLifecycleCoordinator use case for orchestrating
    /// network startup and session readiness.
    pub fn app_lifecycle_coordinator(&self) -> uc_app::usecases::AppLifecycleCoordinator {
        let announcer = Arc::new(uc_app::usecases::DeviceNameAnnouncer::new(
            self.app_runtime.wiring_deps().network_ports.peers.clone(),
            self.app_runtime.wiring_deps().settings.clone(),
        ));
        uc_app::usecases::AppLifecycleCoordinator::from_deps(
            uc_app::usecases::AppLifecycleCoordinatorDeps {
                network: Arc::new(self.core.start_network_after_unlock()),
                announcer: Some(announcer),
                emitter: Arc::new(crate::adapters::lifecycle::TauriSessionReadyEmitter::new(
                    self.app_runtime.app_handle_cell(),
                )),
                status: self.app_runtime.core.lifecycle_status().clone(),
                lifecycle_emitter: Arc::new(uc_app::usecases::LoggingLifecycleEventEmitter),
            },
        )
    }

    pub fn sync_outbound_clipboard(
        &self,
    ) -> uc_app::usecases::clipboard::sync_outbound::SyncOutboundClipboardUseCase {
        uc_app::usecases::clipboard::sync_outbound::SyncOutboundClipboardUseCase::new(
            self.app_runtime
                .wiring_deps()
                .clipboard
                .system_clipboard
                .clone(),
            self.app_runtime
                .wiring_deps()
                .network_ports
                .clipboard
                .clone(),
            self.app_runtime.wiring_deps().network_ports.peers.clone(),
            self.app_runtime
                .wiring_deps()
                .security
                .encryption_session
                .clone(),
            self.app_runtime
                .wiring_deps()
                .device
                .device_identity
                .clone(),
            self.app_runtime.wiring_deps().settings.clone(),
            Arc::new(uc_infra::clipboard::TransferPayloadEncryptorAdapter),
            self.app_runtime
                .wiring_deps()
                .device
                .paired_device_repo
                .clone(),
        )
    }
}

impl<'a> std::ops::Deref for AppUseCases<'a> {
    type Target = uc_app::usecases::CoreUseCases<'a>;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

/// Seed for creating the application runtime.
///
/// This is an assembly context that holds the AppConfig
/// before Tauri setup phase completes. It does NOT contain
/// a fully constructed runtime - that happens in the setup phase.
///
/// ## English
///
/// This struct serves as a bridge between:
/// - Phase 1: Configuration loading (pre-Tauri)
/// - Phase 2: Dependency wiring (Tauri setup)
/// - Phase 3: App construction (post-setup)
///
/// ## 中文
///
/// 此结构作为以下阶段之间的桥梁：
/// - 阶段 1：配置加载（Tauri 之前）
/// - 阶段 2：依赖连接（Tauri 设置）
/// - 阶段 3：应用构造（设置之后）
pub struct AppRuntimeSeed {
    /// Application configuration loaded from TOML
    /// 从 TOML 加载的应用配置
    pub config: AppConfig,
}

/// Create the runtime seed without touching Tauri.
///
/// This function must not depend on Tauri or any UI framework.
/// 不依赖 Tauri 或任何 UI 框架创建运行时种子。
///
/// ## Phase Integration / 阶段集成
///
/// - **Phase 1**: Call this after `load_config()` to create the seed
/// - **Phase 2**: Pass seed to `wire_dependencies()` in Tauri setup
/// - **Phase 3**: Call `create_app()` with wired dependencies
///
/// ## English
///
/// This is the entry point for the bootstrap sequence:
/// 1. `load_config()` → reads TOML into `AppConfig`
/// 2. `create_runtime()` → wraps config in `AppRuntimeSeed`
/// 3. `wire_dependencies()` → creates ports from config
/// 4. `create_app()` → constructs `App` from dependencies
pub fn create_runtime(config: AppConfig) -> anyhow::Result<AppRuntimeSeed> {
    Ok(AppRuntimeSeed { config })
}

/// Create the App instance from wired dependencies.
/// 从已连接的依赖创建 App 实例。
///
/// ## English
///
/// This function is called in Phase 3 (after Tauri setup completes)
/// to construct the final `App` instance from the dependencies
/// that were wired in Phase 2.
///
/// This is a direct construction function - NOT a builder pattern.
/// All dependencies must be provided; no defaults, no optionals.
///
/// ## 中文
///
/// 此函数在阶段 3（Tauri 设置完成后）调用，
/// 用于从阶段 2 中连接的依赖构造最终的 `App` 实例。
///
/// 这是一个直接构造函数 - 不是 Builder 模式。
/// 必须提供所有依赖；无默认值，无可选项。
///
/// # Parameters / 参数
///
/// - `deps`: Application dependencies wired from configuration
///           从配置连接的应用依赖
///
/// # Returns / 返回
///
/// - `App`: Fully constructed application runtime
///          完全构造的应用运行时
///
/// # Phase 3 Integration / 阶段 3 集成
///
/// This function completes the bootstrap sequence:
/// ```text
/// load_config() → create_runtime() → wire_dependencies() → create_app()
///     ↓                 ↓                    ↓                    ↓
///   AppConfig      AppRuntimeSeed        AppDeps               App
/// ```
pub fn create_app(deps: AppDeps) -> App {
    App::new(deps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::noop_network_ports;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;
    use uc_core::clipboard::PolicyError;
    use uc_core::ports::clipboard::{RepresentationCachePort, SpoolQueuePort, SpoolRequest};
    use uc_core::ports::host_event_emitter::{ClipboardHostEvent, HostEvent};
    use uc_core::ports::security::encryption_state::EncryptionStatePort;
    use uc_core::ports::security::key_scope::KeyScopePort;
    use uc_core::ports::*;
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionError, KdfParams, Kek, KeyScope, KeySlot,
        MasterKey, Passphrase,
    };
    use uc_core::security::state::{EncryptionState, EncryptionStateError};
    use uc_core::PeerId;
    use uc_core::{
        Blob, BlobId, ContentHash, DeviceId, PersistedClipboardRepresentation,
        SystemClipboardSnapshot,
    };
    use uc_infra::clipboard::new_in_memory_change_origin;

    fn test_origin() -> std::sync::Arc<dyn uc_core::ports::clipboard::ClipboardChangeOriginPort> {
        new_in_memory_change_origin()
    }

    use uc_platform::ports::{AutostartPort, UiPort};

    struct MockEntryRepository {
        save_calls: Arc<AtomicUsize>,
    }

    struct MockEventWriter {
        insert_calls: Arc<AtomicUsize>,
    }

    struct MockRepresentationPolicy {
        select_calls: Arc<AtomicUsize>,
    }

    struct MockNormalizer {
        normalize_calls: Arc<AtomicUsize>,
    }

    struct MockRepresentationCache {
        put_calls: Arc<AtomicUsize>,
    }

    struct MockSpoolQueue {
        enqueue_calls: Arc<AtomicUsize>,
    }

    struct SuccessfulRepresentationPolicy;

    struct SuccessfulNormalizer;

    #[derive(Default)]
    struct RecordingEmitter {
        events: Mutex<Vec<HostEvent>>,
    }

    struct MockDeviceIdentity;

    struct NoopClipboard;
    struct NoopPort;

    #[async_trait]
    impl ClipboardEntryRepositoryPort for MockEntryRepository {
        async fn save_entry_and_selection(
            &self,
            _entry: &uc_core::ClipboardEntry,
            _selection: &uc_core::ClipboardSelectionDecision,
        ) -> anyhow::Result<()> {
            self.save_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn get_entry(
            &self,
            _entry_id: &uc_core::ids::EntryId,
        ) -> anyhow::Result<Option<uc_core::ClipboardEntry>> {
            Ok(None)
        }

        async fn list_entries(
            &self,
            _limit: usize,
            _offset: usize,
        ) -> anyhow::Result<Vec<uc_core::ClipboardEntry>> {
            Ok(vec![])
        }

        async fn delete_entry(&self, _entry_id: &uc_core::ids::EntryId) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardEventWriterPort for MockEventWriter {
        async fn insert_event(
            &self,
            _event: &uc_core::ClipboardEvent,
            _representations: &Vec<uc_core::PersistedClipboardRepresentation>,
        ) -> anyhow::Result<()> {
            self.insert_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn delete_event_and_representations(
            &self,
            _event_id: &uc_core::ids::EventId,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl SelectRepresentationPolicyPort for MockRepresentationPolicy {
        fn select(
            &self,
            _snapshot: &SystemClipboardSnapshot,
        ) -> std::result::Result<uc_core::clipboard::ClipboardSelection, PolicyError> {
            self.select_calls.fetch_add(1, Ordering::SeqCst);
            Err(PolicyError::NoUsableRepresentation)
        }
    }

    #[async_trait]
    impl ClipboardRepresentationNormalizerPort for MockNormalizer {
        async fn normalize(
            &self,
            _observed: &uc_core::clipboard::ObservedClipboardRepresentation,
        ) -> anyhow::Result<uc_core::PersistedClipboardRepresentation> {
            self.normalize_calls.fetch_add(1, Ordering::SeqCst);
            Err(anyhow::anyhow!("normalize should not be called"))
        }
    }

    impl DeviceIdentityPort for MockDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            DeviceId::new("device-test")
        }
    }

    #[async_trait]
    impl RepresentationCachePort for MockRepresentationCache {
        async fn put(&self, _rep_id: &uc_core::ids::RepresentationId, _bytes: Vec<u8>) {
            self.put_calls.fetch_add(1, Ordering::SeqCst);
        }

        async fn get(&self, _rep_id: &uc_core::ids::RepresentationId) -> Option<Vec<u8>> {
            None
        }

        async fn mark_completed(&self, _rep_id: &uc_core::ids::RepresentationId) {}

        async fn mark_spooling(&self, _rep_id: &uc_core::ids::RepresentationId) {}

        async fn remove(&self, _rep_id: &uc_core::ids::RepresentationId) {}
    }

    #[async_trait]
    impl SpoolQueuePort for MockSpoolQueue {
        async fn enqueue(&self, _request: SpoolRequest) -> anyhow::Result<()> {
            self.enqueue_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    impl SelectRepresentationPolicyPort for SuccessfulRepresentationPolicy {
        fn select(
            &self,
            snapshot: &SystemClipboardSnapshot,
        ) -> std::result::Result<uc_core::clipboard::ClipboardSelection, PolicyError> {
            let rep_id = snapshot
                .representations
                .first()
                .expect("snapshot should contain one representation")
                .id
                .clone();

            Ok(uc_core::clipboard::ClipboardSelection {
                primary_rep_id: rep_id.clone(),
                secondary_rep_ids: vec![],
                preview_rep_id: rep_id.clone(),
                paste_rep_id: rep_id,
                policy_version: uc_core::clipboard::SelectionPolicyVersion::V1,
            })
        }
    }

    #[async_trait]
    impl ClipboardRepresentationNormalizerPort for SuccessfulNormalizer {
        async fn normalize(
            &self,
            observed: &uc_core::clipboard::ObservedClipboardRepresentation,
        ) -> anyhow::Result<uc_core::PersistedClipboardRepresentation> {
            Ok(uc_core::PersistedClipboardRepresentation::new(
                observed.id.clone(),
                observed.format_id.clone(),
                observed.mime.clone(),
                observed.size_bytes(),
                Some(observed.bytes.clone()),
                None,
            ))
        }
    }

    impl HostEventEmitterPort for RecordingEmitter {
        fn emit(
            &self,
            event: HostEvent,
        ) -> Result<(), uc_core::ports::host_event_emitter::EmitError> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }
    }

    impl SystemClipboardPort for NoopClipboard {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            Ok(SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            })
        }

        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardSelectionRepositoryPort for NoopPort {
        async fn get_selection(
            &self,
            _entry_id: &uc_core::ids::EntryId,
        ) -> anyhow::Result<Option<uc_core::ClipboardSelectionDecision>> {
            Ok(None)
        }

        async fn delete_selection(&self, _entry_id: &uc_core::ids::EntryId) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardRepresentationRepositoryPort for NoopPort {
        async fn get_representation(
            &self,
            _event_id: &uc_core::ids::EventId,
            _representation_id: &uc_core::ids::RepresentationId,
        ) -> anyhow::Result<Option<uc_core::PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_id(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
        ) -> anyhow::Result<Option<uc_core::PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_blob_id(
            &self,
            _blob_id: &BlobId,
        ) -> anyhow::Result<Option<uc_core::PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn update_blob_id(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
            _blob_id: &BlobId,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn update_blob_id_if_none(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
            _blob_id: &BlobId,
        ) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn update_processing_result(
            &self,
            _rep_id: &uc_core::ids::RepresentationId,
            _expected_states: &[uc_core::clipboard::PayloadAvailability],
            _blob_id: Option<&BlobId>,
            _new_state: uc_core::clipboard::PayloadAvailability,
            _last_error: Option<&str>,
        ) -> anyhow::Result<uc_core::ports::clipboard::ProcessingUpdateOutcome> {
            Ok(uc_core::ports::clipboard::ProcessingUpdateOutcome::NotFound)
        }
    }

    #[async_trait]
    impl ClipboardPayloadResolverPort for NoopPort {
        async fn resolve(
            &self,
            _representation: &PersistedClipboardRepresentation,
        ) -> anyhow::Result<ResolvedClipboardPayload> {
            Err(anyhow::anyhow!("noop payload resolver"))
        }
    }

    #[async_trait]
    impl EncryptionPort for NoopPort {
        async fn derive_kek(
            &self,
            _passphrase: &Passphrase,
            _salt: &[u8],
            _kdf: &KdfParams,
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
            _wrapped: &EncryptedBlob,
        ) -> Result<MasterKey, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn encrypt_blob(
            &self,
            _master_key: &MasterKey,
            _plaintext: &[u8],
            _aad: &[u8],
            _aead: EncryptionAlgo,
        ) -> Result<EncryptedBlob, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn decrypt_blob(
            &self,
            _master_key: &MasterKey,
            _encrypted: &EncryptedBlob,
            _aad: &[u8],
        ) -> Result<Vec<u8>, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }
    }

    #[async_trait]
    impl EncryptionSessionPort for NoopPort {
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

    #[async_trait]
    impl EncryptionStatePort for NoopPort {
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

    #[async_trait]
    impl KeyScopePort for NoopPort {
        async fn current_scope(
            &self,
        ) -> Result<KeyScope, uc_core::ports::security::key_scope::ScopeError> {
            Err(uc_core::ports::security::key_scope::ScopeError::FailedToGetCurrentScope)
        }
    }

    #[async_trait]
    impl KeyMaterialPort for NoopPort {
        async fn load_kek(&self, _scope: &KeyScope) -> Result<Kek, EncryptionError> {
            Err(EncryptionError::KeyNotFound)
        }

        async fn store_kek(&self, _scope: &KeyScope, _kek: &Kek) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn delete_kek(&self, _scope: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn load_keyslot(&self, _scope: &KeyScope) -> Result<KeySlot, EncryptionError> {
            Err(EncryptionError::KeyNotFound)
        }

        async fn store_keyslot(&self, _keyslot: &KeySlot) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn delete_keyslot(&self, _scope: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    #[async_trait]
    impl DeviceRepositoryPort for NoopPort {
        async fn find_by_id(
            &self,
            _id: &uc_core::device::DeviceId,
        ) -> Result<Option<uc_core::device::Device>, uc_core::ports::errors::DeviceRepositoryError>
        {
            Ok(None)
        }

        async fn save(
            &self,
            _device: uc_core::device::Device,
        ) -> Result<(), uc_core::ports::errors::DeviceRepositoryError> {
            Ok(())
        }

        async fn delete(
            &self,
            _id: &uc_core::device::DeviceId,
        ) -> Result<(), uc_core::ports::errors::DeviceRepositoryError> {
            Ok(())
        }

        async fn list_all(
            &self,
        ) -> Result<Vec<uc_core::device::Device>, uc_core::ports::errors::DeviceRepositoryError>
        {
            Ok(vec![])
        }
    }

    #[async_trait]
    impl ClipboardTransportPort for NoopPort {
        async fn send_clipboard(
            &self,
            _peer_id: &str,
            _encrypted_data: Arc<[u8]>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn broadcast_clipboard(&self, _encrypted_data: Arc<[u8]>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe_clipboard(
            &self,
        ) -> anyhow::Result<
            tokio::sync::mpsc::Receiver<(uc_core::network::ClipboardMessage, Option<Vec<u8>>)>,
        > {
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }
    }

    #[async_trait]
    impl PeerDirectoryPort for NoopPort {
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
            "noop".to_string()
        }

        async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl PairingTransportPort for NoopPort {
        async fn open_pairing_session(
            &self,
            _peer_id: String,
            _session_id: String,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_pairing_on_session(
            &self,
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
    }

    #[async_trait]
    impl NetworkEventPort for NoopPort {
        async fn subscribe_events(
            &self,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<uc_core::network::NetworkEvent>> {
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }
    }

    #[async_trait]
    impl uc_core::ports::NetworkControlPort for NoopPort {
        async fn start_network(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl SetupStatusPort for NoopPort {
        async fn get_status(&self) -> anyhow::Result<uc_core::setup::SetupStatus> {
            Ok(uc_core::setup::SetupStatus::default())
        }

        async fn set_status(&self, _status: &uc_core::setup::SetupStatus) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl uc_core::ports::SecureStoragePort for NoopPort {
        fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, uc_core::ports::SecureStorageError> {
            Ok(None)
        }

        fn set(&self, _key: &str, _value: &[u8]) -> Result<(), uc_core::ports::SecureStorageError> {
            Ok(())
        }

        fn delete(&self, _key: &str) -> Result<(), uc_core::ports::SecureStorageError> {
            Ok(())
        }
    }

    #[async_trait]
    impl BlobStorePort for NoopPort {
        async fn put(
            &self,
            _blob_id: &BlobId,
            _data: &[u8],
        ) -> anyhow::Result<(std::path::PathBuf, Option<i64>)> {
            Ok((
                std::path::PathBuf::from("/tmp/noop"),
                i64::try_from(_data.len()).ok(),
            ))
        }

        async fn get(&self, _blob_id: &BlobId) -> anyhow::Result<Vec<u8>> {
            Ok(vec![])
        }
    }

    #[async_trait]
    impl BlobRepositoryPort for NoopPort {
        async fn insert_blob(&self, _blob: &Blob) -> anyhow::Result<()> {
            Ok(())
        }

        async fn find_by_hash(&self, _content_hash: &ContentHash) -> anyhow::Result<Option<Blob>> {
            Ok(None)
        }
    }

    #[async_trait]
    impl BlobWriterPort for NoopPort {
        async fn write_if_absent(
            &self,
            _content_id: &ContentHash,
            _plaintext_bytes: &[u8],
        ) -> anyhow::Result<Blob> {
            Err(anyhow::anyhow!("noop blob writer"))
        }
    }

    #[async_trait]
    impl ThumbnailRepositoryPort for NoopPort {
        async fn get_by_representation_id(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
        ) -> anyhow::Result<Option<uc_core::clipboard::ThumbnailMetadata>> {
            Ok(None)
        }

        async fn insert_thumbnail(
            &self,
            _metadata: &uc_core::clipboard::ThumbnailMetadata,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ThumbnailGeneratorPort for NoopPort {
        async fn generate_thumbnail(
            &self,
            _image_bytes: &[u8],
        ) -> anyhow::Result<uc_core::ports::clipboard::GeneratedThumbnail> {
            Err(anyhow::anyhow!("noop thumbnail generator"))
        }

        async fn generate_thumbnail_from_rgba(
            &self,
            _rgba_bytes: &[u8],
            _width: u32,
            _height: u32,
        ) -> anyhow::Result<uc_core::ports::clipboard::GeneratedThumbnail> {
            self.generate_thumbnail(&[]).await
        }
    }

    #[async_trait]
    impl SettingsPort for NoopPort {
        async fn load(&self) -> anyhow::Result<uc_core::settings::model::Settings> {
            Err(anyhow::anyhow!("noop settings"))
        }

        async fn save(&self, _settings: &uc_core::settings::model::Settings) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl PairedDeviceRepositoryPort for NoopPort {
        async fn get_by_peer_id(
            &self,
            _peer_id: &PeerId,
        ) -> Result<Option<uc_core::network::PairedDevice>, PairedDeviceRepositoryError> {
            Ok(None)
        }

        async fn list_all(
            &self,
        ) -> Result<Vec<uc_core::network::PairedDevice>, PairedDeviceRepositoryError> {
            Ok(Vec::new())
        }

        async fn upsert(
            &self,
            _device: uc_core::network::PairedDevice,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn set_state(
            &self,
            _peer_id: &PeerId,
            _state: uc_core::network::PairingState,
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

        async fn update_sync_settings(
            &self,
            _peer_id: &PeerId,
            _settings: Option<uc_core::settings::model::SyncSettings>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    #[async_trait]
    impl UiPort for NoopPort {
        async fn open_settings(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl AutostartPort for NoopPort {
        fn is_enabled(&self) -> anyhow::Result<bool> {
            Ok(false)
        }

        fn enable(&self) -> anyhow::Result<()> {
            Ok(())
        }

        fn disable(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl ClockPort for NoopPort {
        fn now_ms(&self) -> i64 {
            0
        }
    }

    impl ContentHashPort for NoopPort {
        fn hash_bytes(&self, _bytes: &[u8]) -> anyhow::Result<ContentHash> {
            Err(anyhow::anyhow!("noop hash"))
        }
    }

    impl uc_core::ports::FileManagerPort for NoopPort {
        fn open_directory(
            &self,
            _path: &std::path::Path,
        ) -> Result<(), uc_core::ports::FileManagerError> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl uc_core::ports::CacheFsPort for NoopPort {
        async fn exists(&self, _path: &std::path::Path) -> bool {
            false
        }
        async fn read_dir(
            &self,
            _path: &std::path::Path,
        ) -> anyhow::Result<Vec<uc_core::ports::CacheFsDirEntry>> {
            Ok(vec![])
        }
        async fn remove_dir_all(&self, _path: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn remove_file(&self, _path: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn dir_size(&self, _path: &std::path::Path) -> anyhow::Result<u64> {
            Ok(0)
        }
    }

    fn test_storage_paths() -> uc_app::app_paths::AppPaths {
        uc_app::app_paths::AppPaths {
            db_path: std::path::PathBuf::from("/tmp/uniclipboard-test/uniclipboard.db"),
            vault_dir: std::path::PathBuf::from("/tmp/uniclipboard-test/vault"),
            settings_path: std::path::PathBuf::from("/tmp/uniclipboard-test/settings.json"),
            logs_dir: std::path::PathBuf::from("/tmp/uniclipboard-test/logs"),
            cache_dir: std::path::PathBuf::from("/tmp/uniclipboard-test-cache"),
            file_cache_dir: std::path::PathBuf::from("/tmp/uniclipboard-test-cache/file-cache"),
            spool_dir: std::path::PathBuf::from("/tmp/uniclipboard-test-cache/spool"),
            app_data_root: std::path::PathBuf::from("/tmp/uniclipboard-test"),
        }
    }

    #[test]
    fn runtime_event_emitter_can_be_swapped_after_setup() {
        let deps = AppDeps {
            clipboard: uc_app::ClipboardPorts {
                clipboard: Arc::new(NoopClipboard),
                system_clipboard: Arc::new(NoopClipboard),
                clipboard_entry_repo: Arc::new(MockEntryRepository {
                    save_calls: Arc::new(AtomicUsize::new(0)),
                }),
                clipboard_event_repo: Arc::new(MockEventWriter {
                    insert_calls: Arc::new(AtomicUsize::new(0)),
                }),
                representation_repo: Arc::new(NoopPort),
                representation_normalizer: Arc::new(MockNormalizer {
                    normalize_calls: Arc::new(AtomicUsize::new(0)),
                }),
                selection_repo: Arc::new(NoopPort),
                representation_policy: Arc::new(MockRepresentationPolicy {
                    select_calls: Arc::new(AtomicUsize::new(0)),
                }),
                representation_cache: Arc::new(MockRepresentationCache {
                    put_calls: Arc::new(AtomicUsize::new(0)),
                }),
                spool_queue: Arc::new(MockSpoolQueue {
                    enqueue_calls: Arc::new(AtomicUsize::new(0)),
                }),
                worker_tx: mpsc::channel(1).0,
                clipboard_change_origin: test_origin(),
                payload_resolver: Arc::new(NoopPort),
            },
            security: uc_app::SecurityPorts {
                encryption: Arc::new(NoopPort),
                encryption_session: Arc::new(NoopPort),
                encryption_state: Arc::new(NoopPort),
                key_scope: Arc::new(NoopPort),
                secure_storage: Arc::new(NoopPort),
                key_material: Arc::new(NoopPort),
            },
            device: uc_app::DevicePorts {
                device_repo: Arc::new(NoopPort),
                device_identity: Arc::new(MockDeviceIdentity),
                paired_device_repo: Arc::new(NoopPort),
            },
            network_ports: noop_network_ports(),
            network_control: Arc::new(NoopPort),
            setup_status: Arc::new(NoopPort),
            storage: uc_app::StoragePorts {
                blob_store: Arc::new(NoopPort),
                blob_repository: Arc::new(NoopPort),
                blob_writer: Arc::new(NoopPort),
                thumbnail_repo: Arc::new(NoopPort),
                thumbnail_generator: Arc::new(NoopPort),
                file_transfer_repo: Arc::new(uc_core::ports::NoopFileTransferRepositoryPort),
            },
            settings: Arc::new(NoopPort),
            system: uc_app::SystemPorts {
                clock: Arc::new(NoopPort),
                hash: Arc::new(NoopPort),
                file_manager: Arc::new(NoopPort),
                cache_fs: Arc::new(NoopPort),
            },
        };
        let setup_ports = uc_bootstrap::assembly::SetupAssemblyPorts::placeholder(&deps);
        let initial_emitter = Arc::new(RecordingEmitter::default());
        let swapped_emitter = Arc::new(RecordingEmitter::default());
        let runtime = AppRuntime::with_setup(
            deps,
            setup_ports,
            test_storage_paths(),
            initial_emitter.clone(),
        );

        runtime.set_event_emitter(swapped_emitter.clone());
        runtime
            .event_emitter()
            .emit(HostEvent::Clipboard(
                ClipboardHostEvent::InboundSubscribeRecovered {
                    recovered_after_attempts: 2,
                },
            ))
            .expect("emit through swapped emitter");

        assert!(initial_emitter.events.lock().unwrap().is_empty());
        let swapped_events = swapped_emitter.events.lock().unwrap();
        assert_eq!(swapped_events.len(), 1);
        match &swapped_events[0] {
            HostEvent::Clipboard(ClipboardHostEvent::InboundSubscribeRecovered {
                recovered_after_attempts,
            }) => assert_eq!(*recovered_after_attempts, 2),
            other => panic!("expected recovered event on swapped emitter, got {other:?}"),
        }
    }
}

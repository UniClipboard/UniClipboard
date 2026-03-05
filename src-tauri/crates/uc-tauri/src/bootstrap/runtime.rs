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

use async_trait::async_trait;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;
use tracing::Instrument;
use uc_app::{
    usecases::{
        space_access::{
            DefaultSpaceAccessCryptoFactory, HmacProofAdapter, SpaceAccessNetworkAdapter,
            SpaceAccessOrchestrator, SpaceAccessPersistenceAdapter,
        },
        PairingConfig, PairingOrchestrator, SetupOrchestrator,
    },
    App, AppDeps,
};
use uc_core::config::AppConfig;
use uc_core::network::DiscoveredPeer;
use uc_core::ports::space::SpaceAccessTransportPort;
use uc_core::ports::{ClipboardChangeHandler, DiscoveryPort, PeerDirectoryPort, TimerPort};
use uc_core::{ClipboardChangeOrigin, SystemClipboardSnapshot};
use uc_infra::time::Timer;

use crate::events::ClipboardEvent;

/// Application runtime with dependencies.
///
/// This struct holds all application dependencies and provides
/// access to use cases through the `usecases()` method.
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
    /// Application dependencies
    pub deps: AppDeps,
    /// Tauri AppHandle for emitting events (optional, set after Tauri setup)
    /// Uses RwLock for interior mutability since Arc<AppRuntime> is shared
    app_handle: Arc<std::sync::RwLock<Option<tauri::AppHandle>>>,
    /// Shared lifecycle status port – stored here so that every call to
    /// `usecases().app_lifecycle_coordinator()` shares the same state.
    lifecycle_status: Arc<dyn uc_app::usecases::LifecycleStatusPort>,
    /// Cached setup orchestrator – shared across all Tauri commands so that
    /// the in-memory setup state machine is not reset on every call.
    ///
    /// 缓存的 Setup 编排器 – 在所有 Tauri 命令间共享，
    /// 避免每次调用都重置内存中的 Setup 状态机。
    setup_orchestrator: Arc<SetupOrchestrator>,
    clipboard_integration_mode: uc_app::usecases::clipboard::ClipboardIntegrationMode,
}

/// Setup wiring dependencies for runtime-level orchestrators.
pub struct SetupRuntimePorts {
    pairing_orchestrator: Arc<PairingOrchestrator>,
    space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    discovery_port: Arc<dyn DiscoveryPort>,
}

impl SetupRuntimePorts {
    /// Create a new SetupRuntimePorts bundle.
    pub fn new(
        pairing_orchestrator: Arc<PairingOrchestrator>,
        space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
        discovery_port: Arc<dyn DiscoveryPort>,
    ) -> Self {
        Self {
            pairing_orchestrator,
            space_access_orchestrator,
            discovery_port,
        }
    }

    /// Create a bundle using the peer-directory port as the discovery adapter.
    pub fn from_network(
        pairing_orchestrator: Arc<PairingOrchestrator>,
        space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
        peers: Arc<dyn PeerDirectoryPort>,
    ) -> Self {
        Self::new(
            pairing_orchestrator,
            space_access_orchestrator,
            Arc::new(NetworkDiscoveryPort { peers }),
        )
    }

    fn placeholder(deps: &AppDeps) -> Self {
        Self::new(
            AppRuntime::placeholder_pairing_orchestrator(deps),
            Arc::new(SpaceAccessOrchestrator::new()),
            Arc::new(EmptyDiscoveryPort),
        )
    }
}

impl AppRuntime {
    /// Create a new AppRuntime from dependencies.
    /// 从依赖创建新的 AppRuntime。
    pub fn new(deps: AppDeps) -> Self {
        let setup_ports = SetupRuntimePorts::placeholder(&deps);
        Self::with_setup(deps, setup_ports)
    }

    /// Create a new AppRuntime with explicit setup orchestrator dependencies.
    pub fn with_setup(deps: AppDeps, setup_ports: SetupRuntimePorts) -> Self {
        let lifecycle_status: Arc<dyn uc_app::usecases::LifecycleStatusPort> =
            Arc::new(crate::adapters::lifecycle::InMemoryLifecycleStatus::new());
        let app_handle = Arc::new(std::sync::RwLock::new(None));
        let clipboard_integration_mode = super::resolve_clipboard_integration_mode();

        let setup_orchestrator = Self::build_setup_orchestrator(
            &deps,
            &lifecycle_status,
            &setup_ports,
            app_handle.clone(),
            clipboard_integration_mode,
        );

        Self {
            deps,
            app_handle,
            lifecycle_status,
            setup_orchestrator,
            clipboard_integration_mode,
        }
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

    /// Get use cases accessor.
    /// 获取用例访问器。
    pub fn usecases(&self) -> UseCases<'_> {
        UseCases::new(self)
    }

    pub fn clipboard_integration_mode(
        &self,
    ) -> uc_app::usecases::clipboard::ClipboardIntegrationMode {
        self.clipboard_integration_mode
    }

    fn build_setup_orchestrator(
        deps: &AppDeps,
        lifecycle_status: &Arc<dyn uc_app::usecases::LifecycleStatusPort>,
        setup_ports: &SetupRuntimePorts,
        app_handle: Arc<std::sync::RwLock<Option<tauri::AppHandle>>>,
        clipboard_integration_mode: uc_app::usecases::clipboard::ClipboardIntegrationMode,
    ) -> Arc<SetupOrchestrator> {
        let initialize_encryption = Arc::new(uc_app::usecases::InitializeEncryption::from_ports(
            deps.encryption.clone(),
            deps.key_material.clone(),
            deps.key_scope.clone(),
            deps.encryption_state.clone(),
            deps.encryption_session.clone(),
        ));
        let mark_setup_complete = Arc::new(uc_app::usecases::MarkSetupComplete::from_ports(
            deps.setup_status.clone(),
        ));

        let announcer = Arc::new(crate::adapters::lifecycle::DeviceNameAnnouncer::new(
            deps.network_ports.peers.clone(),
            deps.settings.clone(),
        ));
        let start_watcher = Arc::new(uc_app::usecases::StartClipboardWatcher::from_port(
            deps.watcher_control.clone(),
            clipboard_integration_mode,
        ));
        let start_network = Arc::new(uc_app::usecases::StartNetworkAfterUnlock::from_port(
            deps.network_control.clone(),
        ));
        let app_lifecycle = Arc::new(uc_app::usecases::AppLifecycleCoordinator::from_deps(
            uc_app::usecases::AppLifecycleCoordinatorDeps {
                watcher: start_watcher,
                network: start_network,
                announcer: Some(announcer),
                emitter: Arc::new(crate::adapters::lifecycle::TauriSessionReadyEmitter::new(
                    app_handle.clone(),
                )),
                status: lifecycle_status.clone(),
                lifecycle_emitter: Arc::new(
                    crate::adapters::lifecycle::LoggingLifecycleEventEmitter,
                ),
            },
        ));
        let crypto_factory = Arc::new(DefaultSpaceAccessCryptoFactory::new(
            deps.encryption.clone(),
            deps.key_material.clone(),
            deps.key_scope.clone(),
            deps.encryption_state.clone(),
            deps.encryption_session.clone(),
        ));
        let transport_port: Arc<Mutex<dyn SpaceAccessTransportPort>> =
            Arc::new(Mutex::new(SpaceAccessNetworkAdapter::new(
                deps.network_ports.pairing.clone(),
                setup_ports.space_access_orchestrator.context(),
            )));
        let proof_port: Arc<dyn uc_core::ports::space::ProofPort> = Arc::new(
            HmacProofAdapter::new_with_encryption_session(deps.encryption_session.clone()),
        );
        let timer_port: Arc<Mutex<dyn TimerPort>> = Arc::new(Mutex::new(Timer::new()));
        let persistence_port = Arc::new(Mutex::new(SpaceAccessPersistenceAdapter::new(
            deps.encryption_state.clone(),
            deps.paired_device_repo.clone(),
        )));
        let setup_event_port = Arc::new(crate::bootstrap::wiring::TauriSetupEventPort::new(
            app_handle,
        ));

        Arc::new(SetupOrchestrator::new(
            initialize_encryption,
            mark_setup_complete,
            deps.setup_status.clone(),
            app_lifecycle,
            setup_ports.pairing_orchestrator.clone(),
            setup_event_port,
            setup_ports.space_access_orchestrator.clone(),
            setup_ports.discovery_port.clone(),
            deps.network_control.clone(),
            crypto_factory,
            deps.network_ports.pairing.clone(),
            transport_port,
            proof_port,
            timer_port,
            persistence_port,
        ))
    }

    fn placeholder_pairing_orchestrator(deps: &AppDeps) -> Arc<PairingOrchestrator> {
        let (orchestrator, _rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            deps.paired_device_repo.clone(),
            "setup-placeholder-device".to_string(),
            "setup-placeholder-device-id".to_string(),
            "setup-placeholder-peer-id".to_string(),
            vec![],
        );
        Arc::new(orchestrator)
    }
}

struct NetworkDiscoveryPort {
    peers: Arc<dyn PeerDirectoryPort>,
}

#[async_trait]
impl DiscoveryPort for NetworkDiscoveryPort {
    async fn list_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
        self.peers.get_discovered_peers().await
    }
}

struct EmptyDiscoveryPort;

#[async_trait]
impl DiscoveryPort for EmptyDiscoveryPort {
    async fn list_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
        Ok(Vec::new())
    }
}

/// Use cases accessor for AppRuntime.
///
/// This struct provides methods to access all use cases with their dependencies
/// pre-wired from the AppRuntime's deps.
///
/// ## Architecture / 架构
///
/// The `UseCases` accessor serves as a factory for creating use case instances.
/// Each method returns a use case with its dependencies already wired from `AppDeps`.
///
/// `UseCases` 访问器作为用例实例的工厂。每个方法返回一个用例，其依赖已从
/// `AppDeps` 连接。
///
/// ## Design Pattern / 设计模式
///
/// This implements the Factory pattern for use cases:
/// - Commands don't need to know which ports a use case needs
/// - All port-to-use-case wiring is centralized in one place
/// - Use cases remain pure (no dependency on AppDeps)
///
/// 这为用例实现了工厂模式：
/// - 命令不需要知道用例需要哪些端口
/// - 所有端口到用例的连接集中在一个地方
/// - 用例保持纯净（不依赖 AppDeps）
///
/// ## Limitations / 限制
///
/// Currently, not all use cases are accessible through this accessor due to
/// architectural constraints with trait objects. Use cases that require
/// generic type parameters cannot be instantiated with `Arc<dyn Trait>`.
///
/// 目前，由于 trait 对象的架构限制，并非所有用例都可以通过此访问器访问。
/// 需要泛型类型参数的用例无法使用 `Arc<dyn Trait>` 实例化。
///
/// AppRuntime 的用例访问器。
pub struct UseCases<'a> {
    runtime: &'a AppRuntime,
}

impl<'a> UseCases<'a> {
    /// Create a new UseCases accessor from AppRuntime.
    /// 从 AppRuntime 创建新的 UseCases 访问器。
    pub fn new(runtime: &'a AppRuntime) -> Self {
        Self { runtime }
    }

    /// Accesses the use case for querying clipboard history.
    ///
    /// # Examples
    ///
    /// ```
    /// # use uc_tauri::bootstrap::AppRuntime;
    /// # use tauri::State;
    /// # async fn example(runtime: State<'_, AppRuntime>) -> Result<(), String> {
    /// let uc = runtime.usecases().list_clipboard_entries();
    /// let entries = uc.execute(50, 0).await.map_err(|e| e.to_string())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn list_clipboard_entries(&self) -> uc_app::usecases::ListClipboardEntries {
        uc_app::usecases::ListClipboardEntries::from_arc(
            self.runtime.deps.clipboard_entry_repo.clone(),
        )
    }

    /// Create a `DeleteClipboardEntry` use case wired with this runtime's clipboard and selection repositories.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use uc_tauri::bootstrap::AppRuntime;
    /// # use tauri::State;
    /// # use uc_core::ids::EntryId;
    /// # async fn example(runtime: State<'_, AppRuntime>, entry_id: &EntryId) -> Result<(), String> {
    /// let uc = runtime.usecases().delete_clipboard_entry();
    /// uc.execute(entry_id).await.map_err(|e| e.to_string())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn delete_clipboard_entry(&self) -> uc_app::usecases::DeleteClipboardEntry {
        uc_app::usecases::DeleteClipboardEntry::from_ports(
            self.runtime.deps.clipboard_entry_repo.clone(),
            self.runtime.deps.selection_repo.clone(),
            self.runtime.deps.clipboard_event_repo.clone(),
        )
    }

    /// Get the GetEntryDetail use case for fetching full clipboard entry content.
    ///
    /// 获取 GetEntryDetail 用例以获取完整剪贴板条目内容。
    ///
    /// ## Example / 示例
    ///
    /// ```rust,no_run
    /// # use uc_tauri::bootstrap::AppRuntime;
    /// # use tauri::State;
    /// # use uc_core::ids::EntryId;
    /// # async fn example(runtime: State<'_, AppRuntime>, entry_id: &EntryId) -> Result<uc_app::usecases::clipboard::get_entry_detail::EntryDetailResult, String> {
    /// let uc = runtime.usecases().get_entry_detail();
    /// let detail = uc.execute(entry_id).await.map_err(|e| e.to_string())?;
    /// # Ok(detail)
    /// # }
    /// ```
    pub fn get_entry_detail(
        &self,
    ) -> uc_app::usecases::clipboard::get_entry_detail::GetEntryDetailUseCase {
        uc_app::usecases::clipboard::get_entry_detail::GetEntryDetailUseCase::new(
            self.runtime.deps.clipboard_entry_repo.clone(),
            self.runtime.deps.selection_repo.clone(),
            self.runtime.deps.representation_repo.clone(),
            self.runtime.deps.blob_store.clone(),
        )
    }

    /// Get the GetEntryResource use case for fetching clipboard resource metadata.
    ///
    /// 获取 GetEntryResource 用例以获取剪贴板资源元信息。
    pub fn get_entry_resource(
        &self,
    ) -> uc_app::usecases::clipboard::get_entry_resource::GetEntryResourceUseCase {
        uc_app::usecases::clipboard::get_entry_resource::GetEntryResourceUseCase::new(
            self.runtime.deps.clipboard_entry_repo.clone(),
            self.runtime.deps.selection_repo.clone(),
            self.runtime.deps.representation_repo.clone(),
        )
    }

    /// Resolve blob resource content by blob id.
    ///
    /// 通过 blob id 解析资源内容。
    pub fn resolve_blob_resource(
        &self,
    ) -> uc_app::usecases::clipboard::resolve_blob_resource::ResolveBlobResourceUseCase {
        uc_app::usecases::clipboard::resolve_blob_resource::ResolveBlobResourceUseCase::new(
            self.runtime.deps.representation_repo.clone(),
            self.runtime.deps.blob_store.clone(),
        )
    }

    /// List paired devices from repository.
    ///
    /// 列出已配对设备。
    pub fn list_paired_devices(&self) -> uc_app::usecases::ListPairedDevices {
        uc_app::usecases::ListPairedDevices::new(self.runtime.deps.paired_device_repo.clone())
    }

    /// Get local peer id from network port.
    ///
    /// 获取本地 Peer ID。
    pub fn get_local_peer_id(&self) -> uc_app::usecases::GetLocalPeerId {
        uc_app::usecases::GetLocalPeerId::new(self.runtime.deps.network_ports.peers.clone())
    }

    /// Get local device info (peer id + device name).
    ///
    /// 获取本地设备信息（Peer ID + 设备名称）。
    pub fn get_local_device_info(&self) -> uc_app::usecases::GetLocalDeviceInfo {
        uc_app::usecases::GetLocalDeviceInfo::new(
            self.runtime.deps.network_ports.peers.clone(),
            self.runtime.deps.settings.clone(),
        )
    }

    /// Announce local device name through the network port.
    ///
    /// 通过网络端口广播本地设备名称。
    pub fn announce_device_name(&self) -> uc_app::usecases::AnnounceDeviceName {
        uc_app::usecases::AnnounceDeviceName::new(self.runtime.deps.network_ports.peers.clone())
    }

    /// List discovered peers from network.
    ///
    /// 列出已发现的对等端。
    pub fn list_discovered_peers(&self) -> uc_app::usecases::ListDiscoveredPeers {
        uc_app::usecases::ListDiscoveredPeers::new(self.runtime.deps.network_ports.peers.clone())
    }

    /// List connected peers from network.
    ///
    /// 列出已连接的对等端。
    pub fn list_connected_peers(&self) -> uc_app::usecases::ListConnectedPeers {
        uc_app::usecases::ListConnectedPeers::new(self.runtime.deps.network_ports.peers.clone())
    }

    /// Update pairing state for a peer.
    ///
    /// 更新对等端配对状态。
    pub fn set_pairing_state(&self) -> uc_app::usecases::SetPairingState {
        uc_app::usecases::SetPairingState::new(self.runtime.deps.paired_device_repo.clone())
    }

    /// Unpair device and remove from repository.
    ///
    /// 取消配对并从存储中删除。
    pub fn unpair_device(&self) -> uc_app::usecases::UnpairDevice {
        uc_app::usecases::UnpairDevice::new(
            self.runtime.deps.network_ports.pairing.clone(),
            self.runtime.deps.paired_device_repo.clone(),
        )
    }

    /// Resolve thumbnail resource content by representation id.
    ///
    /// 通过表示 id 解析缩略图资源内容。
    pub fn resolve_thumbnail_resource(
        &self,
    ) -> uc_app::usecases::clipboard::resolve_thumbnail_resource::ResolveThumbnailResourceUseCase
    {
        uc_app::usecases::clipboard::resolve_thumbnail_resource::ResolveThumbnailResourceUseCase::new(
            self.runtime.deps.thumbnail_repo.clone(),
            self.runtime.deps.blob_store.clone(),
        )
    }

    /// Security use cases / 安全用例
    ///
    /// Get the InitializeEncryption use case for setting up encryption.
    ///
    /// 获取 InitializeEncryption 用例以设置加密。
    ///
    /// ## Example / 示例
    ///
    /// ```rust,no_run
    /// # use uc_tauri::bootstrap::AppRuntime;
    /// # use tauri::State;
    /// # async fn example(runtime: State<'_, AppRuntime>) -> Result<(), String> {
    /// let uc = runtime.usecases().initialize_encryption();
    /// uc.execute(uc_core::security::model::Passphrase("my_pass".to_string()))
    ///     .await
    ///     .map_err(|e| e.to_string())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn initialize_encryption(&self) -> uc_app::usecases::InitializeEncryption {
        uc_app::usecases::InitializeEncryption::from_ports(
            self.runtime.deps.encryption.clone(),
            self.runtime.deps.key_material.clone(),
            self.runtime.deps.key_scope.clone(),
            self.runtime.deps.encryption_state.clone(),
            self.runtime.deps.encryption_session.clone(),
        )
    }

    /// Get the AutoUnlockEncryptionSession use case for startup unlock.
    pub fn auto_unlock_encryption_session(&self) -> uc_app::usecases::AutoUnlockEncryptionSession {
        uc_app::usecases::AutoUnlockEncryptionSession::from_ports(
            self.runtime.deps.encryption_state.clone(),
            self.runtime.deps.key_scope.clone(),
            self.runtime.deps.key_material.clone(),
            self.runtime.deps.encryption.clone(),
            self.runtime.deps.encryption_session.clone(),
        )
    }

    pub fn setup_orchestrator(&self) -> Arc<SetupOrchestrator> {
        self.runtime.setup_orchestrator.clone()
    }

    /// Settings use cases / 设置用例
    ///
    /// Get application settings
    ///
    /// ## Example / 示例
    ///
    /// ```rust,no_run
    /// # use uc_tauri::bootstrap::AppRuntime;
    /// # use tauri::State;
    /// # async fn example(runtime: State<'_, AppRuntime>) -> Result<uc_core::settings::model::Settings, String> {
    /// let uc = runtime.usecases().get_settings();
    /// let settings = uc.execute().await.map_err(|e| e.to_string())?;
    /// # Ok(settings)
    /// # }
    /// ```
    pub fn get_settings(&self) -> uc_app::usecases::GetSettings {
        uc_app::usecases::GetSettings::new(self.runtime.deps.settings.clone())
    }

    /// Update application settings
    ///
    /// ## Example / 示例
    ///
    /// ```rust,no_run
    /// # use uc_tauri::bootstrap::AppRuntime;
    /// # use tauri::State;
    /// # use uc_core::settings::model::Settings;
    /// # async fn example(runtime: State<'_, AppRuntime>, settings: Settings) -> Result<(), String> {
    /// let uc = runtime.usecases().update_settings();
    /// uc.execute(settings).await.map_err(|e| e.to_string())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn update_settings(&self) -> uc_app::usecases::UpdateSettings {
        uc_app::usecases::UpdateSettings::new(self.runtime.deps.settings.clone())
    }

    /// Start the clipboard watcher
    ///
    /// ## Example / 示例
    ///
    /// ```rust,no_run
    /// # use uc_tauri::bootstrap::AppRuntime;
    /// # use tauri::State;
    /// # async fn example(runtime: State<'_, AppRuntime>) -> Result<(), String> {
    /// let uc = runtime.usecases().start_clipboard_watcher();
    /// uc.execute().await.map_err(|e| e.to_string())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn start_clipboard_watcher(&self) -> uc_app::usecases::StartClipboardWatcher {
        uc_app::usecases::StartClipboardWatcher::from_port(
            self.runtime.deps.watcher_control.clone(),
            self.runtime.clipboard_integration_mode,
        )
    }

    /// Start the network runtime
    pub fn start_network(&self) -> uc_app::usecases::StartNetwork {
        uc_app::usecases::StartNetwork::from_port(self.runtime.deps.network_control.clone())
    }

    /// Start the network runtime after unlock
    pub fn start_network_after_unlock(&self) -> uc_app::usecases::StartNetworkAfterUnlock {
        uc_app::usecases::StartNetworkAfterUnlock::from_port(
            self.runtime.deps.network_control.clone(),
        )
    }

    /// List clipboard entry projections (with cross-repo aggregation)
    ///
    /// ## Example / 示例
    ///
    /// ```rust,no_run
    /// # use uc_tauri::bootstrap::AppRuntime;
    /// # use tauri::State;
    /// # async fn example(runtime: State<'_, AppRuntime>) -> Result<Vec<uc_app::usecases::EntryProjectionDto>, String> {
    /// let uc = runtime.usecases().list_entry_projections();
    /// let projections = uc.execute(50, 0).await.map_err(|e| e.to_string())?;
    /// # Ok(projections)
    /// # }
    /// ```
    pub fn list_entry_projections(&self) -> uc_app::usecases::ListClipboardEntryProjections {
        uc_app::usecases::ListClipboardEntryProjections::new(
            self.runtime.deps.clipboard_entry_repo.clone(),
            self.runtime.deps.selection_repo.clone(),
            self.runtime.deps.representation_repo.clone(),
            self.runtime.deps.thumbnail_repo.clone(),
        )
    }

    /// Restore clipboard selection to system clipboard.
    ///
    /// 将历史剪贴板条目恢复到系统剪贴板。
    pub fn restore_clipboard_selection(
        &self,
    ) -> uc_app::usecases::clipboard::restore_clipboard_selection::RestoreClipboardSelectionUseCase
    {
        uc_app::usecases::clipboard::restore_clipboard_selection::RestoreClipboardSelectionUseCase::new(
            self.runtime.deps.clipboard_entry_repo.clone(),
            self.runtime.deps.system_clipboard.clone(),
            self.runtime.deps.selection_repo.clone(),
            self.runtime.deps.representation_repo.clone(),
            self.runtime.deps.blob_store.clone(),
            self.runtime.deps.clipboard_change_origin.clone(),
            self.runtime.clipboard_integration_mode,
        )
    }

    /// Touch clipboard entry active time.
    ///
    /// 更新剪贴板条目活跃时间。
    pub fn touch_clipboard_entry(
        &self,
    ) -> uc_app::usecases::clipboard::touch_clipboard_entry::TouchClipboardEntryUseCase {
        uc_app::usecases::clipboard::touch_clipboard_entry::TouchClipboardEntryUseCase::new(
            self.runtime.deps.clipboard_entry_repo.clone(),
            self.runtime.deps.clock.clone(),
        )
    }

    pub fn sync_inbound_clipboard(
        &self,
    ) -> uc_app::usecases::clipboard::sync_inbound::SyncInboundClipboardUseCase {
        uc_app::usecases::clipboard::sync_inbound::SyncInboundClipboardUseCase::with_capture_dependencies(
            self.runtime.clipboard_integration_mode,
            self.runtime.deps.system_clipboard.clone(),
            self.runtime.deps.clipboard_change_origin.clone(),
            self.runtime.deps.encryption_session.clone(),
            self.runtime.deps.encryption.clone(),
            self.runtime.deps.device_identity.clone(),
            Arc::new(uc_infra::clipboard::TransferPayloadDecryptorAdapter),
            self.runtime.deps.clipboard_entry_repo.clone(),
            self.runtime.deps.clipboard_event_repo.clone(),
            self.runtime.deps.representation_policy.clone(),
            self.runtime.deps.representation_normalizer.clone(),
            self.runtime.deps.representation_cache.clone(),
            self.runtime.deps.spool_queue.clone(),
        )
    }

    pub fn sync_outbound_clipboard(
        &self,
    ) -> uc_app::usecases::clipboard::sync_outbound::SyncOutboundClipboardUseCase {
        uc_app::usecases::clipboard::sync_outbound::SyncOutboundClipboardUseCase::new(
            self.runtime.deps.system_clipboard.clone(),
            self.runtime.deps.network_ports.clipboard.clone(),
            self.runtime.deps.network_ports.peers.clone(),
            self.runtime.deps.encryption_session.clone(),
            self.runtime.deps.device_identity.clone(),
            self.runtime.deps.settings.clone(),
            Arc::new(uc_infra::clipboard::TransferPayloadEncryptorAdapter),
        )
    }

    /// Get the lifecycle status port directly (for status queries).
    ///
    /// 直接获取生命周期状态端口（用于状态查询）。
    pub fn get_lifecycle_status(&self) -> Arc<dyn uc_app::usecases::LifecycleStatusPort> {
        self.runtime.lifecycle_status.clone()
    }

    /// Get the AppLifecycleCoordinator use case for orchestrating
    /// clipboard watcher, network startup, and session readiness.
    ///
    /// 获取 AppLifecycleCoordinator 用例以编排剪贴板监视器、网络启动和会话就绪。
    pub fn app_lifecycle_coordinator(&self) -> uc_app::usecases::AppLifecycleCoordinator {
        let announcer = Arc::new(crate::adapters::lifecycle::DeviceNameAnnouncer::new(
            self.runtime.deps.network_ports.peers.clone(),
            self.runtime.deps.settings.clone(),
        ));
        uc_app::usecases::AppLifecycleCoordinator::from_deps(
            uc_app::usecases::AppLifecycleCoordinatorDeps {
                watcher: Arc::new(self.start_clipboard_watcher()),
                network: Arc::new(self.start_network_after_unlock()),
                announcer: Some(announcer),
                emitter: Arc::new(crate::adapters::lifecycle::TauriSessionReadyEmitter::new(
                    self.runtime.app_handle.clone(),
                )),
                status: self.runtime.lifecycle_status.clone(),
                lifecycle_emitter: Arc::new(
                    crate::adapters::lifecycle::LoggingLifecycleEventEmitter,
                ),
            },
        )
    }

    // NOTE: Other use case methods will be added as the use case design evolves
    // to support trait object instantiation. Currently, use cases with generic
    // type parameters cannot be instantiated through this accessor.
    //
    // 注意：随着用例设计的演进，将添加其他用例方法以支持 trait 对象实例化。
    // 目前，具有泛型类型参数的用例无法通过此访问器实例化。
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

/// Implement ClipboardChangeHandler for AppRuntime.
///
/// This allows AppRuntime to be used as a callback for clipboard change events
/// from the platform layer.
#[async_trait::async_trait]
impl ClipboardChangeHandler for AppRuntime {
    #[tracing::instrument(name = "runtime.on_clipboard_changed", skip(self, snapshot))]
    async fn on_clipboard_changed(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
        let origin = self
            .deps
            .clipboard_change_origin
            .consume_origin_or_default(ClipboardChangeOrigin::LocalCapture)
            .await;
        let outbound_snapshot = snapshot.clone();

        // Create CaptureClipboardUseCase with dependencies
        let usecase = uc_app::usecases::internal::capture_clipboard::CaptureClipboardUseCase::new(
            self.deps.clipboard_entry_repo.clone(),
            self.deps.clipboard_event_repo.clone(),
            self.deps.representation_policy.clone(),
            self.deps.representation_normalizer.clone(),
            self.deps.device_identity.clone(),
            self.deps.representation_cache.clone(),
            self.deps.spool_queue.clone(),
        );

        // Execute capture with the provided snapshot
        match usecase.execute_with_origin(snapshot, origin).await {
            Ok(Some(entry_id)) => {
                tracing::debug!(
                    entry_id = %entry_id,
                    "Successfully captured clipboard"
                );

                // Emit event to frontend if AppHandle is available
                let app_handle_guard = self.app_handle.read().unwrap_or_else(|poisoned| {
                    tracing::error!(
                        "RwLock poisoned in on_clipboard_changed, recovering from poisoned state"
                    );
                    poisoned.into_inner()
                });
                if let Some(app) = app_handle_guard.as_ref() {
                    let event = ClipboardEvent::NewContent {
                        entry_id: entry_id.to_string(),
                        preview: "New clipboard content".to_string(),
                    };

                    if let Err(e) = app.emit("clipboard://event", event) {
                        tracing::warn!("Failed to emit clipboard event to frontend: {}", e);
                    } else {
                        tracing::debug!("Successfully emitted clipboard://event to frontend");
                    }
                } else {
                    tracing::debug!("AppHandle not available, skipping event emission");
                }
                drop(app_handle_guard);

                let outbound_sync_uc = self.usecases().sync_outbound_clipboard();
                let parent_span = tracing::Span::current();
                tauri::async_runtime::spawn(
                    async move {
                        match tokio::task::spawn_blocking(move || {
                            outbound_sync_uc.execute(outbound_snapshot, origin)
                        })
                        .await
                        {
                            Ok(Ok(())) => {
                                tracing::info!("Outbound clipboard sync completed");
                            }
                            Ok(Err(err)) => {
                                tracing::warn!(error = %err, "Outbound clipboard sync failed");
                            }
                            Err(err) => {
                                tracing::warn!(error = %err, "Outbound clipboard sync task join failed");
                            }
                        }
                    }
                    .instrument(parent_span),
                );

                Ok(())
            }
            Ok(None) => {
                tracing::debug!(origin = ?origin, "Clipboard capture skipped for current origin");
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to capture clipboard: {:?}", e);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::noop_network_ports;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use uc_core::clipboard::PolicyError;
    use uc_core::ports::clipboard::{RepresentationCachePort, SpoolQueuePort, SpoolRequest};
    use uc_core::ports::security::encryption_state::EncryptionStatePort;
    use uc_core::ports::security::key_scope::KeyScopePort;
    use uc_core::ports::*;
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionError, KdfParams, Kek, KeyScope, KeySlot,
        MasterKey, Passphrase,
    };
    use uc_core::security::state::{EncryptionState, EncryptionStateError};
    use uc_core::PeerId;
    use uc_core::{Blob, BlobId, ClipboardChangeOrigin, ContentHash, DeviceId};
    use uc_infra::clipboard::InMemoryClipboardChangeOrigin;

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
    impl WatcherControlPort for NoopPort {
        async fn start_watcher(&self) -> Result<(), WatcherControlError> {
            Ok(())
        }

        async fn stop_watcher(&self) -> Result<(), WatcherControlError> {
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
            _encrypted_data: Vec<u8>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn broadcast_clipboard(&self, _encrypted_data: Vec<u8>) -> anyhow::Result<()> {
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
                Some(_data.len() as i64),
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

    #[tokio::test]
    async fn runtime_consumes_origin() {
        let save_calls = Arc::new(AtomicUsize::new(0));
        let insert_calls = Arc::new(AtomicUsize::new(0));
        let select_calls = Arc::new(AtomicUsize::new(0));
        let normalize_calls = Arc::new(AtomicUsize::new(0));
        let cache_put_calls = Arc::new(AtomicUsize::new(0));
        let enqueue_calls = Arc::new(AtomicUsize::new(0));

        let origin_port = Arc::new(InMemoryClipboardChangeOrigin::new());
        origin_port
            .set_next_origin(ClipboardChangeOrigin::LocalRestore, Duration::from_secs(1))
            .await;

        let (worker_tx, _worker_rx) = mpsc::channel(1);

        let deps = AppDeps {
            clipboard: Arc::new(NoopClipboard),
            system_clipboard: Arc::new(NoopClipboard),
            clipboard_entry_repo: Arc::new(MockEntryRepository {
                save_calls: save_calls.clone(),
            }),
            clipboard_event_repo: Arc::new(MockEventWriter {
                insert_calls: insert_calls.clone(),
            }),
            representation_repo: Arc::new(NoopPort),
            representation_normalizer: Arc::new(MockNormalizer {
                normalize_calls: normalize_calls.clone(),
            }),
            selection_repo: Arc::new(NoopPort),
            representation_policy: Arc::new(MockRepresentationPolicy {
                select_calls: select_calls.clone(),
            }),
            representation_cache: Arc::new(MockRepresentationCache {
                put_calls: cache_put_calls.clone(),
            }),
            spool_queue: Arc::new(MockSpoolQueue {
                enqueue_calls: enqueue_calls.clone(),
            }),
            worker_tx,
            encryption: Arc::new(NoopPort),
            encryption_session: Arc::new(NoopPort),
            encryption_state: Arc::new(NoopPort),
            key_scope: Arc::new(NoopPort),
            secure_storage: Arc::new(NoopPort),
            key_material: Arc::new(NoopPort),
            watcher_control: Arc::new(NoopPort),
            device_repo: Arc::new(NoopPort),
            device_identity: Arc::new(MockDeviceIdentity),
            paired_device_repo: Arc::new(NoopPort),
            network_ports: noop_network_ports(),
            network_control: Arc::new(NoopPort),
            setup_status: Arc::new(NoopPort),
            blob_store: Arc::new(NoopPort),
            blob_repository: Arc::new(NoopPort),
            blob_writer: Arc::new(NoopPort),
            thumbnail_repo: Arc::new(NoopPort),
            thumbnail_generator: Arc::new(NoopPort),
            settings: Arc::new(NoopPort),
            ui_port: Arc::new(NoopPort),
            autostart: Arc::new(NoopPort),
            clock: Arc::new(NoopPort),
            hash: Arc::new(NoopPort),
            clipboard_change_origin: origin_port,
        };

        let runtime = AppRuntime::new(deps);
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![],
        };

        let result = runtime.on_clipboard_changed(snapshot).await;
        assert!(result.is_ok());
        assert_eq!(save_calls.load(Ordering::SeqCst), 0);
        assert_eq!(insert_calls.load(Ordering::SeqCst), 0);
        assert_eq!(select_calls.load(Ordering::SeqCst), 0);
        assert_eq!(normalize_calls.load(Ordering::SeqCst), 0);
        assert_eq!(cache_put_calls.load(Ordering::SeqCst), 0);
        assert_eq!(enqueue_calls.load(Ordering::SeqCst), 0);
    }
}

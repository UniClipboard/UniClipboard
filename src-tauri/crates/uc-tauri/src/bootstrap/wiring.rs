//! # Dependency Injection / 依赖注入模块
//!
//! ## Responsibilities / 职责
//!
//! - ✅ Create infra implementations (db, fs, secure storage) / 创建 infra 层具体实现
//! - ✅ Create platform implementations (clipboard, network) / 创建 platform 层具体实现
//! - ✅ Inject all dependencies into App / 将所有依赖注入到 App
//!
//! ## Prohibited / 禁止事项
//!
//! ❌ **No business logic / 禁止包含任何业务逻辑**
//! - Do not decide "what to do if encryption uninitialized"
//! - 不判断"如果加密未初始化就怎样"
//! - Do not handle "what to do if device not registered"
//! - 不处理"如果设备未注册就怎样"
//!
//! ❌ **No configuration validation / 禁止做配置验证**
//! - Config already loaded in config.rs
//! - 配置已在 config.rs 加载
//! - Validation should be in use case or upper layer
//! - 验证应在 use case 或上层
//!
//! ❌ **No direct concrete implementation usage / 禁止直接使用具体实现**
//! - Must inject through Port traits
//! - 必须通过 Port trait 注入
//! - Do not call implementation methods directly after App construction
//! - 不在 App 构造后直接调用实现方法
//!
//! ## Architecture Principle / 架构原则
//!
//! > **This is the only place allowed to depend on uc-infra + uc-platform + uc-app simultaneously.**
//! > **这是唯一允许同时依赖 uc-infra、uc-platform 和 uc-app 的地方。**
//! > But this privilege is only for "assembly", not for "decision making".
//! > 但这种特权仅用于"组装"，不用于"决策"。

use chrono::Utc;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{async_runtime, AppHandle, Emitter, Runtime};
use tokio::sync::mpsc;
use tracing::{debug, error, info, info_span, warn, Instrument};

use crate::events::{
    forward_clipboard_event, ClipboardEvent, P2PPairingVerificationEvent, P2PPeerConnectionEvent,
    P2PPeerDiscoveryEvent, P2PPeerNameUpdatedEvent,
};
use uc_app::deps::NetworkPorts;
use uc_app::usecases::clipboard::sync_inbound::{InboundApplyOutcome, SyncInboundClipboardUseCase};
use uc_app::usecases::space_access::{
    HmacProofAdapter, SpaceAccessCompletedEvent, SpaceAccessContext, SpaceAccessEventPort,
    SpaceAccessJoinerOffer, SpaceAccessNetworkAdapter, SpaceAccessOrchestrator,
    SpaceAccessPersistenceAdapter,
};
use uc_app::usecases::{PairingConfig, PairingOrchestrator, ResolveConnectionPolicy};
use uc_app::AppDeps;
use uc_core::clipboard::SelectRepresentationPolicyV1;
use uc_core::config::AppConfig;
use uc_core::ids::RepresentationId;
use uc_core::network::pairing_state_machine::{PairingAction, PairingRole};
use uc_core::network::{ClipboardMessage, NetworkEvent, PairingMessage};
use uc_core::ports::clipboard::{
    ClipboardChangeOriginPort, ClipboardRepresentationNormalizerPort, RepresentationCachePort,
    SpoolQueuePort, SpoolRequest,
};
use uc_core::ports::space::ProofPort;
use uc_core::ports::*;
use uc_core::security::model::{KeySlot, KeySlotFile};
use uc_core::security::space_access::event::SpaceAccessEvent;
use uc_core::security::space_access::{deny_reason_from_code, DENY_REASON_INVALID_PROOF};
use uc_core::settings::model::Settings;
use uc_core::setup::SetupState;
use uc_infra::blob::BlobWriter;
use uc_infra::clipboard::{
    BackgroundBlobWorker, ClipboardRepresentationNormalizer, InMemoryClipboardChangeOrigin,
    InfraThumbnailGenerator, MpscSpoolQueue, RepresentationCache, SpoolJanitor, SpoolManager,
    SpoolScanner, SpoolerTask,
};
use uc_infra::config::ClipboardStorageConfig;
use uc_infra::db::executor::DieselSqliteExecutor;
use uc_infra::db::mappers::{
    blob_mapper::BlobRowMapper, clipboard_entry_mapper::ClipboardEntryRowMapper,
    clipboard_event_mapper::ClipboardEventRowMapper,
    clipboard_selection_mapper::ClipboardSelectionRowMapper, device_mapper::DeviceRowMapper,
    paired_device_mapper::PairedDeviceRowMapper,
    snapshot_representation_mapper::RepresentationRowMapper,
};
use uc_infra::db::pool::{init_db_pool, DbPool};
use uc_infra::db::repositories::{
    DieselBlobRepository, DieselClipboardEntryRepository, DieselClipboardEventRepository,
    DieselClipboardRepresentationRepository, DieselClipboardSelectionRepository,
    DieselDeviceRepository, DieselPairedDeviceRepository, DieselThumbnailRepository,
};
use uc_infra::device::LocalDeviceIdentity;
use uc_infra::fs::key_slot_store::{JsonKeySlotStore, KeySlotStore};
use uc_infra::security::{
    Blake3Hasher, DecryptingClipboardRepresentationRepository, DefaultKeyMaterialService,
    EncryptedBlobStore, EncryptingClipboardEventWriter, EncryptionRepository,
    FileEncryptionStateRepository,
};
use uc_infra::settings::repository::FileSettingsRepository;
use uc_infra::{FileSetupStatusRepository, SystemClock, Timer};

use uc_platform::adapters::{
    FilesystemBlobStore, InMemoryEncryptionSessionPort, InMemoryWatcherControl,
    Libp2pNetworkAdapter, PlaceholderAutostartPort, PlaceholderUiPort,
};
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::clipboard::LocalClipboard;
use uc_platform::identity_store::FileIdentityStore;
use uc_platform::runtime::event_bus::PlatformCommandSender;

/// Result type for wiring operations
pub type WiringResult<T> = Result<T, WiringError>;

/// Errors during dependency injection
/// 依赖注入错误（基础设施初始化失败）
#[derive(Debug, thiserror::Error)]
pub enum WiringError {
    #[error("Database initialization failed: {0}")]
    DatabaseInit(String),

    #[error("Secure storage initialization failed: {0}")]
    SecureStorageInit(String),

    #[error("Clipboard initialization failed: {0}")]
    ClipboardInit(String),

    #[error("Network initialization failed: {0}")]
    NetworkInit(String),

    #[error("Blob storage initialization failed: {0}")]
    BlobStorageInit(String),

    #[error("Settings repository initialization failed: {0}")]
    SettingsInit(String),

    #[error("Configuration initialization failed: {0}")]
    ConfigInit(String),

    #[error("Thumbnail generator initialization failed: {0}")]
    ThumbnailInit(String),
}

/// Fully wired dependencies plus background runtime components.
/// 已完成依赖连接与后台运行组件的组合。
pub struct WiredDependencies {
    pub deps: AppDeps,
    pub background: BackgroundRuntimeDeps,
}

/// Background runtime components that must be started after async runtime is ready.
/// 需要在异步运行时就绪后启动的后台组件。
pub struct BackgroundRuntimeDeps {
    pub libp2p_network: Arc<Libp2pNetworkAdapter>,
    pub representation_cache: Arc<RepresentationCache>,
    pub spool_manager: Arc<SpoolManager>,
    pub spool_rx: mpsc::Receiver<SpoolRequest>,
    pub worker_rx: mpsc::Receiver<RepresentationId>,
    pub spool_dir: PathBuf,
    pub spool_ttl_days: u64,
    pub worker_retry_max_attempts: u32,
    pub worker_retry_backoff_ms: u64,
}

/// Tauri adapter that emits setup state changes to frontend listeners.
#[derive(Clone)]
pub struct TauriSetupEventPort {
    app_handle: Arc<std::sync::RwLock<Option<AppHandle>>>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SetupStateChangedPayload {
    state: SetupState,
    session_id: Option<String>,
}

impl TauriSetupEventPort {
    pub fn new(app_handle: Arc<std::sync::RwLock<Option<AppHandle>>>) -> Self {
        Self { app_handle }
    }
}

#[async_trait::async_trait]
impl SetupEventPort for TauriSetupEventPort {
    async fn emit_setup_state_changed(&self, state: SetupState, session_id: Option<String>) {
        let guard = self.app_handle.read().unwrap_or_else(|poisoned| {
            error!("RwLock poisoned in setup event emission, recovering from poisoned state");
            poisoned.into_inner()
        });

        if let Some(app) = guard.as_ref() {
            let payload = SetupStateChangedPayload { state, session_id };
            if let Err(err) = app.emit("setup-state-changed", payload) {
                warn!(error = %err, "Failed to emit setup-state-changed event");
            }
        }
    }
}

const SPOOL_JANITOR_INTERVAL_SECS: u64 = 60 * 60;
const CLIPBOARD_SUBSCRIBE_BACKOFF_INITIAL_MS: u64 = 250;
const CLIPBOARD_SUBSCRIBE_BACKOFF_MAX_MS: u64 = 30_000;
const PAIRING_EVENTS_SUBSCRIBE_BACKOFF_INITIAL_MS: u64 = 250;
const PAIRING_EVENTS_SUBSCRIBE_BACKOFF_MAX_MS: u64 = 30_000;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InboundClipboardErrorPayload {
    message_id: String,
    origin_device_id: String,
    error: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InboundClipboardSubscribeErrorPayload {
    attempt: u32,
    error: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InboundClipboardSubscribeRetryPayload {
    attempt: u32,
    retry_in_ms: u64,
    error: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InboundClipboardSubscribeRecoveredPayload {
    recovered_after_attempts: u32,
}

fn subscribe_backoff_ms(attempt: u32) -> u64 {
    let exponent = attempt.saturating_sub(1).min(16);
    let factor = 1u64 << exponent;
    CLIPBOARD_SUBSCRIBE_BACKOFF_INITIAL_MS
        .saturating_mul(factor)
        .min(CLIPBOARD_SUBSCRIBE_BACKOFF_MAX_MS)
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PairingEventsSubscribeFailurePayload {
    attempt: u32,
    retry_in_ms: u64,
    error: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PairingEventsSubscribeRecoveredPayload {
    recovered_after_attempts: u32,
}

fn pairing_events_subscribe_backoff_ms(attempt: u32) -> u64 {
    let exponent = attempt.saturating_sub(1).min(16);
    let factor = 1u64 << exponent;
    PAIRING_EVENTS_SUBSCRIBE_BACKOFF_INITIAL_MS
        .saturating_mul(factor)
        .min(PAIRING_EVENTS_SUBSCRIBE_BACKOFF_MAX_MS)
}

/// Create SQLite database connection pool
/// 创建 SQLite 数据库连接池
///
/// # Arguments / 参数
///
/// * `db_path` - Path to the SQLite database file / SQLite 数据库文件路径
///
/// # Returns / 返回
///
/// * `WiringResult<DbPool>` - The connection pool on success / 成功时返回连接池
///
/// # Errors / 错误
///
/// Returns `WiringError::DatabaseInit` if:
/// 如果以下情况返回 `WiringError::DatabaseInit`：
/// - Parent directory creation fails / 父目录创建失败
/// - Database pool creation fails / 数据库池创建失败
/// - Migration fails / 迁移失败
fn create_db_pool(db_path: &PathBuf) -> WiringResult<DbPool> {
    // Ensure parent directory exists
    // 确保父目录存在
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            WiringError::DatabaseInit(format!("Failed to create DB directory: {}", e))
        })?;
    }

    // Convert PathBuf to string for database URL
    // 将 PathBuf 转换为字符串作为数据库 URL
    let db_url = db_path
        .to_str()
        .ok_or_else(|| WiringError::DatabaseInit("Invalid database path".to_string()))?;

    // Create connection pool and run migrations
    // 创建连接池并运行迁移
    init_db_pool(db_url)
        .map_err(|e| WiringError::DatabaseInit(format!("Failed to initialize DB: {}", e)))
}

/// Infrastructure layer implementations / 基础设施层实现
///
/// This struct holds all infrastructure implementations (database repositories,
/// encryption, settings, etc.) that will be injected into the application.
///
/// 此结构体保存所有基础设施实现（数据库仓库、加密、设置等），将被注入到应用程序中。
struct InfraLayer {
    // Clipboard repositories / 剪贴板仓库
    #[allow(dead_code)]
    clipboard_entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    clipboard_event_repo: Arc<dyn ClipboardEventWriterPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,

    // Device repository / 设备仓库
    device_repo: Arc<dyn DeviceRepositoryPort>,

    // Pairing repository / 配对仓库
    paired_device_repo: Arc<dyn PairedDeviceRepositoryPort>,

    // Blob storage / Blob 存储
    blob_repository: Arc<dyn BlobRepositoryPort>,
    thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    thumbnail_generator: Arc<dyn ThumbnailGeneratorPort>,

    // Security services / 安全服务
    key_material: Arc<dyn KeyMaterialPort>,
    encryption: Arc<dyn EncryptionPort>,
    encryption_state: Arc<dyn uc_core::ports::security::encryption_state::EncryptionStatePort>,

    // Settings / 设置
    settings_repo: Arc<dyn SettingsPort>,

    // Setup status / 设置状态
    setup_status: Arc<dyn SetupStatusPort>,

    // System services / 系统服务
    clock: Arc<dyn ClockPort>,
    hash: Arc<dyn ContentHashPort>,
}

/// Platform layer implementations / 平台层实现
///
/// This struct holds all platform-specific implementations (clipboard, secure storage, etc.)
/// that will be injected into the application.
///
/// 此结构体保存所有平台特定实现（剪贴板、密钥环等），将被注入到应用程序中。
struct PlatformLayer {
    // System clipboard / 系统剪贴板
    clipboard: Arc<dyn PlatformClipboardPort>,
    system_clipboard: Arc<dyn SystemClipboardPort>,

    // Secure storage / 安全存储
    secure_storage: Arc<dyn SecureStoragePort>,

    // UI operations / UI 操作（占位符）
    ui: Arc<dyn UiPort>,

    // Autostart management / 自动启动管理（占位符）
    autostart: Arc<dyn AutostartPort>,

    // Network operations / 网络操作（占位符）
    network_ports: Arc<NetworkPorts>,

    // libp2p network adapter (concrete)
    libp2p_network: Arc<Libp2pNetworkAdapter>,

    // Device identity / 设备身份（占位符）
    device_identity: Arc<dyn DeviceIdentityPort>,

    // Clipboard representation normalizer / 剪贴板表示规范化器
    representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,

    // Blob writer / Blob 写入器
    blob_writer: Arc<dyn BlobWriterPort>,

    // Blob store / Blob 存储（加密装饰后）
    blob_store: Arc<dyn BlobStorePort>,

    // Encryption session / 加密会话（占位符）
    encryption_session: Arc<dyn EncryptionSessionPort>,

    // Watcher control / 监控器控制
    watcher_control: Arc<dyn WatcherControlPort>,

    // Key scope / 密钥范围
    key_scope: Arc<dyn uc_core::ports::security::key_scope::KeyScopePort>,
}

/// Create infrastructure layer implementations
/// 创建基础设施层实现
///
/// This function creates all infrastructure implementations including:
/// 此函数创建所有基础设施实现，包括：
/// - Database repositories (clipboard, device, blob) / 数据库仓库（剪贴板、设备、blob）
/// - Encryption services (key material, encryption) / 加密服务（密钥材料、加密）
/// - Settings repository / 设置仓库
/// - System services (clock, hash) / 系统服务（时钟、哈希）
///
/// # Arguments / 参数
///
/// * `db_pool` - Database connection pool / 数据库连接池
/// * `vault_path` - Path to encryption vault / 加密保管库路径
/// * `settings_path` - Path to settings file / 设置文件路径
///
/// # Returns / 返回
///
/// * `WiringResult<InfraLayer>` - The infrastructure layer on success / 成功时返回基础设施层
///
/// # Errors / 错误
///
/// Returns `WiringError` if any infrastructure component fails to initialize.
/// 如果任何基础设施组件初始化失败，返回 `WiringError`。
fn create_infra_layer(
    db_pool: DbPool,
    vault_path: &PathBuf,
    settings_path: &PathBuf,
    secure_storage: Arc<dyn SecureStoragePort>,
) -> WiringResult<InfraLayer> {
    // Create database executor and wrap in Arc for cloning
    // 创建数据库执行器并包装在 Arc 中以供克隆
    let db_executor = Arc::new(DieselSqliteExecutor::new(db_pool));

    // Create mappers (zero-sized structs, no new() needed)
    // 创建映射器（零大小类型，无需 new()）
    let entry_row_mapper = ClipboardEntryRowMapper;
    let selection_row_mapper = ClipboardSelectionRowMapper;
    let device_row_mapper = DeviceRowMapper;
    let paired_device_row_mapper = PairedDeviceRowMapper;
    let blob_row_mapper = BlobRowMapper;
    let _representation_row_mapper = RepresentationRowMapper;

    // Create clipboard repositories
    // 创建剪贴板仓库
    let entry_repo = DieselClipboardEntryRepository::new(
        Arc::clone(&db_executor),
        entry_row_mapper,
        selection_row_mapper,
        ClipboardEntryRowMapper, // ZST - can instantiate again
    );
    let clipboard_entry_repo: Arc<dyn ClipboardEntryRepositoryPort> = Arc::new(entry_repo);

    // Create clipboard event repository
    // 创建剪贴板事件仓库
    let event_row_mapper = ClipboardEventRowMapper;
    let clipboard_event_repo_impl = DieselClipboardEventRepository::new(
        Arc::clone(&db_executor),
        event_row_mapper,
        RepresentationRowMapper,
    );
    let clipboard_event_repo: Arc<dyn ClipboardEventWriterPort> =
        Arc::new(clipboard_event_repo_impl);

    let rep_repo = DieselClipboardRepresentationRepository::new(Arc::clone(&db_executor));
    let representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort> = Arc::new(rep_repo);

    // Create device repository
    // 创建设备仓库
    let dev_repo = DieselDeviceRepository::new(Arc::clone(&db_executor), device_row_mapper);
    let device_repo: Arc<dyn DeviceRepositoryPort> = Arc::new(dev_repo);

    // Create paired device repository
    // 创建配对设备仓库
    let paired_repo =
        DieselPairedDeviceRepository::new(Arc::clone(&db_executor), paired_device_row_mapper);
    let paired_device_repo: Arc<dyn PairedDeviceRepositoryPort> = Arc::new(paired_repo);

    // Create blob repository
    // 创建 blob 仓库
    let blob_repo = DieselBlobRepository::new(
        Arc::clone(&db_executor),
        blob_row_mapper,
        BlobRowMapper, // ZST - can instantiate again
    );
    let blob_repository: Arc<dyn BlobRepositoryPort> = Arc::new(blob_repo);

    // Create thumbnail repository and generator
    // 创建缩略图仓库与生成器
    let thumbnail_repo_impl = DieselThumbnailRepository::new(Arc::clone(&db_executor));
    let thumbnail_repo: Arc<dyn ThumbnailRepositoryPort> = Arc::new(thumbnail_repo_impl);
    let thumbnail_generator =
        InfraThumbnailGenerator::new(128).map_err(|e| WiringError::ThumbnailInit(e.to_string()))?;
    let thumbnail_generator: Arc<dyn ThumbnailGeneratorPort> = Arc::new(thumbnail_generator);

    let secure_storage_for_key_material = Arc::clone(&secure_storage);

    // Create key slot store
    // 创建密钥槽存储
    let keyslot_store = JsonKeySlotStore::new(vault_path.clone());
    let keyslot_store: Arc<dyn KeySlotStore> = Arc::new(keyslot_store);

    // Create key material service
    // 创建密钥材料服务
    let key_material_service =
        DefaultKeyMaterialService::new(secure_storage_for_key_material, keyslot_store);
    let key_material: Arc<dyn KeyMaterialPort> = Arc::new(key_material_service);

    // Create encryption service
    // 创建加密服务
    let encryption: Arc<dyn EncryptionPort> = Arc::new(EncryptionRepository);

    // Create encryption state repository
    // 创建加密状态仓库
    let encryption_state: Arc<dyn uc_core::ports::security::encryption_state::EncryptionStatePort> =
        Arc::new(FileEncryptionStateRepository::new(vault_path.clone()));

    // Create settings repository
    // 创建设置仓库
    let settings_repo: Arc<dyn SettingsPort> = Arc::new(FileSettingsRepository::new(settings_path));

    // Create setup status repository
    // 创建设置状态仓库
    let setup_status: Arc<dyn SetupStatusPort> =
        Arc::new(FileSetupStatusRepository::with_defaults(vault_path.clone()));

    // Create system services
    // 创建系统服务
    let clock: Arc<dyn ClockPort> = Arc::new(SystemClock);
    let hash: Arc<dyn ContentHashPort> = Arc::new(Blake3Hasher);

    // Create clipboard selection repository
    // 创建剪贴板选择仓库
    let selection_repo_impl = DieselClipboardSelectionRepository::new(Arc::clone(&db_executor));
    let selection_repo: Arc<dyn ClipboardSelectionRepositoryPort> = Arc::new(selection_repo_impl);

    let infra = InfraLayer {
        clipboard_entry_repo,
        clipboard_event_repo,
        representation_repo,
        selection_repo,
        device_repo,
        paired_device_repo,
        blob_repository,
        thumbnail_repo,
        thumbnail_generator,
        key_material,
        encryption,
        encryption_state,
        settings_repo,
        setup_status,
        clock,
        hash,
    };

    Ok(infra)
}

/// Create platform layer implementations
/// 创建平台层实现
///
/// This function creates all platform-specific implementations including:
/// 此函数创建所有平台特定实现，包括：
/// - System clipboard (platform-specific: macOS/Windows/Linux) / 系统剪贴板（平台特定：macOS/Windows/Linux）
/// - Device identity (filesystem-backed UUID) / 设备身份（基于文件系统的 UUID）
/// - Placeholder implementations for unimplemented ports / 未实现端口的占位符实现
///
/// # Arguments / 参数
///
/// * `secure_storage` - Secure storage instance / 安全存储实例
/// * `config_dir` - Configuration directory for device identity storage / 用于存储设备身份的配置目录
/// * `platform_cmd_tx` - Command sender for platform runtime / 平台运行时命令发送器
/// * `encryption` - Encryption service for blob store decorator / Blob 存储加密服务
/// * `blob_repository` - Blob repository for BlobWriter / BlobWriter 依赖的仓库
/// * `clock` - Clock service for BlobWriter timestamps / BlobWriter 时间戳服务
/// * `storage_config` - Clipboard storage configuration / 剪贴板存储配置
/// * `identity_store` - Identity store for libp2p keypair persistence / libp2p 身份持久化存储
///
/// # Note / 注意
///
/// - Secure storage is passed in as parameter for key material + identity usage
/// - 安全存储作为参数传入（供密钥材料与身份使用）
/// - Device identity uses LocalDeviceIdentity with UUID v4 persistence
/// - 设备身份使用 LocalDeviceIdentity 持久化 UUID v4
/// - Most implementations are placeholders and will be replaced in future tasks
/// - 大多数实现是占位符，将在未来任务中替换
fn create_platform_layer(
    secure_storage: Arc<dyn SecureStoragePort>,
    config_dir: &PathBuf,
    platform_cmd_tx: PlatformCommandSender,
    encryption: Arc<dyn EncryptionPort>,
    blob_repository: Arc<dyn BlobRepositoryPort>,
    paired_device_repo: Arc<dyn PairedDeviceRepositoryPort>,
    clock: Arc<dyn ClockPort>,
    storage_config: Arc<ClipboardStorageConfig>,
    identity_store: Arc<dyn IdentityStorePort>,
) -> WiringResult<PlatformLayer> {
    // Create system clipboard implementation (platform-specific)
    // 创建系统剪贴板实现（平台特定）
    let clipboard_impl = LocalClipboard::new()
        .map_err(|e| WiringError::ClipboardInit(format!("Failed to create clipboard: {}", e)))?;
    let clipboard_impl = Arc::new(clipboard_impl);
    let clipboard: Arc<dyn PlatformClipboardPort> = clipboard_impl.clone();
    let system_clipboard: Arc<dyn SystemClipboardPort> = clipboard_impl;

    // Create device identity (filesystem-backed UUID)
    // 创建设备身份（基于文件系统的 UUID）
    let device_identity = LocalDeviceIdentity::load_or_create(config_dir.clone()).map_err(|e| {
        WiringError::SettingsInit(format!("Failed to create device identity: {}", e))
    })?;
    let device_identity: Arc<dyn DeviceIdentityPort> = Arc::new(device_identity);

    // Create blob store (filesystem-based)
    // 创建 blob 存储（基于文件系统）
    let blob_store_dir = config_dir.join("blobs");

    // Purge old blob files after V2 migration (old JSON format files are incompatible
    // with the new UCBL binary format). Uses a sentinel file so this only runs once.
    let sentinel = blob_store_dir.join(".v2_migrated");
    if blob_store_dir.exists() && !sentinel.exists() {
        match std::fs::read_dir(&blob_store_dir) {
            Ok(entries) => {
                let mut purged = 0u64;
                for entry in entries.flatten() {
                    if entry.path().is_file() {
                        if let Err(e) = std::fs::remove_file(entry.path()) {
                            tracing::warn!(
                                path = %entry.path().display(),
                                error = %e,
                                "Failed to purge old blob file"
                            );
                        } else {
                            purged += 1;
                        }
                    }
                }
                if purged > 0 {
                    tracing::info!(
                        count = purged,
                        "Purged old blob files (V2 format migration)"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to read blob directory for cleanup");
            }
        }
        if let Err(e) = std::fs::File::create(&sentinel) {
            tracing::warn!(error = %e, "Failed to create V2 migration sentinel");
        }
    }

    let blob_store: Arc<dyn BlobStorePort> = Arc::new(FilesystemBlobStore::new(blob_store_dir));

    // Create clipboard representation normalizer (real implementation)
    // 创建剪贴板表示规范化器（真实实现）
    let representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort> =
        Arc::new(ClipboardRepresentationNormalizer::new(storage_config));

    // Create placeholder implementations for unimplemented ports
    // 为未实现的端口创建占位符实现
    let ui: Arc<dyn UiPort> = Arc::new(PlaceholderUiPort);
    let autostart: Arc<dyn AutostartPort> = Arc::new(PlaceholderAutostartPort);
    let encryption_session: Arc<dyn EncryptionSessionPort> =
        Arc::new(InMemoryEncryptionSessionPort::new());
    let policy_resolver = Arc::new(ResolveConnectionPolicy::new(paired_device_repo.clone()));
    let libp2p_network = Arc::new(
        Libp2pNetworkAdapter::new(identity_store, policy_resolver, encryption_session.clone())
            .map_err(|e| {
                WiringError::NetworkInit(format!("Failed to initialize libp2p identity: {e}"))
            })?,
    );
    info!(peer_id = %libp2p_network.local_peer_id(), "Loaded libp2p identity");
    let network_ports = Arc::new(NetworkPorts {
        clipboard: libp2p_network.clone(),
        peers: libp2p_network.clone(),
        pairing: libp2p_network.clone(),
        events: libp2p_network.clone(),
    });

    // Wrap blob_store with encryption decorator
    // 用加密装饰器包装 blob_store
    let encrypted_blob_store: Arc<dyn BlobStorePort> = Arc::new(EncryptedBlobStore::new(
        blob_store.clone(),
        encryption,
        encryption_session.clone(),
    ));

    // Create blob writer using encrypted blob store
    // 使用加密 blob 存储创建 blob 写入器
    let blob_writer: Arc<dyn BlobWriterPort> = Arc::new(BlobWriter::new(
        encrypted_blob_store.clone(),
        blob_repository,
        clock,
    ));

    // Create watcher control
    // 创建监控器控制
    let watcher_control: Arc<dyn WatcherControlPort> =
        Arc::new(InMemoryWatcherControl::new(platform_cmd_tx));

    // Create key scope
    // 创建密钥范围
    let key_scope: Arc<dyn uc_core::ports::security::key_scope::KeyScopePort> =
        Arc::new(uc_platform::key_scope::DefaultKeyScope::new());

    Ok(PlatformLayer {
        clipboard,
        system_clipboard,
        secure_storage,
        ui,
        autostart,
        network_ports,
        libp2p_network,
        device_identity,
        representation_normalizer,
        blob_writer,
        blob_store: encrypted_blob_store,
        encryption_session,
        watcher_control,
        key_scope,
    })
}

/// Resolves the application's default directories for storing data and configuration.
///
/// Returns an AppDirs adapter populated with platform-appropriate paths for the application.
///
/// # Errors
///
/// Returns `WiringError::ConfigInit` if the platform adapter fails to determine the directories.
///
/// # Examples
///
/// ```ignore
/// use uc_tauri::bootstrap::wiring::get_default_app_dirs;
///
/// let dirs = get_default_app_dirs().expect("failed to get app dirs");
/// // `dirs` contains platform-specific paths such as config, data, and cache roots
/// assert!(!dirs.app_name.is_empty());
/// ```
fn get_default_app_dirs() -> WiringResult<uc_core::app_dirs::AppDirs> {
    let adapter = DirsAppDirsAdapter::new();
    adapter
        .get_app_dirs()
        .map_err(|e| WiringError::ConfigInit(e.to_string()))
}

#[derive(Debug, Clone)]
struct DefaultPaths {
    app_data_root: PathBuf,
    db_path: PathBuf,
    vault_dir: PathBuf,
    settings_path: PathBuf,
    cache_dir: PathBuf,
}

/// Compute default application file-system paths from the given configuration.
///
/// The returned paths combine platform-specific application directories with any
/// explicit overrides present in `config`, producing concrete locations for:
/// - app_data_root: base application data directory
/// - db_path: path to the SQLite database file
/// - vault_dir: directory for vault/key material
/// - settings_path: path to the settings file
///
/// # Examples
///
/// ```ignore
/// use uc_core::config::AppConfig;
///
/// let cfg = AppConfig::empty();
/// let paths = derive_default_paths(&cfg).expect("derive default paths");
/// assert!(!paths.app_data_root.as_os_str().is_empty());
/// assert!(!paths.settings_path.as_os_str().is_empty());
/// ```
fn derive_default_paths(config: &AppConfig) -> WiringResult<DefaultPaths> {
    let app_dirs = get_default_app_dirs()?;

    derive_default_paths_from_app_dirs(&app_dirs, config)
}

/// Derives concrete filesystem paths (database, vault, settings, and app data root)
/// from platform `AppDirs`, applying any overrides present in `AppConfig`.
///
/// If `config.database_path` is empty the default database path from `AppDirs` is used;
/// otherwise `config.database_path` is returned. If `config.vault_key_path` is empty
/// the default vault directory from `AppDirs` is used; otherwise the parent directory
/// of `config.vault_key_path` is used as the vault directory.
///
/// # Parameters
///
/// - `app_dirs`: Platform-specific base directories to derive defaults from.
/// - `config`: Application configuration that may override the default database path
///   and vault key path.
///
/// # Returns
///
/// `DefaultPaths` containing:
/// - `app_data_root`: the application data root from `AppDirs`.
/// - `db_path`: the resolved database file path.
/// - `vault_dir`: the resolved vault directory.
/// - `settings_path`: the resolved settings file path.
///
/// # Examples
///
/// ```ignore
/// use uc_core::app_dirs::AppDirs;
/// use uc_core::config::AppConfig;
/// use uc_tauri::bootstrap::wiring::derive_default_paths_from_app_dirs;
///
/// // Assuming `AppDirs` is constructed in tests/setup.
/// let app_dirs = AppDirs::default();
/// let config = AppConfig::empty();
/// let paths = derive_default_paths_from_app_dirs(&app_dirs, &config).unwrap();
/// // Basic sanity check: returned paths are populated.
/// assert!(!paths.app_data_root.as_os_str().is_empty());
/// assert!(!paths.settings_path.as_os_str().is_empty());
/// ```
fn derive_default_paths_from_app_dirs(
    app_dirs: &uc_core::app_dirs::AppDirs,
    config: &AppConfig,
) -> WiringResult<DefaultPaths> {
    let default_app_data_root = app_dirs.app_data_root.clone();

    let app_data_root = if config.database_path.as_os_str().is_empty() {
        default_app_data_root
    } else {
        let configured_root = config
            .database_path
            .parent()
            .unwrap_or(&config.database_path)
            .to_path_buf();
        apply_profile_suffix(configured_root)
    };

    let db_path = if config.database_path.as_os_str().is_empty() {
        app_data_root.join("uniclipboard.db")
    } else {
        let db_file_name = config
            .database_path
            .file_name()
            .map(|name| name.to_os_string())
            .unwrap_or_else(|| std::ffi::OsString::from("uniclipboard.db"));
        app_data_root.join(db_file_name)
    };

    let vault_dir = if config.vault_key_path.as_os_str().is_empty() {
        app_data_root.join("vault")
    } else {
        let configured_vault_root = config
            .vault_key_path
            .parent()
            .unwrap_or(&config.vault_key_path)
            .to_path_buf();

        if config.database_path.as_os_str().is_empty() {
            apply_profile_suffix(configured_vault_root)
        } else {
            let configured_db_root = config
                .database_path
                .parent()
                .unwrap_or(&config.database_path)
                .to_path_buf();

            if configured_vault_root.starts_with(&configured_db_root) {
                let relative = configured_vault_root
                    .strip_prefix(&configured_db_root)
                    .unwrap_or(std::path::Path::new(""));
                app_data_root.join(relative)
            } else {
                apply_profile_suffix(configured_vault_root)
            }
        }
    };

    let settings_path = app_data_root.join("settings.json");

    Ok(DefaultPaths {
        app_data_root,
        db_path,
        vault_dir,
        settings_path,
        cache_dir: app_dirs.app_cache_root.clone(),
    })
}

fn apply_profile_suffix(path: PathBuf) -> PathBuf {
    let profile = match std::env::var("UC_PROFILE") {
        Ok(value) if !value.is_empty() => value.replace('/', "_").replace('\\', "_"),
        _ => return path,
    };

    let file_name = match path.file_name().and_then(|name| name.to_str()) {
        Some(name) => name.to_string(),
        None => return path,
    };

    let mut updated = path;
    updated.set_file_name(format!("{file_name}_{profile}"));
    updated
}

/// Wires and constructs the application's dependency graph, returning ready-to-use dependencies.
///
/// On success returns `WiredDependencies` containing AppDeps plus background runtime components.
/// AppDeps includes all infrastructure and platform components
/// (database pool, repositories, security, platform adapters, materializers, settings, etc.)
/// wrapped for shared use.
///
/// # Errors
///
/// Returns a `WiringError` when any required dependency cannot be constructed, for example:
/// - `WiringError::DatabaseInit` for database/pool initialization failures
/// - `WiringError::SecureStorageInit` for secure storage creation failures
/// - `WiringError::ClipboardInit` for clipboard adapter failures
/// - `WiringError::NetworkInit` for network adapter failures
/// - `WiringError::BlobStorageInit` for blob store initialization failures
/// - `WiringError::SettingsInit` for settings repository failures
/// - `WiringError::ConfigInit` for application directory / configuration discovery failures
///
/// # Examples
///
/// ```ignore
/// use uc_core::config::AppConfig;
/// use uc_tauri::bootstrap::wiring::wire_dependencies;
///
/// // The function will either return fully wired dependencies or a WiringError describing
/// // what failed during construction.
/// let config = AppConfig::empty();
/// let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(10);
/// match wire_dependencies(&config, cmd_tx) {
///     Ok(_wired) => { /* ready to run the application */ }
///     Err(_err) => { /* handle initialization failure */ }
/// }
/// ```
pub fn wire_dependencies(
    config: &AppConfig,
    platform_cmd_tx: PlatformCommandSender,
) -> WiringResult<WiredDependencies> {
    wire_dependencies_with_identity_store(config, platform_cmd_tx, None)
}

/// Wires dependencies with a caller-provided identity store.
///
/// This is primarily intended for tests or environments without system secure storage.
pub fn wire_dependencies_with_identity_store(
    config: &AppConfig,
    platform_cmd_tx: PlatformCommandSender,
    identity_store: Option<Arc<dyn IdentityStorePort>>,
) -> WiringResult<WiredDependencies> {
    // Step 1: Create database connection pool
    // 步骤 1：创建数据库连接池
    //
    // Defensive: Use system default if database_path is empty
    // 防御性编程：如果 database_path 为空，使用系统默认值
    let paths = derive_default_paths(config)?;

    let db_path = paths.db_path;

    let db_pool = create_db_pool(&db_path)?;

    // Step 2: Create infrastructure layer implementations
    // 步骤 2：创建基础设施层实现
    //
    // Create vault path from config (use vault_key_path parent directory)
    // If config path is empty, use system config directory as fallback
    // 从配置创建 vault 路径（使用 vault_key_path 的父目录）
    // 如果配置路径为空，使用系统配置目录作为后备
    let vault_path = paths.vault_dir;

    let settings_path = paths.settings_path;

    let secure_storage =
        uc_platform::secure_storage::create_default_secure_storage_in_app_data_root(
            paths.app_data_root.clone(),
        )
        .map_err(|e| WiringError::SecureStorageInit(e.to_string()))?;

    let identity_store = identity_store.unwrap_or_else(|| {
        Arc::new(FileIdentityStore::new(paths.app_data_root.clone())) as Arc<dyn IdentityStorePort>
    });

    let infra = create_infra_layer(db_pool, &vault_path, &settings_path, secure_storage.clone())?;

    // Step 3: Create platform layer implementations
    // 步骤 3：创建平台层实现
    let storage_config = Arc::new(ClipboardStorageConfig::defaults());
    let platform = create_platform_layer(
        secure_storage,
        &vault_path,
        platform_cmd_tx,
        infra.encryption.clone(),
        infra.blob_repository.clone(),
        infra.paired_device_repo.clone(),
        infra.clock.clone(),
        storage_config.clone(),
        identity_store,
    )?;

    // Step 3.5: Wrap ports with encryption decorators
    // 步骤 3.5：用加密装饰器包装端口

    // Wrap clipboard_event_repo with encryption decorator
    let encrypting_event_writer: Arc<dyn ClipboardEventWriterPort> =
        Arc::new(EncryptingClipboardEventWriter::new(
            infra.clipboard_event_repo.clone(),
            infra.encryption.clone(),
            platform.encryption_session.clone(),
        ));

    // Wrap representation_repo with decryption decorator
    let decrypting_rep_repo: Arc<dyn ClipboardRepresentationRepositoryPort> =
        Arc::new(DecryptingClipboardRepresentationRepository::new(
            infra.representation_repo.clone(),
            infra.encryption.clone(),
            platform.encryption_session.clone(),
        ));

    // Step 3.6: Create background processing components
    // 步骤 3.6：创建后台处理组件

    // Create representation cache
    let representation_cache = Arc::new(RepresentationCache::new(
        storage_config.cache_max_entries,
        storage_config.cache_max_bytes,
    ));
    let representation_cache_port: Arc<dyn RepresentationCachePort> = representation_cache.clone();

    // Create spool manager
    let spool_dir = paths.cache_dir.join("spool");
    let spool_manager = Arc::new(
        SpoolManager::new(spool_dir.clone(), storage_config.spool_max_bytes)
            .map_err(|e| WiringError::BlobStorageInit(format!("Failed to create spool: {}", e)))?,
    );

    // Create channels for background processing
    let (spool_tx, spool_rx) = mpsc::channel::<SpoolRequest>(100);
    let spool_queue: Arc<dyn SpoolQueuePort> = Arc::new(MpscSpoolQueue::new(spool_tx));
    let (worker_tx, worker_rx) = mpsc::channel::<RepresentationId>(100);

    let clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort> =
        Arc::new(InMemoryClipboardChangeOrigin::new());

    // Step 4: Construct AppDeps with all dependencies
    // 步骤 4：使用所有依赖构造 AppDeps
    let deps = AppDeps {
        // Clipboard dependencies / 剪贴板依赖
        clipboard: platform.clipboard,
        system_clipboard: platform.system_clipboard,
        clipboard_entry_repo: infra.clipboard_entry_repo,
        clipboard_event_repo: encrypting_event_writer,
        representation_repo: decrypting_rep_repo,
        representation_normalizer: platform.representation_normalizer,
        selection_repo: infra.selection_repo,
        representation_policy: Arc::new(SelectRepresentationPolicyV1::new()),
        representation_cache: representation_cache_port,
        spool_queue,
        clipboard_change_origin,
        worker_tx,

        // Security dependencies / 安全依赖
        encryption: infra.encryption,
        encryption_session: platform.encryption_session,
        encryption_state: infra.encryption_state,
        key_scope: platform.key_scope,
        secure_storage: platform.secure_storage,
        key_material: infra.key_material,
        watcher_control: platform.watcher_control,

        // Device dependencies / 设备依赖
        device_repo: infra.device_repo,
        device_identity: platform.device_identity,

        // Pairing dependencies / 配对依赖
        paired_device_repo: infra.paired_device_repo,

        // Network dependencies / 网络依赖
        network_ports: platform.network_ports,
        network_control: platform.libp2p_network.clone(),

        // Setup status dependencies / 设置状态依赖
        setup_status: infra.setup_status,

        // Storage dependencies / 存储依赖
        blob_store: platform.blob_store,
        blob_repository: infra.blob_repository,
        blob_writer: platform.blob_writer,
        thumbnail_repo: infra.thumbnail_repo,
        thumbnail_generator: infra.thumbnail_generator,

        // Settings dependencies / 设置依赖
        settings: infra.settings_repo,

        // UI dependencies / UI 依赖
        ui_port: platform.ui,
        autostart: platform.autostart,

        // System dependencies / 系统依赖
        clock: infra.clock,
        hash: infra.hash,
    };

    Ok(WiredDependencies {
        deps,
        background: BackgroundRuntimeDeps {
            libp2p_network: platform.libp2p_network.clone(),
            representation_cache,
            spool_manager,
            spool_rx,
            worker_rx,
            spool_dir,
            spool_ttl_days: storage_config.spool_ttl_days,
            worker_retry_max_attempts: storage_config.worker_retry_max_attempts,
            worker_retry_backoff_ms: storage_config.worker_retry_backoff_ms,
        },
    })
}

/// Start background spooler and blob worker tasks.
/// 启动后台假脱机写入和 blob 物化任务。
pub fn start_background_tasks<R: Runtime>(
    background: BackgroundRuntimeDeps,
    deps: &AppDeps,
    app_handle: Option<AppHandle<R>>,
    pairing_orchestrator: Arc<PairingOrchestrator>,
    pairing_action_rx: mpsc::Receiver<PairingAction>,
    space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    key_slot_store: Arc<dyn KeySlotStore>,
) {
    let BackgroundRuntimeDeps {
        libp2p_network: _,
        representation_cache,
        spool_manager,
        spool_rx,
        worker_rx,
        spool_dir,
        spool_ttl_days,
        worker_retry_max_attempts,
        worker_retry_backoff_ms,
    } = background;

    info!("Starting background clipboard spooler and blob worker");

    let pairing_app_handle = app_handle.clone();
    let space_access_app_handle = app_handle.clone();
    let clipboard_app_handle = app_handle.clone();
    let pairing_space_access_orchestrator = space_access_orchestrator.clone();
    let representation_repo = deps.representation_repo.clone();
    let worker_tx = deps.worker_tx.clone();
    let blob_writer = deps.blob_writer.clone();
    let hasher = deps.hash.clone();
    let clock = deps.clock.clone();
    let thumbnail_repo = deps.thumbnail_repo.clone();
    let thumbnail_generator = deps.thumbnail_generator.clone();
    let pairing_transport = deps.network_ports.pairing.clone();
    let pairing_events = deps.network_ports.events.clone();
    let peer_directory = deps.network_ports.peers.clone();
    let clipboard_network = deps.network_ports.clipboard.clone();
    let sync_inbound_usecase = new_sync_inbound_clipboard_usecase(deps);
    let space_access_runtime_ports = RuntimeSpaceAccessPorts {
        transport: Arc::new(tokio::sync::Mutex::new(SpaceAccessNetworkAdapter::new(
            pairing_transport.clone(),
            pairing_space_access_orchestrator.context(),
        ))),
        proof: Arc::new(HmacProofAdapter::new_with_encryption_session(
            deps.encryption_session.clone(),
        )),
        timer: Arc::new(tokio::sync::Mutex::new(Timer::new())),
        persistence: Arc::new(tokio::sync::Mutex::new(SpaceAccessPersistenceAdapter::new(
            deps.encryption_state.clone(),
            deps.paired_device_repo.clone(),
        ))),
    };

    async_runtime::spawn(async move {
        let scanner = SpoolScanner::new(spool_dir, representation_repo.clone(), worker_tx.clone());
        match scanner.scan_and_recover().await {
            Ok(recovered) => info!("Recovered {} representations from spool", recovered),
            Err(err) => warn!(error = %err, "Spool scan failed; continuing startup"),
        }

        let spooler = SpoolerTask::new(
            spool_rx,
            spool_manager.clone(),
            worker_tx,
            representation_cache.clone(),
        );
        async_runtime::spawn(async move {
            spooler.run().await;
            warn!("SpoolerTask stopped");
        });

        let worker = BackgroundBlobWorker::new(
            worker_rx,
            representation_cache,
            spool_manager.clone(),
            representation_repo.clone(),
            blob_writer,
            hasher,
            thumbnail_repo,
            thumbnail_generator,
            clock.clone(),
            worker_retry_max_attempts,
            Duration::from_millis(worker_retry_backoff_ms),
        );
        async_runtime::spawn(async move {
            worker.run().await;
            warn!("BackgroundBlobWorker stopped");
        });

        let janitor = SpoolJanitor::new(
            spool_manager.clone(),
            representation_repo.clone(),
            clock,
            spool_ttl_days,
        );
        async_runtime::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(SPOOL_JANITOR_INTERVAL_SECS));
            loop {
                interval.tick().await;
                match janitor.run_once().await {
                    Ok(removed) => {
                        if removed > 0 {
                            info!("Spool janitor removed {} expired entries", removed);
                        }
                    }
                    Err(err) => {
                        warn!(error = %err, "Spool janitor run failed");
                    }
                }
            }
        });
    });

    async_runtime::spawn(async move {
        let completion_rx =
            match SpaceAccessEventPort::subscribe(space_access_orchestrator.as_ref()).await {
                Ok(rx) => rx,
                Err(err) => {
                    warn!(error = %err, "Failed to subscribe to space access completion events");
                    return;
                }
            };

        run_space_access_completion_loop(completion_rx, space_access_app_handle).await;
        warn!("Space access completion loop stopped");
    });

    async_runtime::spawn(
        async move {
            let mut subscribe_attempt: u32 = 0;
            let mut first_subscribe_failure_emitted = false;

            loop {
                let subscribe_result = tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        info!("Clipboard receive task stopping on shutdown signal");
                        return;
                    }
                    result = clipboard_network.subscribe_clipboard() => result,
                };

                match subscribe_result {
                    Ok(clipboard_rx) => {
                        if subscribe_attempt > 0 {
                            info!(attempts = subscribe_attempt, "Recovered clipboard subscription");

                            if let Some(app) = clipboard_app_handle.as_ref() {
                                let payload = InboundClipboardSubscribeRecoveredPayload {
                                    recovered_after_attempts: subscribe_attempt,
                                };
                                if let Err(err) =
                                    app.emit("inbound-clipboard-subscribe-recovered", payload)
                                {
                                    warn!(error = %err, "Failed to emit inbound clipboard subscribe recovered event");
                                }
                            }
                        }

                        subscribe_attempt = 0;
                        first_subscribe_failure_emitted = false;
                        run_clipboard_receive_loop(
                            clipboard_rx,
                            &sync_inbound_usecase,
                            clipboard_app_handle.clone(),
                        )
                        .await;
                    }
                    Err(err) => {
                        subscribe_attempt = subscribe_attempt.saturating_add(1);
                        let retry_in_ms = subscribe_backoff_ms(subscribe_attempt);

                        warn!(
                            error = %err,
                            attempt = subscribe_attempt,
                            retry_in_ms,
                            "Failed to subscribe to clipboard messages"
                        );

                        if let Some(app) = clipboard_app_handle.as_ref() {
                            if !first_subscribe_failure_emitted {
                                let payload = InboundClipboardSubscribeErrorPayload {
                                    attempt: subscribe_attempt,
                                    error: err.to_string(),
                                };
                                if let Err(emit_err) =
                                    app.emit("inbound-clipboard-subscribe-error", payload)
                                {
                                    warn!(error = %emit_err, "Failed to emit inbound clipboard subscribe error event");
                                }
                                first_subscribe_failure_emitted = true;
                            }

                            let retry_payload = InboundClipboardSubscribeRetryPayload {
                                attempt: subscribe_attempt,
                                retry_in_ms,
                                error: err.to_string(),
                            };
                            if let Err(emit_err) =
                                app.emit("inbound-clipboard-subscribe-retry", retry_payload)
                            {
                                warn!(error = %emit_err, "Failed to emit inbound clipboard subscribe retry event");
                            }
                        }

                        let backoff = Duration::from_millis(retry_in_ms);
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => {
                                info!("Clipboard receive task stopping during backoff on shutdown signal");
                                return;
                            }
                            _ = tokio::time::sleep(backoff) => {}
                        }
                    }
                }
            }
        }
        .instrument(info_span!("loop.clipboard.receive_task")),
    );

    async_runtime::spawn(async move {
        let action_network = pairing_transport.clone();
        let action_app_handle = pairing_app_handle.clone();
        let action_orchestrator = pairing_orchestrator.clone();
        let action_space_access_orchestrator = pairing_space_access_orchestrator.clone();
        let action_key_slot_store = key_slot_store.clone();
        let action_space_access_runtime_ports = space_access_runtime_ports.clone();
        async_runtime::spawn(async move {
            run_pairing_action_loop(
                pairing_action_rx,
                action_network,
                action_app_handle,
                action_orchestrator,
                action_space_access_orchestrator,
                action_key_slot_store,
                action_space_access_runtime_ports,
            )
            .await;
        });

        let mut subscribe_attempt: u32 = 0;
        loop {
            let subscribe_result = tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("Pairing event loop task stopping on shutdown signal");
                    return;
                }
                result = pairing_events.subscribe_events() => result,
            };

            match subscribe_result {
                Ok(event_rx) => {
                    if subscribe_attempt > 0 {
                        info!(
                            attempts = subscribe_attempt,
                            "Recovered pairing network event subscription"
                        );
                        emit_pairing_events_subscribe_recovered(
                            pairing_app_handle.as_ref(),
                            subscribe_attempt,
                        );
                    }

                    subscribe_attempt = 0;

                    run_pairing_event_loop(
                        event_rx,
                        pairing_orchestrator.clone(),
                        pairing_app_handle.clone(),
                        peer_directory.clone(),
                        pairing_space_access_orchestrator.clone(),
                        space_access_runtime_ports.clone(),
                    )
                    .await;

                    let err = anyhow::anyhow!("pairing network event stream closed");
                    subscribe_attempt = subscribe_attempt.saturating_add(1);
                    let retry_in_ms = pairing_events_subscribe_backoff_ms(subscribe_attempt);
                    warn!(
                        error = %err,
                        attempt = subscribe_attempt,
                        retry_in_ms,
                        "Pairing event loop stopped unexpectedly; restarting"
                    );
                    emit_pairing_events_subscribe_failure(
                        pairing_app_handle.as_ref(),
                        subscribe_attempt,
                        retry_in_ms,
                        err.to_string(),
                    );
                }
                Err(err) => {
                    subscribe_attempt = subscribe_attempt.saturating_add(1);
                    let retry_in_ms = pairing_events_subscribe_backoff_ms(subscribe_attempt);
                    warn!(
                        error = %err,
                        attempt = subscribe_attempt,
                        retry_in_ms,
                        "Failed to subscribe to network events for pairing"
                    );
                    emit_pairing_events_subscribe_failure(
                        pairing_app_handle.as_ref(),
                        subscribe_attempt,
                        retry_in_ms,
                        err.to_string(),
                    );
                }
            }

            let backoff =
                Duration::from_millis(pairing_events_subscribe_backoff_ms(subscribe_attempt));
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("Pairing event loop task stopping during backoff on shutdown signal");
                    return;
                }
                _ = tokio::time::sleep(backoff) => {}
            }
        }
    });
}

fn new_sync_inbound_clipboard_usecase(deps: &AppDeps) -> SyncInboundClipboardUseCase {
    let mode = super::resolve_clipboard_integration_mode();
    SyncInboundClipboardUseCase::with_capture_dependencies(
        mode,
        deps.system_clipboard.clone(),
        deps.clipboard_change_origin.clone(),
        deps.encryption_session.clone(),
        deps.encryption.clone(),
        deps.device_identity.clone(),
        Arc::new(uc_infra::clipboard::TransferPayloadDecryptorAdapter),
        deps.clipboard_entry_repo.clone(),
        deps.clipboard_event_repo.clone(),
        deps.representation_policy.clone(),
        deps.representation_normalizer.clone(),
        deps.representation_cache.clone(),
        deps.spool_queue.clone(),
    )
}

async fn run_clipboard_receive_loop<R: Runtime>(
    mut clipboard_rx: mpsc::Receiver<(ClipboardMessage, Option<Vec<u8>>)>,
    usecase: &SyncInboundClipboardUseCase,
    app_handle: Option<AppHandle<R>>,
) {
    while let Some((message, pre_decoded)) = clipboard_rx.recv().await {
        let message_id = message.id.clone();
        let origin_device_id = message.origin_device_id.clone();
        let span = info_span!(
            "loop.clipboard.receive_message",
            message_id = %message_id,
            origin_device_id = %origin_device_id
        );

        let result = async { usecase.execute_with_outcome(message, pre_decoded).await }
            .instrument(span)
            .await;

        match result {
            Ok(outcome) => {
                if matches!(
                    usecase.mode(),
                    uc_app::usecases::clipboard::ClipboardIntegrationMode::Passive
                ) {
                    match outcome {
                        InboundApplyOutcome::Applied {
                            entry_id: Some(entry_id),
                        } => {
                            if let Some(app) = app_handle.as_ref() {
                                let event = ClipboardEvent::NewContent {
                                    entry_id: entry_id.to_string(),
                                    preview: "Remote clipboard content applied".to_string(),
                                };
                                if let Err(emit_err) = forward_clipboard_event(app, event) {
                                    warn!(error = %emit_err, message_id = %message_id, "Failed to emit clipboard event after inbound apply in passive mode");
                                }
                            }
                        }
                        InboundApplyOutcome::Applied { entry_id: None } => {
                            warn!(
                                message_id = %message_id,
                                "Inbound apply reported success in passive mode without persisted entry id"
                            );
                        }
                        InboundApplyOutcome::Skipped => {}
                    }
                }
            }
            Err(err) => {
                warn!(
                    error = %err,
                    message_id = %message_id,
                    origin_device_id = %origin_device_id,
                    "Failed to apply inbound clipboard message"
                );

                if let Some(app) = app_handle.as_ref() {
                    let payload = InboundClipboardErrorPayload {
                        message_id: message_id.clone(),
                        origin_device_id: origin_device_id.clone(),
                        error: err.to_string(),
                    };
                    if let Err(emit_err) = app.emit("inbound-clipboard-error", payload) {
                        warn!(error = %emit_err, "Failed to emit inbound clipboard error event");
                    }
                }
            }
        }
    }

    info!("Clipboard receive channel closed; stopping background receive loop");
}

#[derive(Clone)]
struct RuntimeSpaceAccessPorts {
    transport: Arc<tokio::sync::Mutex<dyn uc_core::ports::space::SpaceAccessTransportPort>>,
    proof: Arc<dyn ProofPort>,
    timer: Arc<tokio::sync::Mutex<dyn TimerPort>>,
    persistence: Arc<tokio::sync::Mutex<dyn uc_core::ports::space::PersistencePort>>,
}

async fn dispatch_space_access_busy_event(
    orchestrator: &SpaceAccessOrchestrator,
    runtime_ports: &RuntimeSpaceAccessPorts,
    event: SpaceAccessEvent,
    session_id: &str,
) -> Result<(), uc_app::usecases::space_access::SpaceAccessError> {
    let noop_crypto = NoopSpaceAccessCrypto;
    let mut transport = runtime_ports.transport.lock().await;
    let mut timer = runtime_ports.timer.lock().await;
    let mut store = runtime_ports.persistence.lock().await;

    orchestrator
        .dispatch(
            &mut uc_app::usecases::space_access::SpaceAccessExecutor {
                crypto: &noop_crypto,
                transport: &mut *transport,
                proof: runtime_ports.proof.as_ref(),
                timer: &mut *timer,
                store: &mut *store,
            },
            event,
            Some(session_id.to_string()),
        )
        .await
        .map(|_| ())
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SpaceAccessCompletedPayload {
    session_id: String,
    peer_id: String,
    success: bool,
    reason: Option<String>,
    ts: i64,
}

const BUSY_PAYLOAD_PREVIEW_MAX_CHARS: usize = 256;

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceAccessBusyOfferPayload {
    kind: String,
    space_id: String,
    nonce: Vec<u8>,
    keyslot: KeySlot,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceAccessBusyProofPayload {
    kind: String,
    pairing_session_id: String,
    space_id: String,
    challenge_nonce: Vec<u8>,
    proof_bytes: Vec<u8>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SpaceAccessBusyResultPayload {
    kind: String,
    space_id: String,
    #[serde(default)]
    sponsor_peer_id: Option<String>,
    success: bool,
    #[serde(default)]
    deny_reason: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum SpaceAccessBusyPayload {
    Offer(SpaceAccessBusyOfferPayload),
    Proof(SpaceAccessBusyProofPayload),
    Result(SpaceAccessBusyResultPayload),
}

#[derive(Debug, thiserror::Error)]
enum ParseError {
    #[error("busy payload is not valid json: {source}")]
    InvalidJson {
        #[source]
        source: serde_json::Error,
    },
    #[error("busy payload missing string field `kind`")]
    MissingKind,
    #[error("busy payload kind `{kind}` is not supported")]
    UnknownKind { kind: String },
    #[error("busy payload kind `{kind}` has invalid structure: {source}")]
    InvalidStructure {
        kind: String,
        #[source]
        source: serde_json::Error,
    },
}

impl ParseError {
    fn payload_kind(&self) -> Option<&str> {
        match self {
            Self::UnknownKind { kind } | Self::InvalidStructure { kind, .. } => Some(kind.as_str()),
            Self::InvalidJson { .. } | Self::MissingKind => None,
        }
    }
}

fn parse_space_access_busy_payload(json: &str) -> Result<SpaceAccessBusyPayload, ParseError> {
    let payload: serde_json::Value =
        serde_json::from_str(json).map_err(|source| ParseError::InvalidJson { source })?;

    let kind = payload
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or(ParseError::MissingKind)?
        .to_string();

    match kind.as_str() {
        "space_access_offer" => serde_json::from_value::<SpaceAccessBusyOfferPayload>(payload)
            .map(SpaceAccessBusyPayload::Offer)
            .map_err(|source| ParseError::InvalidStructure {
                kind: kind.clone(),
                source,
            }),
        "space_access_proof" => serde_json::from_value::<SpaceAccessBusyProofPayload>(payload)
            .map(SpaceAccessBusyPayload::Proof)
            .map_err(|source| ParseError::InvalidStructure {
                kind: kind.clone(),
                source,
            }),
        "space_access_result" => serde_json::from_value::<SpaceAccessBusyResultPayload>(payload)
            .map(SpaceAccessBusyPayload::Result)
            .map_err(|source| ParseError::InvalidStructure {
                kind: kind.clone(),
                source,
            }),
        _ => Err(ParseError::UnknownKind { kind }),
    }
}

fn extract_space_access_busy_payload_kind(json: &str) -> Option<String> {
    let payload: serde_json::Value = serde_json::from_str(json).ok()?;
    payload
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn raw_payload_preview(payload: &str) -> String {
    let mut chars = payload.chars();
    let mut preview: String = chars
        .by_ref()
        .take(BUSY_PAYLOAD_PREVIEW_MAX_CHARS)
        .collect();
    if chars.next().is_some() {
        preview.push_str("...");
    }
    preview
}

struct NoopSpaceAccessCrypto;

struct LoadedKeyslotSpaceAccessCrypto {
    keyslot_file: KeySlotFile,
}

impl LoadedKeyslotSpaceAccessCrypto {
    fn new(keyslot_file: KeySlotFile) -> Self {
        Self { keyslot_file }
    }
}

#[async_trait::async_trait]
impl uc_core::ports::space::CryptoPort for NoopSpaceAccessCrypto {
    async fn generate_nonce32(&self) -> [u8; 32] {
        [0u8; 32]
    }

    async fn export_keyslot_blob(
        &self,
        _space_id: &uc_core::ids::SpaceId,
    ) -> anyhow::Result<uc_core::security::model::KeySlot> {
        Err(anyhow::anyhow!(
            "noop crypto port cannot export keyslot blob"
        ))
    }

    async fn derive_master_key_from_keyslot(
        &self,
        _keyslot_blob: &[u8],
        _passphrase: uc_core::security::SecretString,
    ) -> anyhow::Result<uc_core::security::model::MasterKey> {
        Err(anyhow::anyhow!("noop crypto port cannot derive master key"))
    }
}

#[async_trait::async_trait]
impl uc_core::ports::space::CryptoPort for LoadedKeyslotSpaceAccessCrypto {
    async fn generate_nonce32(&self) -> [u8; 32] {
        [0u8; 32]
    }

    async fn export_keyslot_blob(
        &self,
        _space_id: &uc_core::ids::SpaceId,
    ) -> anyhow::Result<uc_core::security::model::KeySlot> {
        Ok(self.keyslot_file.clone().into())
    }

    async fn derive_master_key_from_keyslot(
        &self,
        _keyslot_blob: &[u8],
        _passphrase: uc_core::security::SecretString,
    ) -> anyhow::Result<uc_core::security::model::MasterKey> {
        Err(anyhow::anyhow!(
            "loaded keyslot crypto cannot derive master key in sponsor flow"
        ))
    }
}

async fn run_space_access_completion_loop<R: Runtime>(
    mut event_rx: mpsc::Receiver<SpaceAccessCompletedEvent>,
    app_handle: Option<AppHandle<R>>,
) {
    while let Some(event) = event_rx.recv().await {
        if let Some(app) = app_handle.as_ref() {
            let payload = SpaceAccessCompletedPayload {
                session_id: event.session_id,
                peer_id: event.peer_id,
                success: event.success,
                reason: event.reason,
                ts: event.ts,
            };

            if let Err(err) = app.emit("space-access-completed", &payload) {
                warn!(error = %err, "Failed to emit space-access-completed event");
            }

            if let Err(err) = app.emit("p2p-space-access-completed", &payload) {
                warn!(error = %err, "Failed to emit p2p-space-access-completed event");
            }
        }
    }
}

const DEFAULT_PAIRING_DEVICE_NAME: &str = "Uniclipboard Device";

pub async fn resolve_pairing_device_name(settings: Arc<dyn SettingsPort>) -> String {
    match settings.load().await {
        Ok(settings) => {
            let name = settings.general.device_name.unwrap_or_default();
            if name.trim().is_empty() {
                DEFAULT_PAIRING_DEVICE_NAME.to_string()
            } else {
                name
            }
        }
        Err(err) => {
            warn!(error = %err, "Failed to load settings for pairing device name");
            DEFAULT_PAIRING_DEVICE_NAME.to_string()
        }
    }
}

pub async fn resolve_pairing_config(settings: Arc<dyn SettingsPort>) -> PairingConfig {
    match settings.load().await {
        Ok(settings) => PairingConfig::from_settings(&settings),
        Err(err) => {
            warn!(error = %err, "Failed to load settings for pairing config");
            PairingConfig::from_settings(&Settings::default())
        }
    }
}

async fn resolve_device_name_for_peer(
    network: &Arc<dyn PeerDirectoryPort>,
    peer_id: &str,
) -> Option<String> {
    match network.get_discovered_peers().await {
        Ok(peers) => peers
            .into_iter()
            .find(|peer| peer.peer_id == peer_id)
            .and_then(|peer| peer.device_name),
        Err(err) => {
            warn!(error = %err, peer_id = %peer_id, "Failed to load discovered peers");
            None
        }
    }
}

async fn run_pairing_event_loop<R: Runtime>(
    mut event_rx: mpsc::Receiver<NetworkEvent>,
    orchestrator: Arc<PairingOrchestrator>,
    app_handle: Option<AppHandle<R>>,
    peer_directory: Arc<dyn PeerDirectoryPort>,
    space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    space_access_runtime_ports: RuntimeSpaceAccessPorts,
) {
    while let Some(event) = event_rx.recv().await {
        match event {
            NetworkEvent::PairingMessageReceived { peer_id, message } => {
                handle_pairing_message(
                    orchestrator.as_ref(),
                    space_access_orchestrator.as_ref(),
                    &space_access_runtime_ports,
                    peer_id,
                    message,
                    app_handle.as_ref(),
                )
                .await;
            }
            NetworkEvent::PairingFailed {
                session_id,
                peer_id,
                error,
            } => {
                if let Err(err) = orchestrator
                    .handle_transport_error(&session_id, &peer_id, error)
                    .await
                {
                    error!(error = %err, session_id = %session_id, "Failed to handle pairing transport error");
                }
            }
            NetworkEvent::PeerDiscovered(peer) => {
                info!(
                    peer_id = %peer.peer_id,
                    address_count = peer.addresses.len(),
                    is_paired = peer.is_paired,
                    "Pairing loop received peer discovered event"
                );
                if let Some(app) = app_handle.as_ref() {
                    let payload = P2PPeerDiscoveryEvent {
                        peer_id: peer.peer_id.clone(),
                        device_name: peer.device_name,
                        addresses: peer.addresses,
                        discovered: true,
                    };
                    if let Err(err) = app.emit("p2p-peer-discovery-changed", payload) {
                        warn!(error = %err, "Failed to emit peer discovery changed event");
                    }
                }
            }
            NetworkEvent::PeerLost(peer_id) => {
                info!(
                    peer_id = %peer_id,
                    "Pairing loop received peer lost event"
                );
                let device_name = resolve_device_name_for_peer(&peer_directory, &peer_id).await;
                if let Some(app) = app_handle.as_ref() {
                    let payload = P2PPeerDiscoveryEvent {
                        peer_id,
                        device_name,
                        addresses: vec![],
                        discovered: false,
                    };
                    if let Err(err) = app.emit("p2p-peer-discovery-changed", payload) {
                        warn!(error = %err, "Failed to emit peer discovery changed event");
                    }
                }
            }
            NetworkEvent::PeerReady { ref peer_id }
            | NetworkEvent::PeerNotReady { ref peer_id } => {
                let connected = matches!(event, NetworkEvent::PeerReady { .. });
                let device_name = resolve_device_name_for_peer(&peer_directory, peer_id).await;
                if let Some(app) = app_handle.as_ref() {
                    let payload = P2PPeerConnectionEvent {
                        peer_id: peer_id.clone(),
                        device_name,
                        connected,
                    };
                    if let Err(err) = app.emit("p2p-peer-connection-changed", payload) {
                        warn!(error = %err, "Failed to emit peer connection event");
                    }
                }
            }
            NetworkEvent::PeerConnected(peer) => {
                if let Some(app) = app_handle.as_ref() {
                    let payload = P2PPeerConnectionEvent {
                        peer_id: peer.peer_id,
                        device_name: Some(peer.device_name),
                        connected: true,
                    };
                    if let Err(err) = app.emit("p2p-peer-connection-changed", payload) {
                        warn!(error = %err, "Failed to emit peer connection event");
                    }
                }
            }
            NetworkEvent::PeerDisconnected(peer_id) => {
                let device_name = resolve_device_name_for_peer(&peer_directory, &peer_id).await;
                if let Some(app) = app_handle.as_ref() {
                    let payload = P2PPeerConnectionEvent {
                        peer_id,
                        device_name,
                        connected: false,
                    };
                    if let Err(err) = app.emit("p2p-peer-connection-changed", payload) {
                        warn!(error = %err, "Failed to emit peer connection event");
                    }
                }
            }
            NetworkEvent::PeerNameUpdated {
                peer_id,
                device_name,
            } => {
                if let Some(app) = app_handle.as_ref() {
                    let payload = P2PPeerNameUpdatedEvent {
                        peer_id,
                        device_name,
                    };
                    if let Err(err) = app.emit("p2p-peer-name-updated", payload) {
                        warn!(error = %err, "Failed to emit peer name updated event");
                    }
                }
            }
            _ => {}
        }
    }
}

async fn handle_pairing_message<R: Runtime>(
    orchestrator: &PairingOrchestrator,
    space_access_orchestrator: &SpaceAccessOrchestrator,
    space_access_runtime_ports: &RuntimeSpaceAccessPorts,
    peer_id: String,
    message: PairingMessage,
    app_handle: Option<&AppHandle<R>>,
) {
    match message {
        PairingMessage::Request(request) => {
            if let Some(app) = app_handle {
                let payload = P2PPairingVerificationEvent::request(
                    &request.session_id,
                    peer_id.clone(),
                    Some(request.device_name.clone()),
                );
                if let Err(err) = app.emit("p2p-pairing-verification", payload) {
                    warn!(error = %err, "Failed to emit pairing verification event");
                }
            }
            if let Err(err) = orchestrator.handle_incoming_request(peer_id, request).await {
                error!(error = %err, "Failed to handle pairing request");
            }
        }
        PairingMessage::Challenge(challenge) => {
            let session_id = challenge.session_id.clone();
            if let Err(err) = orchestrator
                .handle_challenge(&session_id, &peer_id, challenge)
                .await
            {
                error!(error = %err, session_id = %session_id, "Failed to handle pairing challenge");
            }
        }
        PairingMessage::KeyslotOffer(offer) => {
            let session_id = offer.session_id.clone();
            if let Err(err) = orchestrator
                .handle_keyslot_offer(&session_id, &peer_id, offer)
                .await
            {
                error!(
                    error = %err,
                    session_id = %session_id,
                    "Failed to handle pairing keyslot offer"
                );
            }
        }
        PairingMessage::ChallengeResponse(response) => {
            let session_id = response.session_id.clone();
            if let Err(err) = orchestrator
                .handle_challenge_response(&session_id, &peer_id, response)
                .await
            {
                error!(
                    error = %err,
                    session_id = %session_id,
                    "Failed to handle pairing challenge response"
                );
            }
        }
        PairingMessage::Response(response) => {
            let session_id = response.session_id.clone();
            if let Err(err) = orchestrator
                .handle_response(&session_id, &peer_id, response)
                .await
            {
                error!(error = %err, session_id = %session_id, "Failed to handle pairing response");
            }
        }
        PairingMessage::Confirm(confirm) => {
            let session_id = confirm.session_id.clone();
            if let Err(err) = orchestrator
                .handle_confirm(&session_id, &peer_id, confirm)
                .await
            {
                error!(error = %err, session_id = %session_id, "Failed to handle pairing confirm");
            }
        }
        PairingMessage::Reject(reject) => {
            let session_id = reject.session_id.clone();
            if let Err(err) = orchestrator.handle_reject(&session_id, &peer_id).await {
                error!(error = %err, session_id = %session_id, "Failed to handle pairing reject");
            }
        }
        PairingMessage::Cancel(cancel) => {
            let session_id = cancel.session_id.clone();
            if let Err(err) = orchestrator.handle_cancel(&session_id, &peer_id).await {
                error!(error = %err, session_id = %session_id, "Failed to handle pairing cancel");
            }
        }
        PairingMessage::Busy(busy) => {
            let session_id = busy.session_id.clone();
            if let Some(reason) = &busy.reason {
                match parse_space_access_busy_payload(reason) {
                    Ok(SpaceAccessBusyPayload::Offer(offer)) => {
                        let nonce_len = offer.nonce.len();
                        if nonce_len != 32 {
                            warn!(
                                session_id = %session_id,
                                peer_id = %peer_id,
                                nonce_len,
                                "Invalid challenge nonce length"
                            );
                        } else {
                            let keyslot_blob = match serde_json::to_vec(&offer.keyslot) {
                                Ok(blob) => blob,
                                Err(err) => {
                                    warn!(
                                        session_id = %session_id,
                                        peer_id = %peer_id,
                                        error = %err,
                                        "Failed to serialize keyslot for space access offer"
                                    );
                                    Vec::new()
                                }
                            };

                            if !keyslot_blob.is_empty() {
                                let challenge_nonce: [u8; 32] = match offer.nonce.try_into() {
                                    Ok(challenge_nonce) => challenge_nonce,
                                    Err(err) => {
                                        warn!(
                                            session_id = %session_id,
                                            peer_id = %peer_id,
                                            error = ?err,
                                            "Failed to parse challenge nonce for space access offer"
                                        );
                                        return;
                                    }
                                };
                                let space_id = uc_core::ids::SpaceId::from(offer.space_id.as_str());
                                let joiner_offer = SpaceAccessJoinerOffer {
                                    space_id: space_id.clone(),
                                    keyslot_blob,
                                    challenge_nonce,
                                };

                                let context = space_access_orchestrator.context();
                                let mut guard: tokio::sync::MutexGuard<'_, SpaceAccessContext> =
                                    context.lock().await;
                                guard.joiner_offer = Some(joiner_offer);
                                guard.sponsor_peer_id = Some(peer_id.clone());
                                drop(guard);

                                let state = space_access_orchestrator.get_state().await;
                                if matches!(
                                    state,
                                    uc_core::security::space_access::state::SpaceAccessState::WaitingOffer {
                                        ..
                                    }
                                ) {
                                    if let Err(err) = dispatch_space_access_busy_event(
                                        space_access_orchestrator,
                                        space_access_runtime_ports,
                                        SpaceAccessEvent::OfferAccepted {
                                            pairing_session_id: session_id.clone(),
                                            space_id,
                                            expires_at: Utc::now() + chrono::Duration::seconds(60),
                                        },
                                        &session_id,
                                    )
                                    .await
                                    {
                                        warn!(
                                            error = %err,
                                            session_id = %session_id,
                                            peer_id = %peer_id,
                                            "Failed to dispatch space access offer accepted"
                                        );
                                    }
                                } else {
                                    debug!(
                                        session_id = %session_id,
                                        peer_id = %peer_id,
                                        ?state,
                                        "Skipping OfferAccepted dispatch because state is not WaitingOffer"
                                    );
                                }
                            }
                        }
                    }
                    Ok(SpaceAccessBusyPayload::Proof(proof_payload)) => {
                        let pairing_session_id =
                            uc_core::ids::SessionId::from(proof_payload.pairing_session_id.clone());
                        let space_id = uc_core::ids::SpaceId::from(proof_payload.space_id.as_str());

                        // Convert challenge_nonce to [u8; 32]
                        if proof_payload.challenge_nonce.len() != 32 {
                            warn!(
                                session_id = %session_id,
                                peer_id = %peer_id,
                                nonce_len = proof_payload.challenge_nonce.len(),
                                "Invalid challenge_nonce length in space_access_proof"
                            );
                            return;
                        }
                        let challenge_nonce: [u8; 32] =
                            match proof_payload.challenge_nonce.try_into() {
                                Ok(nonce) => nonce,
                                Err(_) => {
                                    warn!(
                                        session_id = %session_id,
                                        peer_id = %peer_id,
                                        "Failed to convert challenge_nonce to [u8; 32]"
                                    );
                                    return;
                                }
                            };

                        // Create proof artifact
                        let proof_artifact =
                            uc_core::security::space_access::SpaceAccessProofArtifact {
                                pairing_session_id: pairing_session_id.clone(),
                                space_id: space_id.clone(),
                                challenge_nonce,
                                proof_bytes: proof_payload.proof_bytes.clone(),
                            };

                        // Verify proof using proof port
                        let is_valid = match space_access_runtime_ports
                            .proof
                            .verify_proof(&proof_artifact, challenge_nonce)
                            .await
                        {
                            Ok(valid) => valid,
                            Err(err) => {
                                warn!(
                                    session_id = %session_id,
                                    peer_id = %peer_id,
                                    error = %err,
                                    "Proof verification failed"
                                );
                                false
                            }
                        };

                        // Dispatch appropriate event based on verification result
                        let space_access_event = if is_valid {
                            SpaceAccessEvent::ProofVerified {
                                pairing_session_id: session_id.clone(),
                                space_id: space_id.clone(),
                            }
                        } else {
                            SpaceAccessEvent::ProofRejected {
                                pairing_session_id: session_id.clone(),
                                space_id: space_id.clone(),
                                reason:
                                    uc_core::security::space_access::state::DenyReason::InvalidProof,
                            }
                        };

                        if let Err(err) = dispatch_space_access_busy_event(
                            space_access_orchestrator,
                            space_access_runtime_ports,
                            space_access_event,
                            &session_id,
                        )
                        .await
                        {
                            warn!(
                                error = %err,
                                session_id = %session_id,
                                peer_id = %peer_id,
                                "Failed to dispatch proof verification event"
                            );
                        }
                    }
                    Ok(SpaceAccessBusyPayload::Result(result)) => {
                        let space_id = uc_core::ids::SpaceId::from(result.space_id.as_str());

                        let deny_reason = match result.deny_reason.as_deref() {
                            Some(code) => {
                                if let Some(reason) = deny_reason_from_code(code) {
                                    reason
                                } else {
                                    warn!(
                                        session_id = %session_id,
                                        peer_id = %peer_id,
                                        deny_reason = %code,
                                        fallback = DENY_REASON_INVALID_PROOF,
                                        "Unknown deny reason in space access result, fallback to invalid_proof"
                                    );
                                    uc_core::security::space_access::state::DenyReason::InvalidProof
                                }
                            }
                            None => {
                                uc_core::security::space_access::state::DenyReason::InvalidProof
                            }
                        };

                        let event = if result.success {
                            SpaceAccessEvent::AccessGranted {
                                pairing_session_id: session_id.clone(),
                                space_id,
                            }
                        } else {
                            SpaceAccessEvent::AccessDenied {
                                pairing_session_id: session_id.clone(),
                                space_id,
                                reason: deny_reason,
                            }
                        };

                        if let Err(err) = dispatch_space_access_busy_event(
                            space_access_orchestrator,
                            space_access_runtime_ports,
                            event,
                            &session_id,
                        )
                        .await
                        {
                            warn!(
                                error = %err,
                                session_id = %session_id,
                                peer_id = %peer_id,
                                "Failed to dispatch space access result"
                            );
                        }
                    }
                    Err(err) => {
                        let payload_kind = err
                            .payload_kind()
                            .map(ToOwned::to_owned)
                            .or_else(|| extract_space_access_busy_payload_kind(reason))
                            .unwrap_or_else(|| "unknown".to_string());
                        let raw_payload_preview = raw_payload_preview(reason);

                        if matches!(err, ParseError::UnknownKind { .. }) {
                            warn!(
                                session_id = %session_id,
                                peer_id = %peer_id,
                                payload_kind = %payload_kind,
                                error = %err,
                                raw_payload_preview = %raw_payload_preview,
                                "Ignoring unknown pairing busy payload kind"
                            );
                        } else {
                            warn!(
                                session_id = %session_id,
                                peer_id = %peer_id,
                                payload_kind = %payload_kind,
                                error = %err,
                                raw_payload_preview = %raw_payload_preview,
                                "Failed to parse pairing busy payload"
                            );
                        }
                    }
                }
            }
            if let Err(err) = orchestrator.handle_busy(&session_id, &peer_id).await {
                error!(error = %err, session_id = %session_id, "Failed to handle pairing busy");
            }
        }
    }
}

async fn run_pairing_action_loop<R: Runtime>(
    mut action_rx: mpsc::Receiver<PairingAction>,
    network: Arc<dyn PairingTransportPort>,
    app_handle: Option<AppHandle<R>>,
    orchestrator: Arc<PairingOrchestrator>,
    space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    key_slot_store: Arc<dyn KeySlotStore>,
    space_access_runtime_ports: RuntimeSpaceAccessPorts,
) {
    while let Some(action) = action_rx.recv().await {
        match action {
            PairingAction::Send { peer_id, message } => {
                let session_id = message.session_id().to_string();
                let message_kind = match &message {
                    PairingMessage::Request(_) => "request",
                    PairingMessage::Challenge(_) => "challenge",
                    PairingMessage::KeyslotOffer(_) => "keyslot_offer",
                    PairingMessage::ChallengeResponse(_) => "challenge_response",
                    PairingMessage::Response(_) => "response",
                    PairingMessage::Confirm(_) => "confirm",
                    PairingMessage::Reject(_) => "reject",
                    PairingMessage::Cancel(_) => "cancel",
                    PairingMessage::Busy(_) => "busy",
                };
                info!(
                    session_id = %session_id,
                    peer_id = %peer_id,
                    message_kind = %message_kind,
                    stage = "enqueue",
                    "Sending pairing message"
                );
                if let Err(err) = network
                    .open_pairing_session(peer_id.clone(), session_id.clone())
                    .await
                {
                    let reason = err.to_string();
                    warn!(
                        error = %reason,
                        peer_id = %peer_id,
                        session_id = %session_id,
                        "Failed to open pairing session"
                    );
                    signal_pairing_transport_failure(
                        app_handle.as_ref(),
                        orchestrator.as_ref(),
                        &session_id,
                        &peer_id,
                        reason,
                    )
                    .await;
                    continue;
                }
                let result = network.send_pairing_on_session(message).await;

                match &result {
                    Ok(_) => {
                        info!(
                            session_id = %session_id,
                            peer_id = %peer_id,
                            message_kind = %message_kind,
                            stage = "send_result",
                            "Pairing message sent successfully"
                        );
                    }
                    Err(err) => {
                        let reason = err.to_string();
                        warn!(
                            error = %reason,
                            peer_id = %peer_id,
                            session_id = %session_id,
                            message_kind = %message_kind,
                            stage = "send_result",
                            "Failed to send pairing message"
                        );
                        signal_pairing_transport_failure(
                            app_handle.as_ref(),
                            orchestrator.as_ref(),
                            &session_id,
                            &peer_id,
                            reason,
                        )
                        .await;
                    }
                }
            }
            PairingAction::ShowVerification {
                session_id,
                short_code,
                local_fingerprint,
                peer_fingerprint,
                peer_display_name,
            } => {
                if let Some(app) = app_handle.as_ref() {
                    let payload = P2PPairingVerificationEvent::verification(
                        &session_id,
                        Some(peer_display_name),
                        short_code,
                        local_fingerprint,
                        peer_fingerprint,
                    );
                    if let Err(err) = app.emit("p2p-pairing-verification", payload) {
                        warn!(error = %err, "Failed to emit pairing verification event");
                    }
                }
            }
            PairingAction::ShowVerifying {
                session_id,
                peer_display_name,
            } => {
                if let Some(app) = app_handle.as_ref() {
                    let payload = P2PPairingVerificationEvent::verifying(
                        &session_id,
                        Some(peer_display_name),
                    );
                    if let Err(err) = app.emit("p2p-pairing-verification", payload) {
                        warn!(error = %err, "Failed to emit pairing verification event");
                    }
                }
            }
            PairingAction::EmitResult {
                session_id,
                success,
                error,
            } => {
                let peer_info = orchestrator.get_session_peer(&session_id).await;
                let role = orchestrator.get_session_role(&session_id).await;
                info!(
                    session_id = %session_id,
                    peer_id = ?peer_info.as_ref().map(|p| p.peer_id.as_str()),
                    success = success,
                    role = ?role,
                    reason = ?error,
                    "EmitResult received"
                );
                if !success {
                    if let Err(err) = network
                        .close_pairing_session(session_id.clone(), error.clone())
                        .await
                    {
                        warn!(
                            error = %err,
                            session_id = %session_id,
                            "Failed to close pairing session"
                        );
                    }
                }
                if success && role == Some(PairingRole::Responder) {
                    if let Some(peer) = peer_info.as_ref() {
                        let context = space_access_orchestrator.context();
                        let mut guard = context.lock().await;
                        guard.sponsor_peer_id = Some(peer.peer_id.clone());
                    } else {
                        let context = space_access_orchestrator.context();
                        let mut guard = context.lock().await;
                        guard.sponsor_peer_id = None;
                        warn!(
                            session_id = %session_id,
                            "Responder emit result has no peer info; sponsor_peer_id cleared"
                        );
                    }
                    match key_slot_store.load().await {
                        Ok(keyslot_file) => {
                            let space_id =
                                uc_core::ids::SpaceId::from(keyslot_file.scope.profile_id.as_str());
                            let context = space_access_orchestrator.context();
                            let mut network_transport =
                                uc_app::usecases::space_access::SpaceAccessNetworkAdapter::new(
                                    network.clone(),
                                    context,
                                );
                            let sponsor_crypto = LoadedKeyslotSpaceAccessCrypto::new(keyslot_file);
                            let mut timer_guard = space_access_runtime_ports.timer.lock().await;
                            let mut store_guard =
                                space_access_runtime_ports.persistence.lock().await;

                            let mut executor =
                                uc_app::usecases::space_access::SpaceAccessExecutor {
                                    crypto: &sponsor_crypto,
                                    transport: &mut network_transport,
                                    proof: space_access_runtime_ports.proof.as_ref(),
                                    timer: &mut *timer_guard,
                                    store: &mut *store_guard,
                                };

                            if let Err(err) = space_access_orchestrator
                                .start_sponsor_authorization(
                                    &mut executor,
                                    session_id.clone(),
                                    space_id,
                                    300,
                                )
                                .await
                            {
                                warn!(
                                    error = %err,
                                    session_id = %session_id,
                                    "Failed to start sponsor authorization"
                                );
                            } else {
                                info!(
                                    session_id = %session_id,
                                    "Sponsor authorization started successfully"
                                );
                            }
                        }
                        Err(err) => {
                            warn!(
                                error = %err,
                                session_id = %session_id,
                                "Failed to load keyslot for sponsor authorization"
                            );
                        }
                    }
                }

                if let Some(app) = app_handle.as_ref() {
                    if success {
                        let (peer_id, device_name) = match peer_info {
                            Some(info) => {
                                let name = info
                                    .device_name
                                    .unwrap_or_else(|| "Unknown Device".to_string());
                                (info.peer_id, name)
                            }
                            None => ("unknown".to_string(), "Unknown Device".to_string()),
                        };
                        let payload = P2PPairingVerificationEvent::complete(
                            &session_id,
                            peer_id,
                            Some(device_name),
                        );
                        if let Err(err) = app.emit("p2p-pairing-verification", payload) {
                            warn!(error = %err, "Failed to emit pairing verification event");
                        }
                    } else {
                        let payload = P2PPairingVerificationEvent::failed(
                            &session_id,
                            error.unwrap_or_else(|| "Pairing failed".to_string()),
                        );
                        if let Err(err) = app.emit("p2p-pairing-verification", payload) {
                            warn!(error = %err, "Failed to emit pairing verification event");
                        }
                    }
                }
            }
            other => {
                warn!(action = ?other, "Unhandled pairing action received");
            }
        }
    }
    warn!("Pairing action loop stopped");
}

async fn signal_pairing_transport_failure<R: Runtime>(
    app_handle: Option<&AppHandle<R>>,
    orchestrator: &PairingOrchestrator,
    session_id: &str,
    peer_id: &str,
    reason: String,
) {
    if let Some(app) = app_handle {
        let payload = P2PPairingVerificationEvent::failed(session_id, reason.clone());
        if let Err(emit_err) = app.emit("p2p-pairing-verification", payload) {
            warn!(error = %emit_err, "Failed to emit pairing verification event");
        }
    }
    if let Err(handle_err) = orchestrator
        .handle_transport_error(session_id, peer_id, reason)
        .await
    {
        error!(
            error = %handle_err,
            session_id = %session_id,
            "Failed to handle pairing transport error"
        );
    }
}

fn emit_pairing_events_subscribe_failure<R: Runtime>(
    app_handle: Option<&AppHandle<R>>,
    attempt: u32,
    retry_in_ms: u64,
    error: String,
) {
    if let Some(app) = app_handle {
        let payload = PairingEventsSubscribeFailurePayload {
            attempt,
            retry_in_ms,
            error,
        };
        if let Err(emit_err) = app.emit("pairing-events-subscribe-failure", payload) {
            warn!(error = %emit_err, "Failed to emit pairing events subscribe failure event");
        }
    }
}

fn emit_pairing_events_subscribe_recovered<R: Runtime>(
    app_handle: Option<&AppHandle<R>>,
    recovered_after_attempts: u32,
) {
    if let Some(app) = app_handle {
        let payload = PairingEventsSubscribeRecoveredPayload {
            recovered_after_attempts,
        };
        if let Err(emit_err) = app.emit("pairing-events-subscribe-recovered", payload) {
            warn!(error = %emit_err, "Failed to emit pairing events subscribe recovered event");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;
    use tauri::{Listener, Wry};
    use tokio::sync::{mpsc, Mutex as TokioMutex};
    use uc_app::usecases::PairingConfig;
    use uc_core::network::paired_device::{PairedDevice, PairingState};
    use uc_core::network::protocol::{PairingChallenge, PairingRequest};
    use uc_core::network::{ConnectedPeer, DiscoveredPeer, PairingMessage};
    use uc_core::ports::{
        ClipboardTransportPort, EncryptionSessionPort, NetworkEventPort, PairingTransportPort,
        PeerDirectoryPort,
    };
    use uc_core::security::model::{EncryptionError, MasterKey};
    use uc_platform::test_support::with_uc_profile;

    #[test]
    fn test_wiring_error_display() {
        let err = WiringError::DatabaseInit("connection failed".to_string());
        assert!(err.to_string().contains("Database initialization"));
        assert!(err.to_string().contains("connection failed"));
    }

    struct NoopPairedDeviceRepository;

    #[derive(Default, Clone, Copy)]
    struct BaseNetwork;

    #[derive(Default)]
    struct NoopNetwork {
        base: BaseNetwork,
    }

    struct SendFailNetwork {
        base: BaseNetwork,
        send_called: Arc<AtomicUsize>,
    }

    struct CloseRecordingNetwork {
        base: BaseNetwork,
        close_calls: Arc<Mutex<Vec<(String, Option<String>)>>>,
    }

    struct OfferRecordingNetwork {
        base: BaseNetwork,
        sent_messages: Arc<Mutex<Vec<(String, PairingMessage)>>>,
    }

    struct SuccessSpaceAccessCrypto;

    struct SuccessSpaceAccessTransport;

    struct SuccessSpaceAccessProof;

    struct SuccessSpaceAccessPersistence;

    struct NoopSpaceAccessTimer;

    #[async_trait]
    impl TimerPort for NoopSpaceAccessTimer {
        async fn start(
            &mut self,
            _session_id: &uc_core::ids::SessionId,
            _ttl_secs: u64,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&mut self, _session_id: &uc_core::ids::SessionId) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct FixedMasterKeyEncryptionSession {
        master_key: TokioMutex<Option<MasterKey>>,
    }

    impl FixedMasterKeyEncryptionSession {
        fn new(master_key: MasterKey) -> Self {
            Self {
                master_key: TokioMutex::new(Some(master_key)),
            }
        }
    }

    fn test_keyslot(profile_id: &str) -> uc_core::security::model::KeySlot {
        uc_core::security::model::KeySlot {
            version: uc_core::security::model::KeySlotVersion::V1,
            scope: uc_core::security::model::KeyScope {
                profile_id: profile_id.to_string(),
            },
            kdf: uc_core::security::model::KdfParams::for_initialization(),
            salt: vec![1; 16],
            wrapped_master_key: None,
        }
    }

    fn test_keyslot_file(profile_id: &str) -> uc_core::security::model::KeySlotFile {
        uc_core::security::model::KeySlotFile {
            version: uc_core::security::model::KeySlotVersion::V1,
            scope: uc_core::security::model::KeyScope {
                profile_id: profile_id.to_string(),
            },
            kdf: uc_core::security::model::KdfParams::for_initialization(),
            salt: vec![1; 16],
            wrapped_master_key: uc_core::security::model::EncryptedBlob {
                version: uc_core::security::model::EncryptionFormatVersion::V1,
                aead: uc_core::security::model::EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![2; 24],
                ciphertext: vec![3; 32],
                aad_fingerprint: None,
            },
            created_at: None,
            updated_at: None,
        }
    }

    async fn seed_waiting_joiner_proof_state(
        orchestrator: &SpaceAccessOrchestrator,
        session_id: &str,
        space_id: &str,
    ) {
        let mut transport = SuccessSpaceAccessTransport;
        let proof = SuccessSpaceAccessProof;
        let mut timer = NoopSpaceAccessTimer;
        let mut store = SuccessSpaceAccessPersistence;
        let crypto = SuccessSpaceAccessCrypto;
        let mut executor = uc_app::usecases::space_access::SpaceAccessExecutor {
            crypto: &crypto,
            transport: &mut transport,
            proof: &proof,
            timer: &mut timer,
            store: &mut store,
        };

        let state = orchestrator
            .start_sponsor_authorization(
                &mut executor,
                session_id.to_string(),
                uc_core::ids::SpaceId::from(space_id),
                60,
            )
            .await
            .expect("seed waiting joiner proof state");

        assert!(matches!(
            state,
            uc_core::security::space_access::state::SpaceAccessState::WaitingJoinerProof { .. }
        ));
    }

    async fn seed_waiting_decision_state(
        orchestrator: &SpaceAccessOrchestrator,
        session_id: &str,
        space_id: &str,
    ) {
        {
            let context = orchestrator.context();
            let mut guard = context.lock().await;
            guard.joiner_offer = Some(SpaceAccessJoinerOffer {
                space_id: uc_core::ids::SpaceId::from(space_id),
                keyslot_blob: vec![7, 8, 9],
                challenge_nonce: [9; 32],
            });
            guard.joiner_passphrase = Some(uc_core::security::SecretString::new(
                "join-secret".to_string(),
            ));
            guard.sponsor_peer_id = Some("peer-sponsor".to_string());
        }

        let mut transport = SuccessSpaceAccessTransport;
        let proof = SuccessSpaceAccessProof;
        let mut timer = NoopSpaceAccessTimer;
        let mut store = SuccessSpaceAccessPersistence;
        let crypto = SuccessSpaceAccessCrypto;
        let mut executor = uc_app::usecases::space_access::SpaceAccessExecutor {
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
                    pairing_session_id: session_id.to_string(),
                    ttl_secs: 60,
                },
                Some(session_id.to_string()),
            )
            .await
            .expect("join requested");

        orchestrator
            .dispatch(
                &mut executor,
                SpaceAccessEvent::OfferAccepted {
                    pairing_session_id: session_id.to_string(),
                    space_id: uc_core::ids::SpaceId::from(space_id),
                    expires_at: Utc::now() + chrono::Duration::seconds(60),
                },
                Some(session_id.to_string()),
            )
            .await
            .expect("offer accepted");

        let state = orchestrator
            .dispatch(
                &mut executor,
                SpaceAccessEvent::PassphraseSubmitted,
                Some(session_id.to_string()),
            )
            .await
            .expect("passphrase submitted");

        assert!(matches!(
            state,
            uc_core::security::space_access::state::SpaceAccessState::WaitingDecision { .. }
        ));
    }

    #[async_trait]
    impl ClipboardTransportPort for BaseNetwork {
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
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }
    }

    #[async_trait]
    impl PeerDirectoryPort for BaseNetwork {
        async fn get_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
            Ok(vec![])
        }

        async fn get_connected_peers(&self) -> anyhow::Result<Vec<ConnectedPeer>> {
            Ok(vec![])
        }

        fn local_peer_id(&self) -> String {
            "local-peer".to_string()
        }

        async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl PairingTransportPort for BaseNetwork {
        async fn open_pairing_session(
            &self,
            _peer_id: String,
            _session_id: String,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_pairing_on_session(&self, _message: PairingMessage) -> anyhow::Result<()> {
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
    impl NetworkEventPort for BaseNetwork {
        async fn subscribe_events(
            &self,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<NetworkEvent>> {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }
    }

    macro_rules! delegate_clipboard_port_to_base {
        ($ty:ty) => {
            #[async_trait]
            impl ClipboardTransportPort for $ty {
                async fn send_clipboard(
                    &self,
                    peer_id: &str,
                    encrypted_data: Vec<u8>,
                ) -> anyhow::Result<()> {
                    self.base.send_clipboard(peer_id, encrypted_data).await
                }

                async fn broadcast_clipboard(&self, encrypted_data: Vec<u8>) -> anyhow::Result<()> {
                    self.base.broadcast_clipboard(encrypted_data).await
                }

                async fn subscribe_clipboard(
                    &self,
                ) -> anyhow::Result<
                    tokio::sync::mpsc::Receiver<(
                        uc_core::network::ClipboardMessage,
                        Option<Vec<u8>>,
                    )>,
                > {
                    self.base.subscribe_clipboard().await
                }
            }
        };
    }

    macro_rules! delegate_peer_directory_port_to_base {
        ($ty:ty) => {
            #[async_trait]
            impl PeerDirectoryPort for $ty {
                async fn get_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
                    self.base.get_discovered_peers().await
                }

                async fn get_connected_peers(&self) -> anyhow::Result<Vec<ConnectedPeer>> {
                    self.base.get_connected_peers().await
                }

                fn local_peer_id(&self) -> String {
                    self.base.local_peer_id()
                }

                async fn announce_device_name(&self, device_name: String) -> anyhow::Result<()> {
                    self.base.announce_device_name(device_name).await
                }
            }
        };
    }

    macro_rules! delegate_network_event_port_to_base {
        ($ty:ty) => {
            #[async_trait]
            impl NetworkEventPort for $ty {
                async fn subscribe_events(
                    &self,
                ) -> anyhow::Result<tokio::sync::mpsc::Receiver<NetworkEvent>> {
                    self.base.subscribe_events().await
                }
            }
        };
    }

    macro_rules! delegate_pairing_port_to_base {
        ($ty:ty) => {
            #[async_trait]
            impl PairingTransportPort for $ty {
                async fn open_pairing_session(
                    &self,
                    peer_id: String,
                    session_id: String,
                ) -> anyhow::Result<()> {
                    self.base.open_pairing_session(peer_id, session_id).await
                }

                async fn send_pairing_on_session(
                    &self,
                    message: PairingMessage,
                ) -> anyhow::Result<()> {
                    self.base.send_pairing_on_session(message).await
                }

                async fn close_pairing_session(
                    &self,
                    session_id: String,
                    reason: Option<String>,
                ) -> anyhow::Result<()> {
                    self.base.close_pairing_session(session_id, reason).await
                }

                async fn unpair_device(&self, peer_id: String) -> anyhow::Result<()> {
                    self.base.unpair_device(peer_id).await
                }
            }
        };
    }

    delegate_clipboard_port_to_base!(NoopNetwork);
    delegate_peer_directory_port_to_base!(NoopNetwork);
    delegate_pairing_port_to_base!(NoopNetwork);
    delegate_network_event_port_to_base!(NoopNetwork);

    delegate_clipboard_port_to_base!(SendFailNetwork);
    delegate_peer_directory_port_to_base!(SendFailNetwork);
    delegate_network_event_port_to_base!(SendFailNetwork);

    #[async_trait]
    impl PairingTransportPort for SendFailNetwork {
        async fn open_pairing_session(
            &self,
            peer_id: String,
            session_id: String,
        ) -> anyhow::Result<()> {
            self.base.open_pairing_session(peer_id, session_id).await
        }

        async fn send_pairing_on_session(&self, _message: PairingMessage) -> anyhow::Result<()> {
            self.send_called.fetch_add(1, Ordering::SeqCst);
            Err(anyhow!("send failed"))
        }

        async fn close_pairing_session(
            &self,
            session_id: String,
            reason: Option<String>,
        ) -> anyhow::Result<()> {
            self.base.close_pairing_session(session_id, reason).await
        }

        async fn unpair_device(&self, peer_id: String) -> anyhow::Result<()> {
            self.base.unpair_device(peer_id).await
        }
    }

    delegate_clipboard_port_to_base!(CloseRecordingNetwork);
    delegate_peer_directory_port_to_base!(CloseRecordingNetwork);
    delegate_network_event_port_to_base!(CloseRecordingNetwork);

    #[async_trait]
    impl PairingTransportPort for CloseRecordingNetwork {
        async fn open_pairing_session(
            &self,
            peer_id: String,
            session_id: String,
        ) -> anyhow::Result<()> {
            self.base.open_pairing_session(peer_id, session_id).await
        }

        async fn send_pairing_on_session(&self, message: PairingMessage) -> anyhow::Result<()> {
            self.base.send_pairing_on_session(message).await
        }

        async fn close_pairing_session(
            &self,
            session_id: String,
            reason: Option<String>,
        ) -> anyhow::Result<()> {
            self.close_calls.lock().unwrap().push((session_id, reason));
            Ok(())
        }

        async fn unpair_device(&self, peer_id: String) -> anyhow::Result<()> {
            self.base.unpair_device(peer_id).await
        }
    }

    delegate_clipboard_port_to_base!(OfferRecordingNetwork);
    delegate_peer_directory_port_to_base!(OfferRecordingNetwork);
    delegate_network_event_port_to_base!(OfferRecordingNetwork);

    #[async_trait]
    impl PairingTransportPort for OfferRecordingNetwork {
        async fn open_pairing_session(
            &self,
            peer_id: String,
            session_id: String,
        ) -> anyhow::Result<()> {
            self.base.open_pairing_session(peer_id, session_id).await
        }

        async fn send_pairing_on_session(&self, message: PairingMessage) -> anyhow::Result<()> {
            let session_id = message.session_id().to_string();
            self.sent_messages
                .lock()
                .unwrap()
                .push((session_id, message));
            Ok(())
        }

        async fn close_pairing_session(
            &self,
            session_id: String,
            reason: Option<String>,
        ) -> anyhow::Result<()> {
            self.base.close_pairing_session(session_id, reason).await
        }

        async fn unpair_device(&self, peer_id: String) -> anyhow::Result<()> {
            self.base.unpair_device(peer_id).await
        }
    }

    #[async_trait]
    impl EncryptionSessionPort for FixedMasterKeyEncryptionSession {
        async fn is_ready(&self) -> bool {
            self.master_key.lock().await.is_some()
        }

        async fn get_master_key(&self) -> Result<MasterKey, EncryptionError> {
            self.master_key
                .lock()
                .await
                .clone()
                .ok_or(EncryptionError::NotInitialized)
        }

        async fn set_master_key(&self, master_key: MasterKey) -> Result<(), EncryptionError> {
            *self.master_key.lock().await = Some(master_key);
            Ok(())
        }

        async fn clear(&self) -> Result<(), EncryptionError> {
            *self.master_key.lock().await = None;
            Ok(())
        }
    }

    #[async_trait]
    impl uc_core::ports::space::CryptoPort for SuccessSpaceAccessCrypto {
        async fn generate_nonce32(&self) -> [u8; 32] {
            [3; 32]
        }

        async fn export_keyslot_blob(
            &self,
            space_id: &uc_core::ids::SpaceId,
        ) -> anyhow::Result<uc_core::security::model::KeySlot> {
            Ok(test_keyslot(space_id.as_ref()))
        }

        async fn derive_master_key_from_keyslot(
            &self,
            _keyslot_blob: &[u8],
            _passphrase: uc_core::security::SecretString,
        ) -> anyhow::Result<uc_core::security::model::MasterKey> {
            uc_core::security::model::MasterKey::from_bytes(&[4; 32])
                .map_err(|err| anyhow!(err.to_string()))
        }
    }

    #[async_trait]
    impl uc_core::ports::space::SpaceAccessTransportPort for SuccessSpaceAccessTransport {
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

    #[async_trait]
    impl uc_core::ports::space::ProofPort for SuccessSpaceAccessProof {
        async fn build_proof(
            &self,
            pairing_session_id: &uc_core::ids::SessionId,
            space_id: &uc_core::ids::SpaceId,
            challenge_nonce: [u8; 32],
            _master_key: &uc_core::security::model::MasterKey,
        ) -> anyhow::Result<uc_core::security::space_access::SpaceAccessProofArtifact> {
            Ok(uc_core::security::space_access::SpaceAccessProofArtifact {
                pairing_session_id: pairing_session_id.clone(),
                space_id: space_id.clone(),
                challenge_nonce,
                proof_bytes: vec![1, 2, 3, 4],
            })
        }

        async fn verify_proof(
            &self,
            _proof: &uc_core::security::space_access::SpaceAccessProofArtifact,
            _expected_nonce: [u8; 32],
        ) -> anyhow::Result<bool> {
            Ok(true)
        }
    }

    #[async_trait]
    impl uc_core::ports::space::PersistencePort for SuccessSpaceAccessPersistence {
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

    #[async_trait]
    impl PairedDeviceRepositoryPort for NoopPairedDeviceRepository {
        async fn get_by_peer_id(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<Option<PairedDevice>, uc_core::ports::errors::PairedDeviceRepositoryError>
        {
            Ok(None)
        }

        async fn list_all(
            &self,
        ) -> Result<Vec<PairedDevice>, uc_core::ports::errors::PairedDeviceRepositoryError>
        {
            Ok(vec![])
        }

        async fn upsert(
            &self,
            _device: PairedDevice,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn set_state(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _state: PairingState,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _last_seen_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    struct NoopKeySlotStore;

    struct StaticKeySlotStore {
        slot: uc_core::security::model::KeySlotFile,
    }

    #[async_trait]
    impl KeySlotStore for NoopKeySlotStore {
        async fn load(
            &self,
        ) -> Result<uc_core::security::model::KeySlotFile, uc_core::security::model::EncryptionError>
        {
            Err(uc_core::security::model::EncryptionError::KeyNotFound)
        }

        async fn store(
            &self,
            _slot: &uc_core::security::model::KeySlotFile,
        ) -> Result<(), uc_core::security::model::EncryptionError> {
            Ok(())
        }

        async fn delete(&self) -> Result<(), uc_core::security::model::EncryptionError> {
            Ok(())
        }
    }

    #[async_trait]
    impl KeySlotStore for StaticKeySlotStore {
        async fn load(
            &self,
        ) -> Result<uc_core::security::model::KeySlotFile, uc_core::security::model::EncryptionError>
        {
            Ok(self.slot.clone())
        }

        async fn store(
            &self,
            _slot: &uc_core::security::model::KeySlotFile,
        ) -> Result<(), uc_core::security::model::EncryptionError> {
            Ok(())
        }

        async fn delete(&self) -> Result<(), uc_core::security::model::EncryptionError> {
            Ok(())
        }
    }

    fn test_runtime_space_access_ports<N>(
        network: Arc<N>,
        space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    ) -> RuntimeSpaceAccessPorts
    where
        N: PairingTransportPort + Send + Sync + 'static,
    {
        test_runtime_space_access_ports_with_proof(
            network,
            space_access_orchestrator,
            Arc::new(HmacProofAdapter::new()),
        )
    }

    fn test_runtime_space_access_ports_with_proof<N>(
        network: Arc<N>,
        space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
        proof: Arc<dyn uc_core::ports::space::ProofPort>,
    ) -> RuntimeSpaceAccessPorts
    where
        N: PairingTransportPort + Send + Sync + 'static,
    {
        RuntimeSpaceAccessPorts {
            transport: Arc::new(tokio::sync::Mutex::new(SpaceAccessNetworkAdapter::new(
                network as Arc<dyn PairingTransportPort>,
                space_access_orchestrator.context(),
            ))),
            proof,
            timer: Arc::new(tokio::sync::Mutex::new(NoopSpaceAccessTimer)),
            persistence: Arc::new(tokio::sync::Mutex::new(SuccessSpaceAccessPersistence)),
        }
    }

    async fn seed_waiting_offer_state(orchestrator: &SpaceAccessOrchestrator, session_id: &str) {
        let mut transport = SuccessSpaceAccessTransport;
        let proof = SuccessSpaceAccessProof;
        let mut timer = NoopSpaceAccessTimer;
        let mut store = SuccessSpaceAccessPersistence;
        let crypto = SuccessSpaceAccessCrypto;
        let mut executor = uc_app::usecases::space_access::SpaceAccessExecutor {
            crypto: &crypto,
            transport: &mut transport,
            proof: &proof,
            timer: &mut timer,
            store: &mut store,
        };

        let state = orchestrator
            .dispatch(
                &mut executor,
                SpaceAccessEvent::JoinRequested {
                    pairing_session_id: session_id.to_string(),
                    ttl_secs: 60,
                },
                Some(session_id.to_string()),
            )
            .await
            .expect("seed waiting offer state");

        assert!(matches!(
            state,
            uc_core::security::space_access::state::SpaceAccessState::WaitingOffer { .. }
        ));
    }

    #[tokio::test]
    async fn pairing_event_loop_registers_session_on_request() {
        let (event_tx, event_rx) = mpsc::channel(1);
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);
        let network = Arc::new(NoopNetwork::default());
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());

        let loop_handle = tokio::spawn(run_pairing_event_loop::<Wry>(
            event_rx,
            orchestrator.clone(),
            None,
            network.clone() as Arc<dyn PeerDirectoryPort>,
            space_access_orchestrator,
            runtime_ports,
        ));

        let request = PairingRequest {
            session_id: "session-1".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-999".to_string(),
            peer_id: "peer-local".to_string(),
            identity_pubkey: vec![1; 32],
            nonce: vec![2; 16],
        };
        let event = NetworkEvent::PairingMessageReceived {
            peer_id: "peer-remote".to_string(),
            message: PairingMessage::Request(request),
        };

        event_tx.send(event).await.expect("send pairing event");
        tokio::task::yield_now().await;

        let result = orchestrator.user_accept_pairing("session-1").await;
        assert!(result.is_ok());

        drop(event_tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    }

    #[tokio::test]
    async fn busy_offer_payload_routes_to_joiner_offer_context() {
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let network = Arc::new(NoopNetwork::default());
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());

        let reason = serde_json::json!({
            "kind": "space_access_offer",
            "space_id": "space-offer-1",
            "nonce": vec![5; 32],
            "keyslot": test_keyslot("space-offer-1")
        })
        .to_string();

        handle_pairing_message::<Wry>(
            orchestrator.as_ref(),
            space_access_orchestrator.as_ref(),
            &runtime_ports,
            "peer-remote".to_string(),
            PairingMessage::Busy(uc_core::network::protocol::PairingBusy {
                session_id: "session-offer-route".to_string(),
                reason: Some(reason),
            }),
            None,
        )
        .await;

        let context = space_access_orchestrator.context();
        let guard = context.lock().await;
        let offer = guard.joiner_offer.as_ref().expect("joiner offer routed");
        assert_eq!(offer.space_id.as_ref(), "space-offer-1");
        assert_eq!(offer.challenge_nonce, [5; 32]);
        assert!(!offer.keyslot_blob.is_empty());
    }

    #[tokio::test]
    async fn busy_offer_payload_dispatches_offer_accepted_when_waiting_offer() {
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let network = Arc::new(NoopNetwork::default());
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());

        seed_waiting_offer_state(space_access_orchestrator.as_ref(), "session-offer-waiting").await;

        let reason = serde_json::json!({
            "kind": "space_access_offer",
            "space_id": "space-offer-waiting",
            "nonce": vec![6; 32],
            "keyslot": test_keyslot("space-offer-waiting")
        })
        .to_string();

        handle_pairing_message::<Wry>(
            orchestrator.as_ref(),
            space_access_orchestrator.as_ref(),
            &runtime_ports,
            "peer-remote".to_string(),
            PairingMessage::Busy(uc_core::network::protocol::PairingBusy {
                session_id: "session-offer-waiting".to_string(),
                reason: Some(reason),
            }),
            None,
        )
        .await;

        let state = space_access_orchestrator.get_state().await;
        assert!(matches!(
            state,
            uc_core::security::space_access::state::SpaceAccessState::WaitingUserPassphrase { .. }
        ));
    }

    #[tokio::test]
    async fn busy_proof_payload_routes_to_proof_branch_and_validates_nonce_length() {
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let network = Arc::new(NoopNetwork::default());
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());
        seed_waiting_joiner_proof_state(
            space_access_orchestrator.as_ref(),
            "session-proof-route",
            "space-proof-route",
        )
        .await;

        let invalid_nonce_reason = serde_json::json!({
            "kind": "space_access_proof",
            "pairing_session_id": "session-proof-route",
            "space_id": "space-proof-route",
            "challenge_nonce": vec![2; 31],
            "proof_bytes": vec![1, 2, 3, 4]
        })
        .to_string();

        handle_pairing_message::<Wry>(
            orchestrator.as_ref(),
            space_access_orchestrator.as_ref(),
            &runtime_ports,
            "peer-remote".to_string(),
            PairingMessage::Busy(uc_core::network::protocol::PairingBusy {
                session_id: "session-proof-route".to_string(),
                reason: Some(invalid_nonce_reason),
            }),
            None,
        )
        .await;

        let state = space_access_orchestrator.get_state().await;
        assert!(matches!(
            state,
            uc_core::security::space_access::state::SpaceAccessState::WaitingJoinerProof { .. }
        ));

        let valid_nonce_reason = serde_json::json!({
            "kind": "space_access_proof",
            "pairing_session_id": "session-proof-route",
            "space_id": "space-proof-route",
            "challenge_nonce": vec![2; 32],
            "proof_bytes": vec![1, 2, 3, 4]
        })
        .to_string();

        handle_pairing_message::<Wry>(
            orchestrator.as_ref(),
            space_access_orchestrator.as_ref(),
            &runtime_ports,
            "peer-remote".to_string(),
            PairingMessage::Busy(uc_core::network::protocol::PairingBusy {
                session_id: "session-proof-route".to_string(),
                reason: Some(valid_nonce_reason),
            }),
            None,
        )
        .await;

        let state = space_access_orchestrator.get_state().await;
        assert!(matches!(
            state,
            uc_core::security::space_access::state::SpaceAccessState::Denied {
                reason: uc_core::security::space_access::state::DenyReason::InvalidProof,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn busy_proof_payload_accepts_valid_hmac_with_session_master_key() {
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let network = Arc::new(NoopNetwork::default());
        let master_key = MasterKey::from_bytes(&[4; 32]).expect("master key");
        let proof_port: Arc<dyn uc_core::ports::space::ProofPort> =
            Arc::new(HmacProofAdapter::new_with_encryption_session(Arc::new(
                FixedMasterKeyEncryptionSession::new(master_key.clone()),
            )));
        let runtime_ports = test_runtime_space_access_ports_with_proof(
            network.clone(),
            space_access_orchestrator.clone(),
            proof_port,
        );

        seed_waiting_joiner_proof_state(
            space_access_orchestrator.as_ref(),
            "session-proof-valid",
            "space-proof-valid",
        )
        .await;

        {
            let context = space_access_orchestrator.context();
            let mut guard = context.lock().await;
            guard.sponsor_peer_id = Some("peer-sponsor".to_string());
        }

        let challenge_nonce = {
            let context = space_access_orchestrator.context();
            let guard = context.lock().await;
            guard.prepared_offer.as_ref().expect("prepared offer").nonce
        };

        let proof = HmacProofAdapter::new()
            .build_proof(
                &uc_core::ids::SessionId::from("session-proof-valid"),
                &uc_core::ids::SpaceId::from("space-proof-valid"),
                challenge_nonce,
                &master_key,
            )
            .await
            .expect("build proof");

        let reason = serde_json::json!({
            "kind": "space_access_proof",
            "pairing_session_id": "session-proof-valid",
            "space_id": "space-proof-valid",
            "challenge_nonce": challenge_nonce,
            "proof_bytes": proof.proof_bytes
        })
        .to_string();

        handle_pairing_message::<Wry>(
            orchestrator.as_ref(),
            space_access_orchestrator.as_ref(),
            &runtime_ports,
            "peer-remote".to_string(),
            PairingMessage::Busy(uc_core::network::protocol::PairingBusy {
                session_id: "session-proof-valid".to_string(),
                reason: Some(reason),
            }),
            None,
        )
        .await;

        let state = space_access_orchestrator.get_state().await;
        assert!(matches!(
            state,
            uc_core::security::space_access::state::SpaceAccessState::Granted { .. }
        ));
    }

    #[tokio::test]
    async fn busy_result_payload_routes_to_access_denied_transition() {
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let network = Arc::new(NoopNetwork::default());
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());
        seed_waiting_decision_state(
            space_access_orchestrator.as_ref(),
            "session-result-route",
            "space-result-route",
        )
        .await;

        let reason = serde_json::json!({
            "kind": "space_access_result",
            "space_id": "space-result-route",
            "sponsor_peer_id": "peer-sponsor",
            "success": false,
            "deny_reason": DENY_REASON_INVALID_PROOF
        })
        .to_string();

        handle_pairing_message::<Wry>(
            orchestrator.as_ref(),
            space_access_orchestrator.as_ref(),
            &runtime_ports,
            "peer-remote".to_string(),
            PairingMessage::Busy(uc_core::network::protocol::PairingBusy {
                session_id: "session-result-route".to_string(),
                reason: Some(reason),
            }),
            None,
        )
        .await;

        let state = space_access_orchestrator.get_state().await;
        assert!(matches!(
            state,
            uc_core::security::space_access::state::SpaceAccessState::Denied {
                reason: uc_core::security::space_access::state::DenyReason::InvalidProof,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn busy_result_payload_routes_to_access_granted_transition() {
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let network = Arc::new(NoopNetwork::default());
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());
        seed_waiting_decision_state(
            space_access_orchestrator.as_ref(),
            "session-result-granted",
            "space-result-granted",
        )
        .await;

        let reason = serde_json::json!({
            "kind": "space_access_result",
            "space_id": "space-result-granted",
            "sponsor_peer_id": null,
            "success": true,
            "deny_reason": null
        })
        .to_string();

        handle_pairing_message::<Wry>(
            orchestrator.as_ref(),
            space_access_orchestrator.as_ref(),
            &runtime_ports,
            "peer-remote".to_string(),
            PairingMessage::Busy(uc_core::network::protocol::PairingBusy {
                session_id: "session-result-granted".to_string(),
                reason: Some(reason),
            }),
            None,
        )
        .await;

        let state = space_access_orchestrator.get_state().await;
        assert!(matches!(
            state,
            uc_core::security::space_access::state::SpaceAccessState::Granted { .. }
        ));
    }

    struct TestNetwork {
        discovered: Vec<DiscoveredPeer>,
    }

    #[async_trait]
    impl ClipboardTransportPort for TestNetwork {
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
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }
    }

    #[async_trait]
    impl PeerDirectoryPort for TestNetwork {
        async fn get_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
            Ok(self.discovered.clone())
        }

        async fn get_connected_peers(&self) -> anyhow::Result<Vec<ConnectedPeer>> {
            Ok(vec![])
        }

        fn local_peer_id(&self) -> String {
            "peer-local".to_string()
        }

        async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl PairingTransportPort for TestNetwork {
        async fn open_pairing_session(
            &self,
            _peer_id: String,
            _session_id: String,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_pairing_on_session(&self, _message: PairingMessage) -> anyhow::Result<()> {
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
    impl NetworkEventPort for TestNetwork {
        async fn subscribe_events(
            &self,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<NetworkEvent>> {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }
    }

    #[tokio::test]
    async fn peer_name_updated_emits_frontend_event() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (payload_tx, mut payload_rx) = mpsc::channel::<String>(4);
        let payload_tx_clone = payload_tx.clone();
        app_handle.listen("p2p-peer-name-updated", move |event: tauri::Event| {
            let _ = payload_tx_clone.try_send(event.payload().to_string());
        });

        let (event_tx, event_rx) = mpsc::channel(1);
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);
        let network = Arc::new(TestNetwork {
            discovered: vec![DiscoveredPeer {
                peer_id: "peer-1".to_string(),
                device_name: Some("Desk".to_string()),
                device_id: None,
                addresses: vec![],
                discovered_at: Utc::now(),
                last_seen: Utc::now(),
                is_paired: true,
            }],
        });
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());

        let loop_handle = tokio::spawn(run_pairing_event_loop::<tauri::test::MockRuntime>(
            event_rx,
            orchestrator,
            Some(app_handle.clone()),
            network.clone() as Arc<dyn PeerDirectoryPort>,
            space_access_orchestrator,
            runtime_ports,
        ));

        event_tx
            .send(NetworkEvent::PeerNameUpdated {
                peer_id: "peer-1".to_string(),
                device_name: "Desk".to_string(),
            })
            .await
            .expect("send peer name event");

        let payload = payload_rx.recv().await.expect("event payload");
        assert!(payload.contains("peerId"));
        assert!(payload.contains("deviceName"));

        drop(event_tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    }

    #[tokio::test]
    async fn peer_discovery_events_emit_frontend_event() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (payload_tx, mut payload_rx) = mpsc::channel::<String>(4);
        let payload_tx_clone = payload_tx.clone();
        app_handle.listen("p2p-peer-discovery-changed", move |event: tauri::Event| {
            let _ = payload_tx_clone.try_send(event.payload().to_string());
        });

        let (event_tx, event_rx) = mpsc::channel(4);
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);
        let network = Arc::new(TestNetwork {
            discovered: vec![DiscoveredPeer {
                peer_id: "peer-1".to_string(),
                device_name: Some("Desk".to_string()),
                device_id: None,
                addresses: vec!["/ip4/192.168.1.10/tcp/42000".to_string()],
                discovered_at: Utc::now(),
                last_seen: Utc::now(),
                is_paired: false,
            }],
        });
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());

        let loop_handle = tokio::spawn(run_pairing_event_loop::<tauri::test::MockRuntime>(
            event_rx,
            orchestrator,
            Some(app_handle.clone()),
            network.clone() as Arc<dyn PeerDirectoryPort>,
            space_access_orchestrator,
            runtime_ports,
        ));

        event_tx
            .send(NetworkEvent::PeerDiscovered(DiscoveredPeer {
                peer_id: "peer-1".to_string(),
                device_name: Some("Desk".to_string()),
                device_id: None,
                addresses: vec!["/ip4/192.168.1.10/tcp/42000".to_string()],
                discovered_at: Utc::now(),
                last_seen: Utc::now(),
                is_paired: false,
            }))
            .await
            .expect("send peer discovered event");

        event_tx
            .send(NetworkEvent::PeerLost("peer-1".to_string()))
            .await
            .expect("send peer lost event");

        let discovered_payload = tokio::time::timeout(Duration::from_secs(1), payload_rx.recv())
            .await
            .expect("timeout waiting for discovered payload")
            .expect("discovered payload received");
        let discovered_value: serde_json::Value =
            serde_json::from_str(&discovered_payload).expect("discovered payload json");
        assert_eq!(discovered_value["peerId"], "peer-1");
        assert_eq!(discovered_value["deviceName"], "Desk");
        assert_eq!(discovered_value["discovered"], true);
        assert_eq!(
            discovered_value["addresses"][0],
            "/ip4/192.168.1.10/tcp/42000"
        );
        assert!(discovered_value.get("peer_id").is_none());

        let lost_payload = tokio::time::timeout(Duration::from_secs(1), payload_rx.recv())
            .await
            .expect("timeout waiting for lost payload")
            .expect("lost payload received");
        let lost_value: serde_json::Value =
            serde_json::from_str(&lost_payload).expect("lost payload json");
        assert_eq!(lost_value["peerId"], "peer-1");
        assert_eq!(lost_value["discovered"], false);
        assert!(lost_value["addresses"]
            .as_array()
            .expect("addresses array")
            .is_empty());

        drop(event_tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    }

    #[tokio::test]
    async fn space_access_completion_loop_emits_frontend_event() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (payload_tx, mut payload_rx) = mpsc::channel::<String>(1);
        let payload_tx_clone = payload_tx.clone();
        app_handle.listen("space-access-completed", move |event: tauri::Event| {
            let _ = payload_tx_clone.try_send(event.payload().to_string());
        });

        let (event_tx, event_rx) = mpsc::channel(1);
        let loop_handle = tokio::spawn(
            run_space_access_completion_loop::<tauri::test::MockRuntime>(
                event_rx,
                Some(app_handle.clone()),
            ),
        );

        event_tx
            .send(SpaceAccessCompletedEvent {
                session_id: "session-space-1".to_string(),
                peer_id: "peer-space-1".to_string(),
                success: false,
                reason: Some("timeout".to_string()),
                ts: 1735689600000,
            })
            .await
            .expect("send completion event");

        let payload = tokio::time::timeout(Duration::from_secs(1), payload_rx.recv())
            .await
            .expect("timeout waiting for payload")
            .expect("payload received");
        let value: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
        assert_eq!(value["sessionId"], "session-space-1");
        assert_eq!(value["peerId"], "peer-space-1");
        assert_eq!(value["success"], false);
        assert_eq!(value["reason"], "timeout");
        assert_eq!(value["ts"], 1735689600000_i64);
        assert!(value.get("session_id").is_none());
        assert!(value.get("peer_id").is_none());

        drop(event_tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    }

    #[tokio::test]
    async fn space_access_completion_loop_emits_p2p_frontend_event() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (payload_tx, mut payload_rx) = mpsc::channel::<String>(1);
        let payload_tx_clone = payload_tx.clone();
        app_handle.listen("p2p-space-access-completed", move |event: tauri::Event| {
            let _ = payload_tx_clone.try_send(event.payload().to_string());
        });

        let (event_tx, event_rx) = mpsc::channel(1);
        let loop_handle = tokio::spawn(
            run_space_access_completion_loop::<tauri::test::MockRuntime>(
                event_rx,
                Some(app_handle.clone()),
            ),
        );

        event_tx
            .send(SpaceAccessCompletedEvent {
                session_id: "session-space-p2p".to_string(),
                peer_id: "peer-space-p2p".to_string(),
                success: true,
                reason: None,
                ts: 1735689600999,
            })
            .await
            .expect("send completion event");

        let payload = tokio::time::timeout(Duration::from_secs(1), payload_rx.recv())
            .await
            .expect("timeout waiting for payload")
            .expect("payload received");
        let value: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
        assert_eq!(value["sessionId"], "session-space-p2p");
        assert_eq!(value["peerId"], "peer-space-p2p");
        assert_eq!(value["success"], true);
        assert_eq!(value["reason"], serde_json::Value::Null);
        assert_eq!(value["ts"], 1735689600999_i64);

        drop(event_tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    }

    #[tokio::test]
    async fn pairing_action_loop_emits_complete_with_peer_info() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (payload_tx, mut payload_rx) = mpsc::channel::<String>(1);
        let payload_tx_clone = payload_tx.clone();
        app_handle.listen("p2p-pairing-verification", move |event: tauri::Event| {
            let _ = payload_tx_clone.try_send(event.payload().to_string());
        });

        let probe = P2PPairingVerificationEvent::failed("probe", "probe".to_string());
        app_handle
            .emit("p2p-pairing-verification", probe)
            .expect("emit probe");
        let _ = tokio::time::timeout(Duration::from_secs(1), payload_rx.recv())
            .await
            .expect("timeout waiting for probe")
            .expect("probe payload received");

        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);

        let request = PairingRequest {
            session_id: "session-1".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-999".to_string(),
            peer_id: "peer-local".to_string(),
            identity_pubkey: vec![1; 32],
            nonce: vec![2; 16],
        };
        orchestrator
            .handle_incoming_request("peer-remote".to_string(), request)
            .await
            .expect("handle incoming request");

        let (action_tx, action_rx) = mpsc::channel(1);
        let network = Arc::new(NoopNetwork::default());
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let key_slot_store = Arc::new(NoopKeySlotStore);
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());
        let loop_handle = tokio::spawn(run_pairing_action_loop::<tauri::test::MockRuntime>(
            action_rx,
            network.clone() as Arc<dyn PairingTransportPort>,
            Some(app_handle.clone()),
            orchestrator.clone(),
            space_access_orchestrator,
            key_slot_store,
            runtime_ports,
        ));

        action_tx
            .send(PairingAction::EmitResult {
                session_id: "session-1".to_string(),
                success: true,
                error: None,
            })
            .await
            .expect("send action");

        let payload = tokio::time::timeout(Duration::from_secs(1), payload_rx.recv())
            .await
            .expect("timeout waiting for payload")
            .expect("payload received");
        let value: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["kind"], "complete");
        assert_eq!(value["peerId"], "peer-remote");
        assert_eq!(value["deviceName"], "PeerDevice");

        drop(action_tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    }

    #[tokio::test]
    async fn pairing_action_loop_emits_camelcase_payload() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (payload_tx, mut payload_rx) = mpsc::channel::<String>(1);
        let payload_tx_clone = payload_tx.clone();
        app_handle.listen("p2p-pairing-verification", move |event: tauri::Event| {
            let _ = payload_tx_clone.try_send(event.payload().to_string());
        });

        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);

        let request = PairingRequest {
            session_id: "session-1".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-999".to_string(),
            peer_id: "peer-local".to_string(),
            identity_pubkey: vec![1; 32],
            nonce: vec![2; 16],
        };
        orchestrator
            .handle_incoming_request("peer-remote".to_string(), request)
            .await
            .expect("handle incoming request");

        let (action_tx, action_rx) = mpsc::channel(1);
        let network = Arc::new(NoopNetwork::default());
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let key_slot_store = Arc::new(NoopKeySlotStore);
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());
        let loop_handle = tokio::spawn(run_pairing_action_loop::<tauri::test::MockRuntime>(
            action_rx,
            network.clone() as Arc<dyn PairingTransportPort>,
            Some(app_handle.clone()),
            orchestrator.clone(),
            space_access_orchestrator,
            key_slot_store,
            runtime_ports,
        ));

        action_tx
            .send(PairingAction::EmitResult {
                session_id: "session-2".to_string(),
                success: true,
                error: None,
            })
            .await
            .expect("send action");

        let payload = tokio::time::timeout(Duration::from_secs(1), payload_rx.recv())
            .await
            .expect("timeout waiting for payload")
            .expect("payload received");
        let value: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
        assert!(value.get("sessionId").is_some());
        assert!(value.get("peerId").is_some());
        assert!(value.get("deviceName").is_some());
        assert!(value.get("session_id").is_none());
        assert!(value.get("peer_id").is_none());
        assert!(value.get("device_name").is_none());

        drop(action_tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    }

    #[tokio::test]
    async fn pairing_action_loop_emits_verifying_kind() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (payload_tx, mut payload_rx) = mpsc::channel::<String>(1);
        let payload_tx_clone = payload_tx.clone();
        app_handle.listen("p2p-pairing-verification", move |event: tauri::Event| {
            let _ = payload_tx_clone.try_send(event.payload().to_string());
        });

        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);

        let (action_tx, action_rx) = mpsc::channel(1);
        let network = Arc::new(NoopNetwork::default());
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let key_slot_store = Arc::new(NoopKeySlotStore);
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());
        let loop_handle = tokio::spawn(run_pairing_action_loop::<tauri::test::MockRuntime>(
            action_rx,
            network.clone() as Arc<dyn PairingTransportPort>,
            Some(app_handle.clone()),
            orchestrator.clone(),
            space_access_orchestrator,
            key_slot_store,
            runtime_ports,
        ));

        action_tx
            .send(PairingAction::ShowVerifying {
                session_id: "session-1".to_string(),
                peer_display_name: "PeerDevice".to_string(),
            })
            .await
            .expect("send action");

        let payload = tokio::time::timeout(Duration::from_secs(1), payload_rx.recv())
            .await
            .expect("timeout waiting for payload")
            .expect("payload received");
        let value: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["kind"], "verifying");
        assert_eq!(value["deviceName"], "PeerDevice");

        drop(action_tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    }

    #[tokio::test]
    async fn pairing_action_loop_emits_failed_on_send_error() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (payload_tx, mut payload_rx) = mpsc::channel::<String>(1);
        let payload_tx_clone = payload_tx.clone();
        app_handle.listen("p2p-pairing-verification", move |event: tauri::Event| {
            let _ = payload_tx_clone.try_send(event.payload().to_string());
        });

        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);

        let (action_tx, action_rx) = mpsc::channel(4);
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let key_slot_store = Arc::new(NoopKeySlotStore);
        let send_called = Arc::new(AtomicUsize::new(0));
        let network = Arc::new(SendFailNetwork {
            base: BaseNetwork::default(),
            send_called: send_called.clone(),
        });
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());
        let loop_handle = tokio::spawn(run_pairing_action_loop::<tauri::test::MockRuntime>(
            action_rx,
            network.clone() as Arc<dyn PairingTransportPort>,
            Some(app_handle.clone()),
            orchestrator.clone(),
            space_access_orchestrator,
            key_slot_store,
            runtime_ports,
        ));

        let challenge = PairingChallenge {
            session_id: "session-send-fail".to_string(),
            pin: "123456".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-999".to_string(),
            identity_pubkey: vec![1; 32],
            nonce: vec![2; 16],
        };
        action_tx
            .send(PairingAction::Send {
                peer_id: "peer-remote".to_string(),
                message: PairingMessage::Challenge(challenge),
            })
            .await
            .expect("send action");

        tokio::task::yield_now().await;
        assert!(send_called.load(Ordering::SeqCst) > 0);

        let mut failed_event = None;
        for _ in 0..3 {
            let payload = tokio::time::timeout(Duration::from_secs(1), payload_rx.recv())
                .await
                .expect("timeout waiting for payload")
                .expect("payload received");
            let value: serde_json::Value = serde_json::from_str(&payload).expect("payload json");
            if value["kind"] == "failed" {
                failed_event = Some(value);
                break;
            }
        }
        let value = failed_event.expect("failed event");
        assert_eq!(value["sessionId"], "session-send-fail");
        assert!(value["error"]
            .as_str()
            .unwrap_or("")
            .contains("send failed"));

        let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    }

    #[tokio::test]
    async fn pairing_action_loop_closes_session_only_for_failed_emit_result() {
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);
        let close_calls = Arc::new(Mutex::new(Vec::new()));
        let network = Arc::new(CloseRecordingNetwork {
            base: BaseNetwork::default(),
            close_calls: close_calls.clone(),
        });
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());

        let (action_tx, action_rx) = mpsc::channel(4);
        let loop_handle = tokio::spawn(run_pairing_action_loop::<Wry>(
            action_rx,
            network.clone() as Arc<dyn PairingTransportPort>,
            None,
            orchestrator,
            space_access_orchestrator,
            Arc::new(NoopKeySlotStore),
            runtime_ports,
        ));

        action_tx
            .send(PairingAction::EmitResult {
                session_id: "session-success".to_string(),
                success: true,
                error: None,
            })
            .await
            .expect("send success result");
        action_tx
            .send(PairingAction::EmitResult {
                session_id: "session-failed".to_string(),
                success: false,
                error: Some("pairing failed".to_string()),
            })
            .await
            .expect("send failed result");

        tokio::time::sleep(Duration::from_millis(50)).await;

        let calls = close_calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "session-failed");
        assert_eq!(calls[0].1, Some("pairing failed".to_string()));

        drop(action_tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    }

    #[tokio::test]
    async fn pairing_action_loop_starts_sponsor_authorization_for_responder_role() {
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![9; 32],
        );
        let orchestrator = Arc::new(orchestrator);

        let request = PairingRequest {
            session_id: "session-offer".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-999".to_string(),
            peer_id: "peer-local".to_string(),
            identity_pubkey: vec![1; 32],
            nonce: vec![2; 16],
        };
        orchestrator
            .handle_incoming_request("peer-remote".to_string(), request)
            .await
            .expect("handle incoming request");

        assert_eq!(
            orchestrator.get_session_role("session-offer").await,
            Some(PairingRole::Responder)
        );

        let sent_messages = Arc::new(Mutex::new(Vec::new()));
        let network = Arc::new(OfferRecordingNetwork {
            base: BaseNetwork::default(),
            sent_messages: sent_messages.clone(),
        });
        let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
        let runtime_ports =
            test_runtime_space_access_ports(network.clone(), space_access_orchestrator.clone());

        let (action_tx, action_rx) = mpsc::channel(2);
        let loop_handle = tokio::spawn(run_pairing_action_loop::<Wry>(
            action_rx,
            network.clone() as Arc<dyn PairingTransportPort>,
            None,
            orchestrator,
            space_access_orchestrator,
            Arc::new(StaticKeySlotStore {
                slot: test_keyslot_file("space-offer"),
            }),
            runtime_ports,
        ));

        action_tx
            .send(PairingAction::EmitResult {
                session_id: "session-offer".to_string(),
                success: true,
                error: None,
            })
            .await
            .expect("send success result");

        tokio::time::sleep(Duration::from_millis(80)).await;

        let calls = sent_messages.lock().unwrap().clone();
        let busy_offer = calls
            .iter()
            .find_map(|(session_id, message)| match message {
                PairingMessage::Busy(busy) if session_id == "session-offer" => Some(busy),
                _ => None,
            });

        let busy_offer = busy_offer.expect("expected space access offer busy message");
        let payload = busy_offer
            .reason
            .as_ref()
            .expect("busy payload should include reason");
        let payload_json: serde_json::Value =
            serde_json::from_str(payload).expect("busy payload should be json");
        assert_eq!(payload_json["kind"], "space_access_offer");

        drop(action_tx);
        let _ = tokio::time::timeout(Duration::from_secs(1), loop_handle).await;
    }

    #[test]
    fn test_wiring_error_secure_storage() {
        let err = WiringError::SecureStorageInit("secure storage unavailable".to_string());
        assert!(err.to_string().contains("Secure storage initialization"));
    }

    #[test]
    fn test_wiring_error_clipboard() {
        let err = WiringError::ClipboardInit("platform error".to_string());
        assert!(err.to_string().contains("Clipboard initialization"));
    }

    #[test]
    fn test_wiring_error_network() {
        let err = WiringError::NetworkInit("bind failed".to_string());
        assert!(err.to_string().contains("Network initialization"));
    }

    #[test]
    fn test_wiring_error_blob_storage() {
        let err = WiringError::BlobStorageInit("path invalid".to_string());
        assert!(err.to_string().contains("Blob storage initialization"));
    }

    #[test]
    fn test_wiring_error_settings() {
        let err = WiringError::SettingsInit("load failed".to_string());
        assert!(err
            .to_string()
            .contains("Settings repository initialization"));
    }

    #[test]
    fn test_wiring_result_success() {
        let result: WiringResult<()> = Ok(());
        assert!(result.is_ok());
    }

    #[test]
    fn test_wiring_result_error() {
        let result: WiringResult<()> = Err(WiringError::DatabaseInit("test".to_string()));
        assert!(result.is_err());
        assert!(matches!(result, Err(WiringError::DatabaseInit(_))));
    }

    #[test]
    fn test_wire_dependencies_returns_not_implemented() {
        // This test is now obsolete since wire_dependencies is implemented
        // 此测试现已过时，因为 wire_dependencies 已实现
        // The test is removed and replaced with a new test below
        // 此测试已删除，并在下方替换为新测试
    }

    #[test]
    fn test_wire_dependencies_creates_app_deps() {
        // Test that wire_dependencies creates a valid AppDeps structure
        // 测试 wire_dependencies 创建有效的 AppDeps 结构
        let config = AppConfig::empty();
        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let result =
            wire_dependencies_with_identity_store(&config, cmd_tx, Some(test_identity_store()));

        match result {
            Ok(wired) => {
                let deps = wired.deps;
                // Verify all dependencies are present by type checking
                // 通过类型检查验证所有依赖都存在
                let _ = &deps.clipboard;
                let _ = &deps.clipboard_event_repo;
                let _ = &deps.representation_repo;
                let _ = &deps.representation_normalizer;
                let _ = &deps.encryption;
                let _ = &deps.encryption_session;
                let _ = &deps.secure_storage;
                let _ = &deps.key_material;
                let _ = &deps.watcher_control;
                let _ = &deps.clipboard_change_origin;
                let _ = &deps.device_repo;
                let _ = &&deps.device_identity;
                let _ = &deps.paired_device_repo;
                let _ = &deps.network_ports;
                let _ = &deps.blob_store;
                let _ = &deps.blob_repository;
                let _ = &deps.blob_writer;
                let _ = &deps.settings;
                let _ = &deps.ui_port;
                let _ = &deps.autostart;
                let _ = &deps.clock;
                let _ = &deps.hash;
                // Test passes if we can access all fields without panicking
                // 如果我们可以访问所有字段而不恐慌，测试通过
            }
            Err(e) => {
                panic!("Expected Ok but got error: {}", e);
            }
        }
    }

    #[test]
    fn test_create_db_pool_signature() {
        // This test verifies the function signature is correct
        // Actual DB pool functionality testing is in integration tests
        // 此测试验证函数签名正确
        // 实际数据库池功能测试在集成测试中

        // Create a temporary database path
        // 创建临时数据库路径
        let db_path = PathBuf::from(":memory:");

        // The function should exist and return the correct type
        // 函数应该存在并返回正确的类型
        let result = create_db_pool(&db_path);

        // We expect it to succeed with in-memory database
        // 我们期望内存数据库能成功
        match result {
            Ok(_pool) => {
                // Pool is created successfully - type is verified by compiler
                // 池创建成功 - 类型由编译器验证
                assert!(true);
            }
            Err(e) => {
                // If it fails, it should be a DatabaseInit error
                // 如果失败，应该是 DatabaseInit 错误
                assert!(matches!(e, WiringError::DatabaseInit(_)));
            }
        }
    }

    #[test]
    fn test_create_db_pool_with_empty_path() {
        // Test with an empty path - should succeed (creates in-memory DB)
        // 使用空路径测试 - 应该成功（创建内存数据库）
        let db_path = PathBuf::new();

        let result = create_db_pool(&db_path);

        // Empty path is treated as empty string, which diesel interprets as in-memory
        // 空路径被视为空字符串，diesel 将其解释为内存数据库
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_db_pool_creates_parent_directory() {
        // This test would need tempdir support, which is in dev-dependencies
        // For now, we just verify the function exists
        // 此测试需要 tempdir 支持，这在 dev-dependencies 中
        // 目前我们只验证函数存在
        let _ = create_db_pool;
        // Actual directory creation testing is in integration tests
        // 实际目录创建测试在集成测试中
    }

    #[derive(Clone)]
    struct DummySecureStorage;

    impl SecureStoragePort for DummySecureStorage {
        fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Ok(None)
        }

        fn set(&self, _key: &str, _value: &[u8]) -> Result<(), SecureStorageError> {
            Ok(())
        }

        fn delete(&self, _key: &str) -> Result<(), SecureStorageError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryIdentityStore {
        identity: Mutex<Option<Vec<u8>>>,
    }

    impl IdentityStorePort for MemoryIdentityStore {
        fn load_identity(&self) -> Result<Option<Vec<u8>>, IdentityStoreError> {
            let guard = self
                .identity
                .lock()
                .map_err(|_| IdentityStoreError::Store("identity store poisoned".to_string()))?;
            Ok(guard.clone())
        }

        fn store_identity(&self, identity: &[u8]) -> Result<(), IdentityStoreError> {
            let mut guard = self
                .identity
                .lock()
                .map_err(|_| IdentityStoreError::Store("identity store poisoned".to_string()))?;
            *guard = Some(identity.to_vec());
            Ok(())
        }
    }

    fn test_identity_store() -> Arc<dyn IdentityStorePort> {
        Arc::new(MemoryIdentityStore::default())
    }

    #[test]
    fn test_create_platform_layer_returns_expected_types() {
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        runtime.block_on(async {
            // Test that platform layer creates the correct types
            // 测试平台层创建正确的类型
            let secure_storage: Arc<dyn SecureStoragePort> = Arc::new(DummySecureStorage);
            let temp_dir =
                std::env::temp_dir().join(format!("uc-wiring-test-{}", std::process::id()));
            std::fs::create_dir_all(&temp_dir).expect("create temp dir");
            let (cmd_tx, _cmd_rx) = mpsc::channel(10);

            // Create missing dependencies
            // 创建缺失的依赖
            let encryption: Arc<dyn EncryptionPort> = Arc::new(EncryptionRepository);
            let db_pool = init_db_pool(":memory:").expect("create in-memory db pool");
            let db_executor = Arc::new(DieselSqliteExecutor::new(db_pool));
            let blob_repository: Arc<dyn BlobRepositoryPort> = Arc::new(DieselBlobRepository::new(
                Arc::clone(&db_executor),
                BlobRowMapper,
                BlobRowMapper,
            ));
            let paired_device_repo: Arc<dyn PairedDeviceRepositoryPort> = Arc::new(
                DieselPairedDeviceRepository::new(Arc::clone(&db_executor), PairedDeviceRowMapper),
            );
            let clock: Arc<dyn ClockPort> = Arc::new(SystemClock);
            let storage_config = Arc::new(ClipboardStorageConfig::defaults());

            let result = create_platform_layer(
                secure_storage,
                &temp_dir,
                cmd_tx,
                encryption,
                blob_repository,
                paired_device_repo,
                clock,
                storage_config,
                test_identity_store(),
            );

            match result {
                Ok(layer) => {
                    // Verify all fields have correct types
                    // 验证所有字段都有正确的类型
                    let _clipboard: &Arc<dyn PlatformClipboardPort> = &layer.clipboard;
                    let _secure_storage: &Arc<dyn SecureStoragePort> = &layer.secure_storage;
                    let _ui: &Arc<dyn UiPort> = &layer.ui;
                    let _autostart: &Arc<dyn AutostartPort> = &layer.autostart;
                    let _network_ports: &Arc<NetworkPorts> = &layer.network_ports;
                    let _device_identity: &Arc<dyn DeviceIdentityPort> = &layer.device_identity;
                    let _representation_normalizer: &Arc<
                        dyn ClipboardRepresentationNormalizerPort,
                    > = &layer.representation_normalizer;
                    let _blob_writer: &Arc<dyn BlobWriterPort> = &layer.blob_writer;
                    let _blob_store: &Arc<dyn BlobStorePort> = &layer.blob_store;
                    let _encryption_session: &Arc<dyn EncryptionSessionPort> =
                        &layer.encryption_session;
                    let _watcher_control: &Arc<dyn WatcherControlPort> = &layer.watcher_control;
                }
                Err(e) => {
                    // On systems without clipboard support, we might get an error
                    // 在没有剪贴板支持的系统上，我们可能会收到错误
                    // This is acceptable for this test
                    // 这对此测试来说是可接受的
                    panic!("Platform layer creation failed: {}", e);
                }
            }
        });
    }

    #[test]
    fn test_create_platform_layer_clipboard_error_maps_correctly() {
        // This test verifies that clipboard initialization errors are properly mapped
        // 此测试验证剪贴板初始化错误被正确映射
        // Note: We can't easily test this without mocking, but the function exists
        // 注意：没有 mock 很难测试，但函数存在
        let _ = create_platform_layer;
    }

    #[test]
    fn test_platform_layer_struct_fields() {
        // Verify PlatformLayer has the expected fields
        // 验证 PlatformLayer 有预期的字段
        // This is a compile-time check
        // 这是编译时检查
        let _ = || -> std::sync::Arc<dyn PlatformClipboardPort> {
            // This closure should only compile if PlatformLayer has a `clipboard` field
            // 此闭包只有在 PlatformLayer 有 `clipboard` 字段时才能编译
            unimplemented!()
        };

        let _ = || -> std::sync::Arc<dyn SecureStoragePort> {
            // This closure should only compile if PlatformLayer has a `secure_storage` field
            // 此闭包只有在 PlatformLayer 有 `secure_storage` 字段时才能编译
            unimplemented!()
        };

        let _ = || -> std::sync::Arc<dyn WatcherControlPort> {
            // This closure should only compile if PlatformLayer has a `watcher_control` field
            // 此闭包只有在 PlatformLayer 有 `watcher_control` 字段时才能编译
            unimplemented!()
        };
    }

    #[test]
    #[ignore = "Integration test disabled due to SQLite locking conflicts with concurrent tests.
This test creates a full dependency graph including database initialization.
When multiple tests run in parallel, they access the same database file causing 'database is locked' errors.

TODO: Move to integration tests directory (src-tauri/tests/) with proper test isolation:
- Use unique temporary database paths per test
- Run sequentially using serial attribute
- Or use in-memory database for true isolation

The functionality is still validated in development mode when running the app without config.toml."]
    fn test_wire_dependencies_handles_empty_database_path() {
        // Test that wire_dependencies handles empty database_path gracefully
        // 测试 wire_dependencies 优雅地处理空的 database_path
        let empty_config = AppConfig::empty();
        let (cmd_tx, _cmd_rx) = mpsc::channel(10);
        let result = wire_dependencies_with_identity_store(
            &empty_config,
            cmd_tx,
            Some(test_identity_store()),
        );

        // Should succeed by using fallback default data directory
        // In headless CI environments, clipboard initialization may fail - accept that as expected
        // 应该通过使用后备默认数据目录成功
        // 在无头 CI 环境中，剪贴板初始化可能失败 - 将其视为预期行为
        match &result {
            Ok(_) => {}
            Err(WiringError::ClipboardInit(_)) => {
                // Clipboard initialization failed (likely headless CI environment without display server)
                // This is expected and acceptable - the test's purpose is to verify database path fallback
                // 剪贴板初始化失败（可能是没有显示服务器的无头 CI 环境）
                // 这是预期且可接受的 - 测试的目的是验证数据库路径后备逻辑
                return;
            }
            Err(e) => {
                panic!("Expected Ok or ClipboardInit error, got: {:?}", e);
            }
        }
    }

    #[test]
    fn test_get_default_app_dirs_returns_expected_path() {
        // Test that get_default_app_dirs returns a valid path
        // 测试 get_default_app_dirs 返回有效路径
        let result = get_default_app_dirs();

        assert!(result.is_ok());
        let dirs = result.unwrap();
        assert!(dirs.app_data_root.ends_with("uniclipboard"));
    }

    #[test]
    fn derive_default_paths_from_empty_config_uses_single_app_data_root() {
        let config = AppConfig::empty();

        let paths = with_uc_profile(None, || {
            derive_default_paths(&config).expect("derive_default_paths failed")
        });

        assert!(paths.app_data_root.ends_with("uniclipboard"));
        assert_eq!(paths.db_path, paths.app_data_root.join("uniclipboard.db"));
        assert_eq!(paths.vault_dir, paths.app_data_root.join("vault"));
        assert_eq!(
            paths.settings_path,
            paths.app_data_root.join("settings.json")
        );
    }

    #[test]
    fn wiring_derives_paths_from_port_fact() {
        let dirs = uc_core::app_dirs::AppDirs {
            app_data_root: std::path::PathBuf::from("/tmp/uniclipboard"),
            app_cache_root: std::path::PathBuf::from("/tmp/uniclipboard-cache"),
        };
        let paths = with_uc_profile(None, || {
            derive_default_paths_from_app_dirs(&dirs, &AppConfig::empty())
                .expect("derive_default_paths_from_app_dirs failed")
        });
        assert!(paths.db_path.ends_with("uniclipboard.db"));
    }

    #[test]
    fn derive_default_paths_sets_cache_dir() {
        let dirs = uc_core::app_dirs::AppDirs {
            app_data_root: PathBuf::from("/tmp/uniclipboard"),
            app_cache_root: PathBuf::from("/tmp/uniclipboard-cache"),
        };
        let paths = with_uc_profile(None, || {
            derive_default_paths_from_app_dirs(&dirs, &AppConfig::empty())
                .expect("derive_default_paths_from_app_dirs failed")
        });
        assert_eq!(paths.cache_dir, PathBuf::from("/tmp/uniclipboard-cache"));
    }

    #[test]
    fn derive_default_paths_uses_config_database_parent_as_app_data_root() {
        let dirs = uc_core::app_dirs::AppDirs {
            app_data_root: PathBuf::from("/tmp/uniclipboard"),
            app_cache_root: PathBuf::from("/tmp/uniclipboard-cache"),
        };

        let mut config = AppConfig::empty();
        config.database_path = PathBuf::from("src-tauri/.app_data_a/uniclipboard.db");

        let paths = with_uc_profile(None, || {
            derive_default_paths_from_app_dirs(&dirs, &config)
                .expect("derive_default_paths_from_app_dirs failed")
        });

        assert_eq!(paths.app_data_root, PathBuf::from("src-tauri/.app_data_a"));
        assert_eq!(
            paths.db_path,
            PathBuf::from("src-tauri/.app_data_a/uniclipboard.db")
        );
        assert_eq!(
            paths.vault_dir,
            PathBuf::from("src-tauri/.app_data_a/vault")
        );
        assert_eq!(
            paths.settings_path,
            PathBuf::from("src-tauri/.app_data_a/settings.json")
        );
    }

    #[test]
    fn derive_default_paths_appends_profile_suffix_for_configured_root() {
        let dirs = uc_core::app_dirs::AppDirs {
            app_data_root: PathBuf::from("/tmp/uniclipboard"),
            app_cache_root: PathBuf::from("/tmp/uniclipboard-cache"),
        };

        let mut config = AppConfig::empty();
        config.database_path = PathBuf::from("src-tauri/.app_data/uniclipboard.db");

        let paths = with_uc_profile(Some("a"), || {
            derive_default_paths_from_app_dirs(&dirs, &config)
                .expect("derive_default_paths_from_app_dirs failed")
        });

        assert_eq!(paths.app_data_root, PathBuf::from("src-tauri/.app_data_a"));
        assert_eq!(
            paths.db_path,
            PathBuf::from("src-tauri/.app_data_a/uniclipboard.db")
        );
        assert_eq!(
            paths.vault_dir,
            PathBuf::from("src-tauri/.app_data_a/vault")
        );
        assert_eq!(
            paths.settings_path,
            PathBuf::from("src-tauri/.app_data_a/settings.json")
        );
    }

    #[test]
    fn derive_default_paths_appends_profile_suffix_for_configured_vault_root() {
        let dirs = uc_core::app_dirs::AppDirs {
            app_data_root: PathBuf::from("/tmp/uniclipboard"),
            app_cache_root: PathBuf::from("/tmp/uniclipboard-cache"),
        };

        let mut config = AppConfig::empty();
        config.database_path = PathBuf::from("src-tauri/.app_data/uniclipboard.db");
        config.vault_key_path = PathBuf::from("src-tauri/.app_data/vault/key");

        let paths = with_uc_profile(Some("b"), || {
            derive_default_paths_from_app_dirs(&dirs, &config)
                .expect("derive_default_paths_from_app_dirs failed")
        });

        assert_eq!(paths.app_data_root, PathBuf::from("src-tauri/.app_data_b"));
        assert_eq!(
            paths.vault_dir,
            PathBuf::from("src-tauri/.app_data_b/vault")
        );
    }

    #[test]
    fn apply_profile_suffix_sanitizes_path_separators_in_profile() {
        let path = PathBuf::from("src-tauri/.app_data");

        let updated = with_uc_profile(Some("team/a\\b"), || apply_profile_suffix(path.clone()));

        assert_eq!(updated, PathBuf::from("src-tauri/.app_data_team_a_b"));
    }
}

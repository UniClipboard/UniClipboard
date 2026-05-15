//! # Pure Dependency Assembly
//!
//! This module contains all pure dependency construction functions that have
//! zero Tauri imports. It is the heart of the `uc-bootstrap` composition root.
//!
//! ## What lives here
//!
//! - `WiredDependencies` struct (output of the wiring process)
//! - `BackgroundRuntimeDeps` struct (background worker dependencies)
//! - All infrastructure and platform layer construction functions
//! - `wire_dependencies`, `get_storage_paths`, `resolve_pairing_device_name`, etc.
//!
//! ## Architecture Principle
//!
//! > **Zero tauri imports in this file.**

use std::path::PathBuf;
use std::sync::Arc;

use tracing::warn;

use uc_application::deps::{
    AppDeps, ClipboardPorts, DevicePorts, MobileSyncPorts, SearchPorts, SecurityPorts,
    StoragePorts, SystemPorts,
};
use uc_application::facade::HostEventEmitterPort;
use uc_core::blob::ports::{BlobReaderPort, BlobWriterPort};
use uc_core::clipboard::SelectRepresentationPolicyV1;
use uc_core::config::AppConfig;
use uc_core::ids::RepresentationId;
use uc_core::ports::blob::BlobReferenceRepositoryPort;
use uc_core::ports::clipboard::{
    ClipboardChangeOriginPort, ClipboardRepresentationNormalizerPort, RepresentationCachePort,
    SpoolQueuePort, SpoolRequest,
};
use uc_core::ports::*;
use uc_infra::blob::{BlobRepositoryPort, BlobStorePort, BlobWriter, FilesystemBlobStore};
use uc_infra::clipboard::{
    clipboard_change_origin, init_clipboard_change_origin, new_in_memory_change_origin,
    ClipboardPayloadResolver, ClipboardRepresentationNormalizer, DurableSpoolQueue,
    InfraThumbnailGenerator, RepresentationCache, SpoolManager,
};
use uc_infra::config::ClipboardStorageConfig;
use uc_infra::db::executor::DieselSqliteExecutor;
use uc_infra::db::mappers::{
    blob_mapper::BlobRowMapper, clipboard_entry_mapper::ClipboardEntryRowMapper,
    clipboard_event_mapper::ClipboardEventRowMapper,
    clipboard_selection_mapper::ClipboardSelectionRowMapper,
    mobile_device_mapper::MobileDeviceRowMapper, peer_address_mapper::PeerAddressRowMapper,
    snapshot_representation_mapper::RepresentationRowMapper,
    space_member_mapper::SpaceMemberRowMapper, trusted_peer_mapper::TrustedPeerRowMapper,
};
use uc_infra::db::pool::{init_db_pool, DbPool};
use uc_infra::db::repositories::{
    DieselBlobMigrationRepository, DieselBlobReferenceRepository, DieselBlobRepository,
    DieselClipboardEntryRepository, DieselClipboardEventRepository,
    DieselClipboardRepresentationRepository, DieselClipboardSelectionRepository,
    DieselFileTransferRepository, DieselMobileDeviceRepository, DieselPeerAddressRepository,
    DieselSpaceMemberRepository, DieselThumbnailRepository, DieselTrustedPeerRepository,
};
use uc_infra::device::LocalDeviceIdentity;
use uc_infra::fs::key_slot_store::JsonKeySlotStore;
use uc_infra::search::{HkdfSearchKeyDerivation, SearchPipeline, SqliteSearchIndex};
use uc_infra::security::{
    Argon2PinHasher, Blake3Hasher, DecryptingClipboardRepresentationRepository, EncryptedBlobStore,
    EncryptingClipboardEventWriter, InMemorySession, KeyMaterialStore,
    Sha256IdentityFingerprintFactory, Sha256ShortCodeGenerator,
};
use uc_infra::settings::repository::FileSettingsRepository;
use uc_infra::{
    FileAppVersionStateRepository, FileFirstSyncStateRepository, FileMigrationStateRepository,
    FileSetupStatusRepository, SystemClock,
};
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::clipboard::{LocalClipboard, NoopSystemClipboard};
use uc_platform::ports::AppDirsPort;

use tokio::sync::mpsc;

/// Result type for wiring operations
pub type WiringResult<T> = Result<T, WiringError>;

/// Errors during dependency injection
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

/// Background runtime components that must be started after async runtime is ready.
pub struct BackgroundRuntimeDeps {
    pub representation_cache: Arc<RepresentationCache>,
    pub spool_manager: Arc<SpoolManager>,
    /// Sender side of the legacy spool channel. Kept alive so that `SpoolerTask`
    /// (which drains `spool_rx`) does not immediately exit when no senders remain.
    /// `DurableSpoolQueue` bypasses this channel and writes to disk directly, so
    /// `spool_tx` is never actually used to send messages in normal operation.
    pub spool_tx: mpsc::Sender<SpoolRequest>,
    pub spool_rx: mpsc::Receiver<SpoolRequest>,
    pub worker_rx: mpsc::Receiver<RepresentationId>,
    pub spool_dir: PathBuf,
    pub file_cache_dir: PathBuf,
    pub spool_ttl_days: u64,
    pub worker_retry_max_attempts: u32,
    pub worker_retry_backoff_ms: u64,
    /// Event-sourced file transfer lifecycle: receiver-side projection
    /// plumbing + sweep/reconcile runtime tasks. Holds a clone of the shared
    /// `emitter_cell` so it automatically sees emitter swaps
    /// (LoggingEventEmitter → DaemonApiEventEmitter). The 5 lifecycle actions
    /// (start/report_progress/complete/fail/cancel) live inside the
    /// `file_transfer_facade` carried on [`WiredDependencies`].
    pub file_transfer_lifecycle: Arc<crate::file_transfer_lifecycle::FileTransferLifecycle>,
    /// Single write boundary for all programmatic clipboard writes.
    /// Centralises guard-registration + write + cleanup-on-error.
    pub clipboard_write_coordinator:
        Arc<uc_application::clipboard_write::ClipboardWriteCoordinator>,
}

/// 进程级一次性装配产出的"持久"部分:进程内常驻的 deps 与旁路资源
/// (repos、storage paths、shared adapters)。
///
/// 一次性消费的 [`BackgroundRuntimeDeps`] (含 spool / blob worker
/// receivers) 通过 [`wire_dependencies`] 的 tuple 返回值单独移交,不再
/// 嵌在 `WiredDependencies` 里 —— 因为 mpsc `Receiver` 不可 Clone, 而
/// `WiredDependencies` 需要被 standalone daemon binary 与 GUI shell
/// 两种入口共用 (`build_process_runtime` clone fan-out 给两条 path)。
///
/// `Clone` 派生:所有字段都是 `Arc<dyn Port>` / `PathBuf` / Clone-able
/// 嵌套 struct,clone 等价于一组 Arc::clone + PathBuf::clone,廉价。启动
/// 期 GUI shell 把同一份 deps fan-out 给 TauriAppRuntime / daemon spawn /
/// process handles —— 不是 reload 多次 clone, 但 fan-out 路径仍存在。
#[derive(Clone)]
pub struct WiredDependencies {
    pub deps: AppDeps,
    /// Shared emitter cell created at wire time with the initial `LoggingHostEventEmitter`.
    /// Callers (GUI bootstrap, non-GUI bootstrap) use this same cell so that
    /// all consumers — CoreRuntime, SetupOrchestrator, and FileTransferOrchestrator —
    /// see the same emitter after any swap.
    pub emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
    /// Trusted-peer repository surfaced at the bootstrap boundary so the
    /// GUI / daemon builders can build the singleton `TrustPeerOrchestrator`
    /// without threading it through `AppDeps` (which is retiring together
    /// with uc-app). Scheduled to move into `uc-application` wiring
    /// infrastructure once uc-app is gone.
    pub trusted_peer_repo: Arc<dyn uc_core::TrustedPeerRepositoryPort>,
    /// Slice 2 Phase 1 · T5：peer address 仓库，由
    /// [`crate::space_setup::build_space_setup_assembly`] 注入 `SpaceSetupFacade`，
    /// 用于配对完成后 best-effort 写对端传输地址 blob。跟
    /// `trusted_peer_repo` 同样绕开 `AppDeps`：消费者在 uc-application 里。
    pub peer_addr_repo: Arc<dyn uc_core::ports::PeerAddressRepositoryPort>,
    /// Slice 3 Phase 1:iroh-blobs store 目录。由 `SpaceSetupAssembly`
    /// 装配 iroh blob handler 时使用。
    pub iroh_blob_store_dir: PathBuf,
    /// iroh 长期 Ed25519 设备身份的文件存储根目录(`<app_data>/iroh-identity[_<profile>]/`)。
    ///
    /// 与 KEK 的系统 keychain 隔离:iroh 设备身份是网络栈的"我是哪台机器"
    /// 标识,**不是**用户秘密;不应跟用户口令派生密钥共用同一条 macOS
    /// keychain 条目,否则启动期 `IrohNodeBuilder::bind` 会在用户没有任何
    /// 操作前触发 keychain 弹窗(违反"只在用户解锁/初始化时访问 keychain"
    /// 的边界规则)。
    pub iroh_identity_dir: PathBuf,
    /// Slice 3 Phase 1:明文 hash → 密文 digest 去重缓存。
    pub blob_reference_repo: Arc<dyn BlobReferenceRepositoryPort>,
    /// Switch-space 重加密迁移：跨重启的阶段持久化 port，落地为
    /// `.migration_state` 文件（与 `.setup_status` 同目录）。消费者
    /// `SpaceSetupFacade::switch_space` / `try_resume_session`，所以同
    /// `peer_addr_repo` 走 WiredDependencies 旁路而不是 AppDeps。
    pub migration_state: Arc<dyn uc_core::ports::setup::MigrationStatePort>,
    /// Switch-space 一次性 migration_key 的 keyring 管理 port。
    pub key_migration: Arc<dyn uc_core::ports::security::KeyMigrationPort>,
    /// Switch-space backup 表 + 主表 inline_data 批量读写 port。
    pub blob_migration_repo: Arc<dyn uc_core::ports::clipboard::BlobMigrationRepoPort>,
    /// 投递结果仓储:`ClipboardSyncFacade` 在 fan-out 完成时写、视图侧读。
    /// 跟 `trusted_peer_repo` / `peer_addr_repo` 一样走 WiredDependencies 旁路,
    /// 因为消费者在 uc-application 里。
    pub entry_delivery_repo: Arc<dyn uc_core::ports::EntryDeliveryRepositoryPort>,
    /// `ClipboardEventRepositoryPort` 的读端口实例,与
    /// `AppDeps.clipboard.clipboard_event_repo` 共享底层 Diesel impl,
    /// 仅供视图层做"反查来源设备"使用。
    pub clipboard_event_reader_repo: Arc<dyn uc_core::ports::ClipboardEventRepositoryPort>,
    /// Mobile sync LAN 端点状态(单例)的具体类型旁路。
    ///
    /// daemon LAN listener 启停时需要调 inherent `set` / `clear`,这两个方法
    /// 不在 `MobileSyncEndpointInfoPort` 上(只读契约 vs 写入事件,见
    /// `MobileSyncPorts.endpoint_info` 的文档)。同一份 Arc 已经装入
    /// `AppDeps.mobile_sync.endpoint_info`,daemon 写、facade 读,共享同一份
    /// 内存,不会出现"两条路径看不到同一个 URL"的撕裂。
    pub mobile_sync_endpoint_info:
        Arc<uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter>,
    /// Application-layer entry point for the 5 file-transfer lifecycle
    /// actions + seed + link. Built alongside `file_transfer_lifecycle` from
    /// the same store + publisher so events stay on a single timeline. Lives
    /// on `WiredDependencies` (process-level, cloneable Arc) rather than on
    /// `BackgroundRuntimeDeps` because the latter is for one-shot mpsc
    /// receivers — the facade itself is shared by GUI shell, daemon-lifecycle
    /// `MobileSyncFacade` 装配, and `build_space_setup_assembly` (iroh path).
    pub file_transfer_facade: Arc<uc_application::facade::FileTransferFacade>,
}

/// Infrastructure layer implementations
struct InfraLayer {
    // Clipboard repositories
    #[allow(dead_code)]
    clipboard_entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    clipboard_event_repo: Arc<dyn ClipboardEventWriterPort>,
    /// 与 `clipboard_event_repo` 共享底层 `DieselClipboardEventRepository`,
    /// 但暴露的是读端口(`ClipboardEventRepositoryPort`),用于视图层反查
    /// 来源设备等只读语义。
    clipboard_event_reader_repo: Arc<dyn uc_core::ports::ClipboardEventRepositoryPort>,
    /// 投递结果仓储,由 `DispatchClipboardEntryUseCase` 写、由
    /// `GetEntryDeliveryViewUseCase` 读。
    entry_delivery_repo: Arc<dyn uc_core::ports::EntryDeliveryRepositoryPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,

    // Membership repository (phase 4b PR-4 起成为唯一持久成员层).
    member_repo: Arc<dyn uc_core::MemberRepositoryPort>,

    // Trusted-peer repository — authoritative write path from phase 0.4.2.
    // Drives `TrustPeerOrchestrator` at the pairing handler's PersistPairedDevice
    // boundary, replacing the previous `paired_device` upsert + `space_member`
    // shadow-write.
    trusted_peer_repo: Arc<dyn uc_core::TrustedPeerRepositoryPort>,

    // Slice 2 Phase 1 · T5：peer address 仓库。pairing 收尾点 best-effort
    // 写入对端传输地址，供 F1 `ensure_reachable_all` 直接拨号。
    peer_addr_repo: Arc<dyn uc_core::ports::PeerAddressRepositoryPort>,

    // Slice 3 Phase 1:明文 hash → 密文 digest 去重缓存。
    blob_reference_repo: Arc<dyn BlobReferenceRepositoryPort>,

    // Switch-space migration ports — see WiredDependencies docs for
    // life-cycle / consumer details.
    migration_state: Arc<dyn uc_core::ports::setup::MigrationStatePort>,
    blob_migration_repo: Arc<dyn uc_core::ports::clipboard::BlobMigrationRepoPort>,

    // Blob storage
    blob_repository: Arc<dyn BlobRepositoryPort>,
    thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    thumbnail_generator: Arc<dyn ThumbnailGeneratorPort>,

    // Security services
    key_material: Arc<KeyMaterialStore>,

    // Settings
    settings_repo: Arc<dyn SettingsPort>,

    // Setup status
    setup_status: Arc<dyn SetupStatusPort>,

    // 升级游标（"上次运行版本"）。落点 = app_data_root/upgrade-cursor.json，
    // 与 vault/keyring/settings.json 同级，profile 隔离由调用方上层保证。
    app_version_state: Arc<dyn AppVersionStatePort>,

    // 首次同步事件去重 flag。落点 = app_data_root/first-sync-state.json，
    // 与 upgrade-cursor.json 同级；schema 三 flag 一文件，port impl 内部
    // tokio::sync::Mutex 串行 read-check-write 保证 fan-out race 安全。
    first_sync_state: Arc<dyn FirstSyncStatePort>,

    // System services
    clock: Arc<dyn ClockPort>,
    hash: Arc<dyn ContentHashPort>,

    // File transfer tracking (projection/read-model port).
    file_transfer_repo: Arc<dyn uc_core::ports::FileTransferRepositoryPort>,

    // File transfer durable event store. Held as the concrete type so the
    // assembly can pass it directly to `build_file_transfer_assembly`
    // (which casts it to `Arc<dyn FileTransferEventStorePort>` before
    // handing it to the publisher and use cases).
    file_transfer_store: Arc<crate::file_transfer_lifecycle::FileTransferEventStore>,

    // Mobile sync 设备仓库 — `DieselMobileDeviceRepository`,跨重启 / 跨进
    // 程稳定的已登记设备列表(替代之前进程内 HashMap)。
    mobile_device_repo: Arc<dyn uc_core::ports::MobileDeviceRepositoryPort>,

    // Mobile sync LAN 端点状态(单例) — daemon listener 启停时调 inherent
    // `set` / `clear` 写它,facade 通过 `MobileSyncEndpointInfoPort` 只读。
    // 持有具体类型是为了让 daemon 拿到写入面;同一份 Arc 通过 unsizing
    // coercion 也能 share 给 AppDeps.mobile_sync.endpoint_info。
    mobile_sync_endpoint_info: Arc<uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter>,
}

/// Platform layer implementations
pub struct PlatformLayer {
    // System clipboard
    pub clipboard: Arc<dyn PlatformClipboardPort>,
    pub system_clipboard: Arc<dyn SystemClipboardPort>,

    // Secure storage
    pub secure_storage: Arc<dyn SecureStoragePort>,

    // Device identity
    pub device_identity: Arc<dyn DeviceIdentityPort>,

    // Clipboard representation normalizer
    pub representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,

    // Blob writer
    pub blob_writer: Arc<dyn BlobWriterPort>,

    // Blob store (encrypted) — exposed to use cases as a read-only port.
    pub blob_store: Arc<dyn BlobReaderPort>,

    // 进程内会话——uc-infra 内部 adapter (SpaceAccessAdapter / BlobCipherAdapter /
    // TransferCipherAdapter / EncryptedBlobStore) 共享同一份 Arc。具体类型,
    // 不再走 EncryptionSessionPort trait dyn 间接层。
    pub session: Arc<InMemorySession>,

    // Current profile
    pub current_profile: Arc<dyn uc_core::ports::security::current_profile::CurrentProfilePort>,
}

/// Create SQLite database connection pool
pub fn create_db_pool(db_path: &PathBuf) -> WiringResult<DbPool> {
    if db_path.as_os_str() != ":memory:" {
        if let Some(parent) = db_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| {
                WiringError::DatabaseInit(format!("Failed to create DB directory: {}", e))
            })?;
        }
    }

    let db_url = db_path
        .to_str()
        .ok_or_else(|| WiringError::DatabaseInit("Invalid database path".to_string()))?;

    init_db_pool(db_url)
        .map_err(|e| WiringError::DatabaseInit(format!("Failed to initialize DB: {}", e)))
}

/// Check if a file starts with the UCBL binary format magic bytes.
/// V2 blobs use magic [0x55, 0x43, 0x42, 0x4C] ("UCBL").
fn is_v2_blob(path: &std::path::Path) -> bool {
    const UCBL_MAGIC: [u8; 4] = [0x55, 0x43, 0x42, 0x4C];
    std::fs::File::open(path)
        .and_then(|mut f| {
            use std::io::Read;
            let mut buf = [0u8; 4];
            f.read_exact(&mut buf)?;
            Ok(buf == UCBL_MAGIC)
        })
        .unwrap_or(false)
}

/// Create infrastructure layer implementations
fn create_infra_layer(
    db_pool: DbPool,
    vault_path: &PathBuf,
    settings_path: &PathBuf,
    app_data_root: &PathBuf,
    secure_storage: Arc<dyn SecureStoragePort>,
) -> WiringResult<InfraLayer> {
    let db_executor = Arc::new(DieselSqliteExecutor::new(db_pool));

    let entry_row_mapper = ClipboardEntryRowMapper;
    let selection_row_mapper = ClipboardSelectionRowMapper;
    let blob_row_mapper = BlobRowMapper;
    let _representation_row_mapper = RepresentationRowMapper;

    let entry_repo = DieselClipboardEntryRepository::new(
        Arc::clone(&db_executor),
        entry_row_mapper,
        selection_row_mapper,
        ClipboardEntryRowMapper, // ZST - can instantiate again
    );
    let clipboard_entry_repo: Arc<dyn ClipboardEntryRepositoryPort> = Arc::new(entry_repo);

    let event_row_mapper = ClipboardEventRowMapper;
    let clipboard_event_repo_impl = Arc::new(DieselClipboardEventRepository::new(
        Arc::clone(&db_executor),
        event_row_mapper,
        RepresentationRowMapper,
    ));
    // 同一份 impl 同时满足"写"和"读"两个端口契约,unsize 两次拿到两个 Arc。
    let clipboard_event_repo: Arc<dyn ClipboardEventWriterPort> =
        Arc::clone(&clipboard_event_repo_impl) as Arc<_>;
    let clipboard_event_reader_repo: Arc<dyn uc_core::ports::ClipboardEventRepositoryPort> =
        clipboard_event_repo_impl as Arc<_>;

    let rep_repo = DieselClipboardRepresentationRepository::new(Arc::clone(&db_executor));
    let representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort> = Arc::new(rep_repo);

    let entry_delivery_repo: Arc<dyn uc_core::ports::EntryDeliveryRepositoryPort> = Arc::new(
        uc_infra::db::repositories::DieselEntryDeliveryRepository::new(Arc::clone(&db_executor)),
    );

    let member_repo_impl =
        DieselSpaceMemberRepository::new(Arc::clone(&db_executor), SpaceMemberRowMapper);
    let member_repo: Arc<dyn uc_core::MemberRepositoryPort> = Arc::new(member_repo_impl);

    let trusted_peer_repo_impl =
        DieselTrustedPeerRepository::new(Arc::clone(&db_executor), TrustedPeerRowMapper);
    let trusted_peer_repo: Arc<dyn uc_core::TrustedPeerRepositoryPort> =
        Arc::new(trusted_peer_repo_impl);

    let peer_addr_repo_impl =
        DieselPeerAddressRepository::new(Arc::clone(&db_executor), PeerAddressRowMapper);
    let peer_addr_repo: Arc<dyn uc_core::ports::PeerAddressRepositoryPort> =
        Arc::new(peer_addr_repo_impl);

    let blob_reference_repo: Arc<dyn BlobReferenceRepositoryPort> =
        Arc::new(DieselBlobReferenceRepository::new(Arc::clone(&db_executor)));

    let blob_repo = DieselBlobRepository::new(
        Arc::clone(&db_executor),
        blob_row_mapper,
        BlobRowMapper, // ZST - can instantiate again
    );
    let blob_repository: Arc<dyn BlobRepositoryPort> = Arc::new(blob_repo);

    let thumbnail_repo_impl = DieselThumbnailRepository::new(Arc::clone(&db_executor));
    let thumbnail_repo: Arc<dyn ThumbnailRepositoryPort> = Arc::new(thumbnail_repo_impl);
    let thumbnail_generator =
        InfraThumbnailGenerator::new(128).map_err(|e| WiringError::ThumbnailInit(e.to_string()))?;
    let thumbnail_generator: Arc<dyn ThumbnailGeneratorPort> = Arc::new(thumbnail_generator);

    let secure_storage_for_key_material = Arc::clone(&secure_storage);

    let keyslot_store = JsonKeySlotStore::new(vault_path.clone());
    let keyslot_store: Arc<dyn uc_infra::fs::key_slot_store::KeySlotStore> =
        Arc::new(keyslot_store);

    let key_material = Arc::new(KeyMaterialStore::new(
        secure_storage_for_key_material,
        keyslot_store,
    ));

    let settings_repo: Arc<dyn SettingsPort> = Arc::new(FileSettingsRepository::new(settings_path));

    let setup_status: Arc<dyn SetupStatusPort> =
        Arc::new(FileSetupStatusRepository::with_defaults(vault_path.clone()));

    // 升级游标——独立小文件，落在 app_data_root 顶层（与 vault/keyring/settings.json
    // 同级），不污染 vault/。schema_version=1，写入走 tempfile + rename 原子化。
    let app_version_state: Arc<dyn AppVersionStatePort> = Arc::new(
        FileAppVersionStateRepository::with_defaults(app_data_root.clone()),
    );

    // 首次同步事件去重 flag——独立小文件 first-sync-state.json，与升级游标同级。
    // 三 flag（attempted / succeeded / file_succeeded）合一，schema_version=1，
    // tempfile + rename 原子化；fan-out race 防护由 port impl 的 Mutex 守护。
    let first_sync_state: Arc<dyn FirstSyncStatePort> = Arc::new(
        FileFirstSyncStateRepository::with_defaults(app_data_root.clone()),
    );

    // Switch-space 4 阶段迁移的状态持久化点；与 setup_status 同目录。
    let migration_state: Arc<dyn uc_core::ports::setup::MigrationStatePort> = Arc::new(
        FileMigrationStateRepository::with_defaults(vault_path.clone()),
    );

    // Switch-space backup 表 + 主表 inline_data 批量 IO；常态业务代码不
    // 应触碰，由 SpaceSetupFacade::switch_space 内部使用。
    let blob_migration_repo: Arc<dyn uc_core::ports::clipboard::BlobMigrationRepoPort> =
        Arc::new(DieselBlobMigrationRepository::new(Arc::clone(&db_executor)));

    let clock: Arc<dyn ClockPort> = Arc::new(SystemClock);
    let hash: Arc<dyn ContentHashPort> = Arc::new(Blake3Hasher);

    let selection_repo_impl = DieselClipboardSelectionRepository::new(Arc::clone(&db_executor));
    let selection_repo: Arc<dyn ClipboardSelectionRepositoryPort> = Arc::new(selection_repo_impl);

    let file_transfer_repo: Arc<dyn uc_core::ports::FileTransferRepositoryPort> =
        Arc::new(DieselFileTransferRepository::new(Arc::clone(&db_executor)));

    let file_transfer_store = Arc::new(
        uc_infra::file_transfer::SqliteReceiverFileTransferStore::new(Arc::clone(&db_executor)),
    );

    let mobile_device_repo: Arc<dyn uc_core::ports::MobileDeviceRepositoryPort> = Arc::new(
        DieselMobileDeviceRepository::new(Arc::clone(&db_executor), MobileDeviceRowMapper),
    );

    // endpoint_info adapter:进程级单例,daemon LAN listener 与 facade 各持
    // 一份 Arc 共享同一份内存。整个进程只跑一次 `wire_dependencies`,这里
    // new 一份就足够。
    let mobile_sync_endpoint_info =
        Arc::new(uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter::new());

    let infra = InfraLayer {
        clipboard_entry_repo,
        clipboard_event_repo,
        clipboard_event_reader_repo,
        entry_delivery_repo,
        representation_repo,
        selection_repo,
        member_repo,
        trusted_peer_repo,
        peer_addr_repo,
        blob_reference_repo,
        migration_state,
        blob_migration_repo,
        blob_repository,
        thumbnail_repo,
        thumbnail_generator,
        key_material,
        settings_repo,
        setup_status,
        app_version_state,
        first_sync_state,
        clock,
        hash,
        file_transfer_repo,
        file_transfer_store,
        mobile_device_repo,
        mobile_sync_endpoint_info,
    };

    Ok(infra)
}

pub fn create_platform_layer(
    secure_storage: Arc<dyn SecureStoragePort>,
    config_dir: &PathBuf,
    blob_repository: Arc<dyn BlobRepositoryPort>,
    _member_repo: Arc<dyn uc_core::MemberRepositoryPort>,
    clock: Arc<dyn ClockPort>,
    storage_config: Arc<ClipboardStorageConfig>,
) -> WiringResult<PlatformLayer> {
    // Slice 1 CLI commands (init/invite/join) do not touch the system
    // clipboard, but a non-bundled CLI launched from a shell lacks the
    // WindowServer / AppKit context that `clipboard-rs` assumes, so
    // `LocalClipboard::new()` panics inside `+[NSPasteboard generalPasteboard]`.
    // When `UC_DISABLE_SYSTEM_CLIPBOARD=1` is set we skip the real
    // adapter entirely and wire in `NoopSystemClipboard`. The CLI sets
    // this variable before bootstrap; GUI / daemon paths leave it unset
    // and get the real adapter.
    let (clipboard, system_clipboard): (
        Arc<dyn PlatformClipboardPort>,
        Arc<dyn SystemClipboardPort>,
    ) = if std::env::var_os("UC_DISABLE_SYSTEM_CLIPBOARD").is_some() {
        tracing::info!(
            "UC_DISABLE_SYSTEM_CLIPBOARD set; substituting NoopSystemClipboard \
             (any clipboard capture / write is a no-op)"
        );
        let noop: Arc<NoopSystemClipboard> = Arc::new(NoopSystemClipboard);
        (noop.clone(), noop)
    } else {
        let clipboard_impl = LocalClipboard::new().map_err(|e| {
            WiringError::ClipboardInit(format!("Failed to create clipboard: {}", e))
        })?;
        let clipboard_impl = Arc::new(clipboard_impl);
        (clipboard_impl.clone(), clipboard_impl)
    };

    let device_identity = LocalDeviceIdentity::load_or_create(config_dir.clone()).map_err(|e| {
        WiringError::SettingsInit(format!("Failed to create device identity: {}", e))
    })?;
    let device_identity: Arc<dyn DeviceIdentityPort> = Arc::new(device_identity);

    let blob_store_dir = config_dir.join("blobs");

    // Purge old blob files after V2 migration (old JSON format files are incompatible
    // with the new UCBL binary format). Uses a sentinel file so this only runs once.
    let sentinel = blob_store_dir.join(".v2_migrated");
    if blob_store_dir.exists() && !sentinel.exists() {
        match std::fs::read_dir(&blob_store_dir) {
            Ok(entries) => {
                let mut purged = 0u64;
                let mut errors = 0u64;
                for entry_result in entries {
                    let entry = match entry_result {
                        Ok(e) => e,
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to read directory entry during V2 migration");
                            errors += 1;
                            continue;
                        }
                    };
                    if entry.path().is_file() {
                        let path = entry.path();
                        if path.file_name().map_or(false, |n| n == ".v2_migrated") {
                            continue;
                        }
                        if is_v2_blob(&path) {
                            continue;
                        }
                        if let Err(e) = std::fs::remove_file(&path) {
                            tracing::warn!(
                                path = %path.display(),
                                error = %e,
                                "Failed to purge old blob file"
                            );
                            errors += 1;
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

                if errors == 0 {
                    if let Err(e) = std::fs::File::create(&sentinel) {
                        tracing::warn!(error = %e, "Failed to create V2 migration sentinel");
                    }
                } else {
                    tracing::warn!(
                        errors = errors,
                        "Skipping V2 migration sentinel: {} errors during cleanup, will retry next startup",
                        errors
                    );
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to read blob directory for cleanup");
            }
        }
    }

    let blob_store: Arc<dyn BlobStorePort> = Arc::new(FilesystemBlobStore::new(blob_store_dir));

    let representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort> =
        Arc::new(ClipboardRepresentationNormalizer::new(storage_config));

    // 进程内会话: uc-infra adapter 共享的具体类型,替换历史
    // InMemoryEncryptionSessionPort + EncryptionSessionPort trait dyn 间接层。
    let session = Arc::new(InMemorySession::new());

    let encrypted_blob_store =
        Arc::new(EncryptedBlobStore::new(blob_store.clone(), session.clone()));

    // BlobWriter needs the put-side (BlobStorePort); use cases need only the
    // read-side (BlobReaderPort). Both views point at the same concrete
    // EncryptedBlobStore instance.
    let encrypted_blob_store_for_writer: Arc<dyn BlobStorePort> = encrypted_blob_store.clone();
    let blob_writer: Arc<dyn BlobWriterPort> = Arc::new(BlobWriter::new(
        encrypted_blob_store_for_writer,
        blob_repository,
        clock,
    ));
    let blob_store_reader: Arc<dyn BlobReaderPort> = encrypted_blob_store;

    let current_profile: Arc<dyn uc_core::ports::security::current_profile::CurrentProfilePort> =
        Arc::new(uc_infra::security::DefaultCurrentProfile::new());

    Ok(PlatformLayer {
        clipboard,
        system_clipboard,
        secure_storage,
        device_identity,
        representation_normalizer,
        blob_writer,
        blob_store: blob_store_reader,
        session,
        current_profile,
    })
}

/// Resolves the application's default directories for storing data and configuration.
pub fn get_default_app_dirs() -> WiringResult<uc_core::app_dirs::AppDirs> {
    let adapter = DirsAppDirsAdapter::new();
    adapter
        .get_app_dirs()
        .map_err(|e| WiringError::ConfigInit(e.to_string()))
}

/// Get resolved storage paths from configuration.
pub fn get_storage_paths(
    config: &uc_core::config::AppConfig,
) -> WiringResult<uc_application::facade::AppPaths> {
    let platform_dirs = get_default_app_dirs()?;
    resolve_app_paths(&platform_dirs, config)
}

/// Build `AppPaths` from platform dirs and config overrides.
pub fn resolve_app_paths(
    platform_dirs: &uc_core::app_dirs::AppDirs,
    config: &AppConfig,
) -> WiringResult<uc_application::facade::AppPaths> {
    let mut paths = uc_application::facade::AppPaths::from_app_dirs(platform_dirs);

    let is_in_memory_db = config.database_path.as_os_str() == ":memory:";

    if is_in_memory_db {
        paths.db_path = config.database_path.clone();
    } else if !config.database_path.as_os_str().is_empty() {
        if config.database_path.is_absolute() {
            // Absolute path: use as-is. In production the path is already inside
            // app_data_root_dir; tests use temp dirs and need the full path respected.
            paths.db_path = config.database_path.clone();
        } else {
            let db_file_name = config
                .database_path
                .file_name()
                .map(|name| name.to_os_string())
                .unwrap_or_else(|| std::ffi::OsString::from("uniclipboard.db"));
            paths.db_path = paths.app_data_root_dir.join(db_file_name);
        }
    }

    if !config.vault_key_path.as_os_str().is_empty() {
        let configured_vault_root = config
            .vault_key_path
            .parent()
            .unwrap_or(&config.vault_key_path)
            .to_path_buf();

        if config.database_path.as_os_str().is_empty() {
            paths.vault_dir = apply_profile_suffix(configured_vault_root);
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
                paths.vault_dir = paths.app_data_root_dir.join(relative);
            } else {
                paths.vault_dir = apply_profile_suffix(configured_vault_root);
            }
        }
    }

    Ok(paths)
}

pub fn apply_profile_suffix(path: PathBuf) -> PathBuf {
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

/// 进程级一次性装配:把 sqlite pool / repos / settings / secure storage /
/// blob store / 所有 adapter 等装配成 [`WiredDependencies`] +
/// [`BackgroundRuntimeDeps`]。
///
/// 整个进程只调用一次 —— GUI shell 在 `build_process_runtime` 里调,
/// standalone daemon binary 同样走这条路径 (两条入口共用)。
///
/// 返回 tuple 把"持久" 与"一次性消费"两类资源分开:`WiredDependencies`
/// 进程内常驻;`BackgroundRuntimeDeps` 含两个 mpsc::Receiver, 在进程
/// 启动期被 `spawn_blob_processing_tasks` 消费一次后不复存在。
///
/// Slice 4 P5b 起 libp2p adapter 已删除,旧的 `wire_dependencies_with_identity_store`
/// 变体随之退场——iroh 栈走 `IrohIdentityStore`(由 `build_space_setup_assembly`
/// 构造,密钥落地 `SecureStoragePort`),不再需要 platform 层
/// `IdentityStorePort` 兼容入口。
pub fn wire_dependencies(
    config: &AppConfig,
) -> WiringResult<(WiredDependencies, BackgroundRuntimeDeps)> {
    let platform_dirs = get_default_app_dirs()?;
    let paths = resolve_app_paths(&platform_dirs, config)?;

    let db_path = paths.db_path;
    let db_pool = create_db_pool(&db_path)?;
    // Clone pool before infra layer consumes it — search bundle needs the same pool.
    let db_pool_for_search = db_pool.clone();

    let vault_path = paths.vault_dir;
    let settings_path = paths.settings_path;
    let app_data_root = paths.app_data_root_dir.clone();

    let secure_storage =
        uc_platform::secure_storage::create_default_secure_storage_in_app_data_root(
            app_data_root.clone(),
        )
        .map_err(|e| WiringError::SecureStorageInit(e.to_string()))?;

    let infra = create_infra_layer(
        db_pool,
        &vault_path,
        &settings_path,
        &app_data_root,
        secure_storage.clone(),
    )?;

    let storage_config = Arc::new(ClipboardStorageConfig::defaults());
    let platform = create_platform_layer(
        secure_storage,
        &vault_path,
        infra.blob_repository.clone(),
        infra.member_repo.clone(),
        infra.clock.clone(),
        storage_config.clone(),
    )?;

    // SpaceAccessPort——单一会话/密钥访问入口。adapter 自管 KeyMaterialStore +
    // InMemorySession + CurrentProfilePort,V1 AEAD 走 v1_aead helper。
    // Phase C 起不再依赖 EncryptionStatePort (已物理删除);adapter 用
    // `key_material.keyslot_exists()` 判断是否已初始化。
    let space_access: Arc<dyn uc_core::ports::space::SpaceAccessPort> =
        Arc::new(uc_infra::security::DefaultSpaceAccessAdapter::new(
            infra.key_material.clone(),
            platform.current_profile.clone(),
            platform.session.clone(),
        ));

    // Wire the search bundle (Phase 92).
    let search_key_derivation: Arc<dyn SearchKeyDerivationPort> = Arc::new(
        HkdfSearchKeyDerivation::new(space_access.clone(), platform.current_profile.clone()),
    );
    let search_index: Arc<dyn SearchIndexPort> = Arc::new(SqliteSearchIndex::new(
        db_pool_for_search,
        platform.current_profile.clone(),
        search_key_derivation.clone(),
    ));
    let search_pipeline = Arc::new(SearchPipeline::new());

    // BlobCipherPort——4 个 decorator 共享的业务 AEAD adapter。
    let blob_cipher: Arc<dyn uc_core::ports::security::BlobCipherPort> = Arc::new(
        uc_infra::security::BlobCipherAdapter::new(platform.session.clone()),
    );

    // TransferCipherPort——uc-application clipboard_sync 的 dispatch_entry /
    // ingest_inbound 通过此 port 加解密 V3 网络字节,与 BlobCipherPort 共享
    // 同一个 InMemorySession。
    let transfer_cipher: Arc<dyn uc_core::ports::security::TransferCipherPort> = Arc::new(
        uc_infra::clipboard::TransferCipherAdapter::new(platform.session.clone()),
    );

    // Wrap ports with encryption decorators
    let encrypting_event_writer: Arc<dyn ClipboardEventWriterPort> =
        Arc::new(EncryptingClipboardEventWriter::new(
            infra.clipboard_event_repo.clone(),
            blob_cipher.clone(),
        ));

    let decrypting_rep_repo: Arc<dyn ClipboardRepresentationRepositoryPort> =
        Arc::new(DecryptingClipboardRepresentationRepository::new(
            infra.representation_repo.clone(),
            blob_cipher.clone(),
        ));

    // Create background processing components
    let representation_cache = Arc::new(RepresentationCache::new(
        storage_config.cache_max_entries,
        storage_config.cache_max_bytes,
    ));
    let representation_cache_port: Arc<dyn RepresentationCachePort> = representation_cache.clone();

    let spool_dir = paths.spool_dir.clone();
    let spool_manager = Arc::new(
        SpoolManager::new(spool_dir.clone(), storage_config.spool_max_bytes)
            .map_err(|e| WiringError::BlobStorageInit(format!("Failed to create spool: {}", e)))?,
    );

    let (spool_tx, spool_rx) = mpsc::channel::<SpoolRequest>(100);
    let (worker_tx, worker_rx) = mpsc::channel::<RepresentationId>(100);

    // DurableSpoolQueue writes bytes to disk synchronously before returning,
    // ensuring spool files survive process exits. The in-memory MpscSpoolQueue
    // used previously only enqueued bytes into a channel; if the app exited
    // before SpoolerTask drained the channel, the bytes were permanently lost.
    let spool_queue: Arc<dyn SpoolQueuePort> = Arc::new(DurableSpoolQueue::new(
        spool_manager.clone(),
        worker_tx.clone(),
    ));

    let origin_impl = new_in_memory_change_origin();
    init_clipboard_change_origin(origin_impl.clone());
    let clipboard_change_origin =
        clipboard_change_origin().expect("clipboard_change_origin not initialized");

    // Extract the concrete file-transfer store before moving the rest of InfraLayer
    // into AppDeps — it is not exposed through uc-app ports (the use cases see it
    // as `Arc<dyn FileTransferEventStorePort>`), so it travels via BackgroundRuntimeDeps.
    let file_transfer_store_arc = Arc::clone(&infra.file_transfer_store);

    // Clone the trusted-peer repository handle before moving `infra` into
    // `AppDeps` below — the daemon-lifecycle builder (build_daemon_lifecycle) needs
    // it to construct the `TrustPeerOrchestrator` singleton (D19). We do not
    // thread it through `AppDeps` because uc-app is retiring (D13) and
    // the repository is consumed solely by uc-application wiring.
    let trusted_peer_repo_for_wiring = Arc::clone(&infra.trusted_peer_repo);
    // Same pattern for `peer_addr_repo` — Slice 2 Phase 1 wiring consumer
    // is `SpaceSetupFacade`, which lives in uc-application, not uc-app.
    let peer_addr_repo_for_wiring = Arc::clone(&infra.peer_addr_repo);
    let blob_reference_repo_for_wiring = Arc::clone(&infra.blob_reference_repo);
    let entry_delivery_repo_for_wiring = Arc::clone(&infra.entry_delivery_repo);
    let clipboard_event_reader_repo_for_wiring = Arc::clone(&infra.clipboard_event_reader_repo);
    let iroh_blob_store_dir_for_wiring =
        apply_profile_suffix(paths.app_data_root_dir.join("iroh-blobs"));
    // iroh 设备身份的文件存储目录。先 mkdir,确保 `FileSecureStorage::with_base_dir`
    // 在首次写身份时不会因目录不存在而失败。`apply_profile_suffix` 与
    // `iroh_blob_store_dir` 用同一规则,保证 multi-profile dev 隔离。
    let iroh_identity_dir_for_wiring =
        apply_profile_suffix(paths.app_data_root_dir.join("iroh-identity"));
    std::fs::create_dir_all(&iroh_identity_dir_for_wiring).map_err(|e| {
        WiringError::SecureStorageInit(format!(
            "failed to create iroh-identity dir {}: {e}",
            iroh_identity_dir_for_wiring.display()
        ))
    })?;

    // Switch-space migration ports for SpaceSetupFacade. Same WiredDependencies
    // bypass pattern as `peer_addr_repo` — consumer lives in uc-application.
    let migration_state_for_wiring = Arc::clone(&infra.migration_state);
    let blob_migration_repo_for_wiring = Arc::clone(&infra.blob_migration_repo);
    // Phase 3 子步骤 3:把 endpoint_info adapter 也通过旁路暴露给 daemon
    // builder——daemon LAN listener 需要具体类型来调 inherent `set` / `clear`
    // 写入面,trait object 拿不到这两个方法。同一份 Arc 已经通过 unsizing
    // coercion 装入 `AppDeps.mobile_sync.endpoint_info`,daemon 写、facade
    // 读,共享同一份内存。
    let mobile_sync_endpoint_info_for_wiring = Arc::clone(&infra.mobile_sync_endpoint_info);
    // `key_migration` adapter consumes secure_storage from PlatformLayer,
    // so it's constructed here at wire_dependencies level rather than in
    // create_infra_layer.
    let key_migration_for_wiring: Arc<dyn uc_core::ports::security::KeyMigrationPort> = Arc::new(
        uc_infra::security::DefaultKeyMigrationAdapter::new(Arc::clone(&platform.secure_storage)),
    );

    // Create payload resolver for resolving staged/processing payloads
    let payload_resolver: Arc<dyn ClipboardPayloadResolverPort> =
        Arc::new(ClipboardPayloadResolver::new(
            representation_cache.clone(),
            spool_manager.clone(),
            worker_tx.clone(),
        ));

    let deps = AppDeps {
        clipboard: ClipboardPorts {
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
            payload_resolver,
        },
        security: SecurityPorts {
            current_profile: platform.current_profile,
            secure_storage: platform.secure_storage,
            space_access: space_access.clone(),
            blob_cipher: blob_cipher.clone(),
            transfer_cipher: transfer_cipher.clone(),
            pin_hasher: Arc::new(Argon2PinHasher),
            short_code: Arc::new(Sha256ShortCodeGenerator),
            fingerprint: Arc::new(Sha256IdentityFingerprintFactory),
        },
        device: DevicePorts {
            device_identity: platform.device_identity,
            member_repo: infra.member_repo,
        },
        setup_status: infra.setup_status,
        app_version_state: infra.app_version_state,
        first_sync_state: infra.first_sync_state,
        storage: StoragePorts {
            blob_store: platform.blob_store,
            blob_writer: platform.blob_writer,
            thumbnail_repo: infra.thumbnail_repo,
            thumbnail_generator: infra.thumbnail_generator,
            file_transfer_repo: infra.file_transfer_repo,
        },
        settings: infra.settings_repo,
        system: SystemPorts {
            clock: infra.clock,
            hash: infra.hash,
            cache_fs: Arc::new(uc_infra::fs::TokioCacheFsAdapter::new()),
        },
        search: SearchPorts {
            search_index,
            search_key_derivation,
            search_pipeline,
        },
        mobile_sync: MobileSyncPorts {
            device_repo: infra.mobile_device_repo,
            endpoint_info: infra.mobile_sync_endpoint_info.clone(),
        },
        analytics: crate::analytics::build_analytics_sink(),
    };

    // Create shared emitter cell at wire time using the logging placeholder.
    // All consumers (CoreRuntime, SetupOrchestrator, FileTransferOrchestrator)
    // hold a clone of this cell and automatically see the emitter after any swap.
    let initial_emitter: Arc<dyn HostEventEmitterPort> =
        Arc::new(crate::non_gui_runtime::LoggingHostEventEmitter);
    let emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>> =
        Arc::new(std::sync::RwLock::new(initial_emitter));

    let crate::file_transfer_lifecycle::FileTransferAssembly {
        lifecycle: file_transfer_lifecycle,
        facade: file_transfer_facade,
    } = crate::file_transfer_lifecycle::build_file_transfer_assembly(
        Arc::clone(&file_transfer_store_arc),
        emitter_cell.clone(),
        deps.storage.file_transfer_repo.clone(),
        deps.system.clock.clone(),
    );

    let clipboard_write_coordinator = build_clipboard_write_coordinator(
        deps.clipboard.system_clipboard.clone(),
        deps.clipboard.clipboard_change_origin.clone(),
    );

    let wired = WiredDependencies {
        deps,
        trusted_peer_repo: trusted_peer_repo_for_wiring,
        peer_addr_repo: peer_addr_repo_for_wiring,
        iroh_blob_store_dir: iroh_blob_store_dir_for_wiring,
        iroh_identity_dir: iroh_identity_dir_for_wiring,
        blob_reference_repo: blob_reference_repo_for_wiring,
        migration_state: migration_state_for_wiring,
        key_migration: key_migration_for_wiring,
        blob_migration_repo: blob_migration_repo_for_wiring,
        mobile_sync_endpoint_info: mobile_sync_endpoint_info_for_wiring,
        entry_delivery_repo: entry_delivery_repo_for_wiring,
        clipboard_event_reader_repo: clipboard_event_reader_repo_for_wiring,
        emitter_cell,
        file_transfer_facade,
    };
    let background = BackgroundRuntimeDeps {
        representation_cache,
        spool_manager,
        spool_tx,
        spool_rx,
        worker_rx,
        spool_dir,
        file_cache_dir: paths.file_cache_dir.clone(),
        spool_ttl_days: storage_config.spool_ttl_days,
        worker_retry_max_attempts: storage_config.worker_retry_max_attempts,
        worker_retry_backoff_ms: storage_config.worker_retry_backoff_ms,
        file_transfer_lifecycle,
        clipboard_write_coordinator,
    };
    Ok((wired, background))
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

/// Constructs a `ClipboardWriteCoordinator` — the single write boundary for all
/// programmatic clipboard writes.
///
/// Centralises the guard-registration + write + cleanup-on-error pattern
/// (previously duplicated across restore_clipboard_selection, copy_file_to_clipboard,
/// and the now-deleted `sync_inbound` libp2p path).
pub fn build_clipboard_write_coordinator(
    system_clipboard: Arc<dyn uc_core::ports::clipboard::SystemClipboardPort>,
    clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
) -> Arc<uc_application::clipboard_write::ClipboardWriteCoordinator> {
    Arc::new(
        uc_application::clipboard_write::ClipboardWriteCoordinator::new(
            system_clipboard,
            clipboard_change_origin,
        ),
    )
}

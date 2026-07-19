//! # Dependency wiring
//!
//! The composition-root core: builds the infrastructure layer (DB pool, repos,
//! encryption decorators, search, blob processing) into an `InfraLayer`, then
//! assembles already prepared host inputs into the `WiredDependencies` and
//! `BackgroundRuntimeDeps` consumed by the process.
//!
//! Infra construction stays co-located with the shared orchestrator because the
//! orchestrator consumes the `InfraLayer` (and the intermediate assembly DTOs)
//! field-by-field; they are one cohesive wiring unit. The output bundle types
//! live in [`crate::internal::deps`].
//!
//! ## Architecture Principle
//!
//! > **Zero tauri imports in this file.**

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use uc_application::deps::{
    AppDeps, ClipboardEntryPorts, ClipboardPorts, ClipboardRepresentationPorts, DevicePorts,
    DirectoryReceivePorts, FileTransferPorts, MobileDevicePorts, MobileSyncPorts, SearchPorts,
    SecurityPorts, SpaceAccessPorts, StoragePorts, SystemPorts,
};
use uc_application::facade::{ConfigMigrationDeps, ConfigMigrationFacade, HostEventEmitterPort};
use uc_core::app_dirs::AppPaths;
use uc_core::clipboard::SelectRepresentationPolicyV1;
use uc_core::ids::{ProfileId, RepresentationId};
use uc_core::ports::blob::BlobReferenceRepositoryPort;
use uc_core::ports::clipboard::{RepresentationCachePort, SelfWriteLedgerPort, SpoolQueuePort};
use uc_core::ports::*;
use uc_infra::blob::BlobRepositoryPort;
use uc_infra::clipboard::{
    clipboard_change_origin, init_clipboard_change_origin, new_in_memory_change_origin,
    ClipboardPayloadResolver, DurableSpoolQueue, InfraThumbnailGenerator, RepresentationCache,
    SpoolManager,
};
use uc_infra::config::ClipboardStorageConfig;
use uc_infra::config_migration::{ConfigMigrationAdapter, ConfigMigrationPaths};
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
    DieselClipboardEntryReplaceRepository, DieselClipboardEntryRepository,
    DieselClipboardEventRepository, DieselClipboardRepresentationRepository,
    DieselClipboardSelectionRepository, DieselEntryAvailabilityRepository,
    DieselFileTransferRepository, DieselInboundReceiveCommitRepository,
    DieselMobileDeviceRepository, DieselPeerAddressRepository, DieselReceiveArtifactLogRepository,
    DieselSpaceMemberRepository, DieselThumbnailRepository, DieselTrustedPeerRepository,
};
use uc_infra::fs::key_slot_store::JsonKeySlotStore;
use uc_infra::network::iroh::IrohIdentityStore;
use uc_infra::search::{HkdfSearchKeyDerivation, SearchPipeline, SqliteSearchIndex};
use uc_infra::security::{
    Argon2PinHasher, Blake3Hasher, DecryptingClipboardRepresentationRepository,
    EncryptingClipboardEventWriter, InMemorySession, KeyMaterialStore,
    Sha256IdentityFingerprintFactory, Sha256ShortCodeGenerator,
};
use uc_infra::settings::repository::FileSettingsRepository;
use uc_infra::{
    FileAppVersionStateRepository, FileFirstSyncStateRepository, FileMigrationStateRepository,
    FileSetupStatusRepository, SystemClock,
};
use uc_observability_contract::analytics::{AnalyticsFacade, AnalyticsPort};

use crate::internal::deps::{
    BackgroundRuntimeDeps, DaemonRuntimeDeps, SharedRuntimeDeps, SyncEngineDeps, WiredDependencies,
    WiringError, WiringResult,
};
use crate::internal::platform::{create_platform_layer, SystemClipboardLayer};

/// Infrastructure layer implementations
struct InfraLayer {
    // Clipboard repositories
    clipboard_entry_ports: ClipboardEntryPorts,
    clipboard_event_repo: Arc<dyn ClipboardEventWriterPort>,
    /// 与 `clipboard_event_repo` 共享底层 `DieselClipboardEventRepository`,
    /// 但暴露的是读端口(`ClipboardEventRepositoryPort`),用于视图层反查
    /// 来源设备等只读语义。
    clipboard_event_reader_repo: Arc<dyn uc_core::ports::ClipboardEventRepositoryPort>,
    /// 投递结果仓储,由 `DispatchClipboardEntryUseCase` 写、由
    /// `GetEntryDeliveryViewUseCase` 读。
    entry_delivery_repo: Arc<dyn uc_core::ports::EntryDeliveryRepositoryPort>,
    /// Shared Diesel executor. Exposed so repos that also need a post-`platform`
    /// dependency (e.g. the entry-file-set repo's per-session path cipher) can be
    /// constructed after space access is wired, over the same connection pool.
    db_executor: Arc<DieselSqliteExecutor>,
    representation_repo: Arc<dyn ClipboardRepresentationStore>,
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

    // Mobile sync 设备仓库 — narrow device-repository intent ports, all backed
    // by one `DieselMobileDeviceRepository` (cross-restart / cross-process
    // stable; coerced per ports.md §8.3).
    mobile_device_ports: MobileDevicePorts,

    // Mobile sync LAN 端点状态(单例) — daemon listener 启停时调 inherent
    // `set` / `clear` 写它,facade 通过 `MobileSyncEndpointInfoPort` 只读。
    // 持有具体类型是为了让 daemon 拿到写入面;同一份 Arc 通过 unsizing
    // coercion 也能 share 给 AppDeps.mobile_sync.endpoint_info。
    mobile_sync_endpoint_info: Arc<uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter>,
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

pub struct CoreWiringInputs {
    pub paths: AppPaths,
    pub secure_storage: Arc<dyn SecureStoragePort>,
    pub legacy_iroh_identity_dir: PathBuf,
    pub iroh_blob_store_dir: PathBuf,
    pub system_clipboard: SystemClipboardLayer,
    pub analytics_sink: Arc<dyn AnalyticsPort>,
    pub analytics_facade: Arc<dyn AnalyticsFacade>,
    pub host_event_emitter: Arc<dyn HostEventEmitterPort>,
}

/// Wire the [`SpaceAccessPorts`] bundle: one `DefaultSpaceAccessAdapter` coerced
/// into every narrow space-access intent port (ports.md §8.3 — the adapter
/// implements the aggregate `SpaceAccessStore`, each intent-port impl delegates
/// to it). The narrow bundle is the only space-access surface the application
/// layer consumes.
fn build_space_access_ports(
    key_material: &Arc<KeyMaterialStore>,
    current_profile: &Arc<dyn uc_core::ports::security::current_profile::CurrentProfilePort>,
    session: &Arc<InMemorySession>,
) -> SpaceAccessPorts {
    let space_access_adapter = Arc::new(uc_infra::security::DefaultSpaceAccessAdapter::new(
        key_material.clone(),
        current_profile.clone(),
        session.clone(),
    ));
    SpaceAccessPorts::from_adapter(space_access_adapter)
}

/// Search bundle (Phase 92): subkey-derivation port, sqlite index, tokenization
/// pipeline. `search_pipeline` is kept as the concrete `Arc<SearchPipeline>`; it
/// coerces to `Arc<dyn SearchPipelinePort>` at the `SearchPorts` literal.
struct SearchAssembly {
    search_index: Arc<dyn SearchIndexPort>,
    search_maintenance: Arc<dyn SearchIndexMaintenancePort>,
    search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
    search_pipeline: Arc<SearchPipeline>,
}

/// Wire the [`SearchAssembly`]. Search only derives a subkey from space access.
/// Takes its own `db_pool` clone (an independent pooled connection).
fn build_search_assembly(
    db_pool_for_search: DbPool,
    space_access_ports: &SpaceAccessPorts,
    current_profile: &Arc<dyn uc_core::ports::security::current_profile::CurrentProfilePort>,
) -> SearchAssembly {
    let search_key_derivation: Arc<dyn SearchKeyDerivationPort> =
        Arc::new(HkdfSearchKeyDerivation::new(
            space_access_ports.derive_subkey.clone(),
            current_profile.clone(),
        ));
    // One concrete adapter, coerced into both the index port and the maintenance
    // port (ports.md §8.3: one Arc behind several narrow ports).
    let sqlite_search_index = Arc::new(SqliteSearchIndex::new(
        db_pool_for_search,
        current_profile.clone(),
        search_key_derivation.clone(),
    ));
    let search_index: Arc<dyn SearchIndexPort> = sqlite_search_index.clone();
    let search_maintenance: Arc<dyn SearchIndexMaintenancePort> = sqlite_search_index;
    let search_pipeline = Arc::new(SearchPipeline::new());
    SearchAssembly {
        search_index,
        search_maintenance,
        search_key_derivation,
        search_pipeline,
    }
}

/// Encryption decorators + cipher ports. `blob_cipher` is the business AEAD
/// adapter shared by the decorators and the transfer cipher; all share the one
/// process `InMemorySession`.
struct CipherDecorators {
    blob_cipher: Arc<dyn uc_core::ports::security::BlobCipherPort>,
    transfer_cipher: Arc<dyn uc_core::ports::security::TransferCipherPort>,
    encrypting_event_writer: Arc<dyn ClipboardEventWriterPort>,
    decrypting_rep_repo: Arc<dyn ClipboardRepresentationStore>,
    representation_ports: ClipboardRepresentationPorts,
}

/// Wire the [`CipherDecorators`]: blob/transfer cipher ports over the session,
/// the encrypting event writer, and the decrypting representation repository
/// (one concrete Arc coerced into each narrow intent port, ports.md §8.3).
fn build_cipher_decorators(
    session: &Arc<InMemorySession>,
    clipboard_event_repo: &Arc<dyn ClipboardEventWriterPort>,
    representation_repo: &Arc<dyn ClipboardRepresentationStore>,
) -> CipherDecorators {
    // BlobCipherPort — business AEAD adapter shared by the decorators.
    let blob_cipher: Arc<dyn uc_core::ports::security::BlobCipherPort> =
        Arc::new(uc_infra::security::BlobCipherAdapter::new(session.clone()));

    // TransferCipherPort — uc-application clipboard_sync encrypts/decrypts V3
    // network bytes through this port, sharing the same InMemorySession.
    let transfer_cipher: Arc<dyn uc_core::ports::security::TransferCipherPort> = Arc::new(
        uc_infra::clipboard::TransferCipherAdapter::new(session.clone()),
    );

    // Wrap ports with encryption decorators.
    let encrypting_event_writer: Arc<dyn ClipboardEventWriterPort> = Arc::new(
        EncryptingClipboardEventWriter::new(clipboard_event_repo.clone(), blob_cipher.clone()),
    );

    // Concrete decorator Arc: coerced into the legacy aggregate port and into
    // each application-facing representation intent port. Reads decrypt;
    // background workers keep the inner store via `infra.representation_repo`.
    let decrypting_rep_repo_concrete = Arc::new(DecryptingClipboardRepresentationRepository::new(
        representation_repo.clone(),
        blob_cipher.clone(),
    ));
    let decrypting_rep_repo: Arc<dyn ClipboardRepresentationStore> =
        decrypting_rep_repo_concrete.clone();
    let representation_ports = ClipboardRepresentationPorts {
        get: decrypting_rep_repo_concrete.clone(),
        get_by_blob_id: decrypting_rep_repo_concrete.clone(),
        list_for_event: decrypting_rep_repo_concrete.clone(),
        update_processing_result: decrypting_rep_repo_concrete,
    };

    CipherDecorators {
        blob_cipher,
        transfer_cipher,
        encrypting_event_writer,
        decrypting_rep_repo,
        representation_ports,
    }
}

/// Background blob-processing components. `representation_cache` /
/// `spool_manager` are concrete (BackgroundRuntimeDeps needs them by-value);
/// `worker_rx` is the non-Clone receiving half of the worker channel.
struct BlobProcessingAssembly {
    representation_cache: Arc<RepresentationCache>,
    representation_cache_port: Arc<dyn RepresentationCachePort>,
    spool_manager: Arc<SpoolManager>,
    spool_queue: Arc<dyn SpoolQueuePort>,
    payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    worker_tx: mpsc::Sender<RepresentationId>,
    worker_rx: mpsc::Receiver<RepresentationId>,
    clipboard_change_origin: Arc<dyn SelfWriteLedgerPort>,
}

/// Wire the [`BlobProcessingAssembly`]: representation cache, spool manager +
/// durable queue, the worker channel, the self-write ledger, and the payload
/// resolver. `spool_dir` is consumed to build the spool manager.
fn build_blob_processing_assembly(
    storage_config: &Arc<ClipboardStorageConfig>,
    spool_dir: PathBuf,
) -> WiringResult<BlobProcessingAssembly> {
    let representation_cache = Arc::new(RepresentationCache::new(
        storage_config.cache_max_entries,
        storage_config.cache_max_bytes,
    ));
    let representation_cache_port: Arc<dyn RepresentationCachePort> = representation_cache.clone();

    let spool_manager = Arc::new(
        SpoolManager::new(spool_dir, storage_config.spool_max_bytes)
            .map_err(|e| WiringError::BlobStorageInit(format!("Failed to create spool: {}", e)))?,
    );

    let (worker_tx, worker_rx) = mpsc::channel::<RepresentationId>(100);

    // DurableSpoolQueue writes bytes to disk synchronously before returning,
    // ensuring spool files survive process exits.
    let spool_queue: Arc<dyn SpoolQueuePort> = Arc::new(DurableSpoolQueue::new(
        spool_manager.clone(),
        worker_tx.clone(),
    ));

    // Self-write ledger: a process-global OnceLock initialised here. Kept inside
    // this bundle so the `clipboard_change_origin()` read never races ahead of
    // `init_clipboard_change_origin`.
    let origin_impl = new_in_memory_change_origin();
    init_clipboard_change_origin(origin_impl.clone());
    let clipboard_change_origin =
        clipboard_change_origin().expect("clipboard_change_origin not initialized");

    // Payload resolver for resolving staged/processing payloads.
    let payload_resolver: Arc<dyn ClipboardPayloadResolverPort> =
        Arc::new(ClipboardPayloadResolver::new(
            representation_cache.clone(),
            spool_manager.clone(),
            worker_tx.clone(),
        ));

    Ok(BlobProcessingAssembly {
        representation_cache,
        representation_cache_port,
        spool_manager,
        spool_queue,
        payload_resolver,
        worker_tx,
        worker_rx,
        clipboard_change_origin,
    })
}

/// Build the whole-installation config-migration facade (export / import preview
/// / staged import). Assembled in the sync wiring context because its inputs
/// (secure storage, db pool, local identity, filesystem layout, profile) are not
/// reconstructable from the abstract `AppDeps` ports; the composed facade travels
/// on `AppDeps.config_migration`.
///
/// The local-identity port reads the device fingerprint for the export manifest
/// from the same secure-storage view used by the running node. Single-user mode
/// pins the profile to `default`.
fn build_config_migration_facade(
    secure_storage: &Arc<dyn SecureStoragePort>,
    db_pool_for_config_migration: DbPool,
    clock: &Arc<dyn ClockPort>,
    setup_status: &Arc<dyn SetupStatusPort>,
    space_access_ports: &SpaceAccessPorts,
    migration_paths: ConfigMigrationPaths,
) -> Arc<ConfigMigrationFacade> {
    let config_migration_profile = ProfileId::from("default");
    let config_migration_local_identity: Arc<dyn LocalIdentityPort> =
        Arc::new(IrohIdentityStore::new(
            secure_storage.clone(),
            Arc::new(Sha256IdentityFingerprintFactory),
        ));
    let config_migration_adapter = Arc::new(ConfigMigrationAdapter::new(
        secure_storage.clone(),
        db_pool_for_config_migration,
        config_migration_local_identity,
        clock.clone(),
        migration_paths,
        config_migration_profile,
    ));
    Arc::new(ConfigMigrationFacade::new(ConfigMigrationDeps {
        export_bundle: config_migration_adapter.clone(),
        preview_import: config_migration_adapter.clone(),
        stage_import: config_migration_adapter.clone(),
        setup_status: setup_status.clone(),
        is_unlocked: space_access_ports.is_unlocked.clone(),
    }))
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
    // Keep a concrete Arc so it can be coerced into each narrow entry intent
    // port. The entry adapter still implements the aggregate ClipboardEntryStore
    // (the intent-port impls delegate to it), but no consumer needs the wide
    // trait object, so it is not exposed through the ports bundle.
    let entry_repo_arc = Arc::new(entry_repo);
    // Availability (DB reps + filesystem) and transactional entry-replace are
    // separate adapters over the same executor; the inbound upgrade path uses
    // them to turn a partial entry into a complete one in place.
    let entry_availability_repo: Arc<dyn uc_core::ports::clipboard::CheckEntryAvailabilityPort> =
        Arc::new(DieselEntryAvailabilityRepository::new(Arc::clone(
            &db_executor,
        )));
    let entry_replace_repo: Arc<dyn uc_core::ports::clipboard::ReplaceEntryContentPort> = Arc::new(
        DieselClipboardEntryReplaceRepository::new(Arc::clone(&db_executor)),
    );
    let clipboard_entry_ports = ClipboardEntryPorts {
        get: entry_repo_arc.clone(),
        list: entry_repo_arc.clone(),
        save: entry_repo_arc.clone(),
        touch: entry_repo_arc.clone(),
        set_favorite: entry_repo_arc.clone(),
        delete: entry_repo_arc.clone(),
        delete_with_receive_state: entry_repo_arc.clone(),
        find_by_snapshot_hash: entry_repo_arc.clone(),
        get_snapshot_hash: entry_repo_arc,
        availability: entry_availability_repo,
        replace_content: entry_replace_repo,
    };

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
    let representation_repo: Arc<dyn ClipboardRepresentationStore> = Arc::new(rep_repo);

    let entry_delivery_repo: Arc<dyn uc_core::ports::EntryDeliveryRepositoryPort> = Arc::new(
        uc_infra::db::repositories::DieselEntryDeliveryRepository::new(Arc::clone(&db_executor)),
    );

    // NOTE: the entry-file-set repo seals its path columns with a per-session
    // subkey, which needs `current_profile` + space access — both wired after
    // `platform`. It is therefore constructed at the orchestrator level once
    // those exist (mirroring the search index), over `infra.db_executor`.

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

    // Keep a concrete Arc so it can be coerced into each narrow device-repo
    // intent port. The adapter implements the aggregate MobileDeviceStore and
    // each intent-port impl delegates to it (ports.md §8.3); only the narrow
    // ports are exposed upward.
    let mobile_device_repo_arc = Arc::new(DieselMobileDeviceRepository::new(
        Arc::clone(&db_executor),
        MobileDeviceRowMapper,
    ));
    let mobile_device_ports = MobileDevicePorts {
        find_by_username: mobile_device_repo_arc.clone(),
        find_by_id: mobile_device_repo_arc.clone(),
        list: mobile_device_repo_arc.clone(),
        save: mobile_device_repo_arc.clone(),
        delete: mobile_device_repo_arc.clone(),
        update: mobile_device_repo_arc,
    };

    // endpoint_info adapter:进程级单例,daemon LAN listener 与 facade 各持
    // 一份 Arc 共享同一份内存。整个进程只跑一次 `wire_dependencies`,这里
    // new 一份就足够。
    let mobile_sync_endpoint_info =
        Arc::new(uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter::new());

    let infra = InfraLayer {
        clipboard_entry_ports,
        clipboard_event_repo,
        clipboard_event_reader_repo,
        entry_delivery_repo,
        db_executor,
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
        mobile_device_ports,
        mobile_sync_endpoint_info,
    };

    Ok(infra)
}

pub fn wire_dependencies_from_inputs(
    inputs: CoreWiringInputs,
) -> WiringResult<(WiredDependencies, BackgroundRuntimeDeps)> {
    let CoreWiringInputs {
        paths,
        secure_storage,
        legacy_iroh_identity_dir,
        iroh_blob_store_dir,
        system_clipboard,
        analytics_sink,
        analytics_facade,
        host_event_emitter,
    } = inputs;
    let db_path = paths.db_path;
    let vault_path = paths.vault_dir;
    let settings_path = paths.settings_path;
    let app_data_root = paths.app_data_root_dir.clone();

    let db_pool = create_db_pool(&db_path)?;
    // Clone pool before infra layer consumes it — search bundle needs the same pool.
    let db_pool_for_search = db_pool.clone();
    // Config-migration export produces a consistent db snapshot via `VACUUM INTO`
    // off its own pooled connection; clone before infra consumes the pool.
    let db_pool_for_config_migration = db_pool.clone();

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
        system_clipboard,
    )?;

    // Space access — single session/key access entry. See
    // `build_space_access_ports` for the §8.3 single-adapter-reuse rationale.
    let space_access_ports = build_space_access_ports(
        &infra.key_material,
        &platform.current_profile,
        &platform.session,
    );

    // Transfer metadata and event payloads are encrypted with two independent
    // profile-scoped subkeys, so their adapters are assembled only after space
    // access and the active profile are available.
    let file_transfer_adapter = Arc::new(DieselFileTransferRepository::new(
        infra.db_executor.clone(),
        space_access_ports.derive_subkey.clone(),
        platform.current_profile.clone(),
    ));
    let file_transfer_privacy_maintenance = Arc::new(
        uc_infra::file_transfer::SqliteFileTransferPrivacyMaintenance::new(
            infra.db_executor.clone(),
        ),
    );
    let file_transfer = FileTransferPorts {
        privacy_maintenance: file_transfer_privacy_maintenance,
        record: Arc::clone(&file_transfer_adapter) as _,
        seed_provisional: Arc::clone(&file_transfer_adapter) as _,
        update_provisional_path: Arc::clone(&file_transfer_adapter) as _,
        list_provisional: Arc::clone(&file_transfer_adapter) as _,
        finalize_provisional: Arc::clone(&file_transfer_adapter) as _,
        entry_summary: Arc::clone(&file_transfer_adapter) as _,
        find_entry_id: Arc::clone(&file_transfer_adapter) as _,
        find_attempt_id: Arc::clone(&file_transfer_adapter) as _,
        list_expired: Arc::clone(&file_transfer_adapter) as _,
        fail_inflight: Arc::clone(&file_transfer_adapter) as _,
        cancel_attempt: Arc::clone(&file_transfer_adapter) as _,
    };
    let file_transfer_store_arc = Arc::new(
        uc_infra::file_transfer::SqliteReceiverFileTransferStore::new(
            infra.db_executor.clone(),
            space_access_ports.derive_subkey.clone(),
            platform.current_profile.clone(),
        ),
    );

    // File-class entry line-level manifest. Its path columns are sealed with a
    // per-session subkey derived from space access, so it is constructed here
    // (after space access + profile exist) rather than in `create_infra_layer`,
    // reusing the shared executor.
    let entry_file_set_repo: Arc<dyn uc_core::ports::clipboard::EntryFileSetRepositoryPort> =
        Arc::new(
            uc_infra::db::repositories::DieselEntryFileSetRepository::new(
                infra.db_executor.clone(),
                space_access_ports.derive_subkey.clone(),
                platform.current_profile.clone(),
            ),
        );

    let directory_attempt_impl = Arc::new(
        uc_infra::db::repositories::DieselEntryReceiveAttemptRepository::new(
            infra.db_executor.clone(),
        ),
    );
    let directory_publish_impl = Arc::new(
        uc_infra::db::repositories::DieselDirectoryPublishLogRepository::new(
            infra.db_executor.clone(),
            space_access_ports.derive_subkey.clone(),
            platform.current_profile.clone(),
        ),
    );
    let receive_artifact_impl = Arc::new(DieselReceiveArtifactLogRepository::new(
        infra.db_executor.clone(),
        space_access_ports.derive_subkey.clone(),
        platform.current_profile.clone(),
    ));
    let inbound_commit_impl = Arc::new(DieselInboundReceiveCommitRepository::new(
        infra.db_executor.clone(),
        space_access_ports.derive_subkey.clone(),
        platform.current_profile.clone(),
    ));
    let directory_receive = DirectoryReceivePorts {
        get_attempt: directory_attempt_impl.clone(),
        list_attempts: directory_attempt_impl.clone(),
        record_publish: directory_publish_impl.clone(),
        get_publish: directory_publish_impl,
        begin_receive: directory_attempt_impl.clone(),
        claim_commit: directory_attempt_impl.clone(),
        request_cancel: directory_attempt_impl.clone(),
        begin_failure: directory_attempt_impl.clone(),
        record_artifacts: receive_artifact_impl.clone(),
        get_artifacts: receive_artifact_impl.clone(),
        list_unsettled_artifacts: receive_artifact_impl,
        commit_inbound: inbound_commit_impl,
        entry_progress: file_transfer_adapter,
        delete_state: directory_attempt_impl.clone(),
        purge_orphans: directory_attempt_impl,
    };

    // The mobile-consumable reference is encrypted with a session-derived
    // subkey, so this register adapter must be assembled after space access and
    // the active profile exist. One concrete adapter is exposed through narrow
    // write, current-read, mobile-read, backfill, and reset ports.
    let active_clipboard_register_impl = Arc::new(
        uc_infra::db::repositories::DieselActiveClipboardRegisterRepository::new(
            infra.db_executor.clone(),
            space_access_ports.derive_subkey.clone(),
            platform.current_profile.clone(),
        ),
    );
    const ACTIVE_CLIPBOARD_SSE_CAPACITY: usize = 64;
    let (active_clipboard_sse_source, _) = tokio::sync::broadcast::channel::<
        uc_core::clipboard::ActiveClipboardState,
    >(ACTIVE_CLIPBOARD_SSE_CAPACITY);
    let active_clipboard_register: Arc<dyn uc_core::ports::clipboard::AdvanceActiveClipboardPort> =
        Arc::new(uc_infra::clipboard::BroadcastingAdvance::new(
            active_clipboard_register_impl.clone(),
            active_clipboard_sse_source.clone(),
        ));
    let active_clipboard_register_load: Arc<
        dyn uc_core::ports::clipboard::LoadActiveClipboardPort,
    > = active_clipboard_register_impl.clone();
    let mobile_consumable_load: Arc<
        dyn uc_core::ports::clipboard::LoadMobileConsumableClipboardPort,
    > = active_clipboard_register_impl.clone();
    let mobile_consumable_backfill_port: Arc<
        dyn uc_core::ports::clipboard::BackfillMobileConsumableClipboardPort,
    > = active_clipboard_register_impl.clone();
    // Single shared consumability probe: every register-advance path (local
    // advancer, inbound apply, backfill) clones this one instance via
    // `ClipboardPorts.mobile_consumability` instead of re-assembling it from
    // the file-set repository.
    let mobile_consumability =
        uc_application::clipboard_write::MobileConsumabilityProbe::new(entry_file_set_repo.clone());
    let mobile_consumable_backfill: Arc<
        dyn uc_application::clipboard_write::MobileConsumableBackfill,
    > = Arc::new(
        uc_application::clipboard_write::MobileConsumableRefBackfill::new(
            active_clipboard_register_load.clone(),
            mobile_consumable_backfill_port,
            mobile_consumability.clone(),
        ),
    );
    let active_clipboard_register_reset: Arc<
        dyn uc_core::ports::clipboard::ResetActiveClipboardPort,
    > = active_clipboard_register_impl;

    // Wire the search bundle (Phase 92). Search only derives a subkey.
    let SearchAssembly {
        search_index,
        search_maintenance,
        search_key_derivation,
        search_pipeline,
    } = build_search_assembly(
        db_pool_for_search,
        &space_access_ports,
        &platform.current_profile,
    );

    // Encryption decorators over the clipboard event/representation repos, plus
    // the blob/transfer cipher ports (all share the one InMemorySession).
    let CipherDecorators {
        blob_cipher,
        transfer_cipher,
        encrypting_event_writer,
        decrypting_rep_repo,
        representation_ports: clipboard_representation_ports,
    } = build_cipher_decorators(
        &platform.session,
        &infra.clipboard_event_repo,
        &infra.representation_repo,
    );

    // Background blob-processing components (cache, spool, durable queue, payload
    // resolver, self-write ledger, worker channel). `worker_rx` is not Clone and
    // travels by-value to BackgroundRuntimeDeps; the rest fan out to AppDeps.
    let spool_dir = paths.spool_dir.clone();
    let BlobProcessingAssembly {
        representation_cache,
        representation_cache_port,
        spool_manager,
        spool_queue,
        payload_resolver,
        worker_tx,
        worker_rx,
        clipboard_change_origin,
    } = build_blob_processing_assembly(&storage_config, spool_dir.clone())?;

    // The host resolves these directories once. The identity directory
    // is only a migration source for old backups; active identity storage uses
    // the secure-storage wrapper created above.
    // The remaining bypass repos are `Arc::clone`d directly from `infra` at the
    // `WiredDependencies` construction site below (infra retains ownership).
    let iroh_blob_store_dir_for_wiring = iroh_blob_store_dir;

    // `key_migration` adapter consumes secure_storage from PlatformLayer,
    // so it's constructed here at wire_dependencies level rather than in
    // create_infra_layer.
    let key_migration_for_wiring: Arc<dyn uc_core::ports::security::KeyMigrationPort> = Arc::new(
        uc_infra::security::DefaultKeyMigrationAdapter::new(Arc::clone(&platform.secure_storage)),
    );

    let system_clipboard_wiring = platform.system_clipboard_wiring;

    // Whole-installation configuration migration (export / import preview /
    // staged import). Assembled in the sync wiring context because its inputs
    // (secure_storage, db pool, local-identity, filesystem layout, profile) are
    // not reconstructable from the abstract `AppDeps` ports; the composed facade
    // travels on `AppDeps.config_migration`.
    let config_migration = build_config_migration_facade(
        &platform.secure_storage,
        db_pool_for_config_migration,
        &infra.clock,
        &infra.setup_status,
        &space_access_ports,
        ConfigMigrationPaths {
            db_path: db_path.clone(),
            vault_dir: vault_path.clone(),
            settings_path: settings_path.clone(),
            app_data_root: app_data_root.clone(),
            iroh_identity_dir: legacy_iroh_identity_dir,
        },
    );

    let deps = AppDeps {
        clipboard: ClipboardPorts {
            clipboard: platform.clipboard,
            system_clipboard: platform.system_clipboard,
            entry_ports: infra.clipboard_entry_ports,
            // Single shared per-identity write coordinator: inbound apply and
            // local capture serialize "find entry by hash → create/replace/skip"
            // on it so the same content never lands as two entries.
            entry_identity_coordinator: Arc::new(
                uc_application::deps::EntryIdentityCoordinator::new(),
            ),
            clipboard_event_repo: encrypting_event_writer,
            clipboard_event_reader_repo: infra.clipboard_event_reader_repo.clone(),
            representation_store: decrypting_rep_repo,
            representation_ports: clipboard_representation_ports,
            representation_normalizer: platform.representation_normalizer,
            selection_repo: infra.selection_repo,
            representation_policy: Arc::new(SelectRepresentationPolicyV1::new()),
            representation_cache: representation_cache_port,
            spool_queue,
            clipboard_change_origin,
            worker_tx,
            payload_resolver,
            active_register: active_clipboard_register,
            active_register_load: active_clipboard_register_load,
            mobile_consumable_load,
            mobile_consumable_backfill,
            mobile_consumability,
            active_register_reset: active_clipboard_register_reset,
        },
        security: SecurityPorts {
            current_profile: platform.current_profile,
            secure_storage: platform.secure_storage,
            space_access_ports,
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
        config_migration,
        app_version_state: infra.app_version_state,
        first_sync_state: infra.first_sync_state,
        storage: StoragePorts {
            blob_store: platform.blob_store,
            blob_writer: platform.blob_writer,
            blob_content_ingest: platform.blob_content_ingest,
            entry_file_set_repo,
            thumbnail_repo: infra.thumbnail_repo,
            thumbnail_generator: infra.thumbnail_generator,
            file_transfer,
            directory_receive,
        },
        settings: infra.settings_repo,
        system: SystemPorts {
            clock: infra.clock,
            hash: infra.hash,
            cache_fs: Arc::new(uc_infra::fs::TokioCacheFsAdapter::new()),
        },
        search: SearchPorts {
            search_index,
            search_maintenance,
            search_key_derivation,
            search_pipeline,
        },
        mobile_sync: MobileSyncPorts {
            devices: infra.mobile_device_ports,
            endpoint_info: infra.mobile_sync_endpoint_info.clone(),
        },
        analytics: analytics_sink,
    };

    // Create shared host-event bus at wire time. The bus starts with the
    // logging emitter pre-registered so non-GUI / CLI processes have a
    // sensible default (event type names go to tracing::debug). Tauri setup
    // and daemon startup `register` their own transports on top — register
    // is additive, never overwrites the logging emitter, and `unregister`
    // can pull a transport off cleanly (e.g. daemon reload).
    let host_event_bus: Arc<uc_application::facade::HostEventBus> =
        Arc::new(uc_application::facade::HostEventBus::new());
    host_event_bus.register("logging", host_event_emitter);
    let receive_readiness =
        Arc::new(uc_application::receive_reconciliation::ReceiveReadinessCoordinator::new());

    let crate::internal::file_transfer::FileTransferAssembly {
        lifecycle: file_transfer_lifecycle,
        facade: file_transfer_facade,
    } = crate::internal::file_transfer::build_file_transfer_assembly(
        Arc::clone(&file_transfer_store_arc),
        Arc::clone(&host_event_bus),
        deps.storage.file_transfer.clone(),
        deps.storage.directory_receive.clone(),
        deps.system.clock.clone(),
        Arc::clone(&receive_readiness),
        uc_infra::fs::FsInboundFileTarget::new(deps.settings.clone()),
        paths.file_cache_dir.clone(),
    );

    let clipboard_write_coordinator = build_clipboard_write_coordinator(
        deps.clipboard.system_clipboard.clone(),
        deps.clipboard.clipboard_change_origin.clone(),
    );

    let wired = WiredDependencies {
        deps,
        system_clipboard_wiring,
        sync_engine: SyncEngineDeps {
            peer_addr_repo: Arc::clone(&infra.peer_addr_repo),
            blob_reference_repo: Arc::clone(&infra.blob_reference_repo),
            blob_migration_repo: Arc::clone(&infra.blob_migration_repo),
            migration_state: Arc::clone(&infra.migration_state),
            key_migration: key_migration_for_wiring,
            iroh_blob_store_dir: iroh_blob_store_dir_for_wiring,
            analytics_facade,
        },
        daemon_runtime: DaemonRuntimeDeps {
            mobile_sync_endpoint_info: Arc::clone(&infra.mobile_sync_endpoint_info),
        },
        shared: SharedRuntimeDeps {
            receive_readiness,
            host_event_bus,
            entry_delivery_repo: Arc::clone(&infra.entry_delivery_repo),
            clipboard_event_reader_repo: Arc::clone(&infra.clipboard_event_reader_repo),
            file_transfer_facade,
            clipboard_write_coordinator: Arc::clone(&clipboard_write_coordinator),
            file_cache_dir: paths.file_cache_dir.clone(),
            trusted_peer_repo: Arc::clone(&infra.trusted_peer_repo),
            active_clipboard_sse_source,
        },
    };
    let background = BackgroundRuntimeDeps {
        representation_cache,
        spool_manager,
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

/// Constructs a `ClipboardWriteCoordinator` — the single write boundary for all
/// programmatic clipboard writes.
///
/// Centralises the guard-registration + write + cleanup-on-error pattern
/// (previously duplicated across restore_clipboard_selection, copy_file_to_clipboard,
/// and the now-deleted `sync_inbound` libp2p path).
fn build_clipboard_write_coordinator(
    system_clipboard: Arc<dyn uc_core::ports::clipboard::SystemClipboardPort>,
    clipboard_change_origin: Arc<dyn SelfWriteLedgerPort>,
) -> Arc<uc_application::clipboard_write::ClipboardWriteCoordinator> {
    Arc::new(
        uc_application::clipboard_write::ClipboardWriteCoordinator::new(
            system_clipboard,
            clipboard_change_origin,
        ),
    )
}

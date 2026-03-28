//! # Pure Dependency Assembly
//!
//! This module contains all pure dependency construction functions that have
//! zero Tauri imports. It is the heart of the `uc-bootstrap` composition root.
//!
//! ## What lives here
//!
//! - `WiredDependencies` struct (output of the wiring process)
//! - `BackgroundRuntimeDeps` struct (background worker dependencies)
//! - `HostEventSetupPort` adapter (pure, no Tauri types)
//! - All infrastructure and platform layer construction functions
//! - `wire_dependencies`, `get_storage_paths`, `resolve_pairing_device_name`, etc.
//!
//! ## Architecture Principle
//!
//! > **Zero tauri imports in this file.**

use std::path::PathBuf;
use std::sync::Arc;

use tracing::{info, warn};

use uc_app::deps::NetworkPorts;
use uc_app::usecases::{PairingConfig, ResolveConnectionPolicy};
use uc_app::{AppDeps, ClipboardPorts, DevicePorts, SecurityPorts, StoragePorts, SystemPorts};
use uc_core::clipboard::SelectRepresentationPolicyV1;
use uc_core::config::AppConfig;
use uc_core::ids::RepresentationId;
use uc_core::ports::clipboard::{
    ClipboardChangeOriginPort, ClipboardRepresentationNormalizerPort, RepresentationCachePort,
    SpoolQueuePort, SpoolRequest,
};
use uc_core::ports::host_event_emitter::{HostEvent, HostEventEmitterPort, SetupHostEvent};
use uc_core::ports::SetupEventPort;
use uc_core::ports::*;
use uc_core::settings::model::Settings;
use uc_infra::blob::BlobWriter;
use uc_infra::clipboard::{
    ClipboardPayloadResolver, ClipboardRepresentationNormalizer, DurableSpoolQueue,
    InMemoryClipboardChangeOrigin, InfraThumbnailGenerator, RepresentationCache, SpoolManager,
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
    DieselDeviceRepository, DieselFileTransferRepository, DieselPairedDeviceRepository,
    DieselThumbnailRepository,
};
use uc_infra::device::LocalDeviceIdentity;
use uc_infra::fs::key_slot_store::JsonKeySlotStore;
use uc_infra::security::{
    Blake3Hasher, DecryptingClipboardRepresentationRepository, DefaultKeyMaterialService,
    EncryptedBlobStore, EncryptingClipboardEventWriter, EncryptionRepository,
    FileEncryptionStateRepository,
};
use uc_infra::settings::repository::FileSettingsRepository;
use uc_infra::{FileSetupStatusRepository, SystemClock};
use uc_platform::adapters::{
    DisabledPairingTransport, FilesystemBlobStore, InMemoryEncryptionSessionPort,
    Libp2pNetworkAdapter, PairingRuntimeOwner,
};
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::clipboard::LocalClipboard;
use uc_platform::identity_store::FileIdentityStore;
use uc_platform::ports::{AppDirsPort, IdentityStorePort};

use tokio::sync::mpsc;

/// A minimal logging event emitter used as the initial placeholder inside the shared
/// emitter cell created at wire time.
///
/// Logs event type names only (no payload content, which may be sensitive).
/// After Tauri setup or non-GUI runtime construction, callers replace this with
/// a `TauriEventEmitter` or keep it as-is for non-GUI paths.
struct WireTimeLoggingEmitter;

impl HostEventEmitterPort for WireTimeLoggingEmitter {
    fn emit(
        &self,
        event: uc_core::ports::host_event_emitter::HostEvent,
    ) -> Result<(), uc_core::ports::host_event_emitter::EmitError> {
        use uc_core::ports::host_event_emitter::HostEvent;
        let event_type = match &event {
            HostEvent::Clipboard(_) => "clipboard",
            HostEvent::PeerDiscovery(_) => "peer_discovery",
            HostEvent::PeerConnection(_) => "peer_connection",
            HostEvent::Transfer(_) => "transfer",
            HostEvent::Pairing(_) => "pairing",
            HostEvent::Realtime(_) => "realtime",
            HostEvent::Setup(_) => "setup",
            HostEvent::SpaceAccess(_) => "space_access",
        };
        tracing::debug!(event_type, "wire-time host event (pre-bootstrap)");
        Ok(())
    }
}

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
    pub libp2p_network: Arc<Libp2pNetworkAdapter>,
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
    /// File transfer lifecycle orchestrator. Holds a clone of the shared emitter_cell so
    /// it automatically sees emitter swaps (LoggingEventEmitter → TauriEventEmitter).
    pub file_transfer_orchestrator: Arc<uc_app::usecases::file_sync::FileTransferOrchestrator>,
}

/// Fully wired dependencies plus background runtime components.
pub struct WiredDependencies {
    pub deps: AppDeps,
    pub background: BackgroundRuntimeDeps,
    /// Shared emitter cell created at wire time with the initial `LoggingHostEventEmitter`.
    /// Callers (GUI bootstrap, non-GUI bootstrap) use this same cell so that
    /// all consumers — CoreRuntime, SetupOrchestrator, and FileTransferOrchestrator —
    /// see the same emitter after any swap.
    pub emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
}

/// HostEventEmitterPort adapter that emits setup state changes to frontend listeners.
///
/// Uses Arc<RwLock<...>> shared cell so that HostEventSetupPort always reads the
/// current emitter after bootstrap swaps it from LoggingEventEmitter to TauriEventEmitter.
/// This eliminates the stale emitter bug described in STATE.md Known Bugs.
#[derive(Clone)]
pub struct HostEventSetupPort {
    emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
}

impl HostEventSetupPort {
    pub fn new(emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>) -> Self {
        Self { emitter_cell }
    }
}

#[async_trait::async_trait]
impl SetupEventPort for HostEventSetupPort {
    async fn emit_setup_state_changed(
        &self,
        state: uc_core::setup::SetupState,
        session_id: Option<String>,
    ) {
        let emitter = self
            .emitter_cell
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Err(err) = emitter.emit(HostEvent::Setup(SetupHostEvent::StateChanged {
            state,
            session_id,
        })) {
            warn!(error = %err, "Failed to emit setup-state-changed");
        }
    }
}

/// Infrastructure layer implementations
struct InfraLayer {
    // Clipboard repositories
    #[allow(dead_code)]
    clipboard_entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    clipboard_event_repo: Arc<dyn ClipboardEventWriterPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,

    // Device repository
    device_repo: Arc<dyn DeviceRepositoryPort>,

    // Pairing repository
    paired_device_repo: Arc<dyn PairedDeviceRepositoryPort>,

    // Blob storage
    blob_repository: Arc<dyn BlobRepositoryPort>,
    thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    thumbnail_generator: Arc<dyn ThumbnailGeneratorPort>,

    // Security services
    key_material: Arc<dyn KeyMaterialPort>,
    encryption: Arc<dyn EncryptionPort>,
    encryption_state: Arc<dyn uc_core::ports::security::encryption_state::EncryptionStatePort>,

    // Settings
    settings_repo: Arc<dyn SettingsPort>,

    // Setup status
    setup_status: Arc<dyn SetupStatusPort>,

    // System services
    clock: Arc<dyn ClockPort>,
    hash: Arc<dyn ContentHashPort>,

    // File transfer tracking
    file_transfer_repo: Arc<dyn uc_core::ports::FileTransferRepositoryPort>,
}

/// Platform layer implementations
pub struct PlatformLayer {
    // System clipboard
    pub clipboard: Arc<dyn PlatformClipboardPort>,
    pub system_clipboard: Arc<dyn SystemClipboardPort>,

    // Secure storage
    pub secure_storage: Arc<dyn SecureStoragePort>,

    // Network operations
    pub network_ports: Arc<NetworkPorts>,

    // libp2p network adapter (concrete)
    pub libp2p_network: Arc<Libp2pNetworkAdapter>,

    // Device identity
    pub device_identity: Arc<dyn DeviceIdentityPort>,

    // Clipboard representation normalizer
    pub representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,

    // Blob writer
    pub blob_writer: Arc<dyn BlobWriterPort>,

    // Blob store (encrypted)
    pub blob_store: Arc<dyn BlobStorePort>,

    // Encryption session
    pub encryption_session: Arc<dyn EncryptionSessionPort>,

    // Key scope
    pub key_scope: Arc<dyn uc_core::ports::security::key_scope::KeyScopePort>,
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
    secure_storage: Arc<dyn SecureStoragePort>,
) -> WiringResult<InfraLayer> {
    let db_executor = Arc::new(DieselSqliteExecutor::new(db_pool));

    let entry_row_mapper = ClipboardEntryRowMapper;
    let selection_row_mapper = ClipboardSelectionRowMapper;
    let device_row_mapper = DeviceRowMapper;
    let paired_device_row_mapper = PairedDeviceRowMapper;
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
    let clipboard_event_repo_impl = DieselClipboardEventRepository::new(
        Arc::clone(&db_executor),
        event_row_mapper,
        RepresentationRowMapper,
    );
    let clipboard_event_repo: Arc<dyn ClipboardEventWriterPort> =
        Arc::new(clipboard_event_repo_impl);

    let rep_repo = DieselClipboardRepresentationRepository::new(Arc::clone(&db_executor));
    let representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort> = Arc::new(rep_repo);

    let dev_repo = DieselDeviceRepository::new(Arc::clone(&db_executor), device_row_mapper);
    let device_repo: Arc<dyn DeviceRepositoryPort> = Arc::new(dev_repo);

    let paired_repo =
        DieselPairedDeviceRepository::new(Arc::clone(&db_executor), paired_device_row_mapper);
    let paired_device_repo: Arc<dyn PairedDeviceRepositoryPort> = Arc::new(paired_repo);

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

    let key_material_service =
        DefaultKeyMaterialService::new(secure_storage_for_key_material, keyslot_store);
    let key_material: Arc<dyn KeyMaterialPort> = Arc::new(key_material_service);

    let encryption: Arc<dyn EncryptionPort> = Arc::new(EncryptionRepository);

    let encryption_state: Arc<dyn uc_core::ports::security::encryption_state::EncryptionStatePort> =
        Arc::new(FileEncryptionStateRepository::new(vault_path.clone()));

    let settings_repo: Arc<dyn SettingsPort> = Arc::new(FileSettingsRepository::new(settings_path));

    let setup_status: Arc<dyn SetupStatusPort> =
        Arc::new(FileSetupStatusRepository::with_defaults(vault_path.clone()));

    let clock: Arc<dyn ClockPort> = Arc::new(SystemClock);
    let hash: Arc<dyn ContentHashPort> = Arc::new(Blake3Hasher);

    let selection_repo_impl = DieselClipboardSelectionRepository::new(Arc::clone(&db_executor));
    let selection_repo: Arc<dyn ClipboardSelectionRepositoryPort> = Arc::new(selection_repo_impl);

    let file_transfer_repo: Arc<dyn uc_core::ports::FileTransferRepositoryPort> =
        Arc::new(DieselFileTransferRepository::new(Arc::clone(&db_executor)));

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
        file_transfer_repo,
    };

    Ok(infra)
}

/// Create platform layer implementations
fn build_network_ports(
    libp2p_network: Arc<Libp2pNetworkAdapter>,
    pairing_runtime_owner: PairingRuntimeOwner,
) -> Arc<NetworkPorts> {
    let pairing: Arc<dyn PairingTransportPort> = match pairing_runtime_owner {
        PairingRuntimeOwner::CurrentProcess => libp2p_network.clone(),
        PairingRuntimeOwner::ExternalDaemon => Arc::new(DisabledPairingTransport),
    };

    Arc::new(NetworkPorts {
        clipboard: libp2p_network.clone(),
        peers: libp2p_network.clone(),
        pairing,
        events: libp2p_network.clone(),
        file_transfer: libp2p_network.clone(),
    })
}

pub fn create_platform_layer(
    secure_storage: Arc<dyn SecureStoragePort>,
    config_dir: &PathBuf,
    encryption: Arc<dyn EncryptionPort>,
    blob_repository: Arc<dyn BlobRepositoryPort>,
    paired_device_repo: Arc<dyn PairedDeviceRepositoryPort>,
    clock: Arc<dyn ClockPort>,
    storage_config: Arc<ClipboardStorageConfig>,
    identity_store: Arc<dyn IdentityStorePort>,
    file_cache_dir: PathBuf,
    pairing_runtime_owner: PairingRuntimeOwner,
) -> WiringResult<PlatformLayer> {
    let clipboard_impl = LocalClipboard::new()
        .map_err(|e| WiringError::ClipboardInit(format!("Failed to create clipboard: {}", e)))?;
    let clipboard_impl = Arc::new(clipboard_impl);
    let clipboard: Arc<dyn PlatformClipboardPort> = clipboard_impl.clone();
    let system_clipboard: Arc<dyn SystemClipboardPort> = clipboard_impl;

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

    let encryption_session: Arc<dyn EncryptionSessionPort> =
        Arc::new(InMemoryEncryptionSessionPort::new());
    let policy_resolver = Arc::new(ResolveConnectionPolicy::new(paired_device_repo.clone()));
    let transfer_decryptor: Arc<dyn TransferPayloadDecryptorPort> =
        Arc::new(uc_infra::clipboard::TransferPayloadDecryptorAdapter);
    let transfer_encryptor: Arc<dyn TransferPayloadEncryptorPort> =
        Arc::new(uc_infra::clipboard::TransferPayloadEncryptorAdapter);
    let libp2p_network = Arc::new(
        Libp2pNetworkAdapter::new(
            identity_store,
            policy_resolver,
            encryption_session.clone(),
            transfer_decryptor,
            transfer_encryptor,
            file_cache_dir,
            pairing_runtime_owner,
        )
        .map_err(|e| {
            WiringError::NetworkInit(format!("Failed to initialize libp2p identity: {e}"))
        })?,
    );
    info!(peer_id = %libp2p_network.local_peer_id(), "Loaded libp2p identity");
    let network_ports = build_network_ports(libp2p_network.clone(), pairing_runtime_owner);

    let encrypted_blob_store: Arc<dyn BlobStorePort> = Arc::new(EncryptedBlobStore::new(
        blob_store.clone(),
        encryption,
        encryption_session.clone(),
    ));

    let blob_writer: Arc<dyn BlobWriterPort> = Arc::new(BlobWriter::new(
        encrypted_blob_store.clone(),
        blob_repository,
        clock,
    ));

    let key_scope: Arc<dyn uc_core::ports::security::key_scope::KeyScopePort> =
        Arc::new(uc_platform::key_scope::DefaultKeyScope::new());

    Ok(PlatformLayer {
        clipboard,
        system_clipboard,
        secure_storage,
        network_ports,
        libp2p_network,
        device_identity,
        representation_normalizer,
        blob_writer,
        blob_store: encrypted_blob_store,
        encryption_session,
        key_scope,
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
) -> WiringResult<uc_app::app_paths::AppPaths> {
    let platform_dirs = get_default_app_dirs()?;
    resolve_app_paths(&platform_dirs, config)
}

/// Resolve the effective `AppDirs` by applying config overrides.
pub fn resolve_app_dirs(
    platform_dirs: &uc_core::app_dirs::AppDirs,
    config: &AppConfig,
) -> uc_core::app_dirs::AppDirs {
    let is_in_memory_db = config.database_path.as_os_str() == ":memory:";
    let config_overrides_root = !config.database_path.as_os_str().is_empty() && !is_in_memory_db;

    if config_overrides_root {
        let raw_root = config
            .database_path
            .parent()
            .unwrap_or(&config.database_path)
            .to_path_buf();
        let abs_root = if raw_root.is_relative() {
            std::env::current_dir().unwrap_or_default().join(&raw_root)
        } else {
            raw_root
        };
        let app_data_root = apply_profile_suffix(abs_root);
        let app_cache_root = app_data_root.join("cache");
        uc_core::app_dirs::AppDirs {
            app_data_root,
            app_cache_root,
        }
    } else {
        platform_dirs.clone()
    }
}

/// Build `AppPaths` from platform dirs and config overrides.
pub fn resolve_app_paths(
    platform_dirs: &uc_core::app_dirs::AppDirs,
    config: &AppConfig,
) -> WiringResult<uc_app::app_paths::AppPaths> {
    let resolved_dirs = resolve_app_dirs(platform_dirs, config);
    let mut paths = uc_app::app_paths::AppPaths::from_app_dirs(&resolved_dirs);

    let is_in_memory_db = config.database_path.as_os_str() == ":memory:";

    if is_in_memory_db {
        paths.db_path = config.database_path.clone();
    } else if !config.database_path.as_os_str().is_empty() {
        let db_file_name = config
            .database_path
            .file_name()
            .map(|name| name.to_os_string())
            .unwrap_or_else(|| std::ffi::OsString::from("uniclipboard.db"));
        paths.db_path = paths.app_data_root.join(db_file_name);
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
                paths.vault_dir = paths.app_data_root.join(relative);
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

/// Wires and constructs the application's dependency graph, returning ready-to-use dependencies.
pub fn wire_dependencies(
    config: &AppConfig,
    pairing_runtime_owner: PairingRuntimeOwner,
) -> WiringResult<WiredDependencies> {
    wire_dependencies_with_identity_store(config, None, pairing_runtime_owner)
}

/// Wires dependencies with a caller-provided identity store.
///
/// This is primarily intended for tests or environments without system secure storage.
pub fn wire_dependencies_with_identity_store(
    config: &AppConfig,
    identity_store: Option<Arc<dyn IdentityStorePort>>,
    pairing_runtime_owner: PairingRuntimeOwner,
) -> WiringResult<WiredDependencies> {
    let platform_dirs = get_default_app_dirs()?;
    let paths = resolve_app_paths(&platform_dirs, config)?;

    let db_path = paths.db_path;
    let db_pool = create_db_pool(&db_path)?;

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

    let storage_config = Arc::new(ClipboardStorageConfig::defaults());
    let platform = create_platform_layer(
        secure_storage,
        &vault_path,
        infra.encryption.clone(),
        infra.blob_repository.clone(),
        infra.paired_device_repo.clone(),
        infra.clock.clone(),
        storage_config.clone(),
        identity_store,
        paths.file_cache_dir.clone(),
        pairing_runtime_owner,
    )?;

    // Wrap ports with encryption decorators
    let encrypting_event_writer: Arc<dyn ClipboardEventWriterPort> =
        Arc::new(EncryptingClipboardEventWriter::new(
            infra.clipboard_event_repo.clone(),
            infra.encryption.clone(),
            platform.encryption_session.clone(),
        ));

    let decrypting_rep_repo: Arc<dyn ClipboardRepresentationRepositoryPort> =
        Arc::new(DecryptingClipboardRepresentationRepository::new(
            infra.representation_repo.clone(),
            infra.encryption.clone(),
            platform.encryption_session.clone(),
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

    let clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort> =
        Arc::new(InMemoryClipboardChangeOrigin::new());

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
            encryption: infra.encryption,
            encryption_session: platform.encryption_session,
            encryption_state: infra.encryption_state,
            key_scope: platform.key_scope,
            secure_storage: platform.secure_storage,
            key_material: infra.key_material,
        },
        device: DevicePorts {
            device_repo: infra.device_repo,
            device_identity: platform.device_identity,
            paired_device_repo: infra.paired_device_repo,
        },
        network_ports: platform.network_ports,
        network_control: platform.libp2p_network.clone(),
        setup_status: infra.setup_status,
        storage: StoragePorts {
            blob_store: platform.blob_store,
            blob_repository: infra.blob_repository,
            blob_writer: platform.blob_writer,
            thumbnail_repo: infra.thumbnail_repo,
            thumbnail_generator: infra.thumbnail_generator,
            file_transfer_repo: infra.file_transfer_repo,
        },
        settings: infra.settings_repo,
        system: SystemPorts {
            clock: infra.clock,
            hash: infra.hash,
            file_manager: Arc::new(uc_platform::file_manager::NativeFileManagerAdapter::new()),
            cache_fs: Arc::new(uc_infra::fs::TokioCacheFsAdapter::new()),
        },
    };

    // Create shared emitter cell at wire time using the logging placeholder.
    // All consumers (CoreRuntime, SetupOrchestrator, FileTransferOrchestrator)
    // hold a clone of this cell and automatically see the emitter after any swap.
    let initial_emitter: Arc<dyn HostEventEmitterPort> = Arc::new(WireTimeLoggingEmitter);
    let emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>> =
        Arc::new(std::sync::RwLock::new(initial_emitter));

    let file_transfer_orchestrator = build_file_transfer_orchestrator(
        deps.storage.file_transfer_repo.clone(),
        emitter_cell.clone(),
        deps.system.clock.clone(),
    );

    Ok(WiredDependencies {
        deps,
        background: BackgroundRuntimeDeps {
            libp2p_network: platform.libp2p_network.clone(),
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
            file_transfer_orchestrator,
        },
        emitter_cell,
    })
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

// ---------------------------------------------------------------------------
// SetupAssemblyPorts — network/external adapter ports for SetupOrchestrator
// ---------------------------------------------------------------------------

use tokio::sync::Mutex as TokioMutex;
use uc_app::usecases::space_access::SpaceAccessOrchestrator;
use uc_app::usecases::{
    DeviceAnnouncer, LifecycleEventEmitter, LifecycleStatusPort, PairingOrchestrator,
    SessionReadyEmitter, SetupOrchestrator, SetupPairingFacadePort,
};
use uc_core::ports::space::SpaceAccessTransportPort;
use uc_core::ports::{DiscoveryPort, TimerPort};

/// Bundle of network/external adapter ports needed to assemble the SetupOrchestrator.
///
/// Replaces SetupRuntimePorts from runtime.rs. Contains ONLY network/external
/// adapter ports that the caller (main.rs/wiring.rs) provides and that are NOT
/// shared with AppRuntime or CoreRuntime. All shared/dual-use values
/// (emitter_cell, lifecycle_status, session_ready_emitter) are separate
/// parameters to build_setup_orchestrator(), ensuring with_setup() can pass
/// the SAME instance to both the orchestrator and AppRuntime/CoreRuntime.
pub struct SetupAssemblyPorts {
    pub setup_pairing_facade: Arc<dyn SetupPairingFacadePort>,
    pub space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    pub discovery_port: Arc<dyn DiscoveryPort>,
    pub device_announcer: Option<Arc<dyn DeviceAnnouncer>>,
    pub lifecycle_emitter: Arc<dyn LifecycleEventEmitter>,
}

impl SetupAssemblyPorts {
    /// Create a bundle using the peer-directory port as the discovery adapter.
    pub fn from_network(
        pairing_orchestrator: Arc<PairingOrchestrator>,
        space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
        peers: Arc<dyn uc_core::ports::PeerDirectoryPort>,
        device_announcer: Option<Arc<dyn DeviceAnnouncer>>,
        lifecycle_emitter: Arc<dyn LifecycleEventEmitter>,
    ) -> Self {
        struct NetworkDiscoveryPort {
            peers: Arc<dyn uc_core::ports::PeerDirectoryPort>,
        }
        #[async_trait::async_trait]
        impl DiscoveryPort for NetworkDiscoveryPort {
            async fn list_discovered_peers(
                &self,
            ) -> anyhow::Result<Vec<uc_core::network::DiscoveredPeer>> {
                self.peers.get_discovered_peers().await
            }
        }
        Self {
            setup_pairing_facade: pairing_orchestrator,
            space_access_orchestrator,
            discovery_port: Arc::new(NetworkDiscoveryPort { peers }),
            device_announcer,
            lifecycle_emitter,
        }
    }

    /// Create placeholder ports for tests. All ports are noop implementations.
    /// Used by AppRuntime::new() for command tests that don't exercise setup flow.
    ///
    /// NOTE: Shared state (emitter_cell, lifecycle_status, clipboard_integration_mode)
    /// and with_setup()-constructed adapters (session_ready_emitter) are NOT created
    /// here — they are created by AppRuntime::new() / with_setup() and passed
    /// separately to build_setup_orchestrator().
    pub fn placeholder(_deps: &uc_app::AppDeps) -> Self {
        struct EmptyDiscoveryPort;
        #[async_trait::async_trait]
        impl DiscoveryPort for EmptyDiscoveryPort {
            async fn list_discovered_peers(
                &self,
            ) -> anyhow::Result<Vec<uc_core::network::DiscoveredPeer>> {
                Ok(Vec::new())
            }
        }

        struct NoopSetupPairingFacade;

        #[async_trait::async_trait]
        impl SetupPairingFacadePort for NoopSetupPairingFacade {
            async fn subscribe(
                &self,
            ) -> anyhow::Result<
                tokio::sync::mpsc::Receiver<uc_app::usecases::pairing::PairingDomainEvent>,
            > {
                let (_tx, rx) = tokio::sync::mpsc::channel(1);
                Ok(rx)
            }

            async fn initiate_pairing(&self, _peer_id: String) -> anyhow::Result<String> {
                Err(anyhow::anyhow!(
                    "setup pairing facade placeholder cannot initiate pairing"
                ))
            }

            async fn accept_pairing(&self, _session_id: &str) -> anyhow::Result<()> {
                Ok(())
            }

            async fn reject_pairing(&self, _session_id: &str) -> anyhow::Result<()> {
                Ok(())
            }

            async fn cancel_pairing(&self, _session_id: &str) -> anyhow::Result<()> {
                Ok(())
            }

            async fn verify_pairing(
                &self,
                _session_id: &str,
                _pin_matches: bool,
            ) -> anyhow::Result<()> {
                Ok(())
            }
        }

        Self {
            setup_pairing_facade: Arc::new(NoopSetupPairingFacade),
            space_access_orchestrator: Arc::new(SpaceAccessOrchestrator::new()),
            discovery_port: Arc::new(EmptyDiscoveryPort),
            device_announcer: None,
            lifecycle_emitter: Arc::new(uc_app::usecases::LoggingLifecycleEventEmitter),
        }
    }
}

/// Constructs a `FileTransferOrchestrator` for file transfer lifecycle management.
///
/// Uses the shared `emitter_cell` so the orchestrator automatically sees emitter swaps
/// (e.g., `LoggingHostEventEmitter` → `TauriEventEmitter` after Tauri setup).
pub fn build_file_transfer_orchestrator(
    file_transfer_repo: Arc<dyn uc_core::ports::FileTransferRepositoryPort>,
    emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
    clock: Arc<dyn ClockPort>,
) -> Arc<uc_app::usecases::file_sync::FileTransferOrchestrator> {
    let tracker = Arc::new(
        uc_app::usecases::file_sync::TrackInboundTransfersUseCase::new(file_transfer_repo),
    );
    Arc::new(uc_app::usecases::file_sync::FileTransferOrchestrator::new(
        tracker,
        emitter_cell,
        clock,
    ))
}

/// Build the SetupOrchestrator with all required adapters.
///
/// This is the single composition point for SetupOrchestrator (RNTM-05).
pub fn build_setup_orchestrator(
    deps: &uc_app::AppDeps,
    ports: SetupAssemblyPorts,
    lifecycle_status: Arc<dyn LifecycleStatusPort>,
    emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
    session_ready_emitter: Arc<dyn SessionReadyEmitter>,
) -> Arc<SetupOrchestrator> {
    use uc_app::usecases::{
        AppLifecycleCoordinator, AppLifecycleCoordinatorDeps, InitializeEncryption,
        MarkSetupComplete, StagedPairedDeviceStore, StartNetworkAfterUnlock,
    };

    let initialize_encryption = Arc::new(InitializeEncryption::from_ports(
        deps.security.encryption.clone(),
        deps.security.key_material.clone(),
        deps.security.key_scope.clone(),
        deps.security.encryption_state.clone(),
        deps.security.encryption_session.clone(),
    ));
    let mark_setup_complete = Arc::new(MarkSetupComplete::from_ports(deps.setup_status.clone()));

    let start_network = Arc::new(StartNetworkAfterUnlock::from_port(
        deps.network_control.clone(),
    ));
    let app_lifecycle = Arc::new(AppLifecycleCoordinator::from_deps(
        AppLifecycleCoordinatorDeps {
            network: start_network,
            announcer: ports.device_announcer,
            emitter: session_ready_emitter,
            status: lifecycle_status,
            lifecycle_emitter: ports.lifecycle_emitter,
        },
    ));
    let crypto_factory = Arc::new(
        uc_app::usecases::space_access::DefaultSpaceAccessCryptoFactory::new(
            deps.security.encryption.clone(),
            deps.security.key_material.clone(),
            deps.security.key_scope.clone(),
            deps.security.encryption_state.clone(),
            deps.security.encryption_session.clone(),
        ),
    );
    let transport_port: Arc<TokioMutex<dyn SpaceAccessTransportPort>> = Arc::new(TokioMutex::new(
        uc_app::usecases::space_access::SpaceAccessNetworkAdapter::new(
            deps.network_ports.pairing.clone(),
            ports.space_access_orchestrator.context(),
        ),
    ));
    let proof_port: Arc<dyn uc_core::ports::space::ProofPort> = Arc::new(
        uc_app::usecases::space_access::HmacProofAdapter::new_with_encryption_session(
            deps.security.encryption_session.clone(),
        ),
    );
    let timer_port: Arc<TokioMutex<dyn TimerPort>> =
        Arc::new(TokioMutex::new(uc_infra::time::Timer::new()));
    let persistence_port = Arc::new(TokioMutex::new(
        uc_app::usecases::space_access::SpaceAccessPersistenceAdapter::new(
            deps.security.encryption_state.clone(),
            deps.device.paired_device_repo.clone(),
            Arc::new(StagedPairedDeviceStore::new()),
        ),
    ));
    let setup_event_port = Arc::new(HostEventSetupPort::new(emitter_cell));

    Arc::new(SetupOrchestrator::new(
        initialize_encryption,
        mark_setup_complete,
        deps.setup_status.clone(),
        app_lifecycle,
        ports.setup_pairing_facade,
        setup_event_port,
        ports.space_access_orchestrator,
        ports.discovery_port,
        deps.network_control.clone(),
        crypto_factory,
        deps.network_ports.pairing.clone(),
        transport_port,
        proof_port,
        timer_port,
        persistence_port,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;
    use tokio::sync::mpsc;
    use uc_app::usecases::setup::SetupPairingFacadePort;
    use uc_core::network::{ConnectionPolicy, PairingState, ResolvedConnectionPolicy};
    use uc_core::ports::host_event_emitter::{EmitError, HostEvent};
    use uc_core::ports::{
        ConnectionPolicyResolverError, ConnectionPolicyResolverPort, SetupEventPort,
    };
    use uc_platform::adapters::{InMemoryEncryptionSessionPort, PairingRuntimeOwner};
    use uc_platform::ports::IdentityStoreError;

    struct RecordingEmitter {
        events: Arc<StdMutex<Vec<String>>>,
    }

    impl HostEventEmitterPort for RecordingEmitter {
        fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
            self.events.lock().unwrap().push(format!("{:?}", event));
            Ok(())
        }
    }

    /// Local noop emitter for tests (replaces LoggingEventEmitter from uc-tauri)
    struct NoopEventEmitter;

    impl HostEventEmitterPort for NoopEventEmitter {
        fn emit(&self, _event: HostEvent) -> Result<(), EmitError> {
            Ok(())
        }
    }

    struct RecordingSetupPairingFacade;

    #[async_trait::async_trait]
    impl SetupPairingFacadePort for RecordingSetupPairingFacade {
        async fn subscribe(
            &self,
        ) -> anyhow::Result<mpsc::Receiver<uc_app::usecases::pairing::PairingDomainEvent>> {
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }

        async fn initiate_pairing(&self, _peer_id: String) -> anyhow::Result<String> {
            Ok("session-1".to_string())
        }

        async fn accept_pairing(&self, _session_id: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn reject_pairing(&self, _session_id: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn cancel_pairing(&self, _session_id: &str) -> anyhow::Result<()> {
            Ok(())
        }

        async fn verify_pairing(
            &self,
            _session_id: &str,
            _pin_matches: bool,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// SC#4: HostEventSetupPort read-through test.
    ///
    /// Validates that HostEventSetupPort, when given a shared cell, sees a swapped emitter.
    #[tokio::test]
    async fn setup_state_emission_survives_emitter_swap() {
        // 1. Create cell with initial noop emitter
        let initial: Arc<dyn HostEventEmitterPort> = Arc::new(NoopEventEmitter);
        let cell = Arc::new(std::sync::RwLock::new(initial));

        // 2. Create HostEventSetupPort with the cell
        let setup_port = HostEventSetupPort::new(cell.clone());

        // 3. Swap emitter to recording emitter
        let events = Arc::new(StdMutex::new(vec![]));
        let recording: Arc<dyn HostEventEmitterPort> = Arc::new(RecordingEmitter {
            events: events.clone(),
        });
        *cell.write().unwrap() = recording;

        // 4. Emit through setup_port — should reach recording emitter
        setup_port
            .emit_setup_state_changed(uc_core::setup::SetupState::Welcome, None)
            .await;

        // 5. Verify event reached new emitter
        let recorded = events.lock().unwrap();
        assert_eq!(
            recorded.len(),
            1,
            "Setup event should reach the new emitter after swap"
        );
        assert!(
            recorded[0].contains("Welcome"),
            "Event should contain Welcome state, got: {}",
            recorded[0]
        );
    }

    #[test]
    fn setup_assembly_ports_accept_app_layer_pairing_facade() {
        let ports = SetupAssemblyPorts {
            setup_pairing_facade: Arc::new(RecordingSetupPairingFacade),
            space_access_orchestrator: Arc::new(SpaceAccessOrchestrator::new()),
            discovery_port: Arc::new(EmptyDiscoveryPortForFacadeTest),
            device_announcer: None,
            lifecycle_emitter: Arc::new(uc_app::usecases::LoggingLifecycleEventEmitter),
        };

        let _facade = ports.setup_pairing_facade.clone();
    }

    struct EmptyDiscoveryPortForFacadeTest;

    #[async_trait::async_trait]
    impl DiscoveryPort for EmptyDiscoveryPortForFacadeTest {
        async fn list_discovered_peers(
            &self,
        ) -> anyhow::Result<Vec<uc_core::network::DiscoveredPeer>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct TestIdentityStore {
        data: StdMutex<Option<Vec<u8>>>,
    }

    impl IdentityStorePort for TestIdentityStore {
        fn load_identity(&self) -> Result<Option<Vec<u8>>, IdentityStoreError> {
            Ok(self.data.lock().expect("lock test identity store").clone())
        }

        fn store_identity(&self, identity: &[u8]) -> Result<(), IdentityStoreError> {
            *self.data.lock().expect("lock test identity store") = Some(identity.to_vec());
            Ok(())
        }
    }

    struct TrustedResolver;

    #[async_trait::async_trait]
    impl ConnectionPolicyResolverPort for TrustedResolver {
        async fn resolve_for_peer(
            &self,
            _peer_id: &uc_core::PeerId,
        ) -> Result<ResolvedConnectionPolicy, ConnectionPolicyResolverError> {
            Ok(ResolvedConnectionPolicy {
                pairing_state: PairingState::Trusted,
                allowed: ConnectionPolicy::allowed_protocols(PairingState::Trusted),
            })
        }
    }

    #[tokio::test]
    async fn gui_network_ports_pairing_is_disabled_transport() {
        let libp2p_network = Arc::new(
            Libp2pNetworkAdapter::new(
                Arc::new(TestIdentityStore::default()),
                Arc::new(TrustedResolver),
                Arc::new(InMemoryEncryptionSessionPort::default()),
                Arc::new(uc_infra::clipboard::TransferPayloadDecryptorAdapter),
                Arc::new(uc_infra::clipboard::TransferPayloadEncryptorAdapter),
                PathBuf::from("/tmp/test-file-cache"),
                PairingRuntimeOwner::ExternalDaemon,
            )
            .expect("create libp2p network"),
        );

        let gui_ports =
            build_network_ports(libp2p_network.clone(), PairingRuntimeOwner::ExternalDaemon);
        let daemon_ports =
            build_network_ports(libp2p_network.clone(), PairingRuntimeOwner::CurrentProcess);

        let gui_error = gui_ports
            .pairing
            .open_pairing_session("peer-1".to_string(), "session-1".to_string())
            .await
            .expect_err("gui pairing transport must be disabled");
        assert_eq!(
            gui_error.to_string(),
            "local pairing runtime is disabled in this process"
        );

        let daemon_error = daemon_ports
            .pairing
            .open_pairing_session("peer-1".to_string(), "session-1".to_string())
            .await
            .expect_err("daemon pairing transport should use libp2p adapter");
        assert_ne!(
            daemon_error.to_string(),
            "local pairing runtime is disabled in this process"
        );
    }
}

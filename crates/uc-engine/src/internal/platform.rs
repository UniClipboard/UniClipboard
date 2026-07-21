//! Host adapter assembly: clipboard + secure storage + device identity.
//!
//! Receives host-selected clipboard and secure-storage implementations, then
//! constructs the encrypted blob store, device identity, and representation
//! normalizer used by the rest of engine wiring.

use std::path::PathBuf;
use std::sync::Arc;

use crate::internal::clipboard::SystemClipboardWiring;
use crate::internal::deps::{WiringError, WiringResult};
use uc_core::blob::ports::{BlobContentIngestPort, BlobReaderPort, BlobWriterPort};
use uc_core::ids::ProfileId;
use uc_core::ports::clipboard::ClipboardRepresentationNormalizerPort;
use uc_core::ports::*;
use uc_infra::blob::{BlobRepositoryPort, BlobStorePort, BlobWriter, FilesystemBlobStore};
use uc_infra::clipboard::ClipboardRepresentationNormalizer;
use uc_infra::config::ClipboardStorageConfig;
use uc_infra::device::LocalDeviceIdentity;
use uc_infra::security::{EncryptedBlobStore, InMemorySession};

/// Platform layer implementations
pub struct PlatformLayer {
    // System clipboard
    pub clipboard: Arc<dyn PlatformClipboardPort>,
    pub system_clipboard: Arc<dyn SystemClipboardPort>,
    /// Which adapter flavor `clipboard` / `system_clipboard` actually are.
    pub system_clipboard_wiring: SystemClipboardWiring,

    // Secure storage
    pub secure_storage: Arc<dyn SecureStoragePort>,

    // Device identity
    pub device_identity: Arc<dyn DeviceIdentityPort>,

    // Clipboard representation normalizer
    pub representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,

    // Blob writer
    pub blob_writer: Arc<dyn BlobWriterPort>,

    // Path-ingest view of the same blob writer that also returns the content
    // hash (used by capture to derive file snapshot identity).
    pub blob_content_ingest: Arc<dyn BlobContentIngestPort>,

    // Blob store (encrypted) — exposed to use cases as a read-only port.
    pub blob_store: Arc<dyn BlobReaderPort>,

    // 进程内会话——uc-infra 内部 adapter (SpaceAccessAdapter / BlobCipherAdapter /
    // TransferCipherAdapter / EncryptedBlobStore) 共享同一份 Arc。具体类型,
    // 不再走 EncryptionSessionPort trait dyn 间接层。
    pub session: Arc<InMemorySession>,

    // Current profile
    pub current_profile: Arc<dyn uc_core::ports::security::current_profile::CurrentProfilePort>,
}

pub struct SystemClipboardLayer {
    clipboard: Arc<dyn PlatformClipboardPort>,
    system_clipboard: Arc<dyn SystemClipboardPort>,
    wiring: SystemClipboardWiring,
}

impl SystemClipboardLayer {
    pub fn new(
        clipboard: Arc<dyn PlatformClipboardPort>,
        system_clipboard: Arc<dyn SystemClipboardPort>,
        wiring: SystemClipboardWiring,
    ) -> Self {
        Self {
            clipboard,
            system_clipboard,
            wiring,
        }
    }

    pub fn into_parts(
        self,
    ) -> (
        Arc<dyn PlatformClipboardPort>,
        Arc<dyn SystemClipboardPort>,
        SystemClipboardWiring,
    ) {
        (self.clipboard, self.system_clipboard, self.wiring)
    }
}

pub fn create_platform_layer(
    secure_storage: Arc<dyn SecureStoragePort>,
    profile_id: ProfileId,
    config_dir: &PathBuf,
    blob_repository: Arc<dyn BlobRepositoryPort>,
    _member_repo: Arc<dyn uc_core::MemberRepositoryPort>,
    clock: Arc<dyn ClockPort>,
    storage_config: Arc<ClipboardStorageConfig>,
    system_clipboard: SystemClipboardLayer,
) -> WiringResult<PlatformLayer> {
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
                        if path.file_name().is_some_and(|n| n == ".v2_migrated") {
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
    // One concrete BlobWriter, surfaced as both the write-side port and the
    // content-ingest port (capture needs the content hash; other writers need
    // only the BlobId). Both views point at the same instance.
    let blob_writer_concrete = Arc::new(BlobWriter::new(
        encrypted_blob_store_for_writer,
        blob_repository,
        clock,
    ));
    let blob_writer: Arc<dyn BlobWriterPort> = blob_writer_concrete.clone();
    let blob_content_ingest: Arc<dyn BlobContentIngestPort> = blob_writer_concrete;
    let blob_store_reader: Arc<dyn BlobReaderPort> = encrypted_blob_store;

    let current_profile = current_profile_for(profile_id);

    Ok(PlatformLayer {
        clipboard: system_clipboard.clipboard,
        system_clipboard: system_clipboard.system_clipboard,
        system_clipboard_wiring: system_clipboard.wiring,
        secure_storage,
        device_identity,
        representation_normalizer,
        blob_writer,
        blob_content_ingest,
        blob_store: blob_store_reader,
        session,
        current_profile,
    })
}

pub fn current_profile_for(
    profile_id: impl Into<ProfileId>,
) -> Arc<dyn uc_core::ports::security::current_profile::CurrentProfilePort> {
    Arc::new(uc_infra::security::DefaultCurrentProfile::for_profile(
        profile_id.into(),
    ))
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

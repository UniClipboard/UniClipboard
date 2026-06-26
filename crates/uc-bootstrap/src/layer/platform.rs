//! Platform adapter assembly: OS clipboard + secure storage + device identity.
//!
//! Decides — once, in the composition root — whether this process talks to the
//! real OS clipboard or a no-op substitute, then constructs the encrypted blob
//! store, device identity, and representation normalizer the rest of wiring
//! hangs off of.

use std::path::PathBuf;
use std::sync::Arc;

use uc_core::blob::ports::{BlobReaderPort, BlobWriterPort};
use uc_core::ports::clipboard::ClipboardRepresentationNormalizerPort;
use uc_core::ports::*;
use uc_infra::blob::{BlobRepositoryPort, BlobStorePort, BlobWriter, FilesystemBlobStore};
use uc_infra::clipboard::ClipboardRepresentationNormalizer;
use uc_infra::config::ClipboardStorageConfig;
use uc_infra::device::LocalDeviceIdentity;
use uc_infra::security::{EncryptedBlobStore, InMemorySession};
use uc_platform::clipboard::{LocalClipboard, NoopSystemClipboard};

use crate::wiring::deps::{WiringError, WiringResult};

/// Platform layer implementations
/// Outcome of the system-clipboard wiring decision made in
/// [`create_platform_layer`] — the single place that decides whether this
/// process talks to the real OS clipboard.
///
/// Carried through [`PlatformLayer`] / [`WiredDependencies`] so hosts align
/// their own clipboard-dependent assembly (e.g. the daemon's OS-clipboard
/// watcher worker) with the wiring instead of re-deciding — or re-probing —
/// on their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemClipboardWiring {
    /// The real OS clipboard adapter is wired; clipboard integration works.
    Real,
    /// `NoopSystemClipboard` is wired (explicitly disabled via
    /// `UC_DISABLE_SYSTEM_CLIPBOARD`, or no graphical session to talk to).
    /// Captures and writes are no-ops; nothing OS-clipboard-bound should be
    /// assembled on top.
    Noop,
}

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

    // Blob store (encrypted) — exposed to use cases as a read-only port.
    pub blob_store: Arc<dyn BlobReaderPort>,

    // 进程内会话——uc-infra 内部 adapter (SpaceAccessAdapter / BlobCipherAdapter /
    // TransferCipherAdapter / EncryptedBlobStore) 共享同一份 Arc。具体类型,
    // 不再走 EncryptionSessionPort trait dyn 间接层。
    pub session: Arc<InMemorySession>,

    // Current profile
    pub current_profile: Arc<dyn uc_core::ports::security::current_profile::CurrentProfilePort>,
}

fn noop_system_clipboard() -> (Arc<dyn PlatformClipboardPort>, Arc<dyn SystemClipboardPort>) {
    let noop: Arc<NoopSystemClipboard> = Arc::new(NoopSystemClipboard);
    (noop.clone(), noop)
}

pub fn create_platform_layer(
    secure_storage: Arc<dyn SecureStoragePort>,
    config_dir: &PathBuf,
    blob_repository: Arc<dyn BlobRepositoryPort>,
    _member_repo: Arc<dyn uc_core::MemberRepositoryPort>,
    clock: Arc<dyn ClockPort>,
    storage_config: Arc<ClipboardStorageConfig>,
) -> WiringResult<PlatformLayer> {
    // Which system clipboard adapter to wire. Two reasons to substitute the
    // no-op adapter, both decided here in the composition root (uc-platform
    // only reports capability):
    //
    // 1. `UC_DISABLE_SYSTEM_CLIPBOARD=1` — explicit opt-out. Slice 1 CLI
    //    commands (init/invite/join) do not touch the system clipboard, but a
    //    non-bundled CLI launched from a shell lacks the WindowServer / AppKit
    //    context that `clipboard-rs` assumes, so `LocalClipboard::new()` panics
    //    inside `+[NSPasteboard generalPasteboard]`. The CLI sets this variable
    //    before bootstrap.
    // 2. No graphical session (headless Linux server / container): the OS has
    //    no clipboard, so the real adapter can only fail to connect. Wiring the
    //    no-op adapter keeps the daemon bootable for history / transfer / API
    //    duties (issue #1021: `uniclip join` on Ubuntu Server died on
    //    ClipboardInit and the CLI saw only an opaque 30s health timeout).
    //
    // Inside a graphical session, a real init failure stays a hard error —
    // degrading there would mask a genuine platform bug.
    // Parse the opt-out as a boolean-like flag (documented contract is
    // `UC_DISABLE_SYSTEM_CLIPBOARD=1`): only truthy values opt out, so a
    // leftover `0` / `false` in the environment keeps the real adapter.
    let disable_system_clipboard = std::env::var("UC_DISABLE_SYSTEM_CLIPBOARD")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        });
    let system_clipboard_wiring = if disable_system_clipboard {
        tracing::info!(
            "UC_DISABLE_SYSTEM_CLIPBOARD set; substituting NoopSystemClipboard \
             (any clipboard capture / write is a no-op)"
        );
        SystemClipboardWiring::Noop
    } else if uc_platform::capability::detect_system_clipboard_capability()
        == uc_platform::capability::SystemClipboardCapability::NoDisplaySession
    {
        tracing::warn!(
            "no graphical session detected (DISPLAY / WAYLAND_DISPLAY unset); \
             substituting NoopSystemClipboard so the process can still serve \
             history / transfer / API duties on this headless host"
        );
        SystemClipboardWiring::Noop
    } else {
        SystemClipboardWiring::Real
    };
    let (clipboard, system_clipboard): (
        Arc<dyn PlatformClipboardPort>,
        Arc<dyn SystemClipboardPort>,
    ) = match system_clipboard_wiring {
        SystemClipboardWiring::Noop => noop_system_clipboard(),
        SystemClipboardWiring::Real => {
            let clipboard_impl = LocalClipboard::new().map_err(|e| {
                WiringError::ClipboardInit(format!("Failed to create clipboard: {}", e))
            })?;
            let clipboard_impl = Arc::new(clipboard_impl);
            (clipboard_impl.clone(), clipboard_impl)
        }
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
        system_clipboard_wiring,
        secure_storage,
        device_identity,
        representation_normalizer,
        blob_writer,
        blob_store: blob_store_reader,
        session,
        current_profile,
    })
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

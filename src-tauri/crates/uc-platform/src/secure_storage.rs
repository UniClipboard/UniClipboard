//! Secure storage selection and default secure storage factory.

use std::{fs, path::PathBuf, sync::Arc};
use tracing::{debug, error, info, warn};

use uc_core::ports::SecureStoragePort;

use crate::{
    capability::{detect_storage_capability, SecureStorageCapability},
    file_secure_storage::FileSecureStorage,
    system_secure_storage::SystemSecureStorage,
};

/// Sentinel key used to probe whether the platform secret service is actually
/// reachable from this process. Picked so it's clearly internal and unlikely
/// to ever collide with a real keyring entry.
const SYSTEM_STORAGE_PROBE_KEY: &str = "__uc_secure_storage_probe__";

#[derive(Debug, thiserror::Error)]
pub enum SecureStorageFactoryError {
    #[error("secure storage unsupported: {capability:?}")]
    Unsupported { capability: SecureStorageCapability },

    #[error("failed to initialize file-based secure storage: {0}")]
    FileBasedInit(#[from] std::io::Error),
}

/// Probe whether `SystemSecureStorage` can actually round-trip a call to the
/// platform secret service. `Ok` means `get` succeeded — including the "no
/// such entry" case, which is the expected outcome for the sentinel key.
/// `Err` means the secret service is reachable in principle (env says we have
/// a desktop + DBus) but the real call was rejected at runtime: snap AppArmor
/// blocking `org.freedesktop.Secret.Service.OpenSession`, gnome-keyring
/// locked and unable to prompt in this context, KWallet disabled, etc. Those
/// failures used to crash daemon bootstrap; callers can now degrade
/// gracefully to file-based KEK instead.
fn probe_system_storage_reachable(storage: &SystemSecureStorage) -> Result<(), String> {
    storage
        .get(SYSTEM_STORAGE_PROBE_KEY)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn secure_storage_from_capability(
    capability: SecureStorageCapability,
) -> Result<Arc<dyn SecureStoragePort>, SecureStorageFactoryError> {
    secure_storage_from_capability_with_base_dir(capability, None)
}

/// Create a secure storage instance matching the provided secure storage capability.
///
/// If `capability` indicates system storage, returns a system-backed implementation wrapped in
/// `Arc<dyn SecureStoragePort>`. If `capability` is `FileBasedKeystore`, returns a file-backed
/// implementation using the provided `base_dir`. If `base_dir` is `None`,
/// returns `SecureStorageFactoryError::FileBasedInit` with `std::io::ErrorKind::NotFound`.
/// If `capability` is `Unsupported`, returns `SecureStorageFactoryError::Unsupported` containing
/// the provided capability.
///
/// The `base_dir` argument supplies the application data root required for file-based storage;
/// when present the directory will be created if it does not exist.
///
/// # Examples
///
/// ```ignore
/// # use std::sync::Arc;
/// # use std::path::PathBuf;
/// # use uc_platform::capability::SecureStorageCapability;
/// # use uc_platform::secure_storage::secure_storage_from_capability_with_base_dir;
/// let temp_dir = std::env::temp_dir();
/// let storage = secure_storage_from_capability_with_base_dir(
///     SecureStorageCapability::FileBasedKeystore,
///     Some(temp_dir),
/// );
/// assert!(storage.is_ok());
/// ```
fn secure_storage_from_capability_with_base_dir(
    capability: SecureStorageCapability,
    base_dir: Option<PathBuf>,
) -> Result<Arc<dyn SecureStoragePort>, SecureStorageFactoryError> {
    match capability {
        SecureStorageCapability::SystemKeyring => {
            Ok(Arc::new(SystemSecureStorage::new()) as Arc<dyn SecureStoragePort>)
        }
        SecureStorageCapability::FileBasedKeystore => {
            if let Some(base_dir) = base_dir {
                fs::create_dir_all(&base_dir)?;
                Ok(Arc::new(FileSecureStorage::with_base_dir(base_dir))
                    as Arc<dyn SecureStoragePort>)
            } else {
                Err(SecureStorageFactoryError::FileBasedInit(
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "File-based secure storage requires app data root",
                    ),
                ))
            }
        }
        SecureStorageCapability::Unsupported => {
            Err(SecureStorageFactoryError::Unsupported { capability })
        }
    }
}

/// Create a default secure storage implementation based on the detected storage capability.
///
/// The function selects an appropriate secure storage implementation for the current
/// environment:
/// - If system secure storage is available, returns a system-backed implementation.
/// - If only a file-based keystore is available, returns a `FileBasedInit` error
///   indicating an application data root is required.
/// - If secure storage is unsupported, returns an `Unsupported` error.
///
/// # Returns
///
/// `Ok(Arc<dyn SecureStoragePort>)` with the selected storage on success; otherwise
/// an appropriate `SecureStorageFactoryError` describing why storage could not be
/// created (`FileBasedInit` when an app data root is required, or
/// `Unsupported` when no secure storage is available).
///
/// # Examples
///
/// ```
/// use uc_platform::secure_storage::create_default_secure_storage;
/// let _ = create_default_secure_storage();
/// ```
pub fn create_default_secure_storage(
) -> Result<Arc<dyn SecureStoragePort>, SecureStorageFactoryError> {
    let capability = detect_storage_capability();
    debug!(capability = ?capability, "Detected secure storage capability");

    match capability {
        SecureStorageCapability::SystemKeyring => {
            info!("Using system secure storage");
            secure_storage_from_capability(capability)
        }
        SecureStorageCapability::FileBasedKeystore => {
            warn!(
                "File-based secure storage requires app data root; use create_default_secure_storage_in_app_data_root"
            );
            Err(SecureStorageFactoryError::FileBasedInit(
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "File-based secure storage requires app data root",
                ),
            ))
        }
        SecureStorageCapability::Unsupported => {
            error!(capability = ?capability, "Secure storage unsupported");
            Err(SecureStorageFactoryError::Unsupported { capability })
        }
    }
}

/// Create a default secure storage using `app_data_root` when a file-based keystore is required.
///
/// Detects the platform's secure storage capability and returns an appropriate `SecureStoragePort`:
/// - If system secure storage is available, returns the system-backed implementation.
/// - If a file-based keystore is detected, initializes a file-backed implementation rooted at
///   `app_data_root`.
/// - If secure storage is unsupported, returns `SecureStorageFactoryError::Unsupported`.
///
/// # Parameters
///
/// - `app_data_root`: Path to the application's data root used to initialize file-based storage.
///
/// # Errors
///
/// Returns `SecureStorageFactoryError::Unsupported` when secure storage is not available.
/// Returns `SecureStorageFactoryError::FileBasedInit` if initialization of file-based storage fails.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use uc_platform::secure_storage::{
///     create_default_secure_storage_in_app_data_root, SecureStorageFactoryError,
/// };
/// let app_data_root = std::env::temp_dir().join("my_app_storage");
/// let res = create_default_secure_storage_in_app_data_root(app_data_root);
/// // On platforms with system secure storage support this may still return Ok.
/// assert!(matches!(res, Ok(_)) || matches!(res, Err(SecureStorageFactoryError::Unsupported { .. })));
/// ```
pub fn create_default_secure_storage_in_app_data_root(
    app_data_root: PathBuf,
) -> Result<Arc<dyn SecureStoragePort>, SecureStorageFactoryError> {
    let capability = detect_storage_capability();
    debug!(capability = ?capability, "Detected secure storage capability");

    match capability {
        SecureStorageCapability::SystemKeyring => {
            // capability detection is env-only (DISPLAY + DBUS_SESSION_BUS_ADDRESS),
            // it cannot tell whether the secret service actually accepts our calls.
            // snap AppArmor refusing OpenSession is the canonical failure we hit
            // (see `password-manager-service` plug in snapcraft.yaml). Round-trip
            // a probe before committing to system keyring; on failure degrade to
            // FileSecureStorage so daemon bootstrap can still complete instead of
            // crashing the GUI with an opaque "in-process daemon start failed".
            let system_storage = SystemSecureStorage::new();
            match probe_system_storage_reachable(&system_storage) {
                Ok(()) => {
                    info!("Using system secure storage");
                    Ok(Arc::new(system_storage) as Arc<dyn SecureStoragePort>)
                }
                Err(probe_err) => {
                    warn!(
                        probe_error = %probe_err,
                        "System secure storage probe failed; falling back to file-based KEK. \
                         Check snap interface connections (e.g. password-manager-service) or \
                         keyring daemon availability."
                    );
                    Ok(
                        Arc::new(FileSecureStorage::new_in_app_data_root(app_data_root)?)
                            as Arc<dyn SecureStoragePort>,
                    )
                }
            }
        }
        SecureStorageCapability::FileBasedKeystore => {
            warn!("Using file-based secure storage (insecure dev fallback for WSL/headless environments)");
            Ok(
                Arc::new(FileSecureStorage::new_in_app_data_root(app_data_root)?)
                    as Arc<dyn SecureStoragePort>,
            )
        }
        SecureStorageCapability::Unsupported => {
            error!(capability = ?capability, "Secure storage unsupported");
            Err(SecureStorageFactoryError::Unsupported { capability })
        }
    }
}

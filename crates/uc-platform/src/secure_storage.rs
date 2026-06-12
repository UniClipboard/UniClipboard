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
///
/// On Linux the production wiring uses the stricter
/// `probe_system_storage_integrity` instead; this function is still compiled
/// for parity / future use, hence the `dead_code` allowance there.
#[cfg_attr(target_os = "linux", allow(dead_code))]
fn probe_system_storage_reachable(storage: &dyn SecureStoragePort) -> Result<(), String> {
    storage
        .get(SYSTEM_STORAGE_PROBE_KEY)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Linux-only: round-trip 32 random bytes through the platform secret service
/// and verify the read-back is byte-identical. Catches backends that silently
/// mangle binary payloads — most notably KWallet's freedesktop Secret-Service
/// bridge (`kwalletd5` / `kwalletd6`), which routes values through
/// `QString::fromUtf8()` internally and replaces every non-UTF-8 byte with
/// `U+FFFD` (3 bytes when re-encoded). A 32-byte random KEK round-tripped
/// through KWallet's bridge typically comes back at ~60–65 bytes and fails
/// every subsequent `Kek::from_bytes` parse — see issue #838.
///
/// Returns `Ok(())` only when write + read + byte-equality all succeed.
/// On any failure (including reachability failures previously caught by
/// `probe_system_storage_reachable`) the caller falls back to
/// `FileSecureStorage`. The sentinel entry is best-effort cleaned up
/// regardless of outcome so we don't leave probe state in the user's wallet.
///
/// Not wired into macOS / Windows production paths: every `set` on macOS
/// Keychain risks a fresh authorization prompt, and Windows Credential
/// Manager has no comparable text-mangling pathology — the lighter
/// reachability probe is enough on those platforms. The function is still
/// compiled there so its unit tests (which exercise pure trait-object
/// behavior) run on every host.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn probe_system_storage_integrity(storage: &dyn SecureStoragePort) -> Result<(), String> {
    use rand::{rngs::OsRng, TryRngCore};

    let mut probe = [0u8; 32];
    OsRng
        .try_fill_bytes(&mut probe)
        .map_err(|e| format!("rng failure while preparing integrity probe: {e}"))?;

    storage
        .set(SYSTEM_STORAGE_PROBE_KEY, &probe)
        .map_err(|e| format!("integrity probe write failed: {e}"))?;

    let read_result = storage.get(SYSTEM_STORAGE_PROBE_KEY);

    // Best-effort cleanup of the sentinel regardless of the read outcome —
    // leaving a probe entry behind would be sloppy and (on backends that
    // mangle bytes) confuse future inspection.
    let _ = storage.delete(SYSTEM_STORAGE_PROBE_KEY);

    match read_result.map_err(|e| format!("integrity probe read failed: {e}"))? {
        Some(bytes) if bytes == probe => Ok(()),
        Some(bytes) => Err(format!(
            "secret service did not preserve binary payload \
             (wrote 32 bytes, read {} bytes back); \
             matches KWallet's Secret-Service bridge mangling values via \
             QString::fromUtf8() — see issue #838",
            bytes.len()
        )),
        None => Err(
            "integrity probe write reported success but read returned no entry; \
             secret service is not persisting writes for this process"
                .into(),
        ),
    }
}

/// Upper bound for the Linux Secret-Service probe.
///
/// Must stay comfortably below the GUI's 8s daemon-startup window (the bootstrap
/// `StartupTimeout`) so that a *blocked* probe still leaves enough time for the
/// rest of daemon wiring + HTTP listen after we degrade to file-based KEK. A
/// healthy, already-unlocked gnome-keyring round-trips set/get/delete in well
/// under a second, so 3s only ever trips on genuinely stuck backends.
#[cfg(target_os = "linux")]
const SYSTEM_STORAGE_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Run a secure-storage `probe` on a detached worker thread, bounded by
/// `timeout`. Returns the probe's own `Result` if it finishes in time, or an
/// `Err` describing the timeout otherwise.
///
/// Why bound it at all: on Linux the probe issues *synchronous* D-Bus
/// Secret-Service calls. In a minimal Wayland session (hyprland / sway) the
/// environment looks like a desktop — `DISPLAY` + `DBUS_SESSION_BUS_ADDRESS`
/// are both present — so capability detection picks `SystemKeyring`, yet there
/// may be no running secret service and no unlock prompter. The D-Bus call then
/// *blocks* (waiting on service activation or an unlock agent) instead of
/// failing fast, so the graceful file-based fallback in
/// `create_default_secure_storage_in_app_data_root` never fires and daemon
/// bootstrap hangs past the GUI's startup window. Bounding the probe lets us
/// treat "blocks too long" the same as "failed fast": degrade to file-based.
///
/// The worker thread is intentionally detached. A blocked D-Bus call cannot be
/// cancelled from outside; the thread unwinds on its own once the underlying
/// call returns or hits the D-Bus layer's own timeout (~25s default). It owns
/// only a cloned `SystemSecureStorage` (a `String`), so letting it linger is
/// cheap.
///
/// Compiled on every platform so its unit tests run on every host; only the
/// Linux production path actually calls it (macOS / Windows keep the cheap
/// reachability probe inline).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn run_probe_with_timeout<F>(timeout: std::time::Duration, probe: F) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String> + Send + 'static,
{
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("uc-keyring-probe".into())
        .spawn(move || {
            // Receiver may already be gone if we timed out; ignore the send error.
            let _ = tx.send(probe());
        })
        .map_err(|e| format!("failed to spawn keyring probe thread: {e}"))?;

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "system secret service did not respond within {timeout:?}; \
             treating as unavailable. Common on minimal Wayland sessions \
             (hyprland/sway) with no running or unlocked secret service."
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            // Worker panicked before sending — treat as a probe failure.
            Err("keyring probe thread terminated unexpectedly".to_string())
        }
    }
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
    // Portable ("green") builds must not write the KEK into a per-user system
    // secret store (Windows Credential Manager / macOS Keychain / Secret
    // Service): that would leave a trace outside the portable folder and break
    // the "runs from a USB stick, leaves nothing behind" contract. Keep the KEK
    // in a file under the portable data root instead.
    if crate::portable::is_portable() {
        info!("Portable mode: storing KEK as a file under the portable data root (skipping system secure storage)");
        return Ok(
            Arc::new(FileSecureStorage::new_in_app_data_root(app_data_root)?)
                as Arc<dyn SecureStoragePort>,
        );
    }

    let capability = detect_storage_capability();
    debug!(capability = ?capability, "Detected secure storage capability");

    match capability {
        SecureStorageCapability::SystemKeyring => {
            // capability detection is env-only (DISPLAY + DBUS_SESSION_BUS_ADDRESS),
            // it cannot tell whether the secret service actually accepts our calls.
            // snap AppArmor refusing OpenSession is the canonical reachability
            // failure (see `password-manager-service` plug in snapcraft.yaml);
            // KWallet's Secret-Service bridge corrupting binary payloads is the
            // canonical integrity failure (issue #838). Probe before committing
            // to system keyring; on failure degrade to FileSecureStorage so
            // daemon bootstrap can still complete instead of crashing with an
            // opaque "invalid KEK length" later in the unlock path.
            let system_storage = SystemSecureStorage::new();

            // Linux runs the binary round-trip integrity probe; macOS/Windows
            // stick to the cheap reachability probe — a `set` on macOS would
            // risk a fresh Keychain authorization prompt every launch, and
            // Windows Credential Manager has no equivalent text-mangling
            // pathology that would be worth the extra write for.
            //
            // The Linux probe is bounded by a timeout: its synchronous D-Bus
            // Secret-Service calls can *block* (not just fail) when the session
            // looks like a desktop but has no running/unlocked secret service —
            // see `run_probe_with_timeout`. A blocked probe degrades to
            // file-based KEK just like an outright failure.
            #[cfg(target_os = "linux")]
            let probe_result = {
                let storage_for_probe = system_storage.clone();
                run_probe_with_timeout(SYSTEM_STORAGE_PROBE_TIMEOUT, move || {
                    probe_system_storage_integrity(&storage_for_probe)
                })
            };
            #[cfg(not(target_os = "linux"))]
            let probe_result = probe_system_storage_reachable(&system_storage);

            match probe_result {
                Ok(()) => {
                    info!("Using system secure storage");
                    Ok(Arc::new(system_storage) as Arc<dyn SecureStoragePort>)
                }
                Err(probe_err) => {
                    warn!(
                        probe_error = %probe_err,
                        "System secure storage probe failed; falling back to file-based KEK. \
                         Common causes: snap AppArmor blocking Secret-Service access \
                         (check `password-manager-service` plug), keyring daemon not \
                         running, or KWallet's Secret-Service bridge mangling binary \
                         values (issue #838)."
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use uc_core::ports::SecureStorageError;

    /// `SecureStoragePort` that preserves bytes verbatim — models a
    /// well-behaved backend like gnome-keyring.
    #[derive(Default)]
    struct HonestStorage {
        map: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SecureStoragePort for HonestStorage {
        fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Ok(self.map.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
            self.map
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_vec());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
            self.map.lock().unwrap().remove(key);
            Ok(())
        }
    }

    /// `SecureStoragePort` that models KWallet's Secret-Service bridge: every
    /// stored value is funneled through `String::from_utf8_lossy`, replacing
    /// invalid UTF-8 sequences with `U+FFFD` (3 bytes when re-encoded as
    /// UTF-8). Mirrors `kwalletd`'s internal `QString::fromUtf8` round-trip
    /// closely enough to drive integrity-probe assertions in pure CPU tests.
    #[derive(Default)]
    struct KWalletLikeStorage {
        map: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SecureStoragePort for KWalletLikeStorage {
        fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Ok(self.map.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
            let mangled = String::from_utf8_lossy(value).into_owned().into_bytes();
            self.map.lock().unwrap().insert(key.to_string(), mangled);
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
            self.map.lock().unwrap().remove(key);
            Ok(())
        }
    }

    /// `SecureStoragePort` whose `set` always fails — models snap AppArmor
    /// refusing `OpenSession` or a locked keyring rejecting writes.
    struct WriteRejectingStorage;

    impl SecureStoragePort for WriteRejectingStorage {
        fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Ok(None)
        }
        fn set(&self, _key: &str, _value: &[u8]) -> Result<(), SecureStorageError> {
            Err(SecureStorageError::PermissionDenied(
                "denied by test".into(),
            ))
        }
        fn delete(&self, _key: &str) -> Result<(), SecureStorageError> {
            Ok(())
        }
    }

    #[test]
    fn integrity_probe_passes_on_honest_backend() {
        let storage = HonestStorage::default();
        let result = probe_system_storage_integrity(&storage);
        assert!(result.is_ok(), "honest backend must pass: {result:?}");
        // Sentinel must be cleaned up after a successful probe.
        assert!(
            storage
                .map
                .lock()
                .unwrap()
                .get(SYSTEM_STORAGE_PROBE_KEY)
                .is_none(),
            "sentinel entry must be cleaned up after success"
        );
    }

    #[test]
    fn integrity_probe_catches_kwallet_byte_mangling() {
        let storage = KWalletLikeStorage::default();
        let result = probe_system_storage_integrity(&storage);
        let err = result.expect_err("KWallet-like backend must be rejected");
        assert!(
            err.contains("did not preserve binary payload"),
            "error must point at byte mismatch, got: {err}"
        );
        assert!(
            err.contains("KWallet") || err.contains("#838"),
            "error must mention KWallet or #838 for triage clarity, got: {err}"
        );
        // Sentinel still cleaned up even on integrity failure.
        assert!(
            storage
                .map
                .lock()
                .unwrap()
                .get(SYSTEM_STORAGE_PROBE_KEY)
                .is_none(),
            "sentinel entry must be cleaned up after mismatch"
        );
    }

    #[test]
    fn integrity_probe_reports_write_failure() {
        let result = probe_system_storage_integrity(&WriteRejectingStorage);
        let err = result.expect_err("write rejection must surface as Err");
        assert!(
            err.contains("integrity probe write failed"),
            "error must explain the write failed, got: {err}"
        );
    }

    #[test]
    fn probe_timeout_trips_on_blocking_probe() {
        // A probe that outlives the timeout must read as a failure so the
        // caller degrades to file-based KEK instead of hanging bootstrap.
        let result = run_probe_with_timeout(std::time::Duration::from_millis(50), || {
            std::thread::sleep(std::time::Duration::from_secs(30));
            Ok(())
        });
        let err = result.expect_err("a probe slower than the timeout must be Err");
        assert!(
            err.contains("did not respond"),
            "timeout error must explain non-response, got: {err}"
        );
    }

    #[test]
    fn probe_timeout_passes_through_fast_ok() {
        let result = run_probe_with_timeout(std::time::Duration::from_secs(5), || Ok(()));
        assert!(result.is_ok(), "a fast Ok must pass through: {result:?}");
    }

    #[test]
    fn probe_timeout_passes_through_fast_err() {
        let result = run_probe_with_timeout(std::time::Duration::from_secs(5), || {
            Err("backend rejected the call".to_string())
        });
        let err = result.expect_err("a fast Err must pass through unchanged");
        assert!(
            err.contains("backend rejected the call"),
            "original probe error must be preserved, got: {err}"
        );
    }
}

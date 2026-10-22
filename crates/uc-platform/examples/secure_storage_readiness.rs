//! Nonvisual E2E probe for a private Linux session bus and isolated profile.
use uc_platform::secure_storage::create_default_secure_storage_in_app_data_root;

fn main() -> anyhow::Result<()> {
    #[cfg(feature = "test-helpers")]
    if let Ok(mode) = std::env::var("UC_READINESS_E2E_MODE") {
        return model::run(mode);
    }
    let root = std::env::var_os("UC_READINESS_E2E_ROOT")
        .ok_or_else(|| anyhow::anyhow!("UC_READINESS_E2E_ROOT is required"))?;
    let root = std::path::PathBuf::from(root);
    if !root.join("secure-storage-source.json").is_file() {
        anyhow::bail!("an explicit System source record is required");
    }
    let selected = create_default_secure_storage_in_app_data_root(root.clone())?;
    let start = std::time::Instant::now();
    // This read invokes the production integrity probe; it writes no content or credentials.
    let result = selected.storage.get("__uc_readiness_e2e_absent__");
    println!(
        "ok={} elapsed_ms={} file_store_created={} error={:?}",
        result.is_ok(),
        start.elapsed().as_millis(),
        root.join("keyring").exists(),
        result.as_ref().err().map(ToString::to_string)
    );
    result.map(|_| ()).map_err(anyhow::Error::new)
}

#[cfg(feature = "test-helpers")]
mod model {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };
    use std::time::{Duration, Instant};
    use uc_platform::ports::{SecureStorageError, SecureStorageProvider};
    struct Service {
        mode: String,
        started: Instant,
        calls: AtomicUsize,
        value: Mutex<Option<Vec<u8>>>,
    }
    impl SecureStorageProvider for Service {
        fn set(&self, _: &str, bytes: &[u8]) -> Result<(), SecureStorageError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.mode.as_str() {
                "delayed" if self.started.elapsed() < Duration::from_secs(1) => {
                    return Err(SecureStorageError::Unavailable(
                        "DBus error: The name is not activatable".into(),
                    ))
                }
                "unavailable" => {
                    return Err(SecureStorageError::Unavailable(
                        "org.freedesktop.DBus.Error.NameHasNoOwner".into(),
                    ))
                }
                "service-unknown" => {
                    return Err(SecureStorageError::Unavailable(
                        "org.freedesktop.DBus.Error.ServiceUnknown".into(),
                    ))
                }
                "denied" => {
                    return Err(SecureStorageError::PermissionDenied("access denied".into()))
                }
                "denied-with-owner" => {
                    return Err(SecureStorageError::PermissionDenied(
                        "access denied: NameHasNoOwner".into(),
                    ))
                }
                "binary" => {
                    return Err(SecureStorageError::Other(
                        "invalid UTF-8 content_type text/plain text encoding".into(),
                    ))
                }
                "blocked" => std::thread::sleep(Duration::from_secs(10)),
                _ => {}
            }
            *self
                .value
                .lock()
                .map_err(|_| SecureStorageError::Other("poison".into()))? = Some(bytes.to_vec());
            Ok(())
        }
        fn get(&self, _: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            if self.mode == "mismatch" {
                return Ok(Some(vec![0; 33]));
            }
            Ok(self
                .value
                .lock()
                .map_err(|_| SecureStorageError::Other("poison".into()))?
                .clone())
        }
        fn delete(&self, _: &str) -> Result<(), SecureStorageError> {
            Ok(())
        }
    }
    pub fn run(mode: String) -> anyhow::Result<()> {
        let service = Arc::new(Service {
            mode: mode.clone(),
            started: Instant::now(),
            calls: AtomicUsize::new(0),
            value: Mutex::new(None),
        });
        let result = uc_platform::secure_storage::probe_system_storage_for_e2e(service.clone());
        let elapsed = service.started.elapsed();
        let calls = service.calls.load(Ordering::SeqCst);
        let passed = match mode.as_str() {
            "delayed" => result.is_ok() && calls > 1 && elapsed < Duration::from_secs(3),
            "unavailable" | "service-unknown" => {
                result.is_err()
                    && calls > 1
                    && elapsed >= Duration::from_millis(2800)
                    && elapsed < Duration::from_secs(4)
            }
            "blocked" => {
                result.is_err()
                    && calls == 1
                    && elapsed >= Duration::from_secs(3)
                    && elapsed < Duration::from_secs(4)
            }
            _ => result.is_err() && calls == 1 && elapsed < Duration::from_secs(1),
        };
        println!(
            "case={mode} passed={passed} calls={calls} elapsed_ms={} error={:?}",
            elapsed.as_millis(),
            result.err()
        );
        if !passed {
            anyhow::bail!("failure model did not match");
        }
        Ok(())
    }
}

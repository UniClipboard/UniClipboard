use std::sync::{Arc, OnceLock};

use crate::ports::{SecureStorageError, SecureStorageProvider};

/// One provider remains authoritative for the entire run, including failures.
pub(super) struct ProbedSecureStorage {
    inner: Arc<dyn SecureStorageProvider>,
    outcome: OnceLock<Result<(), String>>,
}

impl ProbedSecureStorage {
    pub(super) fn new(inner: Arc<dyn SecureStorageProvider>) -> Self {
        Self {
            inner,
            outcome: OnceLock::new(),
        }
    }

    fn selected(&self) -> Result<&dyn SecureStorageProvider, SecureStorageError> {
        let result = self.outcome.get_or_init(|| {
            #[cfg(target_os = "linux")]
            let result = {
                let inner = Arc::clone(&self.inner);
                super::run_probe_with_timeout(super::SYSTEM_STORAGE_PROBE_TIMEOUT, move || {
                    super::probe_system_storage_until_ready(inner.as_ref())
                })
            };
            #[cfg(not(target_os = "linux"))]
            let result = super::probe_system_storage_reachable(self.inner.as_ref());
            if let Err(error) = &result {
                tracing::warn!(probe_error = %error, "system secure store is unavailable for this run");
            }
            result
        });
        match result {
            Ok(()) => Ok(self.inner.as_ref()),
            Err(error) => Err(SecureStorageError::Unavailable(error.clone())),
        }
    }
}

impl SecureStorageProvider for ProbedSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        self.selected()?.get(key)
    }
    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        self.selected()?.set(key, value)
    }
    fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
        self.selected()?.delete(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct DeniedStorage {
        calls: AtomicUsize,
    }

    impl DeniedStorage {
        fn deny<T>(&self) -> Result<T, SecureStorageError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(SecureStorageError::PermissionDenied("test denial".into()))
        }
    }

    impl SecureStorageProvider for DeniedStorage {
        fn get(&self, _: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            self.deny()
        }
        fn set(&self, _: &str, _: &[u8]) -> Result<(), SecureStorageError> {
            self.deny()
        }
        fn delete(&self, _: &str) -> Result<(), SecureStorageError> {
            self.deny()
        }
    }

    #[test]
    fn construction_never_accesses_secrets_and_failure_never_looks_empty() {
        let inner = Arc::new(DeniedStorage::default());
        let storage = ProbedSecureStorage::new(inner.clone());
        assert_eq!(inner.calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            storage.get("key"),
            Err(SecureStorageError::Unavailable(_))
        ));
        let calls = inner.calls.load(Ordering::SeqCst);
        assert!(calls > 0);
        assert!(storage.get("key").is_err());
        assert!(storage.set("key", b"replacement").is_err());
        assert!(storage.delete("key").is_err());
        assert_eq!(inner.calls.load(Ordering::SeqCst), calls);
    }

    #[test]
    fn successful_probe_is_reused() {
        let temporary = tempfile::tempdir().unwrap();
        let inner = Arc::new(
            crate::file_secure_storage::FileSecureStorage::with_base_dir(
                temporary.path().join("keys"),
            ),
        );
        let storage = ProbedSecureStorage::new(inner);
        storage.set("key", b"value").unwrap();
        assert!(storage.outcome.get().unwrap().is_ok());
        assert_eq!(storage.get("key").unwrap(), Some(b"value".to_vec()));
        storage.delete("key").unwrap();
        assert_eq!(storage.get("key").unwrap(), None);
    }
}

use std::sync::{Arc, OnceLock};

use crate::ports::{SecureStorageError, SecureStorageProvider};
use crate::system_secure_storage::is_binary_content_rejected;

/// Outcome of the one-time startup probe, cached for the process lifetime.
enum ProbeOutcome {
    /// The system store round-tripped the probe; use it as normal.
    Ready,
    /// The system store deterministically rejects binary writes (hardcoded
    /// `text/plain` content_type + strict UTF-8 validation on the backend).
    /// Every retry would fail identically, so this run uses the file key
    /// store instead of reporting the whole system store unavailable.
    FellBackToFile,
    /// Any other probe failure: genuinely unreachable, locked, denied, or
    /// byte-mangling (KWallet #838). No fallback; the caller reports
    /// unavailability.
    Unavailable(String),
}

/// Construction is inert. A failed probe stays failed until the next startup.
pub(super) struct ProbedSecureStorage {
    inner: Arc<dyn SecureStorageProvider>,
    /// Used only when the probe finds the deterministic binary-content
    /// rejection described on `ProbeOutcome::FellBackToFile`. `None` when no
    /// file store is available to fall back to (e.g. no app data root).
    file_fallback: Option<Arc<dyn SecureStorageProvider>>,
    outcome: OnceLock<ProbeOutcome>,
}

impl ProbedSecureStorage {
    pub(super) fn with_file_fallback(
        inner: Arc<dyn SecureStorageProvider>,
        file_fallback: Option<Arc<dyn SecureStorageProvider>>,
    ) -> Self {
        Self {
            inner,
            file_fallback,
            outcome: OnceLock::new(),
        }
    }

    fn selected(&self) -> Result<&dyn SecureStorageProvider, SecureStorageError> {
        match self.outcome.get_or_init(|| {
            #[cfg(target_os = "linux")]
            let result = {
                let inner = Arc::clone(&self.inner);
                super::run_probe_with_timeout(super::SYSTEM_STORAGE_PROBE_TIMEOUT, move || {
                    super::probe_system_storage_integrity(inner.as_ref())
                })
            };
            #[cfg(not(target_os = "linux"))]
            let result = super::probe_system_storage_reachable(self.inner.as_ref());

            match result {
                Ok(()) => ProbeOutcome::Ready,
                Err(error)
                    if self.file_fallback.is_some() && is_binary_content_rejected(&error) =>
                {
                    tracing::warn!(
                        probe_error = %error,
                        "system secret service cannot store binary secrets (hardcoded \
                         text/plain content_type rejected by a strict backend); \
                         falling back to the file key store for this run"
                    );
                    ProbeOutcome::FellBackToFile
                }
                Err(error) => {
                    tracing::warn!(
                        probe_error = %error,
                        "system secure store is unavailable for this run"
                    );
                    ProbeOutcome::Unavailable(error)
                }
            }
        }) {
            ProbeOutcome::Ready => Ok(self.inner.as_ref()),
            // Only ever produced above when `file_fallback.is_some()`; fall back to
            // `inner` in the unreachable case rather than panicking on a stale read.
            ProbeOutcome::FellBackToFile => Ok(self
                .file_fallback
                .as_deref()
                .unwrap_or_else(|| self.inner.as_ref())),
            ProbeOutcome::Unavailable(error) => Err(SecureStorageError::Unavailable(error.clone())),
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
        let storage = ProbedSecureStorage::with_file_fallback(inner.clone(), None);
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

    /// The exact wording observed in t-0155's diagnostic log.
    const BINARY_CONTENT_REJECTION_TEXT: &str = "DBus error: Secret value contains invalid \
        UTF-8 sequences but content_type declares text encoding; use \
        application/octet-stream for binary data";

    /// Rejects every call with the deterministic binary-content-type error text,
    /// regardless of which cfg-selected probe function (`get` on non-Linux,
    /// `set`+`get` on Linux) is exercised by the test host.
    #[derive(Default)]
    struct BinaryContentRejectingStorage;

    impl SecureStorageProvider for BinaryContentRejectingStorage {
        fn get(&self, _: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Err(SecureStorageError::Other(
                BINARY_CONTENT_REJECTION_TEXT.to_string(),
            ))
        }
        fn set(&self, _: &str, _: &[u8]) -> Result<(), SecureStorageError> {
            Err(SecureStorageError::Other(
                BINARY_CONTENT_REJECTION_TEXT.to_string(),
            ))
        }
        fn delete(&self, _: &str) -> Result<(), SecureStorageError> {
            Err(SecureStorageError::Other(
                BINARY_CONTENT_REJECTION_TEXT.to_string(),
            ))
        }
    }

    /// Reproduces t-0155: a backend that deterministically rejects binary writes
    /// (strict UTF-8 validation on a hardcoded `text/plain` content_type) must not
    /// be treated as a generic "system storage unavailable" outage — it must fall
    /// back to the file key store for this run, since every retry would fail
    /// identically.
    #[test]
    fn deterministic_binary_content_rejection_falls_back_to_file_store() {
        let temporary = tempfile::tempdir().unwrap();
        let fallback = Arc::new(
            crate::file_secure_storage::FileSecureStorage::with_base_dir(
                temporary.path().join("keys"),
            ),
        );
        let storage = ProbedSecureStorage::with_file_fallback(
            Arc::new(BinaryContentRejectingStorage),
            Some(Arc::clone(&fallback) as Arc<dyn SecureStorageProvider>),
        );

        storage.set("key", b"value").expect(
            "a deterministic binary-content rejection must fall back to the file \
             key store instead of reporting the whole system store unavailable",
        );
        assert_eq!(storage.get("key").unwrap(), Some(b"value".to_vec()));
        // The write actually landed in the fallback, not in some third place.
        assert_eq!(fallback.get("key").unwrap(), Some(b"value".to_vec()));
    }

    /// A genuine outage (locked keyring, AppArmor denial, etc.) must still fail
    /// closed even when a file fallback is configured — only the specific binary-
    /// content rejection may use it.
    #[test]
    fn genuine_unavailability_does_not_use_the_file_fallback() {
        let temporary = tempfile::tempdir().unwrap();
        let fallback = Arc::new(
            crate::file_secure_storage::FileSecureStorage::with_base_dir(
                temporary.path().join("keys"),
            ),
        );
        let storage = ProbedSecureStorage::with_file_fallback(
            Arc::new(DeniedStorage::default()),
            Some(fallback as Arc<dyn SecureStorageProvider>),
        );

        assert!(matches!(
            storage.get("key"),
            Err(SecureStorageError::Unavailable(_))
        ));
    }

    #[test]
    fn successful_probe_is_reused() {
        let temporary = tempfile::tempdir().unwrap();
        let inner = Arc::new(
            crate::file_secure_storage::FileSecureStorage::with_base_dir(
                temporary.path().join("keys"),
            ),
        );
        let storage = ProbedSecureStorage::with_file_fallback(inner, None);
        storage.set("key", b"value").unwrap();
        assert!(matches!(
            storage.outcome.get().unwrap(),
            ProbeOutcome::Ready
        ));
        assert_eq!(storage.get("key").unwrap(), Some(b"value".to_vec()));
        storage.delete("key").unwrap();
        assert_eq!(storage.get("key").unwrap(), None);
    }
}

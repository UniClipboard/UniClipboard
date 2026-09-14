//! Read-through compatibility for explicitly listed legacy secrets.
//! Reads never migrate or delete files, so backup inventory remains read-only.
//! Explicit writes go to primary; explicit deletion removes both copies.

use std::sync::Arc;

use crate::ports::{SecureStorageError, SecureStorageProvider};

pub struct FallbackSecureStorage {
    primary: Arc<dyn SecureStorageProvider>,
    legacy: Arc<dyn SecureStorageProvider>,
    legacy_keys: Vec<String>,
}

impl FallbackSecureStorage {
    pub fn new(
        primary: Arc<dyn SecureStorageProvider>,
        legacy: Arc<dyn SecureStorageProvider>,
        legacy_keys: Vec<String>,
    ) -> Self {
        Self {
            primary,
            legacy,
            legacy_keys,
        }
    }

    fn has_legacy_key(&self, key: &str) -> bool {
        self.legacy_keys.iter().any(|candidate| candidate == key)
    }
}

impl SecureStorageProvider for FallbackSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        match self.primary.get(key)? {
            Some(value) => Ok(Some(value)),
            None if self.has_legacy_key(key) => self.legacy.get(key),
            None => Ok(None),
        }
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        self.primary.set(key, value)
    }

    fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
        // Remove fallback first; a failure must not reveal an old value by
        // deleting primary and then failing to remove legacy.
        if self.has_legacy_key(key) {
            self.legacy.delete(key)?;
        }
        self.primary.delete(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_secure_storage::FileSecureStorage;

    #[test]
    fn legacy_reads_do_not_mutate_either_store_and_deletion_cannot_resurrect_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let primary = Arc::new(FileSecureStorage::with_base_dir(
            temporary.path().join("primary"),
        ));
        let legacy = Arc::new(FileSecureStorage::with_base_dir(
            temporary.path().join("legacy"),
        ));
        legacy.set("identity", b"old").unwrap();
        let store =
            FallbackSecureStorage::new(primary.clone(), legacy.clone(), vec!["identity".into()]);
        for _ in 0..2 {
            assert_eq!(store.get("identity").unwrap(), Some(b"old".to_vec()));
            assert_eq!(primary.get("identity").unwrap(), None);
            assert_eq!(legacy.get("identity").unwrap(), Some(b"old".to_vec()));
        }
        assert!(!temporary.path().join("primary").exists());
        store.set("identity", b"new").unwrap();
        assert_eq!(store.get("identity").unwrap(), Some(b"new".to_vec()));
        store.delete("identity").unwrap();
        assert_eq!(store.get("identity").unwrap(), None);
        assert_eq!(legacy.get("identity").unwrap(), None);
    }

    #[test]
    fn unlisted_keys_do_not_use_legacy() {
        let temporary = tempfile::tempdir().unwrap();
        let primary = Arc::new(FileSecureStorage::with_base_dir(
            temporary.path().join("primary"),
        ));
        let legacy = Arc::new(FileSecureStorage::with_base_dir(
            temporary.path().join("legacy"),
        ));
        legacy.set("other", b"old").unwrap();
        let store = FallbackSecureStorage::new(primary, legacy.clone(), vec!["identity".into()]);
        assert_eq!(store.get("other").unwrap(), None);
        store.delete("other").unwrap();
        assert_eq!(legacy.get("other").unwrap(), Some(b"old".to_vec()));
    }

    struct DeniedStorage;

    impl SecureStorageProvider for DeniedStorage {
        fn get(&self, _: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Err(SecureStorageError::PermissionDenied("test".into()))
        }
        fn set(&self, _: &str, _: &[u8]) -> Result<(), SecureStorageError> {
            Err(SecureStorageError::PermissionDenied("test".into()))
        }
        fn delete(&self, _: &str) -> Result<(), SecureStorageError> {
            Err(SecureStorageError::PermissionDenied("test".into()))
        }
    }

    #[test]
    fn inaccessible_primary_is_not_treated_as_an_empty_store() {
        let temporary = tempfile::tempdir().unwrap();
        let legacy = Arc::new(FileSecureStorage::with_base_dir(
            temporary.path().join("legacy"),
        ));
        legacy.set("identity", b"old").unwrap();
        let store =
            FallbackSecureStorage::new(Arc::new(DeniedStorage), legacy, vec!["identity".into()]);
        assert!(matches!(
            store.get("identity"),
            Err(SecureStorageError::PermissionDenied(_))
        ));
    }

    #[test]
    fn failed_legacy_deletion_keeps_primary() {
        let temporary = tempfile::tempdir().unwrap();
        let primary = Arc::new(FileSecureStorage::with_base_dir(
            temporary.path().join("primary"),
        ));
        primary.set("identity", b"current").unwrap();
        let store = FallbackSecureStorage::new(
            primary.clone(),
            Arc::new(DeniedStorage),
            vec!["identity".into()],
        );
        assert!(store.delete("identity").is_err());
        assert_eq!(primary.get("identity").unwrap(), Some(b"current".to_vec()));
    }
}

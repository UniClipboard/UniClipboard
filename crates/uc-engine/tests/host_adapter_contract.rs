use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use uc_engine::{HostCapabilityError, HostSecureStorage};

#[derive(Default)]
struct MemoryHostSecureStorage {
    values: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryHostSecureStorage {
    fn values(&self) -> MutexGuard<'_, HashMap<String, Vec<u8>>> {
        match self.values.lock() {
            Ok(values) => values,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl HostSecureStorage for MemoryHostSecureStorage {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        Ok(self.values().get(key).cloned())
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), HostCapabilityError> {
        self.values().insert(key.to_owned(), value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), HostCapabilityError> {
        self.values().remove(key);
        Ok(())
    }
}

#[test]
fn secure_storage_adapter_preserves_secret_bytes() {
    let storage = uc_engine::internal::host_adapters::adapt_secure_storage(Box::new(
        MemoryHostSecureStorage::default(),
    ));
    let secret = [0, 1, 2, 127, 128, 255];

    storage.set("identity", &secret).unwrap();
    assert_eq!(
        storage.get("identity").unwrap().as_deref(),
        Some(&secret[..])
    );
    storage.delete("identity").unwrap();
    assert!(storage.get("identity").unwrap().is_none());
}

struct FailingHostSecureStorage {
    category: uc_engine::HostCapabilityErrorCategory,
}

impl HostSecureStorage for FailingHostSecureStorage {
    fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, HostCapabilityError> {
        Err(HostCapabilityError::new(self.category, "private detail"))
    }

    fn set(&self, _key: &str, _value: &[u8]) -> Result<(), HostCapabilityError> {
        Err(HostCapabilityError::new(self.category, "private detail"))
    }

    fn delete(&self, _key: &str) -> Result<(), HostCapabilityError> {
        Err(HostCapabilityError::new(self.category, "private detail"))
    }
}

#[test]
fn secure_storage_adapter_preserves_stable_error_categories() {
    use uc_core::ports::SecureStorageError;
    use uc_engine::HostCapabilityErrorCategory;

    let unavailable = uc_engine::internal::host_adapters::adapt_secure_storage(Box::new(
        FailingHostSecureStorage {
            category: HostCapabilityErrorCategory::Unavailable,
        },
    ));
    let denied = uc_engine::internal::host_adapters::adapt_secure_storage(Box::new(
        FailingHostSecureStorage {
            category: HostCapabilityErrorCategory::PermissionDenied,
        },
    ));

    assert!(matches!(
        unavailable.get("identity"),
        Err(SecureStorageError::Unavailable(_))
    ));
    assert!(matches!(
        denied.set("identity", b"secret"),
        Err(SecureStorageError::PermissionDenied(_))
    ));
}

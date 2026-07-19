use std::sync::Arc;

use uc_core::ports::{SecureStorageError, SecureStoragePort};

use crate::{HostCapabilityError, HostCapabilityErrorCategory, HostSecureStorage};

struct HostSecureStorageAdapter {
    host: Box<dyn HostSecureStorage>,
}

impl SecureStoragePort for HostSecureStorageAdapter {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError> {
        self.host.get(key).map_err(map_secure_storage_error)
    }

    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError> {
        self.host.set(key, value).map_err(map_secure_storage_error)
    }

    fn delete(&self, key: &str) -> Result<(), SecureStorageError> {
        self.host.delete(key).map_err(map_secure_storage_error)
    }
}

fn map_secure_storage_error(error: HostCapabilityError) -> SecureStorageError {
    let message = error.to_string();
    match error.category() {
        HostCapabilityErrorCategory::Unavailable => SecureStorageError::Unavailable(message),
        HostCapabilityErrorCategory::PermissionDenied => {
            SecureStorageError::PermissionDenied(message)
        }
        HostCapabilityErrorCategory::InvalidHandle | HostCapabilityErrorCategory::Io => {
            SecureStorageError::Other(message)
        }
    }
}

pub fn adapt_secure_storage(host: Box<dyn HostSecureStorage>) -> Arc<dyn SecureStoragePort> {
    Arc::new(HostSecureStorageAdapter { host })
}

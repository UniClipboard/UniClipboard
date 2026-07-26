use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecureStorageError {
    #[error("secure storage unavailable: {0}")]
    Unavailable(String),

    #[error("secure storage access denied: {0}")]
    PermissionDenied(String),

    #[error("secure storage data corrupt: {0}")]
    Corrupt(String),

    #[error("secure storage failed: {0}")]
    Other(String),
}

pub trait SecureStorageProvider: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, SecureStorageError>;

    fn set(&self, key: &str, value: &[u8]) -> Result<(), SecureStorageError>;

    fn delete(&self, key: &str) -> Result<(), SecureStorageError>;
}

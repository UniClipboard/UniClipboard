use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdentityStoreError {
    #[error("identity store failed: {0}")]
    Store(String),

    #[error("identity data corrupt: {0}")]
    Corrupt(String),
}

pub trait IdentityStorePort: Send + Sync {
    /// Load the stored identity bytes, if any.
    fn load_identity(&self) -> Result<Option<Vec<u8>>, IdentityStoreError>;

    /// Store identity bytes. Must be idempotent (overwrite if exists).
    fn store_identity(&self, identity: &[u8]) -> Result<(), IdentityStoreError>;
}

#[cfg(test)]
mockall::mock! {
    pub IdentityStore {}

    impl IdentityStorePort for IdentityStore {
        fn load_identity(&self) -> Result<Option<Vec<u8>>, IdentityStoreError>;
        fn store_identity(&self, identity: &[u8]) -> Result<(), IdentityStoreError>;
    }
}

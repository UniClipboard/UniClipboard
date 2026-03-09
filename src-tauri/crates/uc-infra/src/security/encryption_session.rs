use async_trait::async_trait;
use tokio::sync::RwLock;

use uc_core::ports::EncryptionSessionPort;
use uc_core::security::model::{EncryptionError, MasterKey};

pub struct InMemoryEncryptionSession {
    key: RwLock<Option<MasterKey>>,
}

impl InMemoryEncryptionSession {
    pub fn new() -> Self {
        Self {
            key: RwLock::new(None),
        }
    }
}

#[async_trait]
impl EncryptionSessionPort for InMemoryEncryptionSession {
    async fn is_ready(&self) -> bool {
        self.key.read().await.is_some()
    }

    async fn get_master_key(&self) -> Result<MasterKey, EncryptionError> {
        self.key
            .read()
            .await
            .clone()
            .ok_or(EncryptionError::NotInitialized) // 或 KeyUnavailable
    }

    async fn set_master_key(&self, master_key: MasterKey) -> Result<(), EncryptionError> {
        *self.key.write().await = Some(master_key);
        Ok(())
    }

    async fn clear(&self) -> Result<(), EncryptionError> {
        *self.key.write().await = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ports::EncryptionSessionPort;
    use uc_core::security::model::{EncryptionError, MasterKey};

    #[tokio::test]
    async fn new_session_is_not_ready() {
        let session = InMemoryEncryptionSession::new();

        assert!(!session.is_ready().await);
        let err = session
            .get_master_key()
            .await
            .expect_err("expected NotInitialized");
        assert!(matches!(err, EncryptionError::NotInitialized));
    }

    #[tokio::test]
    async fn set_master_key_makes_session_ready_and_gets_value() {
        let session = InMemoryEncryptionSession::new();
        let key = MasterKey([7u8; 32]);

        session
            .set_master_key(key.clone())
            .await
            .expect("set master key");

        assert!(session.is_ready().await);
        let stored = session.get_master_key().await.expect("get master key");
        assert_eq!(stored, key);
    }

    #[tokio::test]
    async fn clear_resets_session_state() {
        let session = InMemoryEncryptionSession::new();
        let key = MasterKey([1u8; 32]);

        session.set_master_key(key).await.expect("set master key");
        session.clear().await.expect("clear");

        assert!(!session.is_ready().await);
        let err = session
            .get_master_key()
            .await
            .expect_err("expected NotInitialized");
        assert!(matches!(err, EncryptionError::NotInitialized));
    }

    #[tokio::test]
    async fn set_master_key_overwrites_existing() {
        let session = InMemoryEncryptionSession::new();
        let key_a = MasterKey([1u8; 32]);
        let key_b = MasterKey([2u8; 32]);

        session
            .set_master_key(key_a.clone())
            .await
            .expect("set key a");
        session
            .set_master_key(key_b.clone())
            .await
            .expect("set key b");

        let stored = session.get_master_key().await.expect("get master key");
        assert_eq!(stored, key_b);
    }
}

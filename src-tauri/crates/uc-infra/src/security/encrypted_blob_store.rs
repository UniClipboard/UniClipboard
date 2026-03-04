//! Encrypted blob store decorator.
//!
//! Wraps an inner BlobStorePort and transparently encrypts/decrypts
//! blob data using the session's MasterKey.

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::debug;

use uc_core::{
    ports::{BlobStorePort, EncryptionPort, EncryptionSessionPort},
    security::aad,
    security::model::EncryptionAlgo,
    BlobId,
};

/// Decorator that encrypts/decrypts blob data transparently.
pub struct EncryptedBlobStore {
    inner: Arc<dyn BlobStorePort>,
    encryption: Arc<dyn EncryptionPort>,
    session: Arc<dyn EncryptionSessionPort>,
}

impl EncryptedBlobStore {
    pub fn new(
        inner: Arc<dyn BlobStorePort>,
        encryption: Arc<dyn EncryptionPort>,
        session: Arc<dyn EncryptionSessionPort>,
    ) -> Self {
        Self {
            inner,
            encryption,
            session,
        }
    }
}

#[async_trait]
impl BlobStorePort for EncryptedBlobStore {
    async fn put(&self, blob_id: &BlobId, data: &[u8]) -> Result<(PathBuf, Option<i64>)> {
        // 1. Get master key from session
        let master_key = self
            .session
            .get_master_key()
            .await
            .context("encryption session not ready - cannot encrypt blob")?;

        // 2. Encrypt the data
        let aad = aad::for_blob(blob_id);
        let encrypted_blob = self
            .encryption
            .encrypt_blob(&master_key, data, &aad, EncryptionAlgo::XChaCha20Poly1305)
            .await
            .context("failed to encrypt blob data")?;

        // 3. Serialize encrypted blob to bytes
        let encrypted_bytes =
            serde_json::to_vec(&encrypted_blob).context("failed to serialize encrypted blob")?;

        debug!(
            "Encrypted blob {} ({} bytes plaintext -> {} bytes ciphertext)",
            blob_id.as_ref(),
            data.len(),
            encrypted_bytes.len()
        );

        // 4. Store encrypted bytes
        // Temporarily return (path, None) until Plan 02 implements binary format
        let (path, _) = self.inner.put(blob_id, &encrypted_bytes).await?;
        Ok((path, None))
    }

    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
        // 1. Get encrypted bytes from inner store
        let encrypted_bytes = self
            .inner
            .get(blob_id)
            .await
            .context("failed to read encrypted blob from storage")?;

        // 2. Deserialize encrypted blob
        let encrypted_blob: uc_core::security::model::EncryptedBlob = serde_json::from_slice(
            &encrypted_bytes,
        )
        .context("failed to deserialize encrypted blob - data may be corrupted or unencrypted")?;

        // 3. Get master key from session
        let master_key = self
            .session
            .get_master_key()
            .await
            .context("encryption session not ready - cannot decrypt blob")?;

        // 4. Decrypt the data
        let aad = aad::for_blob(blob_id);
        let plaintext = self
            .encryption
            .decrypt_blob(&master_key, &encrypted_blob, &aad)
            .await
            .context("failed to decrypt blob - key mismatch or data corrupted")?;

        debug!(
            "Decrypted blob {} ({} bytes)",
            blob_id.as_ref(),
            plaintext.len()
        );

        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use uc_core::{
        security::aad,
        security::model::{EncryptedBlob, EncryptionFormatVersion, MasterKey},
        BlobId,
    };

    /// Mock BlobStorePort that stores encrypted bytes in memory
    struct MockBlobStore {
        storage: Arc<Mutex<std::collections::HashMap<BlobId, Vec<u8>>>>,
    }

    impl MockBlobStore {
        fn new() -> Self {
            Self {
                storage: Arc::new(Mutex::new(std::collections::HashMap::new())),
            }
        }

        fn get_stored(&self, blob_id: &BlobId) -> Option<Vec<u8>> {
            self.storage.lock().unwrap().get(blob_id).cloned()
        }
    }

    #[async_trait]
    impl BlobStorePort for MockBlobStore {
        async fn put(&self, blob_id: &BlobId, data: &[u8]) -> Result<(PathBuf, Option<i64>)> {
            self.storage
                .lock()
                .unwrap()
                .insert(blob_id.clone(), data.to_vec());
            Ok((
                PathBuf::from(format!("/fake/path/{}", blob_id.as_ref())),
                None,
            ))
        }

        async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
            self.storage
                .lock()
                .unwrap()
                .get(blob_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("blob not found"))
        }
    }

    /// Mock EncryptionPort
    struct MockEncryption {
        should_fail_encrypt: bool,
        should_fail_decrypt: bool,
    }

    impl MockEncryption {
        fn new() -> Self {
            Self {
                should_fail_encrypt: false,
                should_fail_decrypt: false,
            }
        }

        fn fail_on_encrypt(mut self) -> Self {
            self.should_fail_encrypt = true;
            self
        }
    }

    #[async_trait]
    impl uc_core::ports::EncryptionPort for MockEncryption {
        async fn derive_kek(
            &self,
            _passphrase: &uc_core::security::model::Passphrase,
            _salt: &[u8],
            _kdf_params: &uc_core::security::model::KdfParams,
        ) -> Result<uc_core::security::model::Kek, uc_core::security::model::EncryptionError>
        {
            Ok(uc_core::security::model::Kek([0u8; 32]))
        }

        async fn wrap_master_key(
            &self,
            _kek: &uc_core::security::model::Kek,
            _master_key: &MasterKey,
            _aead: uc_core::security::model::EncryptionAlgo,
        ) -> Result<EncryptedBlob, uc_core::security::model::EncryptionError> {
            Ok(EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: uc_core::security::model::EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![0u8; 24],
                ciphertext: vec![0u8; 32],
                aad_fingerprint: None,
            })
        }

        async fn unwrap_master_key(
            &self,
            _kek: &uc_core::security::model::Kek,
            _blob: &EncryptedBlob,
        ) -> Result<MasterKey, uc_core::security::model::EncryptionError> {
            MasterKey::from_bytes(&[0u8; 32])
        }

        async fn encrypt_blob(
            &self,
            _master_key: &MasterKey,
            plaintext: &[u8],
            _aad: &[u8],
            _algo: uc_core::security::model::EncryptionAlgo,
        ) -> Result<EncryptedBlob, uc_core::security::model::EncryptionError> {
            if self.should_fail_encrypt {
                return Err(uc_core::security::model::EncryptionError::EncryptFailed);
            }
            // Return a deterministic encrypted blob
            Ok(EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: uc_core::security::model::EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![0u8; 24],
                ciphertext: plaintext.to_vec(),
                aad_fingerprint: None,
            })
        }

        async fn decrypt_blob(
            &self,
            _master_key: &MasterKey,
            blob: &EncryptedBlob,
            _aad: &[u8],
        ) -> Result<Vec<u8>, uc_core::security::model::EncryptionError> {
            if self.should_fail_decrypt {
                return Err(uc_core::security::model::EncryptionError::CorruptedBlob);
            }
            Ok(blob.ciphertext.clone())
        }
    }

    /// Mock EncryptionSessionPort
    struct MockEncryptionSession {
        master_key: Option<MasterKey>,
    }

    impl MockEncryptionSession {
        fn new() -> Self {
            Self { master_key: None }
        }

        fn with_master_key(mut self, key: MasterKey) -> Self {
            self.master_key = Some(key);
            self
        }
    }

    #[async_trait]
    impl EncryptionSessionPort for MockEncryptionSession {
        async fn is_ready(&self) -> bool {
            self.master_key.is_some()
        }

        async fn get_master_key(
            &self,
        ) -> Result<MasterKey, uc_core::security::model::EncryptionError> {
            self.master_key
                .clone()
                .ok_or(uc_core::security::model::EncryptionError::Locked)
        }

        async fn set_master_key(
            &self,
            _master_key: MasterKey,
        ) -> Result<(), uc_core::security::model::EncryptionError> {
            Ok(())
        }

        async fn clear(&self) -> Result<(), uc_core::security::model::EncryptionError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_encrypted_store_encrypts_on_put() {
        // Test that data is encrypted before being stored
        let inner = Arc::new(MockBlobStore::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let store = EncryptedBlobStore::new(inner.clone(), encryption, session);

        let blob_id = BlobId::from("test-blob");
        let data = b"test plaintext data";

        let result = store.put(&blob_id, data).await;

        assert!(result.is_ok(), "put should succeed");

        // Verify the stored data is an encrypted blob (JSON serializable)
        let stored_data = inner.get_stored(&blob_id).expect("blob should be stored");
        let encrypted_blob: EncryptedBlob = serde_json::from_slice(&stored_data)
            .expect("stored data should be a valid encrypted blob");

        assert_eq!(encrypted_blob.version, EncryptionFormatVersion::V1);
        assert_eq!(
            encrypted_blob.aead,
            uc_core::security::model::EncryptionAlgo::XChaCha20Poly1305
        );
        assert_eq!(encrypted_blob.ciphertext, data.to_vec());
    }

    #[tokio::test]
    async fn test_encrypted_store_decrypts_on_get() {
        // Test that data is decrypted when retrieved
        let inner = Arc::new(MockBlobStore::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let store = EncryptedBlobStore::new(inner.clone(), encryption, session);

        let blob_id = BlobId::from("test-blob");
        let data = b"test plaintext data";

        // First put the data
        store.put(&blob_id, data).await.unwrap();

        // Then get it back
        let retrieved = store.get(&blob_id).await;

        assert!(retrieved.is_ok(), "get should succeed");
        assert_eq!(
            retrieved.unwrap(),
            data.to_vec(),
            "retrieved data should match original"
        );
    }

    #[tokio::test]
    async fn test_encrypted_store_fails_put_when_session_not_ready() {
        // Test that put fails when encryption session is not ready
        let inner = Arc::new(MockBlobStore::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new()); // No master key

        let store = EncryptedBlobStore::new(inner, encryption, session);

        let blob_id = BlobId::from("test-blob");
        let data = b"test data";

        let result = store.put(&blob_id, data).await;

        assert!(result.is_err(), "put should fail when session not ready");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("encryption session not ready"),
            "error should indicate session not ready: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_encrypted_store_fails_get_when_session_not_ready() {
        // Test that get fails when encryption session is not ready
        // First put some data (with a valid session)
        let inner = Arc::new(MockBlobStore::new());
        let encryption_good = Arc::new(MockEncryption::new());
        let session_good = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let store_good = EncryptedBlobStore::new(inner.clone(), encryption_good, session_good);

        let blob_id = BlobId::from("test-blob");
        let data = b"test data";

        store_good.put(&blob_id, data).await.unwrap();

        // Now try to get with a session that's not ready
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new()); // No master key

        let store = EncryptedBlobStore::new(inner, encryption, session);

        let result = store.get(&blob_id).await;

        assert!(result.is_err(), "get should fail when session not ready");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("encryption session not ready"),
            "error should indicate session not ready: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_encrypted_store_propagates_encrypt_errors() {
        // Test that encryption errors are propagated
        let inner = Arc::new(MockBlobStore::new());
        let encryption = Arc::new(MockEncryption::new().fail_on_encrypt());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let store = EncryptedBlobStore::new(inner, encryption, session);

        let blob_id = BlobId::from("test-blob");
        let data = b"test data";

        let result = store.put(&blob_id, data).await;

        assert!(result.is_err(), "put should fail when encryption fails");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("failed to encrypt blob data"),
            "error should indicate encryption failure: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_aad_generation_includes_blob_id() {
        // Test that AAD generation includes the blob ID
        let blob_id1 = BlobId::from("blob-1");
        let blob_id2 = BlobId::from("blob-2");

        let aad1 = aad::for_blob(&blob_id1);
        let aad2 = aad::for_blob(&blob_id2);

        assert_ne!(aad1, aad2, "AAD should differ for different blob IDs");

        // Same blob ID should produce same AAD
        let aad1_again = aad::for_blob(&blob_id1);
        assert_eq!(
            aad1, aad1_again,
            "AAD should be deterministic for same blob ID"
        );

        // AAD should contain blob ID
        let aad_str = String::from_utf8(aad1).unwrap();
        assert!(aad_str.contains("blob-1"), "AAD should contain blob ID");
    }
}

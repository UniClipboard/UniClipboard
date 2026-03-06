//! Decrypting clipboard event repository decorator.
//!
//! Wraps ClipboardEventRepositoryPort and decrypts ObservedClipboardRepresentation.bytes on read.

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::debug;

use uc_core::{
    clipboard::ObservedClipboardRepresentation,
    ids::{EventId, RepresentationId},
    ports::{ClipboardEventRepositoryPort, EncryptionPort, EncryptionSessionPort},
    security::aad,
    security::model::EncryptedBlob,
};

/// Decorator that decrypts ObservedClipboardRepresentation.bytes on read.
pub struct DecryptingClipboardEventRepository {
    inner: Arc<dyn ClipboardEventRepositoryPort>,
    encryption: Arc<dyn EncryptionPort>,
    session: Arc<dyn EncryptionSessionPort>,
}

impl DecryptingClipboardEventRepository {
    pub fn new(
        inner: Arc<dyn ClipboardEventRepositoryPort>,
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
impl ClipboardEventRepositoryPort for DecryptingClipboardEventRepository {
    async fn get_representation(
        &self,
        event_id: &EventId,
        representation_id: &str,
    ) -> Result<ObservedClipboardRepresentation> {
        // Get from inner
        let mut observed = self
            .inner
            .get_representation(event_id, representation_id)
            .await?;

        // Decrypt bytes if present
        if !observed.bytes.is_empty() {
            // Try to deserialize as encrypted blob
            match serde_json::from_slice::<EncryptedBlob>(&observed.bytes) {
                Ok(encrypted_blob) => {
                    // Get master key
                    let master_key = self
                        .session
                        .get_master_key()
                        .await
                        .context("encryption session not ready - cannot decrypt")?;

                    // Decrypt
                    let aad = aad::for_inline(event_id, &RepresentationId::from(representation_id));
                    let plaintext = self
                        .encryption
                        .decrypt_blob(&master_key, &encrypted_blob, &aad)
                        .await
                        .context("failed to decrypt representation bytes")?;

                    debug!(
                        "Decrypted representation bytes for {} ({} bytes)",
                        representation_id,
                        plaintext.len()
                    );

                    observed.bytes = plaintext;
                }
                Err(_) => {
                    // Not encrypted blob format - this could be:
                    // 1. Old unencrypted data (hard fail as per spec)
                    // 2. Corrupted data
                    anyhow::bail!(
                        "representation {} bytes are not in encrypted format - \
                         data may be from before encryption was enabled or corrupted",
                        representation_id
                    );
                }
            }
        }

        Ok(observed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use uc_core::{
        clipboard::ObservedClipboardRepresentation,
        ids::{EventId, RepresentationId},
        security::aad,
        security::model::{EncryptedBlob, EncryptionAlgo, EncryptionFormatVersion, MasterKey},
    };

    /// Mock ClipboardEventRepositoryPort
    struct MockEventRepo {
        storage: Arc<
            Mutex<std::collections::HashMap<(EventId, String), ObservedClipboardRepresentation>>,
        >,
    }

    impl MockEventRepo {
        fn new() -> Self {
            Self {
                storage: Arc::new(Mutex::new(std::collections::HashMap::new())),
            }
        }

        fn store(&self, event_id: &EventId, rep_id: &str, rep: ObservedClipboardRepresentation) {
            self.storage
                .lock()
                .unwrap()
                .insert((event_id.clone(), rep_id.to_string()), rep);
        }
    }

    #[async_trait]
    impl ClipboardEventRepositoryPort for MockEventRepo {
        async fn get_representation(
            &self,
            event_id: &EventId,
            representation_id: &str,
        ) -> Result<ObservedClipboardRepresentation> {
            self.storage
                .lock()
                .unwrap()
                .get(&(event_id.clone(), representation_id.to_string()))
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("representation not found"))
        }
    }

    /// Mock EncryptionPort
    struct MockEncryption {
        should_fail_decrypt: bool,
    }

    impl MockEncryption {
        fn new() -> Self {
            Self {
                should_fail_decrypt: false,
            }
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
            _aead: EncryptionAlgo,
        ) -> Result<EncryptedBlob, uc_core::security::model::EncryptionError> {
            Ok(EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
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
            _plaintext: &[u8],
            _aad: &[u8],
            _algo: EncryptionAlgo,
        ) -> Result<EncryptedBlob, uc_core::security::model::EncryptionError> {
            Ok(EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![0u8; 24],
                ciphertext: vec![],
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

    /// Creates an encrypted representation for testing
    fn create_encrypted_observed_representation(
        plaintext: &[u8],
    ) -> ObservedClipboardRepresentation {
        let encrypted_blob = EncryptedBlob {
            version: EncryptionFormatVersion::V1,
            aead: EncryptionAlgo::XChaCha20Poly1305,
            nonce: vec![0u8; 24],
            ciphertext: plaintext.to_vec(),
            aad_fingerprint: None,
        };
        let encrypted_bytes = serde_json::to_vec(&encrypted_blob).unwrap();

        ObservedClipboardRepresentation::new(
            uc_core::ids::RepresentationId::from("test-rep"),
            uc_core::ids::FormatId::from("public.utf8-plain-text"),
            Some(uc_core::clipboard::MimeType("text/plain".to_string())),
            encrypted_bytes,
        )
    }

    /// Creates an unencrypted representation for testing
    fn create_unencrypted_observed_representation(
        plaintext: &[u8],
    ) -> ObservedClipboardRepresentation {
        ObservedClipboardRepresentation::new(
            uc_core::ids::RepresentationId::from("test-rep"),
            uc_core::ids::FormatId::from("public.utf8-plain-text"),
            Some(uc_core::clipboard::MimeType("text/plain".to_string())),
            plaintext.to_vec(),
        )
    }

    #[tokio::test]
    async fn test_decrypting_repo_decrypts_bytes() {
        // Test that bytes are decrypted when retrieved
        let inner = Arc::new(MockEventRepo::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let repo = DecryptingClipboardEventRepository::new(inner.clone(), encryption, session);

        let event_id = EventId::new();
        let rep_id = "test-rep";
        let plaintext = b"test plaintext data";

        // Store an encrypted representation
        inner.store(
            &event_id,
            rep_id,
            create_encrypted_observed_representation(plaintext),
        );

        // Retrieve it - should be decrypted
        let result = repo.get_representation(&event_id, rep_id).await;

        assert!(result.is_ok(), "get_representation should succeed");
        let observed = result.unwrap();
        assert_eq!(
            observed.bytes,
            plaintext.to_vec(),
            "bytes should be decrypted"
        );
    }

    #[tokio::test]
    async fn test_decrypting_repo_fails_for_unencrypted_data() {
        // Test that unencrypted data causes an error
        let inner = Arc::new(MockEventRepo::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let repo = DecryptingClipboardEventRepository::new(inner.clone(), encryption, session);

        let event_id = EventId::new();
        let rep_id = "test-rep";
        let plaintext = b"test data";

        // Store an unencrypted representation
        inner.store(
            &event_id,
            rep_id,
            create_unencrypted_observed_representation(plaintext),
        );

        // Try to retrieve it - should fail
        let result = repo.get_representation(&event_id, rep_id).await;

        assert!(
            result.is_err(),
            "get_representation should fail for unencrypted data"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not in encrypted format"),
            "error should indicate data is not encrypted: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_decrypting_repo_fails_when_session_not_ready() {
        // Test that an error is returned when the encryption session is not ready
        let inner = Arc::new(MockEventRepo::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new()); // No master key

        let repo = DecryptingClipboardEventRepository::new(inner.clone(), encryption, session);

        let event_id = EventId::new();
        let rep_id = "test-rep";
        let plaintext = b"test data";

        // Store an encrypted representation
        inner.store(
            &event_id,
            rep_id,
            create_encrypted_observed_representation(plaintext),
        );

        // Try to retrieve it - should fail
        let result = repo.get_representation(&event_id, rep_id).await;

        assert!(
            result.is_err(),
            "get_representation should fail when session not ready"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("encryption session not ready"),
            "error should indicate session not ready: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_aad_generation_is_deterministic() {
        // Test that AAD generation is deterministic for same event and rep
        let event_id = EventId::from("test-event-id");
        let rep_id = RepresentationId::from("test-rep-id");

        let aad1 = aad::for_inline(&event_id, &rep_id);
        let aad2 = aad::for_inline(&event_id, &rep_id);

        assert_eq!(aad1, aad2, "AAD should be deterministic for same inputs");

        // Different event ID should produce different AAD
        let different_event_id = EventId::from("different-event-id");
        let aad3 = aad::for_inline(&different_event_id, &rep_id);
        assert_ne!(aad1, aad3, "AAD should differ for different event IDs");
    }
}

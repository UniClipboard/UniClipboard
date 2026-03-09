//! Decrypting clipboard representation repository decorator.
//!
//! Wraps ClipboardRepresentationRepositoryPort and decrypts inline_data on read.

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::debug;

use uc_core::ports::clipboard::ProcessingUpdateOutcome;
use uc_core::{
    clipboard::{PayloadAvailability, PersistedClipboardRepresentation},
    ids::{EventId, RepresentationId},
    ports::{ClipboardRepresentationRepositoryPort, EncryptionPort, EncryptionSessionPort},
    security::aad,
    security::model::EncryptedBlob,
    BlobId,
};

/// Decorator that decrypts representation inline_data on read.
pub struct DecryptingClipboardRepresentationRepository {
    inner: Arc<dyn ClipboardRepresentationRepositoryPort>,
    encryption: Arc<dyn EncryptionPort>,
    session: Arc<dyn EncryptionSessionPort>,
}

impl DecryptingClipboardRepresentationRepository {
    pub fn new(
        inner: Arc<dyn ClipboardRepresentationRepositoryPort>,
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
impl ClipboardRepresentationRepositoryPort for DecryptingClipboardRepresentationRepository {
    async fn get_representation(
        &self,
        event_id: &EventId,
        representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        // Get from inner
        let rep_opt = self
            .inner
            .get_representation(event_id, representation_id)
            .await?;

        let Some(rep) = rep_opt else {
            return Ok(None);
        };

        // Decrypt inline_data if present
        let decrypted_inline_data = if let Some(ref encrypted_bytes) = rep.inline_data {
            // Deserialize encrypted blob
            let encrypted_blob: EncryptedBlob = serde_json::from_slice(encrypted_bytes)
                .context("failed to deserialize encrypted inline_data - data may be corrupted")?;

            // Get master key
            let master_key = self
                .session
                .get_master_key()
                .await
                .context("encryption session not ready - cannot decrypt")?;

            // Decrypt
            let aad = aad::for_inline(event_id, representation_id);
            let plaintext = self
                .encryption
                .decrypt_blob(&master_key, &encrypted_blob, &aad)
                .await
                .context("failed to decrypt inline_data")?;

            debug!(
                "Decrypted inline_data for rep {} ({} bytes)",
                representation_id.as_ref(),
                plaintext.len()
            );

            Some(plaintext)
        } else {
            None
        };

        // Return representation with decrypted data
        Ok(Some(PersistedClipboardRepresentation::new(
            rep.id,
            rep.format_id,
            rep.mime_type,
            rep.size_bytes,
            decrypted_inline_data,
            rep.blob_id,
        )))
    }

    async fn get_representation_by_id(
        &self,
        representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        let rep_opt = self
            .inner
            .get_representation_by_id(representation_id)
            .await?;

        let Some(rep) = rep_opt else {
            return Ok(None);
        };

        if rep.inline_data.is_some() {
            debug!(
                "Skipping inline_data decryption for rep {}: event_id unavailable",
                representation_id.as_ref()
            );
        }

        Ok(Some(rep))
    }

    async fn get_representation_by_blob_id(
        &self,
        blob_id: &BlobId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        let rep_opt = self.inner.get_representation_by_blob_id(blob_id).await?;

        let Some(rep) = rep_opt else {
            return Ok(None);
        };

        if rep.inline_data.is_some() {
            debug!(
                "Skipping inline_data decryption for blob {}: event_id unavailable",
                blob_id.as_ref()
            );
        }

        Ok(Some(rep))
    }

    async fn update_blob_id(
        &self,
        representation_id: &RepresentationId,
        blob_id: &BlobId,
    ) -> Result<()> {
        // No encryption needed for blob_id update - just delegate
        self.inner.update_blob_id(representation_id, blob_id).await
    }

    async fn update_blob_id_if_none(
        &self,
        representation_id: &RepresentationId,
        blob_id: &BlobId,
    ) -> Result<bool> {
        // No encryption needed for blob_id update - just delegate
        self.inner
            .update_blob_id_if_none(representation_id, blob_id)
            .await
    }

    async fn update_processing_result(
        &self,
        rep_id: &RepresentationId,
        expected_states: &[PayloadAvailability],
        blob_id: Option<&BlobId>,
        new_state: PayloadAvailability,
        last_error: Option<&str>,
    ) -> Result<ProcessingUpdateOutcome> {
        // Delegate to inner repo - this method is for state updates, not data reading
        // The returned representation may contain encrypted inline_data, which is expected
        // for update operations. Use get_representation to get decrypted data.
        self.inner
            .update_processing_result(rep_id, expected_states, blob_id, new_state, last_error)
            .await
    }

    async fn update_mime_type(
        &self,
        rep_id: &RepresentationId,
        mime: &uc_core::clipboard::MimeType,
    ) -> Result<()> {
        self.inner.update_mime_type(rep_id, mime).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use uc_core::{
        clipboard::{MimeType, PersistedClipboardRepresentation},
        ids::{BlobId, EventId, FormatId, RepresentationId},
        security::aad,
        security::model::{EncryptedBlob, EncryptionAlgo, EncryptionFormatVersion, MasterKey},
    };

    /// Mock ClipboardRepresentationRepositoryPort
    struct MockRepresentationRepo {
        storage: Arc<
            Mutex<
                std::collections::HashMap<
                    (EventId, RepresentationId),
                    PersistedClipboardRepresentation,
                >,
            >,
        >,
    }

    impl MockRepresentationRepo {
        fn new() -> Self {
            Self {
                storage: Arc::new(Mutex::new(std::collections::HashMap::new())),
            }
        }

        fn store(&self, event_id: &EventId, rep: PersistedClipboardRepresentation) {
            self.storage
                .lock()
                .unwrap()
                .insert((event_id.clone(), rep.id.clone()), rep);
        }
    }

    #[async_trait]
    impl ClipboardRepresentationRepositoryPort for MockRepresentationRepo {
        async fn get_representation(
            &self,
            event_id: &EventId,
            representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(self
                .storage
                .lock()
                .unwrap()
                .get(&(event_id.clone(), representation_id.clone()))
                .cloned())
        }

        async fn get_representation_by_id(
            &self,
            representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(self
                .storage
                .lock()
                .unwrap()
                .iter()
                .find_map(|((_event_id, rep_id), rep)| {
                    if rep_id == representation_id {
                        Some(rep.clone())
                    } else {
                        None
                    }
                }))
        }

        async fn get_representation_by_blob_id(
            &self,
            _blob_id: &BlobId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn update_blob_id(
            &self,
            representation_id: &RepresentationId,
            blob_id: &BlobId,
        ) -> Result<()> {
            // Update blob_id in all stored representations
            for (_, rep) in self.storage.lock().unwrap().iter_mut() {
                if rep.id == *representation_id {
                    rep.blob_id = Some(blob_id.clone());
                }
            }
            Ok(())
        }

        async fn update_blob_id_if_none(
            &self,
            representation_id: &RepresentationId,
            blob_id: &BlobId,
        ) -> Result<bool> {
            // Update blob_id only if it's None
            let mut updated = false;
            for (_, rep) in self.storage.lock().unwrap().iter_mut() {
                if rep.id == *representation_id {
                    if rep.blob_id.is_none() {
                        rep.blob_id = Some(blob_id.clone());
                        updated = true;
                    }
                }
            }
            Ok(updated)
        }

        async fn update_processing_result(
            &self,
            rep_id: &RepresentationId,
            expected_states: &[PayloadAvailability],
            blob_id: Option<&BlobId>,
            new_state: PayloadAvailability,
            last_error: Option<&str>,
        ) -> Result<ProcessingUpdateOutcome> {
            // Find and update representation
            for ((_, id), rep) in self.storage.lock().unwrap().iter_mut() {
                if id == rep_id {
                    // Check if current state is in expected_states
                    let current_state = rep.payload_state();
                    if !expected_states.contains(&current_state) {
                        return Ok(ProcessingUpdateOutcome::StateMismatch);
                    }

                    // Update fields and return new representation with updated state
                    return Ok(ProcessingUpdateOutcome::Updated(
                        PersistedClipboardRepresentation::new_with_state(
                            rep.id.clone(),
                            rep.format_id.clone(),
                            rep.mime_type.clone(),
                            rep.size_bytes,
                            rep.inline_data.clone(),
                            blob_id.cloned(),
                            new_state,
                            last_error.map(|s| s.to_string()),
                        )?,
                    ));
                }
            }
            Ok(ProcessingUpdateOutcome::NotFound)
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
    fn create_encrypted_representation(
        rep_id: RepresentationId,
        plaintext: &[u8],
    ) -> PersistedClipboardRepresentation {
        let encrypted_blob = EncryptedBlob {
            version: EncryptionFormatVersion::V1,
            aead: EncryptionAlgo::XChaCha20Poly1305,
            nonce: vec![0u8; 24],
            ciphertext: plaintext.to_vec(),
            aad_fingerprint: None,
        };
        let encrypted_bytes = serde_json::to_vec(&encrypted_blob).unwrap();

        PersistedClipboardRepresentation::new(
            rep_id,
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType("text/plain".to_string())),
            plaintext.len() as i64,
            Some(encrypted_bytes),
            None,
        )
    }

    #[tokio::test]
    async fn test_decrypting_repo_decrypts_inline_data() {
        // Test that inline data is decrypted when retrieved
        let inner = Arc::new(MockRepresentationRepo::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let repo =
            DecryptingClipboardRepresentationRepository::new(inner.clone(), encryption, session);

        let event_id = EventId::new();
        let rep_id = RepresentationId::new();
        let plaintext = b"test plaintext data";

        // Store an encrypted representation
        inner.store(
            &event_id,
            create_encrypted_representation(rep_id.clone(), plaintext),
        );

        // Retrieve it - should be decrypted
        let result = repo.get_representation(&event_id, &rep_id).await;

        assert!(result.is_ok(), "get_representation should succeed");
        let rep_opt = result.unwrap();
        assert!(rep_opt.is_some(), "representation should exist");

        let rep = rep_opt.unwrap();
        assert_eq!(
            rep.inline_data,
            Some(plaintext.to_vec()),
            "inline data should be decrypted"
        );
    }

    #[tokio::test]
    async fn test_decrypting_repo_preserves_representation_without_inline_data() {
        // Test that representations without inline data are passed through unchanged
        let inner = Arc::new(MockRepresentationRepo::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let repo =
            DecryptingClipboardRepresentationRepository::new(inner.clone(), encryption, session);

        let event_id = EventId::new();
        let rep_id = RepresentationId::new();

        // Store a representation without inline data
        let rep = PersistedClipboardRepresentation::new(
            rep_id.clone(),
            FormatId::from("public.png"),
            Some(MimeType("image/png".to_string())),
            0,
            None,
            Some(BlobId::from("blob-123")),
        );
        inner.store(&event_id, rep);

        // Retrieve it - should be unchanged
        let result = repo.get_representation(&event_id, &rep_id).await;

        assert!(result.is_ok(), "get_representation should succeed");
        let rep_opt = result.unwrap();
        assert!(rep_opt.is_some(), "representation should exist");

        let retrieved_rep = rep_opt.unwrap();
        assert!(
            retrieved_rep.inline_data.is_none(),
            "inline data should remain None"
        );
        assert_eq!(retrieved_rep.blob_id, Some(BlobId::from("blob-123")));
    }

    #[tokio::test]
    async fn test_decrypting_repo_returns_none_for_missing_representation() {
        // Test that None is returned for non-existent representations
        let inner = Arc::new(MockRepresentationRepo::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let repo = DecryptingClipboardRepresentationRepository::new(inner, encryption, session);

        let event_id = EventId::new();
        let rep_id = RepresentationId::new();

        let result = repo.get_representation(&event_id, &rep_id).await;

        assert!(result.is_ok(), "get_representation should succeed");
        assert!(result.unwrap().is_none(), "representation should not exist");
    }

    #[tokio::test]
    async fn test_decrypting_repo_fails_when_session_not_ready() {
        // Test that an error is returned when the encryption session is not ready
        let inner = Arc::new(MockRepresentationRepo::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new()); // No master key

        let repo =
            DecryptingClipboardRepresentationRepository::new(inner.clone(), encryption, session);

        let event_id = EventId::new();
        let rep_id = RepresentationId::new();
        let plaintext = b"test data";

        // Store an encrypted representation
        inner.store(
            &event_id,
            create_encrypted_representation(rep_id.clone(), plaintext),
        );

        // Try to retrieve it - should fail
        let result = repo.get_representation(&event_id, &rep_id).await;

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
    async fn test_decrypting_repo_delegates_update_blob_id() {
        // Test that update_blob_id is delegated without modification
        let inner = Arc::new(MockRepresentationRepo::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new());

        let repo =
            DecryptingClipboardRepresentationRepository::new(inner.clone(), encryption, session);

        let rep_id = RepresentationId::new();
        let blob_id = BlobId::from("new-blob");

        let result = repo.update_blob_id(&rep_id, &blob_id).await;

        assert!(result.is_ok(), "update_blob_id should succeed");
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

//! Encrypting clipboard event writer decorator.
//!
//! Wraps ClipboardEventWriterPort and encrypts inline_data before storage.

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::debug;

use uc_core::{
    clipboard::{ClipboardEvent, PersistedClipboardRepresentation},
    ids::EventId,
    ports::{ClipboardEventWriterPort, EncryptionPort, EncryptionSessionPort},
    security::aad,
    security::model::EncryptionAlgo,
};

/// Decorator that encrypts representation inline_data before storage.
pub struct EncryptingClipboardEventWriter {
    inner: Arc<dyn ClipboardEventWriterPort>,
    encryption: Arc<dyn EncryptionPort>,
    session: Arc<dyn EncryptionSessionPort>,
}

impl EncryptingClipboardEventWriter {
    pub fn new(
        inner: Arc<dyn ClipboardEventWriterPort>,
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
impl ClipboardEventWriterPort for EncryptingClipboardEventWriter {
    async fn insert_event(
        &self,
        event: &ClipboardEvent,
        representations: &Vec<PersistedClipboardRepresentation>,
    ) -> Result<()> {
        // Get master key from session
        let master_key = self
            .session
            .get_master_key()
            .await
            .context("encryption session not ready - cannot encrypt clipboard data")?;

        // Encrypt inline_data for each representation
        let mut encrypted_reps = Vec::with_capacity(representations.len());

        for rep in representations {
            let encrypted_inline_data = if let Some(ref plaintext) = rep.inline_data {
                // Encrypt the inline data
                let aad = aad::for_inline(&event.event_id, &rep.id);
                let encrypted_blob = self
                    .encryption
                    .encrypt_blob(
                        &master_key,
                        plaintext,
                        &aad,
                        EncryptionAlgo::XChaCha20Poly1305,
                    )
                    .await
                    .context("failed to encrypt inline_data")?;

                // Serialize to bytes
                let encrypted_bytes = serde_json::to_vec(&encrypted_blob)
                    .context("failed to serialize encrypted inline_data")?;

                debug!(
                    "Encrypted inline_data for rep {} ({} bytes -> {} bytes)",
                    rep.id.as_ref(),
                    plaintext.len(),
                    encrypted_bytes.len()
                );

                Some(encrypted_bytes)
            } else {
                None
            };

            // Create new representation with encrypted inline_data
            encrypted_reps.push(PersistedClipboardRepresentation::new(
                rep.id.clone(),
                rep.format_id.clone(),
                rep.mime_type.clone(),
                rep.size_bytes,
                encrypted_inline_data,
                rep.blob_id.clone(),
            ));
        }

        // Delegate to inner with encrypted representations
        self.inner.insert_event(event, &encrypted_reps).await
    }

    async fn delete_event_and_representations(&self, event_id: &EventId) -> Result<()> {
        // Deletion doesn't need encryption - just delegate
        self.inner.delete_event_and_representations(event_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use uc_core::{
        clipboard::{ClipboardEvent, MimeType, PersistedClipboardRepresentation, SnapshotHash},
        ids::{BlobId, DeviceId, EventId, FormatId, RepresentationId},
        security::aad,
        security::model::{EncryptedBlob, EncryptionFormatVersion, MasterKey},
        ContentHash,
    };

    /// Mock ClipboardEventWriterPort that captures inserted representations
    struct MockEventWriter {
        inserted_reps: Arc<Mutex<Vec<PersistedClipboardRepresentation>>>,
        deleted_event_ids: Arc<Mutex<Vec<EventId>>>,
    }

    impl MockEventWriter {
        fn new() -> Self {
            Self {
                inserted_reps: Arc::new(Mutex::new(Vec::new())),
                deleted_event_ids: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn get_inserted_reps(&self) -> Vec<PersistedClipboardRepresentation> {
            self.inserted_reps.lock().unwrap().clone()
        }

        fn get_deleted_event_ids(&self) -> Vec<EventId> {
            self.deleted_event_ids.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ClipboardEventWriterPort for MockEventWriter {
        async fn insert_event(
            &self,
            _event: &ClipboardEvent,
            representations: &Vec<PersistedClipboardRepresentation>,
        ) -> Result<()> {
            self.inserted_reps
                .lock()
                .unwrap()
                .extend(representations.clone());
            Ok(())
        }

        async fn delete_event_and_representations(&self, event_id: &EventId) -> Result<()> {
            self.deleted_event_ids
                .lock()
                .unwrap()
                .push(event_id.clone());
            Ok(())
        }
    }

    /// Mock EncryptionPort
    struct MockEncryption {
        should_fail: bool,
    }

    impl MockEncryption {
        fn new() -> Self {
            Self { should_fail: false }
        }

        fn fail_on_encrypt(mut self) -> Self {
            self.should_fail = true;
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
            if self.should_fail {
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
            _blob: &EncryptedBlob,
            _aad: &[u8],
        ) -> Result<Vec<u8>, uc_core::security::model::EncryptionError> {
            Ok(vec![])
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

    /// Creates a test clipboard event
    fn create_test_event() -> ClipboardEvent {
        let content_hash = ContentHash::from(&[0u8; 32]);
        ClipboardEvent {
            event_id: EventId::new(),
            captured_at_ms: 12345,
            source_device: DeviceId::new("test-device"),
            snapshot_hash: SnapshotHash(content_hash),
        }
    }

    /// Creates a test representation with inline data
    fn create_test_representation_with_inline_data() -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType("text/plain".to_string())),
            16,
            Some(b"test plaintext data".to_vec()),
            None,
        )
    }

    /// Creates a test representation without inline data
    fn create_test_representation_without_inline_data() -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("public.png"),
            Some(MimeType("image/png".to_string())),
            0,
            None,
            Some(BlobId::from("blob-id-123")),
        )
    }

    #[tokio::test]
    async fn test_encrypting_writer_encrypts_inline_data() {
        // Test that inline data is encrypted before being passed to inner writer
        let inner = Arc::new(MockEventWriter::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let writer = EncryptingClipboardEventWriter::new(inner.clone(), encryption, session);

        let event = create_test_event();
        let representations = vec![create_test_representation_with_inline_data()];

        let result = writer.insert_event(&event, &representations).await;

        assert!(result.is_ok(), "insert_event should succeed");

        let inserted_reps = inner.get_inserted_reps();
        assert_eq!(
            inserted_reps.len(),
            1,
            "should have inserted one representation"
        );

        let inserted_rep = &inserted_reps[0];
        assert!(
            inserted_rep.inline_data.is_some(),
            "should have inline data"
        );

        // Verify the inline data is an encrypted blob (serializes to JSON with expected fields)
        let encrypted_bytes = inserted_rep.inline_data.as_ref().unwrap();
        let encrypted_blob: EncryptedBlob = serde_json::from_slice(encrypted_bytes)
            .expect("inline data should be a valid encrypted blob");

        assert_eq!(encrypted_blob.version, EncryptionFormatVersion::V1);
        assert_eq!(
            encrypted_blob.aead,
            uc_core::security::model::EncryptionAlgo::XChaCha20Poly1305
        );
        assert_eq!(encrypted_blob.nonce.len(), 24);
        // Ciphertext should contain the original plaintext
        assert_eq!(encrypted_blob.ciphertext, b"test plaintext data".to_vec());
    }

    #[tokio::test]
    async fn test_encrypting_writer_preserves_representation_without_inline_data() {
        // Test that representations without inline data are passed through unchanged
        let inner = Arc::new(MockEventWriter::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let writer = EncryptingClipboardEventWriter::new(inner.clone(), encryption, session);

        let event = create_test_event();
        let representations = vec![create_test_representation_without_inline_data()];

        let result = writer.insert_event(&event, &representations).await;

        assert!(result.is_ok(), "insert_event should succeed");

        let inserted_reps = inner.get_inserted_reps();
        assert_eq!(
            inserted_reps.len(),
            1,
            "should have inserted one representation"
        );

        let inserted_rep = &inserted_reps[0];
        assert!(
            inserted_rep.inline_data.is_none(),
            "should not have inline data"
        );
        assert_eq!(inserted_rep.blob_id, Some(BlobId::from("blob-id-123")));
    }

    #[tokio::test]
    async fn test_encrypting_writer_handles_multiple_representations() {
        // Test that multiple representations are encrypted correctly
        let inner = Arc::new(MockEventWriter::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let writer = EncryptingClipboardEventWriter::new(inner.clone(), encryption, session);

        let event = create_test_event();
        let representations = vec![
            create_test_representation_with_inline_data(),
            create_test_representation_without_inline_data(),
            create_test_representation_with_inline_data(),
        ];

        let result = writer.insert_event(&event, &representations).await;

        assert!(result.is_ok(), "insert_event should succeed");

        let inserted_reps = inner.get_inserted_reps();
        assert_eq!(
            inserted_reps.len(),
            3,
            "should have inserted three representations"
        );

        // First representation should have encrypted inline data
        assert!(inserted_reps[0].inline_data.is_some());

        // Second representation should have no inline data
        assert!(inserted_reps[1].inline_data.is_none());

        // Third representation should have encrypted inline data
        assert!(inserted_reps[2].inline_data.is_some());
    }

    #[tokio::test]
    async fn test_encrypting_writer_fails_when_session_not_ready() {
        // Test that an error is returned when the encryption session is not ready
        let inner = Arc::new(MockEventWriter::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new()); // No master key

        let writer = EncryptingClipboardEventWriter::new(inner.clone(), encryption, session);

        let event = create_test_event();
        let representations = vec![create_test_representation_with_inline_data()];

        let result = writer.insert_event(&event, &representations).await;

        assert!(
            result.is_err(),
            "insert_event should fail when session not ready"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("encryption session not ready"),
            "error should indicate session not ready: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_encrypting_writer_propagates_encryption_errors() {
        // Test that encryption errors are propagated
        let inner = Arc::new(MockEventWriter::new());
        let encryption = Arc::new(MockEncryption::new().fail_on_encrypt());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );

        let writer = EncryptingClipboardEventWriter::new(inner.clone(), encryption, session);

        let event = create_test_event();
        let representations = vec![create_test_representation_with_inline_data()];

        let result = writer.insert_event(&event, &representations).await;

        assert!(
            result.is_err(),
            "insert_event should fail when encryption fails"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("failed to encrypt inline_data"),
            "error should indicate encryption failure: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_encrypting_writer_delegates_deletion() {
        // Test that deletion is delegated to inner writer without modification
        let inner = Arc::new(MockEventWriter::new());
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(MockEncryptionSession::new());

        let writer = EncryptingClipboardEventWriter::new(inner.clone(), encryption, session);

        let event_id = EventId::new();

        let result = writer.delete_event_and_representations(&event_id).await;

        assert!(
            result.is_ok(),
            "delete_event_and_representations should succeed"
        );

        let deleted_ids = inner.get_deleted_event_ids();
        assert_eq!(deleted_ids.len(), 1, "should have deleted one event");
        assert_eq!(deleted_ids[0], event_id, "should delete the correct event");
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

        // Different rep ID should produce different AAD
        let different_rep_id = RepresentationId::from("different-rep-id");
        let aad4 = aad::for_inline(&event_id, &different_rep_id);
        assert_ne!(
            aad1, aad4,
            "AAD should differ for different representation IDs"
        );
    }
}

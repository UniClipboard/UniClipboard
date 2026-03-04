//! Encrypted blob store decorator.
//!
//! Wraps an inner BlobStorePort and transparently encrypts/decrypts
//! blob data using the session's MasterKey. Uses UCBL binary format
//! with zstd compression for efficient on-disk storage.
//!
//! # Binary Format (V2)
//!
//! ```text
//! [UCBL magic: 4B] [version: 1B] [nonce: 24B] [ciphertext: NB]
//! ```
//!
//! Total header: 29 bytes before ciphertext.

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info_span, Instrument};

use uc_core::{
    ports::{BlobStorePort, EncryptionPort, EncryptionSessionPort},
    security::aad,
    security::model::{EncryptedBlob, EncryptionAlgo, EncryptionFormatVersion},
    BlobId,
};

/// Magic bytes identifying a UniClipboard blob file ("UCBL")
const BLOB_MAGIC: [u8; 4] = [0x55, 0x43, 0x42, 0x4C];
/// Binary format version (v1 of the binary format, not to be confused with AAD v2)
const BLOB_FORMAT_VERSION: u8 = 0x01;
/// Header size: magic(4) + version(1) + nonce(24) = 29 bytes
const BLOB_HEADER_SIZE: usize = 4 + 1 + 24;
/// zstd compression level (3 = default, good speed/ratio balance)
const ZSTD_LEVEL: i32 = 3;
/// Maximum decompressed size to prevent zip bombs (500 MB)
const MAX_DECOMPRESSED_SIZE: usize = 500 * 1024 * 1024;

/// Serializes a nonce and ciphertext into the UCBL binary format.
fn serialize_blob(nonce: &[u8; 24], ciphertext: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(BLOB_HEADER_SIZE + ciphertext.len());
    buf.extend_from_slice(&BLOB_MAGIC);
    buf.push(BLOB_FORMAT_VERSION);
    buf.extend_from_slice(nonce);
    buf.extend_from_slice(ciphertext);
    buf
}

/// Parses the UCBL binary format, extracting nonce and ciphertext.
fn parse_blob(data: &[u8]) -> Result<(&[u8; 24], &[u8])> {
    if data.len() < BLOB_HEADER_SIZE {
        return Err(anyhow::anyhow!(
            "blob file truncated: {} bytes < {} header",
            data.len(),
            BLOB_HEADER_SIZE
        ));
    }
    if data[0..4] != BLOB_MAGIC {
        return Err(anyhow::anyhow!("invalid blob magic bytes"));
    }
    if data[4] != BLOB_FORMAT_VERSION {
        return Err(anyhow::anyhow!(
            "unsupported blob format version: {}",
            data[4]
        ));
    }
    let nonce: &[u8; 24] = data[5..29]
        .try_into()
        .map_err(|_| anyhow::anyhow!("nonce extraction failed"))?;
    Ok((nonce, &data[29..]))
}

/// Decorator that encrypts/decrypts blob data transparently.
///
/// Uses UCBL binary format with zstd compression:
/// - Write: compress -> encrypt -> serialize to binary
/// - Read: parse binary -> decrypt -> decompress
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
        let plaintext_size = data.len();

        // 1. Get master key from session
        let master_key = self
            .session
            .get_master_key()
            .await
            .context("encryption session not ready - cannot encrypt blob")?;

        // 2. Compress plaintext with zstd
        let compressed =
            zstd::bulk::compress(data, ZSTD_LEVEL).context("failed to compress blob data")?;
        let compressed_size = compressed.len();

        // 3. Build AAD with v2 format
        let aad = aad::for_blob_v2(blob_id);

        // 4. Encrypt compressed data
        let encrypted_blob = self
            .encryption
            .encrypt_blob(
                &master_key,
                &compressed,
                &aad,
                EncryptionAlgo::XChaCha20Poly1305,
            )
            .await
            .context("failed to encrypt blob data")?;

        // 5. Extract nonce as [u8; 24]
        let nonce: [u8; 24] = encrypted_blob
            .nonce
            .as_slice()
            .try_into()
            .context("encrypted blob nonce is not 24 bytes")?;

        // 6. Serialize to UCBL binary format
        let binary_data = serialize_blob(&nonce, &encrypted_blob.ciphertext);
        let on_disk_size = binary_data.len() as i64;

        // 7. Write to inner store
        let (path, _) = self
            .inner
            .put(blob_id, &binary_data)
            .instrument(info_span!("inner_blob_put", blob_id = %blob_id.as_ref()))
            .await?;

        debug!(
            blob_id = %blob_id.as_ref(),
            plaintext_size,
            compressed_size,
            on_disk_size,
            "Wrote V2 blob (compress -> encrypt -> UCBL binary)"
        );

        // 8. Return path and on-disk size
        Ok((path, Some(on_disk_size)))
    }

    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
        // 1. Read binary data from inner store
        let binary_data = self
            .inner
            .get(blob_id)
            .instrument(info_span!("inner_blob_get", blob_id = %blob_id.as_ref()))
            .await
            .context("failed to read encrypted blob from storage")?;

        // 2. Parse UCBL binary header
        let (nonce, ciphertext) = parse_blob(&binary_data)?;

        // 3. Reconstruct EncryptedBlob struct
        let encrypted_blob = EncryptedBlob {
            version: EncryptionFormatVersion::V1,
            aead: EncryptionAlgo::XChaCha20Poly1305,
            nonce: nonce.to_vec(),
            ciphertext: ciphertext.to_vec(),
            aad_fingerprint: None,
        };

        // 4. Get master key from session
        let master_key = self
            .session
            .get_master_key()
            .await
            .context("encryption session not ready - cannot decrypt blob")?;

        // 5. Build AAD with v2 format
        let aad = aad::for_blob_v2(blob_id);

        // 6. Decrypt
        let compressed = self
            .encryption
            .decrypt_blob(&master_key, &encrypted_blob, &aad)
            .await
            .context("failed to decrypt blob - key mismatch or data corrupted")?;

        // 7. Decompress
        let plaintext = zstd::bulk::decompress(&compressed, MAX_DECOMPRESSED_SIZE)
            .context("failed to decompress blob data - data may be corrupted")?;

        debug!(
            blob_id = %blob_id.as_ref(),
            on_disk_size = binary_data.len(),
            compressed_size = compressed.len(),
            plaintext_size = plaintext.len(),
            "Read V2 blob (UCBL binary -> decrypt -> decompress)"
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

    /// Mock EncryptionPort that passes data through (plaintext == ciphertext)
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
            // Mock: passthrough plaintext as ciphertext, deterministic nonce
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
            // Mock: passthrough ciphertext as plaintext
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

    // --- Helper to build a ready store ---
    fn make_store(inner: Arc<MockBlobStore>) -> (EncryptedBlobStore, Arc<MockBlobStore>) {
        let encryption = Arc::new(MockEncryption::new());
        let session = Arc::new(
            MockEncryptionSession::new()
                .with_master_key(MasterKey::from_bytes(&[0u8; 32]).unwrap()),
        );
        let store = EncryptedBlobStore::new(inner.clone(), encryption, session);
        (store, inner)
    }

    // ========================================================================
    // Binary format helper tests
    // ========================================================================

    #[test]
    fn test_serialize_parse_roundtrip() {
        let nonce: [u8; 24] = [0xAB; 24];
        let ciphertext = b"hello encrypted world";

        let serialized = serialize_blob(&nonce, ciphertext);
        let (parsed_nonce, parsed_ciphertext) =
            parse_blob(&serialized).expect("parse should succeed");

        assert_eq!(parsed_nonce, &nonce);
        assert_eq!(parsed_ciphertext, ciphertext);
    }

    #[test]
    fn test_parse_rejects_truncated_data() {
        // Less than 29 bytes
        let data = vec![0u8; 10];
        let result = parse_blob(&data);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("truncated"),
            "error should mention truncated: {}",
            err_msg
        );
    }

    #[test]
    fn test_parse_rejects_wrong_magic() {
        let mut data = vec![0u8; 30];
        data[0..4].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]); // wrong magic
        data[4] = BLOB_FORMAT_VERSION;

        let result = parse_blob(&data);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("invalid"),
            "error should mention invalid: {}",
            err_msg
        );
    }

    #[test]
    fn test_parse_rejects_wrong_version() {
        let mut data = vec![0u8; 30];
        data[0..4].copy_from_slice(&BLOB_MAGIC);
        data[4] = 0xFF; // unsupported version

        let result = parse_blob(&data);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unsupported"),
            "error should mention unsupported: {}",
            err_msg
        );
    }

    #[test]
    fn test_serialize_produces_correct_header() {
        let nonce: [u8; 24] = [0x42; 24];
        let ciphertext = b"test";
        let serialized = serialize_blob(&nonce, ciphertext);

        // Check magic bytes
        assert_eq!(&serialized[0..4], &BLOB_MAGIC);
        // Check version
        assert_eq!(serialized[4], BLOB_FORMAT_VERSION);
        // Check nonce
        assert_eq!(&serialized[5..29], &nonce);
        // Check ciphertext
        assert_eq!(&serialized[29..], ciphertext);
        // Check total length
        assert_eq!(serialized.len(), BLOB_HEADER_SIZE + ciphertext.len());
    }

    // ========================================================================
    // EncryptedBlobStore integration tests
    // ========================================================================

    #[tokio::test]
    async fn test_encrypted_store_encrypts_on_put() {
        // Test that data is compressed, encrypted, and stored in UCBL binary format
        let inner = Arc::new(MockBlobStore::new());
        let (store, inner) = make_store(inner);

        let blob_id = BlobId::from("test-blob");
        let data = b"test plaintext data";

        let result = store.put(&blob_id, data).await;
        assert!(result.is_ok(), "put should succeed");

        // Verify the stored data is in UCBL binary format (not JSON)
        let stored_data = inner.get_stored(&blob_id).expect("blob should be stored");

        // Should start with UCBL magic bytes
        assert_eq!(
            &stored_data[0..4],
            &BLOB_MAGIC,
            "stored data should start with UCBL magic"
        );
        assert_eq!(
            stored_data[4], BLOB_FORMAT_VERSION,
            "stored data should have correct version"
        );
        assert!(
            stored_data.len() >= BLOB_HEADER_SIZE,
            "stored data should be at least header size"
        );

        // The ciphertext portion should be zstd-compressed plaintext
        // (since MockEncryption passes through plaintext as ciphertext)
        let (_, ciphertext) = parse_blob(&stored_data).expect("should parse as valid UCBL");
        let decompressed = zstd::bulk::decompress(ciphertext, MAX_DECOMPRESSED_SIZE)
            .expect("ciphertext should be valid zstd");
        assert_eq!(
            decompressed, data,
            "decompressed ciphertext should match original plaintext"
        );
    }

    #[tokio::test]
    async fn test_encrypted_store_decrypts_on_get() {
        // Test that data is decrypted and decompressed when retrieved
        let inner = Arc::new(MockBlobStore::new());
        let (store, _) = make_store(inner);

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
    async fn test_put_returns_compressed_size() {
        let inner = Arc::new(MockBlobStore::new());
        let (store, _) = make_store(inner);

        let blob_id = BlobId::from("test-blob");
        let data = b"test plaintext data for compression size check";

        let (path, compressed_size) = store.put(&blob_id, data).await.unwrap();

        assert!(!path.as_os_str().is_empty(), "path should not be empty");
        assert!(compressed_size.is_some(), "compressed_size should be Some");
        let size = compressed_size.unwrap();
        assert!(
            size > 0,
            "compressed_size should be positive, got: {}",
            size
        );
        // on_disk_size = BLOB_HEADER_SIZE + ciphertext.len()
        assert!(
            size >= BLOB_HEADER_SIZE as i64,
            "on_disk_size should be at least header size"
        );
    }

    #[tokio::test]
    async fn test_roundtrip_with_compression() {
        // Full roundtrip: put(plaintext) -> get() -> should return identical plaintext
        let inner = Arc::new(MockBlobStore::new());
        let (store, _) = make_store(inner);

        let blob_id = BlobId::from("roundtrip-blob");
        // Use a larger payload to ensure compression is meaningful
        let data = "Hello, this is a test of the V2 binary blob format with zstd compression. \
                     It should compress and decompress correctly through the full pipeline."
            .as_bytes();

        store.put(&blob_id, data).await.unwrap();
        let retrieved = store.get(&blob_id).await.unwrap();

        assert_eq!(
            retrieved, data,
            "roundtrip should preserve plaintext exactly"
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
        // First put some data (with a valid session)
        let inner = Arc::new(MockBlobStore::new());
        let (store_good, inner) = make_store(inner);

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
    async fn test_aad_v2_is_used() {
        // Verify that AAD v2 format is used (not v1)
        let blob_id = BlobId::from("aad-test");

        let aad_v2 = aad::for_blob_v2(&blob_id);
        let aad_str = String::from_utf8(aad_v2).unwrap();

        assert!(
            aad_str.starts_with("uc:blob:v2|"),
            "AAD v2 should have v2 prefix, got: {}",
            aad_str
        );
        assert!(aad_str.contains("aad-test"), "AAD should contain blob ID");
    }
}

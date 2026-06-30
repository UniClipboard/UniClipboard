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
//!
//! # Slice 3 改造
//!
//! 不再走 `BlobCipherPort`——本 decorator 的 wire format（UCBL 二进制）与
//! 4 个剪切板 decorator 用的 JSON `EncryptedBlob` 字节布局不兼容,共享
//! 同一个 port 会破坏既有 `.blob` 文件的可读性（V1 数据兼容 ironclad 不变量）。
//!
//! 改用 `super::v1_aead` 私有 helper 直接调底层 AEAD: 算法行为与历史
//! `EncryptionPort::encrypt_blob` 字节级一致,保证既有 UCBL 文件继续可读。

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info_span, Instrument};

use uc_core::{blob::ports::BlobReaderPort, crypto::aad, BlobId, ContentHash};

use super::session::InMemorySession;
use super::v1_aead;
use crate::blob::{BlobStorePort, StoredPathBlob};

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
    session: Arc<InMemorySession>,
}

impl EncryptedBlobStore {
    pub fn new(inner: Arc<dyn BlobStorePort>, session: Arc<InMemorySession>) -> Self {
        Self { inner, session }
    }
}

#[async_trait]
impl BlobStorePort for EncryptedBlobStore {
    async fn put(&self, blob_id: &BlobId, data: &[u8]) -> Result<(PathBuf, Option<i64>)> {
        let plaintext_size = data.len();

        let master_key = self
            .session
            .get_master_key()
            .context("encryption session not ready - cannot encrypt blob")?;

        let compressed =
            zstd::bulk::compress(data, ZSTD_LEVEL).context("failed to compress blob data")?;
        let compressed_size = compressed.len();

        let aad_bytes = aad::for_blob_v2(blob_id);

        // Slice 3 起直接走 v1_aead helper,跳过 EncryptedBlob 结构包装——
        // EncryptedBlobStore 自己用 UCBL 二进制 wire format,不需要 JSON 包装。
        let blob = v1_aead::encrypt_blob_xchacha(&master_key, &compressed, &aad_bytes)
            .context("failed to encrypt blob data")?;

        let nonce: [u8; 24] = blob
            .nonce
            .as_slice()
            .try_into()
            .context("encrypted blob nonce is not 24 bytes")?;

        let binary_data = serialize_blob(&nonce, &blob.ciphertext);
        let on_disk_size = binary_data.len() as i64;

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

        Ok((path, Some(on_disk_size)))
    }

    async fn put_from_path(
        &self,
        blob_id: &BlobId,
        source_path: &std::path::Path,
    ) -> Result<StoredPathBlob> {
        // AEAD wire format(v1_aead::encrypt_blob_xchacha)是 one-shot,目前不支持流式
        // 加密。这里先把源文件整文件读进内存再走 put() —— 与 capture-side 调用方约定:
        // 加密 store 启用时,path-backed ingest 的"任意大小"语义降级为"内存里能放得下",
        // 流式 AEAD 重构属于独立 phase。
        // No source path in the error context: a clipboard file path is user
        // content (usernames / sensitive filenames) and would leak through the
        // propagated error chain. Correlate by blob_id instead.
        let bytes = tokio::fs::read(source_path).await.with_context(|| {
            format!("failed to read source file for encryption (blob {blob_id})")
        })?;
        // Hash the exact plaintext buffer that gets compressed+encrypted below,
        // in the same read pass — no second read of the source can observe a
        // rewritten file, so the recorded identity matches the stored blob.
        let content_hash = ContentHash::from(blake3::hash(&bytes).as_bytes());
        let size_bytes = bytes.len() as u64;
        let (storage_path, compressed_size) = self.put(blob_id, &bytes).await?;
        Ok(StoredPathBlob {
            storage_path,
            content_hash,
            size_bytes,
            compressed_size,
        })
    }

    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
        <Self as BlobReaderPort>::get(self, blob_id).await
    }

    async fn delete(&self, blob_id: &BlobId) -> Result<()> {
        // Encryption is transparent to deletion: drop the stored bytes via the
        // inner store.
        self.inner.delete(blob_id).await
    }
}

#[async_trait]
impl BlobReaderPort for EncryptedBlobStore {
    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
        let binary_data = self
            .inner
            .get(blob_id)
            .instrument(info_span!("inner_blob_get", blob_id = %blob_id.as_ref()))
            .await
            .context("failed to read encrypted blob from storage")?;

        let (nonce, ciphertext) = parse_blob(&binary_data)?;

        let master_key = self
            .session
            .get_master_key()
            .context("encryption session not ready - cannot decrypt blob")?;

        let aad_bytes = aad::for_blob_v2(blob_id);

        // 直接走 v1_aead 解 (nonce + ciphertext + aad),跳过 EncryptedBlob 重构。
        let compressed = v1_aead::decrypt_blob_xchacha(&master_key, nonce, ciphertext, &aad_bytes)
            .context("failed to decrypt blob - key mismatch or data corrupted")?;

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
    use crate::blob::FilesystemBlobStore;
    use crate::security::secrets::MasterKey;

    fn hash_of(bytes: &[u8]) -> ContentHash {
        ContentHash::from(blake3::hash(bytes).as_bytes())
    }

    fn store_with_key(dir: PathBuf) -> EncryptedBlobStore {
        let inner: Arc<dyn BlobStorePort> = Arc::new(FilesystemBlobStore::new(dir));
        let session = Arc::new(InMemorySession::new());
        session.set_master_key(MasterKey::from_bytes(&[7u8; 32]).unwrap());
        EncryptedBlobStore::new(inner, session)
    }

    #[tokio::test]
    async fn put_from_path_hashes_plaintext_and_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store_with_key(tmp.path().join("blobs"));

        let src = tmp.path().join("plain.bin");
        let content = b"encrypted store: recorded identity is the plaintext hash".to_vec();
        tokio::fs::write(&src, &content).await.unwrap();

        let blob_id = BlobId::new();
        let stored = store.put_from_path(&blob_id, &src).await.unwrap();

        assert_eq!(stored.size_bytes, content.len() as u64);
        // Identity is the device-independent *plaintext* hash, never the ciphertext.
        assert_eq!(stored.content_hash, hash_of(&content));
        // Encrypted store tracks the on-disk (ciphertext) size.
        assert!(stored.compressed_size.is_some());

        // Decrypts back to the same plaintext, which hashes to the recorded id.
        let got = BlobReaderPort::get(&store, &blob_id).await.unwrap();
        assert_eq!(got, content);
        assert_eq!(hash_of(&got), stored.content_hash);
    }

    #[tokio::test]
    async fn delete_removes_encrypted_blob_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let store = store_with_key(tmp.path().join("blobs"));

        let src = tmp.path().join("plain.bin");
        tokio::fs::write(&src, b"bytes").await.unwrap();
        let blob_id = BlobId::new();
        store.put_from_path(&blob_id, &src).await.unwrap();
        assert!(BlobReaderPort::get(&store, &blob_id).await.is_ok());

        store.delete(&blob_id).await.unwrap();
        assert!(BlobReaderPort::get(&store, &blob_id).await.is_err());
        store.delete(&blob_id).await.unwrap();
    }
}

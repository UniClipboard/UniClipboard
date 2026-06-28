//! Filesystem-based blob storage.
//! 基于文件系统的 blob 存储。

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::debug;
use uc_core::blob::ports::BlobReaderPort;
use uc_core::BlobId;

use crate::blob::hashing::{copy_and_hash, stream_hash_file};
use crate::blob::{BlobStorePort, StoredPathBlob};

/// Filesystem-based blob storage.
pub struct FilesystemBlobStore {
    base_dir: PathBuf,
}

impl FilesystemBlobStore {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    async fn ensure_dir(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.base_dir)
            .await
            .context("Failed to create blob directory")
    }

    fn blob_path(&self, blob_id: &BlobId) -> PathBuf {
        self.base_dir.join(blob_id.as_str())
    }
}

#[async_trait::async_trait]
impl BlobStorePort for FilesystemBlobStore {
    async fn put(&self, blob_id: &BlobId, data: &[u8]) -> Result<(PathBuf, Option<i64>)> {
        self.ensure_dir().await?;
        let path = self.blob_path(blob_id);

        let mut file = tokio::fs::File::create(&path)
            .await
            .context("Failed to create blob file")?;
        tokio::io::AsyncWriteExt::write_all(&mut file, data)
            .await
            .context("Failed to write blob data")?;
        tokio::io::AsyncWriteExt::flush(&mut file)
            .await
            .context("Failed to flush blob data")?;
        file.sync_all().await.context("Failed to sync blob file")?;

        // Raw filesystem store doesn't track compression.
        Ok((path, None))
    }

    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
        <Self as BlobReaderPort>::get(self, blob_id).await
    }

    async fn put_from_path(&self, blob_id: &BlobId, source_path: &Path) -> Result<StoredPathBlob> {
        self.ensure_dir().await?;
        let dest = self.blob_path(blob_id);
        let source = source_path.to_path_buf();

        // 优先 hardlink:同卷下 O(1) 完成 ingest,无额外磁盘占用、无 byte copy。
        // 跨卷(EXDEV)或某些文件系统(SMB/NFS 部分配置)不支持 hardlink,回退到 copy。
        let link_dest = dest.clone();
        let link_source = source.clone();
        let (content_hash, size_bytes) =
            match tokio::task::spawn_blocking(move || std::fs::hard_link(&link_source, &link_dest))
                .await
                .context("hardlink join failed")?
            {
                Ok(()) => {
                    debug!(
                        blob_id = %blob_id,
                        "Hardlinked source file into blob store; hashing stored blob"
                    );
                    // Hash the destination (== the linked inode), not the source:
                    // the recorded identity is then of the exact bytes the blob
                    // points at, closing the hash/store divergence window even if
                    // the source path is replaced right after the link.
                    let hash_dest = dest.clone();
                    tokio::task::spawn_blocking(move || stream_hash_file(&hash_dest))
                        .await
                        .context("blob hash join failed")??
                }
                Err(err) => {
                    debug!(
                        blob_id = %blob_id,
                        error = %err,
                        "Hardlink failed; streaming copy+hash (likely EXDEV or unsupported FS)"
                    );
                    // Streaming copy that hashes in the same pass: the source is
                    // read exactly once and the hash is of the bytes written to
                    // dest, so no second read can observe a rewritten source.
                    let copy_source = source.clone();
                    let copy_dest = dest.clone();
                    tokio::task::spawn_blocking(move || copy_and_hash(&copy_source, &copy_dest))
                        .await
                        .context("blob copy join failed")?
                        // No source path in the context: it is user content. The
                        // dest path is our own blob-store location (blob_id).
                        .with_context(|| format!("failed to copy source into blob {blob_id}"))?
                }
            };

        debug!(
            blob_id = %blob_id,
            size_bytes,
            "Persisted source file into blob store"
        );

        Ok(StoredPathBlob {
            storage_path: dest,
            content_hash,
            size_bytes,
            // Raw filesystem store doesn't track compression.
            compressed_size: None,
        })
    }

    async fn delete(&self, blob_id: &BlobId) -> Result<()> {
        let path = self.blob_path(blob_id);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {
                debug!(blob_id = %blob_id, "Deleted blob from filesystem store");
                Ok(())
            }
            // Idempotent: an already-absent blob is a no-op, not an error.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => {
                Err(err).with_context(|| format!("failed to delete blob {}", path.display()))
            }
        }
    }
}

#[async_trait::async_trait]
impl BlobReaderPort for FilesystemBlobStore {
    async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
        let path = self.blob_path(blob_id);
        let mut file = tokio::fs::File::open(&path)
            .await
            .context("Failed to open blob file")?;

        let mut data = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut file, &mut data)
            .await
            .context("Failed to read blob data")?;

        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ContentHash;

    fn hash_of(bytes: &[u8]) -> ContentHash {
        ContentHash::from(blake3::hash(bytes).as_bytes())
    }

    #[tokio::test]
    async fn put_from_path_records_hash_matching_stored_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FilesystemBlobStore::new(tmp.path().join("blobs"));

        let src = tmp.path().join("source.bin");
        let content = b"toctou-regression: recorded hash must match stored bytes".to_vec();
        tokio::fs::write(&src, &content).await.unwrap();

        let blob_id = BlobId::new();
        let stored = store.put_from_path(&blob_id, &src).await.unwrap();

        // The returned identity is derived from the bytes the store persisted.
        assert_eq!(stored.size_bytes, content.len() as u64);
        assert_eq!(stored.content_hash, hash_of(&content));
        assert_eq!(stored.compressed_size, None);

        // And those persisted bytes hash back to the very same identity.
        let got = BlobReaderPort::get(&store, &blob_id).await.unwrap();
        assert_eq!(got, content);
        assert_eq!(hash_of(&got), stored.content_hash);
    }

    #[tokio::test]
    async fn delete_removes_blob_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let store = FilesystemBlobStore::new(tmp.path().join("blobs"));

        let blob_id = BlobId::new();
        // Deleting an absent blob is a no-op, not an error.
        store.delete(&blob_id).await.unwrap();

        let src = tmp.path().join("source.bin");
        tokio::fs::write(&src, b"bytes").await.unwrap();
        store.put_from_path(&blob_id, &src).await.unwrap();
        assert!(BlobReaderPort::get(&store, &blob_id).await.is_ok());

        store.delete(&blob_id).await.unwrap();
        assert!(BlobReaderPort::get(&store, &blob_id).await.is_err());
        // A second delete still succeeds.
        store.delete(&blob_id).await.unwrap();
    }
}

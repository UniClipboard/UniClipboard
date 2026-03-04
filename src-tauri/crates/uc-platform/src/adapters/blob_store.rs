//! Filesystem-based blob storage
//! 基于文件系统的 blob 存储

use anyhow::{Context, Result};
use std::path::PathBuf;
use uc_core::ports::BlobStorePort;
use uc_core::BlobId;

/// Filesystem-based blob storage
/// 基于文件系统的 blob 存储
pub struct FilesystemBlobStore {
    base_dir: PathBuf,
}

impl FilesystemBlobStore {
    /// Create a new blob store with the given base directory
    /// 使用给定基础目录创建新的 blob 存储
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Ensure the blob directory exists
    /// 确保 blob 目录存在
    async fn ensure_dir(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.base_dir)
            .await
            .context("Failed to create blob directory")
    }

    /// Get the full path for a blob ID
    /// 获取 blob ID 的完整路径
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

        // Raw filesystem store doesn't track compression
        Ok((path, None))
    }

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

    #[tokio::test]
    async fn test_blob_path() {
        let store = FilesystemBlobStore::new(PathBuf::from("/tmp/blobs"));
        let blob_id = BlobId::from("test-blob-123");
        let path = store.blob_path(&blob_id);

        assert_eq!(path, PathBuf::from("/tmp/blobs/test-blob-123"));
    }

    #[tokio::test]
    async fn test_ensure_dir_creates_directory() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let blob_dir = temp_dir.path().join("blobs");
        let store = FilesystemBlobStore::new(blob_dir.clone());

        store.ensure_dir().await.unwrap();

        assert!(blob_dir.exists());
        assert!(blob_dir.is_dir());
    }
}

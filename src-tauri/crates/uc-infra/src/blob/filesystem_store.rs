//! Filesystem-based blob storage.
//! 基于文件系统的 blob 存储。

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};
use uc_core::blob::ports::BlobReaderPort;
use uc_core::BlobId;

use crate::blob::BlobStorePort;

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

    async fn put_from_path(
        &self,
        blob_id: &BlobId,
        source_path: &Path,
    ) -> Result<(PathBuf, Option<i64>)> {
        self.ensure_dir().await?;
        let dest = self.blob_path(blob_id);
        let source = source_path.to_path_buf();

        // 优先 hardlink:同卷下 O(1) 完成 ingest,无额外磁盘占用、无 byte copy。
        // 跨卷(EXDEV)或某些文件系统(SMB/NFS 部分配置)不支持 hardlink,回退到 copy。
        let link_dest = dest.clone();
        let link_source = source.clone();
        match tokio::task::spawn_blocking(move || std::fs::hard_link(&link_source, &link_dest))
            .await
            .context("hardlink join failed")?
        {
            Ok(()) => {
                debug!(
                    blob_id = %blob_id,
                    source = %source.display(),
                    dest = %dest.display(),
                    "Hardlinked source file into blob store"
                );
                return Ok((dest, None));
            }
            Err(err) => {
                warn!(
                    blob_id = %blob_id,
                    source = %source.display(),
                    dest = %dest.display(),
                    error = %err,
                    "Hardlink failed; falling back to streaming copy (likely EXDEV or unsupported FS)"
                );
            }
        }

        // 流式 copy 回退:tokio::fs::copy 内部按块读写,常驻内存极小。
        let copied = tokio::fs::copy(&source, &dest).await.with_context(|| {
            format!("failed to copy {} -> {}", source.display(), dest.display())
        })?;
        debug!(
            blob_id = %blob_id,
            source = %source.display(),
            dest = %dest.display(),
            bytes = copied,
            "Copied source file into blob store"
        );

        // 与 put() 行为对齐:确保字节真正落盘后再返回。
        let dest_file = tokio::fs::File::open(&dest)
            .await
            .context("failed to reopen blob file for sync")?;
        dest_file
            .sync_all()
            .await
            .context("failed to sync blob file after copy")?;

        Ok((dest, None))
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

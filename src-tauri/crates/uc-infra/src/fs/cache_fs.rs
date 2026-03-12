//! Tokio-based implementation of the CacheFsPort.
//! 基于 Tokio 的 CacheFsPort 实现。

use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use uc_core::ports::cache_fs::{CacheFsPort, DirEntry};

/// Tokio filesystem adapter for cache operations.
/// 用于缓存操作的 Tokio 文件系统适配器。
pub struct TokioCacheFsAdapter;

impl TokioCacheFsAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CacheFsPort for TokioCacheFsAdapter {
    async fn exists(&self, path: &Path) -> bool {
        tokio::fs::try_exists(path).await.unwrap_or(false)
    }

    async fn read_dir(&self, path: &Path) -> Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read directory: {}", e))?;

        while let Some(entry) = read_dir
            .next_entry()
            .await
            .with_context(|| format!("Failed to read entry in directory: {}", path.display()))?
        {
            let entry_path = entry.path();
            let is_dir = entry_path.is_dir();
            entries.push(DirEntry {
                path: entry_path,
                is_dir,
            });
        }

        Ok(entries)
    }

    async fn remove_dir_all(&self, path: &Path) -> Result<()> {
        tokio::fs::remove_dir_all(path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to remove directory: {}", e))
    }

    async fn remove_file(&self, path: &Path) -> Result<()> {
        tokio::fs::remove_file(path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to remove file: {}", e))
    }

    async fn dir_size(&self, path: &Path) -> Result<u64> {
        compute_dir_size(path).await
    }
}

/// Recursively calculate directory size in bytes.
/// 递归计算目录大小（字节数）。
///
/// Returns `Ok(0)` for non-existent paths. Returns an error if a path
/// exists but cannot be read (e.g. permission denied).
async fn compute_dir_size(path: &Path) -> Result<u64> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(0);
    }

    let metadata = tokio::fs::metadata(path)
        .await
        .with_context(|| format!("Failed to read metadata for: {}", path.display()))?;

    if metadata.is_file() {
        return Ok(metadata.len());
    }

    let mut total: u64 = 0;
    let mut entries = tokio::fs::read_dir(path)
        .await
        .with_context(|| format!("Failed to read directory: {}", path.display()))?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .with_context(|| format!("Failed to read entry in directory: {}", path.display()))?
    {
        let entry_path = entry.path();
        if entry_path.is_dir() {
            total += Box::pin(compute_dir_size(&entry_path)).await?;
        } else {
            let meta = tokio::fs::metadata(&entry_path).await.with_context(|| {
                format!("Failed to read metadata for: {}", entry_path.display())
            })?;
            total += meta.len();
        }
    }

    Ok(total)
}

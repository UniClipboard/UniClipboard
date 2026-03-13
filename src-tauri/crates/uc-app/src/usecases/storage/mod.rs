//! Storage management use cases.
//! 存储管理用例。

pub mod clear_cache;
pub mod get_storage_stats;
pub mod open_data_directory;

pub use clear_cache::ClearCache;
pub use get_storage_stats::{GetStorageStats, StorageStatsResult};
pub use open_data_directory::OpenDataDirectory;

use anyhow::{Context, Result};
use std::path::Path;

/// Recursively calculate directory size in bytes.
/// 递归计算目录大小（字节数）。
///
/// Returns `Ok(0)` for non-existent paths. Returns an error if a path exists
/// but cannot be read (e.g. permission denied).
pub(crate) async fn dir_size(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }

    if path.is_file() {
        let meta = tokio::fs::metadata(path)
            .await
            .with_context(|| format!("Failed to read metadata for file: {}", path.display()))?;
        return Ok(meta.len());
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
            total += Box::pin(dir_size(&entry_path)).await?;
        } else {
            let meta = tokio::fs::metadata(&entry_path).await.with_context(|| {
                format!("Failed to read metadata for: {}", entry_path.display())
            })?;
            total += meta.len();
        }
    }

    Ok(total)
}

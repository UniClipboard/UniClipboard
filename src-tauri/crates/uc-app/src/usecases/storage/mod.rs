//! Storage management use cases.
//! 存储管理用例。

pub mod clear_cache;
pub mod get_storage_stats;
pub mod open_data_directory;

pub use clear_cache::ClearCache;
pub use get_storage_stats::{GetStorageStats, StorageStatsResult};
pub use open_data_directory::OpenDataDirectory;

use std::path::Path;

/// Recursively calculate directory size in bytes.
/// 递归计算目录大小（字节数）。
pub(crate) async fn dir_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }

    if path.is_file() {
        return tokio::fs::metadata(path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
    }

    let mut total: u64 = 0;
    let mut entries = match tokio::fs::read_dir(path).await {
        Ok(entries) => entries,
        Err(_) => return 0,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let entry_path = entry.path();
        if entry_path.is_dir() {
            total += Box::pin(dir_size(&entry_path)).await;
        } else {
            total += tokio::fs::metadata(&entry_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
        }
    }

    total
}

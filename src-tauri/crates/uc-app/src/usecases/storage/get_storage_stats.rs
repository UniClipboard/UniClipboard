//! Use case for computing storage statistics.
//! 计算存储统计信息的用例。

use crate::app_paths::AppPaths;
use anyhow::Result;

use super::dir_size;

/// Result of storage statistics computation.
/// 存储统计计算结果。
#[derive(Debug, Clone)]
pub struct StorageStatsResult {
    pub database_bytes: u64,
    pub vault_bytes: u64,
    pub cache_bytes: u64,
    pub logs_bytes: u64,
    pub total_bytes: u64,
    pub data_dir: String,
}

/// Use case for computing storage statistics across application directories.
/// 计算应用各目录存储统计信息的用例。
pub struct GetStorageStats {
    storage_paths: AppPaths,
}

impl GetStorageStats {
    pub fn new(storage_paths: AppPaths) -> Self {
        Self { storage_paths }
    }

    #[tracing::instrument(name = "usecase.get_storage_stats.execute", skip(self))]
    pub async fn execute(&self) -> Result<StorageStatsResult> {
        let paths = &self.storage_paths;

        let (database_bytes, vault_bytes, cache_bytes, logs_bytes) = tokio::try_join!(
            dir_size(&paths.db_path),
            dir_size(&paths.vault_dir),
            dir_size(&paths.cache_dir),
            dir_size(&paths.logs_dir),
        )
        .inspect_err(|e| {
            tracing::error!(error = %e, "Failed to compute storage stats");
        })?;

        let total_bytes = database_bytes + vault_bytes + cache_bytes + logs_bytes;
        let data_dir = paths.app_data_root.to_string_lossy().to_string();

        tracing::info!(
            database_bytes,
            vault_bytes,
            cache_bytes,
            logs_bytes,
            total_bytes,
            "Storage stats computed"
        );

        Ok(StorageStatsResult {
            database_bytes,
            vault_bytes,
            cache_bytes,
            logs_bytes,
            total_bytes,
            data_dir,
        })
    }
}

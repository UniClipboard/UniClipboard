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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_paths(root: &TempDir, cache: &TempDir) -> AppPaths {
        let root_path = root.path().to_path_buf();
        AppPaths {
            db_path: root_path.join("uniclipboard.db"),
            vault_dir: root_path.join("vault"),
            settings_path: root_path.join("settings.json"),
            logs_dir: root_path.join("logs"),
            cache_dir: cache.path().to_path_buf(),
            app_data_root: root_path,
        }
    }

    #[tokio::test]
    async fn returns_zero_for_nonexistent_directories() {
        let root = TempDir::new().unwrap();
        let cache = TempDir::new().unwrap();
        let paths = make_paths(&root, &cache);
        // Don't create any subdirectories — all paths are nonexistent
        let uc = GetStorageStats::new(paths.clone());
        let result = uc.execute().await.unwrap();

        assert_eq!(result.database_bytes, 0);
        assert_eq!(result.vault_bytes, 0);
        assert_eq!(result.logs_bytes, 0);
        assert_eq!(result.total_bytes, 0);
        assert_eq!(result.data_dir, paths.app_data_root.to_string_lossy());
    }

    #[tokio::test]
    async fn sums_bytes_across_directories() {
        let root = TempDir::new().unwrap();
        let cache = TempDir::new().unwrap();
        let paths = make_paths(&root, &cache);

        // Create db file (single file path)
        std::fs::write(&paths.db_path, vec![0u8; 100]).unwrap();

        // Create vault dir with a file
        std::fs::create_dir_all(&paths.vault_dir).unwrap();
        std::fs::write(paths.vault_dir.join("blob.dat"), vec![0u8; 200]).unwrap();

        // Create logs dir with a file
        std::fs::create_dir_all(&paths.logs_dir).unwrap();
        std::fs::write(paths.logs_dir.join("app.log"), vec![0u8; 50]).unwrap();

        // Cache dir already exists (tempdir), add a file
        std::fs::write(cache.path().join("tmp.bin"), vec![0u8; 30]).unwrap();

        let uc = GetStorageStats::new(paths);
        let result = uc.execute().await.unwrap();

        assert_eq!(result.database_bytes, 100);
        assert_eq!(result.vault_bytes, 200);
        assert_eq!(result.logs_bytes, 50);
        assert_eq!(result.cache_bytes, 30);
        assert_eq!(result.total_bytes, 380);
    }

    #[tokio::test]
    async fn data_dir_field_matches_app_data_root() {
        let root = TempDir::new().unwrap();
        let cache = TempDir::new().unwrap();
        let paths = make_paths(&root, &cache);
        let expected = paths.app_data_root.to_string_lossy().to_string();

        let uc = GetStorageStats::new(paths);
        let result = uc.execute().await.unwrap();

        assert_eq!(result.data_dir, expected);
    }
}

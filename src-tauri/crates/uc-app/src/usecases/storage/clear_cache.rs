//! Use case for clearing the application cache directory.
//! 清除应用缓存目录的用例。

use crate::app_paths::AppPaths;
use anyhow::Result;
use uc_core::app_dirs::AppDirs;

use super::dir_size;

/// Use case for clearing cache directory contents.
/// 清除缓存目录内容的用例。
pub struct ClearCache {
    app_dirs: AppDirs,
}

impl ClearCache {
    pub fn new(app_dirs: AppDirs) -> Self {
        Self { app_dirs }
    }

    /// Clears cache directory contents and returns the number of bytes freed.
    /// 清除缓存目录内容并返回释放的字节数。
    #[tracing::instrument(name = "usecase.clear_cache.execute", skip(self))]
    pub async fn execute(&self) -> Result<u64> {
        let paths = AppPaths::from_app_dirs(&self.app_dirs);
        let freed = dir_size(&paths.cache_dir).await;

        if paths.cache_dir.exists() {
            let mut entries = tokio::fs::read_dir(&paths.cache_dir)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to read cache dir: {}", e))?;

            while let Ok(Some(entry)) = entries.next_entry().await {
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    if let Err(e) = tokio::fs::remove_dir_all(&entry_path).await {
                        tracing::warn!(path = %entry_path.display(), error = %e, "Failed to remove cache subdirectory");
                    }
                } else if let Err(e) = tokio::fs::remove_file(&entry_path).await {
                    tracing::warn!(path = %entry_path.display(), error = %e, "Failed to remove cache file");
                }
            }
        }

        tracing::info!(freed_bytes = freed, "Cache cleared");
        Ok(freed)
    }
}

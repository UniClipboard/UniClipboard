//! Use case for clearing the application cache directory.
//! 清除应用缓存目录的用例。

use std::sync::Arc;

use crate::app_paths::AppPaths;
use anyhow::Result;
use uc_core::ports::cache_fs::CacheFsPort;

/// Use case for clearing cache directory contents.
/// 清除缓存目录内容的用例。
pub struct ClearCache {
    storage_paths: AppPaths,
    cache_fs: Arc<dyn CacheFsPort>,
}

impl ClearCache {
    pub fn new(storage_paths: AppPaths, cache_fs: Arc<dyn CacheFsPort>) -> Self {
        Self {
            storage_paths,
            cache_fs,
        }
    }

    /// Clears cache directory contents and returns the number of bytes freed.
    /// 清除缓存目录内容并返回释放的字节数。
    #[tracing::instrument(name = "usecase.clear_cache.execute", skip(self))]
    pub async fn execute(&self) -> Result<u64> {
        let paths = &self.storage_paths;
        let size_before = self.cache_fs.dir_size(&paths.cache_dir).await;

        if self.cache_fs.exists(&paths.cache_dir).await {
            let entries = self
                .cache_fs
                .read_dir(&paths.cache_dir)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to read cache dir: {}", e))?;

            for entry in entries {
                if entry.is_dir {
                    if let Err(e) = self.cache_fs.remove_dir_all(&entry.path).await {
                        tracing::warn!(path = %entry.path.display(), error = %e, "Failed to remove cache subdirectory");
                    }
                } else if let Err(e) = self.cache_fs.remove_file(&entry.path).await {
                    tracing::warn!(path = %entry.path.display(), error = %e, "Failed to remove cache file");
                }
            }
        }

        let size_after = self.cache_fs.dir_size(&paths.cache_dir).await;
        let freed = size_before.saturating_sub(size_after);

        tracing::info!(freed_bytes = freed, "Cache cleared");
        Ok(freed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use uc_core::ports::cache_fs::DirEntry;

    struct MockCacheFs {
        entries: Vec<DirEntry>,
        size_before: u64,
        size_after: u64,
        exists: bool,
        dir_size_call_count: AtomicU32,
    }

    #[async_trait::async_trait]
    impl CacheFsPort for MockCacheFs {
        async fn exists(&self, _path: &Path) -> bool {
            self.exists
        }

        async fn read_dir(&self, _path: &Path) -> Result<Vec<DirEntry>> {
            Ok(self.entries.clone())
        }

        async fn remove_dir_all(&self, _path: &Path) -> Result<()> {
            Ok(())
        }

        async fn remove_file(&self, _path: &Path) -> Result<()> {
            Ok(())
        }

        async fn dir_size(&self, _path: &Path) -> u64 {
            let call = self.dir_size_call_count.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                self.size_before
            } else {
                self.size_after
            }
        }
    }

    fn test_storage_paths() -> AppPaths {
        AppPaths {
            db_path: PathBuf::from("/tmp/test-data/uniclipboard.db"),
            vault_dir: PathBuf::from("/tmp/test-data/vault"),
            settings_path: PathBuf::from("/tmp/test-data/settings.json"),
            logs_dir: PathBuf::from("/tmp/test-data/logs"),
            cache_dir: PathBuf::from("/tmp/test-cache"),
            app_data_root: PathBuf::from("/tmp/test-data"),
        }
    }

    #[tokio::test]
    async fn execute_returns_freed_bytes_when_cache_exists() {
        let cache_fs = Arc::new(MockCacheFs {
            entries: vec![
                DirEntry {
                    path: PathBuf::from("/tmp/test-cache/subdir"),
                    is_dir: true,
                },
                DirEntry {
                    path: PathBuf::from("/tmp/test-cache/file.tmp"),
                    is_dir: false,
                },
            ],
            size_before: 1024,
            size_after: 0,
            exists: true,
            dir_size_call_count: AtomicU32::new(0),
        });

        let uc = ClearCache::new(test_storage_paths(), cache_fs);
        let freed = uc.execute().await.unwrap();
        assert_eq!(freed, 1024);
    }

    #[tokio::test]
    async fn execute_returns_zero_when_cache_dir_missing() {
        let cache_fs = Arc::new(MockCacheFs {
            entries: vec![],
            size_before: 0,
            size_after: 0,
            exists: false,
            dir_size_call_count: AtomicU32::new(0),
        });

        let uc = ClearCache::new(test_storage_paths(), cache_fs);
        let freed = uc.execute().await.unwrap();
        assert_eq!(freed, 0);
    }
}

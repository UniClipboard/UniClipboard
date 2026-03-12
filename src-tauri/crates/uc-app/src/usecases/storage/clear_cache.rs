//! Use case for clearing the application cache directory.
//! 清除应用缓存目录的用例。

use std::sync::Arc;

use crate::app_paths::AppPaths;
use anyhow::Result;
use uc_core::app_dirs::AppDirs;
use uc_core::ports::cache_fs::CacheFsPort;

/// Use case for clearing cache directory contents.
/// 清除缓存目录内容的用例。
pub struct ClearCache {
    app_dirs: AppDirs,
    cache_fs: Arc<dyn CacheFsPort>,
}

impl ClearCache {
    pub fn new(app_dirs: AppDirs, cache_fs: Arc<dyn CacheFsPort>) -> Self {
        Self { app_dirs, cache_fs }
    }

    /// Clears cache directory contents and returns the number of bytes freed.
    /// 清除缓存目录内容并返回释放的字节数。
    #[tracing::instrument(name = "usecase.clear_cache.execute", skip(self))]
    pub async fn execute(&self) -> Result<u64> {
        let paths = AppPaths::from_app_dirs(&self.app_dirs);
        let freed = self.cache_fs.dir_size(&paths.cache_dir).await;

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

        tracing::info!(freed_bytes = freed, "Cache cleared");
        Ok(freed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use uc_core::ports::cache_fs::DirEntry;

    struct MockCacheFs {
        entries: Vec<DirEntry>,
        size: u64,
        exists: bool,
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
            self.size
        }
    }

    fn test_app_dirs() -> AppDirs {
        AppDirs {
            app_data_root: PathBuf::from("/tmp/test-data"),
            app_cache_root: PathBuf::from("/tmp/test-cache"),
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
            size: 1024,
            exists: true,
        });

        let uc = ClearCache::new(test_app_dirs(), cache_fs);
        let freed = uc.execute().await.unwrap();
        assert_eq!(freed, 1024);
    }

    #[tokio::test]
    async fn execute_returns_zero_when_cache_dir_missing() {
        let cache_fs = Arc::new(MockCacheFs {
            entries: vec![],
            size: 0,
            exists: false,
        });

        let uc = ClearCache::new(test_app_dirs(), cache_fs);
        let freed = uc.execute().await.unwrap();
        assert_eq!(freed, 0);
    }
}

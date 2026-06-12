use std::path::PathBuf;
use std::sync::Arc;

use tracing::instrument;
use uc_core::ports::CacheFsPort;

#[derive(Clone)]
pub struct StorageFacadeDeps {
    pub db_path: PathBuf,
    pub vault_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub app_data_root_dir: PathBuf,
    pub cache_fs: Arc<dyn CacheFsPort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageStatsView {
    pub total_bytes: u64,
    pub database_bytes: u64,
    pub vault_bytes: u64,
    pub cache_bytes: u64,
    pub logs_bytes: u64,
    pub data_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearCacheResultView {
    pub freed_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageFacadeError {
    #[error("failed to compute storage stats: {0}")]
    Stats(String),
    #[error("failed to clear cache: {0}")]
    ClearCache(String),
}

pub struct StorageFacade {
    deps: StorageFacadeDeps,
}

impl StorageFacade {
    pub fn new(deps: StorageFacadeDeps) -> Self {
        Self { deps }
    }

    #[instrument(skip_all)]
    pub async fn stats(&self) -> Result<StorageStatsView, StorageFacadeError> {
        let deps = &self.deps;
        let (database_bytes, vault_bytes, cache_bytes, logs_bytes) = tokio::try_join!(
            deps.cache_fs.dir_size(&deps.db_path),
            deps.cache_fs.dir_size(&deps.vault_dir),
            deps.cache_fs.dir_size(&deps.cache_dir),
            deps.cache_fs.dir_size(&deps.logs_dir),
        )
        .map_err(|err| {
            tracing::error!(error = %err, "storage facade: failed to compute storage stats");
            StorageFacadeError::Stats(err.to_string())
        })?;

        let total_bytes = database_bytes + vault_bytes + cache_bytes + logs_bytes;
        tracing::info!(
            database_bytes,
            vault_bytes,
            cache_bytes,
            logs_bytes,
            total_bytes,
            "storage facade: stats computed"
        );

        Ok(StorageStatsView {
            total_bytes,
            database_bytes,
            vault_bytes,
            cache_bytes,
            logs_bytes,
            data_dir: deps.app_data_root_dir.to_string_lossy().to_string(),
        })
    }

    #[instrument(skip_all)]
    pub async fn clear_cache(&self) -> Result<ClearCacheResultView, StorageFacadeError> {
        let deps = &self.deps;
        let size_before = deps
            .cache_fs
            .dir_size(&deps.cache_dir)
            .await
            .map_err(|err| StorageFacadeError::ClearCache(err.to_string()))?;

        if deps.cache_fs.exists(&deps.cache_dir).await {
            let entries = deps
                .cache_fs
                .read_dir(&deps.cache_dir)
                .await
                .map_err(|err| {
                    StorageFacadeError::ClearCache(format!("failed to read cache dir: {err}"))
                })?;

            for entry in entries {
                if entry.is_dir {
                    if let Err(err) = deps.cache_fs.remove_dir_all(&entry.path).await {
                        tracing::warn!(
                            path = %entry.path.display(),
                            error = %err,
                            "storage facade: failed to remove cache subdirectory"
                        );
                    }
                } else if let Err(err) = deps.cache_fs.remove_file(&entry.path).await {
                    tracing::warn!(
                        path = %entry.path.display(),
                        error = %err,
                        "storage facade: failed to remove cache file"
                    );
                }
            }
        }

        let size_after = deps
            .cache_fs
            .dir_size(&deps.cache_dir)
            .await
            .map_err(|err| StorageFacadeError::ClearCache(err.to_string()))?;
        let freed_bytes = size_before.saturating_sub(size_after);

        tracing::info!(freed_bytes, "storage facade: cache cleared");
        Ok(ClearCacheResultView { freed_bytes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use anyhow::Result;
    use async_trait::async_trait;
    use tempfile::TempDir;
    use tokio::fs;
    use uc_core::ports::cache_fs::DirEntry;

    struct TokioCacheFs;

    #[async_trait]
    impl CacheFsPort for TokioCacheFs {
        async fn exists(&self, path: &std::path::Path) -> bool {
            fs::try_exists(path).await.unwrap_or(false)
        }

        async fn read_dir(&self, path: &std::path::Path) -> Result<Vec<DirEntry>> {
            let mut entries = Vec::new();
            let mut read_dir = fs::read_dir(path).await?;
            while let Some(entry) = read_dir.next_entry().await? {
                let file_type = entry.file_type().await?;
                entries.push(DirEntry {
                    path: entry.path(),
                    is_dir: file_type.is_dir(),
                });
            }
            Ok(entries)
        }

        async fn remove_dir_all(&self, path: &std::path::Path) -> Result<()> {
            fs::remove_dir_all(path).await?;
            Ok(())
        }

        async fn remove_file(&self, path: &std::path::Path) -> Result<()> {
            fs::remove_file(path).await?;
            Ok(())
        }

        async fn dir_size(&self, path: &std::path::Path) -> Result<u64> {
            if !self.exists(path).await {
                return Ok(0);
            }

            let metadata = fs::metadata(path).await?;
            if metadata.is_file() {
                return Ok(metadata.len());
            }

            let mut total = 0_u64;
            let mut stack = vec![path.to_path_buf()];
            while let Some(dir) = stack.pop() {
                let mut read_dir = fs::read_dir(dir).await?;
                while let Some(entry) = read_dir.next_entry().await? {
                    let metadata = entry.metadata().await?;
                    if metadata.is_dir() {
                        stack.push(entry.path());
                    } else if metadata.is_file() {
                        total += metadata.len();
                    }
                }
            }
            Ok(total)
        }

        async fn read_file(&self, path: &std::path::Path) -> Result<Option<Vec<u8>>> {
            match fs::read(path).await {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            }
        }

        async fn write_file(&self, path: &std::path::Path, contents: &[u8]) -> Result<()> {
            fs::write(path, contents).await?;
            Ok(())
        }

        async fn metadata(
            &self,
            path: &std::path::Path,
        ) -> Result<Option<uc_core::ports::cache_fs::FileMetadata>> {
            match fs::metadata(path).await {
                Ok(m) => Ok(Some(uc_core::ports::cache_fs::FileMetadata {
                    size_bytes: m.len(),
                    is_dir: m.is_dir(),
                    modified_unix_ms: m
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as i64),
                })),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            }
        }

        async fn remove_dir(&self, path: &std::path::Path) -> Result<()> {
            fs::remove_dir(path).await?;
            Ok(())
        }
    }

    async fn write_bytes(path: &std::path::Path, len: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.expect("create parent");
        }
        fs::write(path, vec![b'x'; len]).await.expect("write file");
    }

    fn facade_for(temp: &TempDir) -> StorageFacade {
        let data = temp.path().join("data");
        let cache = temp.path().join("cache");
        StorageFacade::new(StorageFacadeDeps {
            db_path: data.join("uniclipboard.db"),
            vault_dir: data.join("vault"),
            cache_dir: cache,
            logs_dir: data.join("logs"),
            app_data_root_dir: data,
            cache_fs: Arc::new(TokioCacheFs),
        })
    }

    #[tokio::test]
    async fn stats_returns_application_storage_sizes() {
        let temp = TempDir::new().expect("temp dir");
        let facade = facade_for(&temp);

        write_bytes(&facade.deps.db_path, 3).await;
        write_bytes(&facade.deps.vault_dir.join("secret.bin"), 5).await;
        write_bytes(&facade.deps.cache_dir.join("a.tmp"), 7).await;
        write_bytes(&facade.deps.logs_dir.join("daemon.log"), 11).await;

        let stats = facade.stats().await.expect("stats");

        assert_eq!(stats.database_bytes, 3);
        assert_eq!(stats.vault_bytes, 5);
        assert_eq!(stats.cache_bytes, 7);
        assert_eq!(stats.logs_bytes, 11);
        assert_eq!(stats.total_bytes, 26);
        assert_eq!(
            stats.data_dir,
            temp.path().join("data").to_string_lossy().to_string()
        );
    }

    #[tokio::test]
    async fn clear_cache_removes_cache_children_and_reports_freed_bytes() {
        let temp = TempDir::new().expect("temp dir");
        let facade = facade_for(&temp);

        write_bytes(&facade.deps.cache_dir.join("a.tmp"), 7).await;
        write_bytes(&facade.deps.cache_dir.join("nested/b.tmp"), 11).await;

        let result = facade.clear_cache().await.expect("clear cache");

        assert_eq!(result.freed_bytes, 18);
        assert!(facade.deps.cache_dir.exists());
        assert!(!facade.deps.cache_dir.join("a.tmp").exists());
        assert!(!facade.deps.cache_dir.join("nested").exists());
    }
}

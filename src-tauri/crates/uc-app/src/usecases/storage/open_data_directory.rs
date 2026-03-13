//! Use case for opening the application data directory in the system file manager.
//! 在系统文件管理器中打开应用数据目录的用例。

use std::sync::Arc;

use crate::app_paths::AppPaths;
use anyhow::Result;
use uc_core::ports::file_manager::FileManagerPort;

/// Use case for opening the data directory in the native file manager.
/// 在原生文件管理器中打开数据目录的用例。
pub struct OpenDataDirectory {
    storage_paths: AppPaths,
    file_manager: Arc<dyn FileManagerPort>,
}

impl OpenDataDirectory {
    pub fn new(storage_paths: AppPaths, file_manager: Arc<dyn FileManagerPort>) -> Self {
        Self {
            storage_paths,
            file_manager,
        }
    }

    #[tracing::instrument(name = "usecase.open_data_directory.execute", skip(self))]
    pub async fn execute(&self) -> Result<()> {
        let dir = &self.storage_paths.app_data_root;
        self.file_manager
            .open_directory(dir)
            .map_err(anyhow::Error::from)?;

        tracing::info!(dir = %dir.display(), "Opened data directory");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use uc_core::ports::file_manager::FileManagerError;

    struct MockFileManager {
        opened: Mutex<Vec<PathBuf>>,
        should_fail: bool,
    }

    impl MockFileManager {
        fn new(should_fail: bool) -> Self {
            Self {
                opened: Mutex::new(Vec::new()),
                should_fail,
            }
        }
    }

    impl FileManagerPort for MockFileManager {
        fn open_directory(&self, path: &Path) -> Result<(), FileManagerError> {
            if self.should_fail {
                return Err(FileManagerError::OpenFailed(path.display().to_string()));
            }
            self.opened.lock().unwrap().push(path.to_path_buf());
            Ok(())
        }
    }

    fn test_paths() -> AppPaths {
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
    async fn opens_app_data_root_directory() {
        let fm = Arc::new(MockFileManager::new(false));
        let paths = test_paths();
        let expected = paths.app_data_root.clone();

        let uc = OpenDataDirectory::new(paths, fm.clone());
        uc.execute().await.unwrap();

        let opened = fm.opened.lock().unwrap();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0], expected);
    }

    #[tokio::test]
    async fn returns_error_when_file_manager_fails() {
        let fm = Arc::new(MockFileManager::new(true));
        let uc = OpenDataDirectory::new(test_paths(), fm);

        let result = uc.execute().await;
        assert!(result.is_err());
    }
}

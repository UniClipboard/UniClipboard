//! Use case for opening the application data directory in the system file manager.
//! 在系统文件管理器中打开应用数据目录的用例。

use std::sync::Arc;

use anyhow::Result;
use uc_core::app_dirs::AppDirs;
use uc_core::ports::file_manager::{FileManagerError, FileManagerPort};

/// Use case for opening the data directory in the native file manager.
/// 在原生文件管理器中打开数据目录的用例。
pub struct OpenDataDirectory {
    app_dirs: AppDirs,
    file_manager: Arc<dyn FileManagerPort>,
}

impl OpenDataDirectory {
    pub fn new(app_dirs: AppDirs, file_manager: Arc<dyn FileManagerPort>) -> Self {
        Self {
            app_dirs,
            file_manager,
        }
    }

    #[tracing::instrument(name = "usecase.open_data_directory.execute", skip(self))]
    pub async fn execute(&self) -> Result<()> {
        let dir = &self.app_dirs.app_data_root;
        self.file_manager.open_directory(dir).map_err(|e| match e {
            FileManagerError::DirectoryNotFound(msg) => {
                anyhow::anyhow!("Data directory does not exist: {}", msg)
            }
            FileManagerError::OpenFailed(msg) => {
                anyhow::anyhow!("Failed to open directory: {}", msg)
            }
        })?;

        tracing::info!(dir = %dir.display(), "Opened data directory");
        Ok(())
    }
}

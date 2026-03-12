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

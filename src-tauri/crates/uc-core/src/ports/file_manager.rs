//! Port for opening directories in the system file manager.
//! 用于在系统文件管理器中打开目录的端口。

use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FileManagerError {
    #[error("Directory does not exist: {0}")]
    DirectoryNotFound(String),
    #[error("Failed to open directory: {0}")]
    OpenFailed(String),
}

/// Port for opening directories in the platform's native file manager.
/// 用于在平台原生文件管理器中打开目录的端口。
pub trait FileManagerPort: Send + Sync {
    fn open_directory(&self, path: &Path) -> Result<(), FileManagerError>;
}

//! Native file manager adapter for opening directories.
//! 原生文件管理器适配器，用于打开目录。

use std::path::Path;
use uc_core::ports::file_manager::{FileManagerError, FileManagerPort};

/// Opens directories using the platform's native file manager.
/// 使用平台原生文件管理器打开目录。
pub struct NativeFileManagerAdapter;

impl NativeFileManagerAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl FileManagerPort for NativeFileManagerAdapter {
    fn open_directory(&self, path: &Path) -> Result<(), FileManagerError> {
        if !path.exists() {
            return Err(FileManagerError::DirectoryNotFound(
                path.display().to_string(),
            ));
        }

        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(path)
                .spawn()
                .map_err(|e| FileManagerError::OpenFailed(e.to_string()))?;
        }

        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer")
                .arg(path)
                .spawn()
                .map_err(|e| FileManagerError::OpenFailed(e.to_string()))?;
        }

        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open")
                .arg(path)
                .spawn()
                .map_err(|e| FileManagerError::OpenFailed(e.to_string()))?;
        }

        Ok(())
    }
}

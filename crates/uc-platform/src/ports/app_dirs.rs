use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    pub app_data_root: PathBuf,
    pub app_cache_root: PathBuf,
    pub app_log_dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum AppDirsError {
    #[error("system data-local directory unavailable")]
    DataLocalDirUnavailable,

    #[error("system cache directory unavailable")]
    CacheDirUnavailable,

    #[error("platform error: {0}")]
    Platform(String),
}

pub trait AppDirsProvider: Send + Sync {
    fn get_app_dirs(&self) -> Result<AppDirs, AppDirsError>;
}

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppDirsError {
    #[error("system data-local directory unavailable")]
    DataLocalDirUnavailable,

    #[error("system cache directory unavailable")]
    CacheDirUnavailable,

    #[error("platform error: {0}")]
    Platform(String),
}

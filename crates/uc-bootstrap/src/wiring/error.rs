/// Result type for desktop host preparation.
pub type WiringResult<T> = Result<T, WiringError>;

/// Failures that can occur while preparing desktop host capabilities.
#[derive(Debug, thiserror::Error)]
pub enum WiringError {
    #[error("Secure storage initialization failed: {0}")]
    SecureStorageInit(String),

    #[error("Clipboard initialization failed: {0}")]
    ClipboardInit(String),

    #[error("Configuration initialization failed: {0}")]
    ConfigInit(String),
}

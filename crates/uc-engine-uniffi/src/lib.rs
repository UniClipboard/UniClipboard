//! UniFFI bindings for the public `uc-engine` interface.

mod runtime;

pub use runtime::{MobileEngine, SpaceCreated};

uniffi::setup_scaffolding!();

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BindingErrorCategory {
    InvalidInput,
    InvalidState,
    Unauthorized,
    NotFound,
    Conflict,
    Unavailable,
    DeadlineExceeded,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum BindingError {
    #[error("engine error {code} ({category:?})")]
    Engine {
        code: u32,
        category: BindingErrorCategory,
        retryable: bool,
    },
    #[error("host capability unavailable")]
    HostUnavailable,
    #[error("host capability permission denied")]
    HostPermissionDenied,
    #[error("host file handle invalid")]
    HostInvalidHandle,
    #[error("host input/output failed")]
    HostIo,
    #[error("binding runtime unavailable")]
    RuntimeUnavailable,
    #[error("binding engine already stopped")]
    AlreadyStopped,
    #[error("engine returned an unexpected result")]
    UnexpectedResult,
}

impl From<uc_engine::EngineError> for BindingError {
    fn from(error: uc_engine::EngineError) -> Self {
        let category = match error.category() {
            uc_engine::EngineErrorCategory::InvalidInput => BindingErrorCategory::InvalidInput,
            uc_engine::EngineErrorCategory::InvalidState => BindingErrorCategory::InvalidState,
            uc_engine::EngineErrorCategory::Unauthorized => BindingErrorCategory::Unauthorized,
            uc_engine::EngineErrorCategory::NotFound => BindingErrorCategory::NotFound,
            uc_engine::EngineErrorCategory::Conflict => BindingErrorCategory::Conflict,
            uc_engine::EngineErrorCategory::Unavailable => BindingErrorCategory::Unavailable,
            uc_engine::EngineErrorCategory::DeadlineExceeded => {
                BindingErrorCategory::DeadlineExceeded
            }
            uc_engine::EngineErrorCategory::Internal => BindingErrorCategory::Internal,
        };
        Self::Engine {
            code: error.code(),
            category,
            retryable: error.is_retryable(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum HostBindingError {
    #[error("host capability unavailable")]
    Unavailable,
    #[error("host capability permission denied")]
    PermissionDenied,
    #[error("host file handle invalid")]
    InvalidHandle,
    #[error("host input/output failed")]
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BindingConfig {
    pub app_version: String,
    pub profile_id: String,
}

#[uniffi::export(with_foreign)]
pub trait BindingHost: Send + Sync {
    fn private_data_directory(&self) -> Result<String, HostBindingError>;
    fn cache_directory(&self) -> Result<String, HostBindingError>;
    fn temporary_directory(&self) -> Result<String, HostBindingError>;
    fn secure_storage_get(&self, key: String) -> Result<Option<Vec<u8>>, HostBindingError>;
    fn secure_storage_set(&self, key: String, value: Vec<u8>) -> Result<(), HostBindingError>;
    fn secure_storage_delete(&self, key: String) -> Result<(), HostBindingError>;
}

#[uniffi::export]
pub fn core_version() -> String {
    format!("core-v{}", env!("CARGO_PKG_VERSION"))
}

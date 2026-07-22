//! UniFFI bindings for the public `uc-engine` interface.

mod runtime;

pub use runtime::{InvitationIssued, MobileEngine, SendReport, SpaceCreated, SpaceJoined};

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
        Self::Engine {
            code: error.code(),
            category: map_error_category(error.category()),
            retryable: error.is_retryable(),
        }
    }
}

impl From<uc_engine::EngineError> for BindingFailure {
    fn from(error: uc_engine::EngineError) -> Self {
        Self {
            code: error.code(),
            category: map_error_category(error.category()),
            retryable: error.is_retryable(),
        }
    }
}

fn map_error_category(category: uc_engine::EngineErrorCategory) -> BindingErrorCategory {
    match category {
        uc_engine::EngineErrorCategory::InvalidInput => BindingErrorCategory::InvalidInput,
        uc_engine::EngineErrorCategory::InvalidState => BindingErrorCategory::InvalidState,
        uc_engine::EngineErrorCategory::Unauthorized => BindingErrorCategory::Unauthorized,
        uc_engine::EngineErrorCategory::NotFound => BindingErrorCategory::NotFound,
        uc_engine::EngineErrorCategory::Conflict => BindingErrorCategory::Conflict,
        uc_engine::EngineErrorCategory::Unavailable => BindingErrorCategory::Unavailable,
        uc_engine::EngineErrorCategory::DeadlineExceeded => BindingErrorCategory::DeadlineExceeded,
        uc_engine::EngineErrorCategory::Internal => BindingErrorCategory::Internal,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BindingEngineState {
    Running,
    Quiescing,
    Quiesced,
    Suspended,
    ShuttingDown,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BindingFailure {
    pub code: u32,
    pub category: BindingErrorCategory,
    pub retryable: bool,
}

#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct BindingFileMetadata {
    pub display_name: String,
    pub size_bytes: u64,
    pub mime_type: Option<String>,
}

#[derive(Clone, PartialEq, Eq, uniffi::Enum)]
pub enum BindingClipboardRepresentation {
    Inline {
        format: String,
        mime_type: Option<String>,
        bytes: Vec<u8>,
    },
    File {
        format: String,
        handle: String,
        display_name: String,
        mime_type: Option<String>,
        size_bytes: u64,
    },
}

impl std::fmt::Debug for BindingClipboardRepresentation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = formatter.debug_struct("BindingClipboardRepresentation");
        match self {
            Self::Inline {
                format,
                mime_type,
                bytes,
            } => debug
                .field("kind", &"inline")
                .field("format", format)
                .field("mime_type", mime_type)
                .field("byte_len", &bytes.len()),
            Self::File {
                format,
                mime_type,
                size_bytes,
                ..
            } => debug
                .field("kind", &"file")
                .field("format", format)
                .field("handle", &"[REDACTED]")
                .field("display_name", &"[REDACTED]")
                .field("mime_type", mime_type)
                .field("size_bytes", size_bytes),
        };
        debug.finish()
    }
}

#[derive(Clone, PartialEq, Eq, uniffi::Record)]
pub struct BindingClipboardSnapshot {
    pub observed_at_ms: i64,
    pub representations: Vec<BindingClipboardRepresentation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BindingClipboardRestoreMode {
    Standard,
    PlainText,
    FilePaths,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BindingClipboardRestoreOutcome {
    Restored,
    PayloadUnavailable,
    NotApplicable,
}

impl std::fmt::Debug for BindingClipboardSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BindingClipboardSnapshot")
            .field("observed_at_ms", &self.observed_at_ms)
            .field("representation_count", &self.representations.len())
            .finish()
    }
}

impl std::fmt::Debug for BindingFileMetadata {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BindingFileMetadata")
            .field("display_name", &"[REDACTED]")
            .field("size_bytes", &self.size_bytes)
            .field("mime_type", &self.mime_type)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BindingOperationTerminal {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BindingRefreshReason {
    ConsumerLagged,
    StateInvalidated,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum BindingEvent {
    StateChanged {
        state: BindingEngineState,
    },
    OperationFinished {
        operation_id: String,
        terminal: BindingOperationTerminal,
        failure: Option<BindingFailure>,
    },
    RefreshRequired {
        reason: BindingRefreshReason,
    },
    Fatal {
        failure: BindingFailure,
    },
    Changed {
        kind: String,
    },
}

#[uniffi::export(with_foreign)]
pub trait BindingHost: Send + Sync {
    fn private_data_directory(&self) -> Result<String, HostBindingError>;
    fn cache_directory(&self) -> Result<String, HostBindingError>;
    fn temporary_directory(&self) -> Result<String, HostBindingError>;
    fn secure_storage_get(&self, key: String) -> Result<Option<Vec<u8>>, HostBindingError>;
    fn secure_storage_set(&self, key: String, value: Vec<u8>) -> Result<(), HostBindingError>;
    fn secure_storage_delete(&self, key: String) -> Result<(), HostBindingError>;
    fn file_metadata(&self, handle: String) -> Result<BindingFileMetadata, HostBindingError>;
    fn file_read_chunk(
        &self,
        handle: String,
        offset: u64,
        max_bytes: u32,
    ) -> Result<Vec<u8>, HostBindingError>;
    fn file_write_chunk(
        &self,
        handle: String,
        offset: u64,
        bytes: Vec<u8>,
    ) -> Result<(), HostBindingError>;
    fn file_finish_write(&self, handle: String) -> Result<(), HostBindingError>;
    fn clipboard_read(&self) -> Result<BindingClipboardSnapshot, HostBindingError>;
    fn clipboard_write(&self, snapshot: BindingClipboardSnapshot) -> Result<(), HostBindingError>;
}

#[uniffi::export]
pub fn core_version() -> String {
    format!("core-v{}", env!("CARGO_PKG_VERSION"))
}

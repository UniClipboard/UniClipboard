use serde::Serialize;

/// Typed command error taxonomy for Tauri command boundary.
///
/// Serializes to {"code": "...", "message": "..."} for frontend discriminated union handling.
#[derive(Debug, Clone, Serialize, specta::Type, thiserror::Error)]
#[serde(tag = "code", content = "message")]
pub enum CommandError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("internal error: {0}")]
    InternalError(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("cancelled: {0}")]
    Cancelled(String),

    #[error("validation error: {0}")]
    ValidationError(String),

    #[error("conflict: {0}")]
    Conflict(String),
}

impl CommandError {
    /// Convenience constructor wrapping any Display as InternalError.
    pub fn internal(err: impl std::fmt::Display) -> Self {
        CommandError::InternalError(err.to_string())
    }
}

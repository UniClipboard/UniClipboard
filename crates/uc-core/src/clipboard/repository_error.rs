//! Domain error for clipboard entry/representation persistence ports.

/// Domain error returned by clipboard entry and representation repository
/// ports. Implementations must translate their underlying storage errors into
/// this enum and must not leak third-party error types to callers.
#[derive(Debug, thiserror::Error)]
pub enum ClipboardRepositoryError {
    /// The referenced entry, event, or representation does not exist.
    #[error("clipboard record not found: {0}")]
    NotFound(String),
    /// The persistence layer failed to complete the operation.
    #[error("storage failure: {0}")]
    Storage(String),
}

//! Search domain error type — boundary error for SearchIndexPort and SearchKeyDerivationPort.
//!
//! Infrastructure implementations use `anyhow::Error` internally and map to SearchError
//! only at the port boundary (per D-05).

/// Typed error enum for the search port boundary.
///
/// Phase 92 daemon maps these variants to HTTP status codes:
/// - `InvalidQuery` → 400 Bad Request
/// - `SessionLocked` → 423 Locked
/// - `IndexNotReady` → 503 Service Unavailable
#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// Query is structurally invalid — e.g. mixed AND/OR operators.
    /// Maps to HTTP 400 in daemon layer.
    #[error("invalid query: {0}")]
    InvalidQuery(String),

    /// Encryption session is locked — search key cannot be derived.
    /// Maps to HTTP 423 Locked in daemon layer.
    #[error("encryption session is locked")]
    SessionLocked,

    /// Search index version mismatch or rebuild window in progress.
    #[error("search index not ready")]
    IndexNotReady,

    /// Search index is not wired / disabled for current profile.
    #[error("search index unavailable")]
    IndexUnavailable,

    /// Catch-all for internal failures that cross the port boundary.
    /// Infra adapters should map anyhow::Error into this when necessary.
    #[error("internal search error: {0}")]
    Internal(String),
}

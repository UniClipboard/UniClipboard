//! Canonical daemon HTTP error body (ADR-008).
//!
//! Relocated from `uc-webserver/src/api/dto/error.rs` so it can be shared by the
//! generated TypeScript client and the native Rust `uc-daemon-client`. The
//! axum-coupled `ApiError` carrier, its constructors, `IntoResponse`, and
//! `log_facade_failure` stay in the webserver and import this body type.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Canonical daemon HTTP error body.
///
/// Wire shape: `{ "code": "<machine_code>", "message": "<human text>" }` plus an
/// optional `details` object. `code` is a stable snake_case token (e.g.
/// `not_found`, `bad_request`, `runtime_unavailable`, `conflict`,
/// `internal_error`, `payload_unavailable`). `message` is human-readable
/// English; setup-v2 error classifiers and the clipboard restore-410 handler
/// substring-match it, so the strings are LOAD-BEARING — do not silently reword.
///
/// `Deserialize` is added (the original webserver struct was Serialize-only) so
/// the Rust `uc-daemon-client` and tests can decode error bodies.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiErrorResponse {
    pub code: String,
    pub message: String,
    /// Optional structured context (per §0.3). E.g. the restore-410
    /// `payload_unavailable` error carries `{ entry_id, rep_id, state }`.
    /// Omitted from the wire when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiErrorResponse {
    /// Construct an error body with no structured details.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    /// Construct an error body carrying structured `details`.
    pub fn with_details(
        code: impl Into<String>,
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: Some(details),
        }
    }
}

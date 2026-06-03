//! HTTP error surface for daemon API handlers.
//!
//! # Why this module exists
//!
//! Every `facade::*Error` variant carries a specific cause (network not
//! started, sponsor unreachable, storage write failure, …). By the time the
//! webserver middleware sees the response it only has the final HTTP status —
//! the cause chain has already been flattened into a `String` inside `ApiError`
//! and the original variant is gone. Earlier Sentry Issues like
//! "daemon http upstream unavailable" carried zero signal for exactly this
//! reason: they were echoes of an error that had already been thrown away one
//! layer deeper, in the handler's own `map_err`.
//!
//! [`log_facade_failure`] solves this by emitting the root-cause `ERROR` event
//! at the **mapping point** — where the typed facade error is still in scope —
//! with structured `facade / op / error_variant / status` fields. Sentry can
//! then bucket distinct failure modes (SponsorUnreachable, Timeout,
//! ConnectionLost, …) into distinct Issues instead of collapsing them into
//! one mega-group keyed off the middleware's generic message.
//!
//! # Rule for new handlers
//!
//! **Any handler that maps a facade error onto a 5xx `ApiError` (or
//! `(StatusCode::5xx, Json)`) MUST call [`log_facade_failure`] at the mapping
//! point.** The helper itself filters out 4xx, so handlers can call it
//! unconditionally on every variant.
//!
//! Concretely, prefer this shape:
//!
//! ```ignore
//! fn map_foo_err(err: FooError) -> ApiError {
//!     use FooError as E;
//!     let (variant, api): (&'static str, ApiError) = match err {
//!         E::NotFound        => ("not_found",        ApiError::not_found("...")),
//!         E::ServiceDown(m)  => ("service_down",     ApiError::service_unavailable(m)),
//!         E::Internal(m)     => ("internal",         ApiError::internal(m)),
//!     };
//!     log_facade_failure("foo", "do_thing", variant, api.status, &api.message);
//!     api
//! }
//! ```
//!
//! # Anti-patterns
//!
//! - Calling `tracing::error!(error = %e, "X failed")` inside a `map_err`
//!   closure or middleware: produces an ERROR event with no `facade / op /
//!   error_variant`, which Sentry can only group by message — exactly the
//!   "mega-group" failure mode this module was added to fix.
//! - Logging the cause once in the handler AND once again in middleware: the
//!   middleware path was already downgraded to `WARN` (see `api/server.rs`)
//!   precisely so the in-handler `ERROR` is the single root-cause event.
//! - Using `error_kind = "<some-string>"` as a fake root-cause attribute on a
//!   middleware-level event: Sentry will index it as if it were a real bucket
//!   key, producing the same collapse the structured `facade / op /
//!   error_variant` triple is meant to prevent.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

// The canonical error body now lives in the contract crate (ADR-008 §C.2) so it
// can be shared by the generated TS client and the native Rust client. The
// axum-coupled `ApiError` carrier, its constructors, `IntoResponse`, and
// `log_facade_failure` stay here.
pub use uc_daemon_contract::api::dto::error::ApiErrorResponse;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: String,
    pub message: String,
    /// Optional structured payload echoed into [`ApiErrorResponse::details`].
    ///
    /// Carries the typed facade error's extra fields (e.g. mobile-sync's
    /// `{ max }` / `{ username }` / `{ min, got }`) across the wire so the FE
    /// error translators can reconstruct their discriminated unions from
    /// `code` + these fields — the same `{ code, ...details }` shape the
    /// `clipboard_resend` / `restore` handlers already emit. The named
    /// constructors leave this `None`; attach via [`ApiError::with_details`].
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "runtime_unavailable",
            message,
        )
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    /// Shared constructor: status + stable snake_case `code` token + message,
    /// no structured details. Public callers go through the named constructors
    /// or build the struct literally (e.g. `map_*_err` with a semantic code).
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.to_string(),
            message: message.into(),
            details: None,
        }
    }

    /// Attach a structured `details` JSON payload, consumed by the FE error
    /// translators. Chainable: `ApiError::conflict(msg).with_details(json!({...}))`.
    /// Independent of [`log_facade_failure`] (which keys off status, not body).
    #[must_use]
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ApiErrorResponse {
            code: self.code,
            message: self.message,
            details: self.details,
        };
        (self.status, Json(body)).into_response()
    }
}

// Emit the root-cause ERROR at the point we still have the original facade
// enum variant. The webserver middleware only sees the final HTTP status by
// the time the error has been flattened into a String, which is why earlier
// Sentry Issues like "daemon http upstream unavailable" carried zero signal
// — they were echoes of an error that had already been thrown away. Logging
// here keeps `facade / op / error_variant` queryable in Sentry so distinct
// failure modes (SponsorUnreachable, Timeout, ConnectionLost, etc.) don't
// all collapse into one mega-group. Covers any 5xx (500 + 503) since both
// share the same "cause chain compressed to a String" problem.
pub fn log_facade_failure(
    facade: &'static str,
    op: &'static str,
    variant: &'static str,
    status: StatusCode,
    message: &str,
) {
    if status.is_server_error() {
        tracing::error!(
            facade,
            op,
            error_variant = variant,
            status = status.as_u16(),
            "{} facade returned {} ({}): {}",
            facade,
            status.as_u16(),
            variant,
            message,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::json;

    /// `with_details` must survive `IntoResponse` so FE translators can read the
    /// structured fields off `DaemonApiError.details` (e.g. mobile-sync's
    /// `{ max }` / `{ username }`). Without the thread-through the body would
    /// drop them and the FE would lose the i18n interpolation values.
    #[tokio::test]
    async fn into_response_threads_structured_details() {
        let err = ApiError::conflict("username already taken: alice")
            .with_details(json!({ "username": "alice" }));
        assert_eq!(err.status, StatusCode::CONFLICT);

        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body: ApiErrorResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.code, "conflict");
        assert_eq!(body.message, "username already taken: alice");
        assert_eq!(body.details.unwrap()["username"], "alice");
    }

    /// The named constructors leave `details` absent so existing endpoints emit
    /// the same bare `{ code, message }` body as before this field was added.
    #[tokio::test]
    async fn named_constructors_emit_no_details() {
        let resp = ApiError::not_found("entry not found").into_response();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let body: ApiErrorResponse = serde_json::from_slice(&bytes).unwrap();
        assert!(body.details.is_none());
    }
}

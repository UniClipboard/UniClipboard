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
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct ApiErrorResponse {
    pub code: String,
    pub message: String,
}

impl ApiErrorResponse {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "internal_error".to_string(),
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            code: "bad_request".to_string(),
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            code: "unauthorized".to_string(),
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(self)).into_response()
    }
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error".to_string(),
            message: message.into(),
        }
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "runtime_unavailable".to_string(),
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "bad_request".to_string(),
            message: message.into(),
        }
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "conflict".to_string(),
            message: message.into(),
        }
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized".to_string(),
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found".to_string(),
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ApiErrorResponse {
            code: self.code,
            message: self.message,
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

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

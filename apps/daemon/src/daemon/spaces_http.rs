//! Windows daemon HTTP service contract for multi-space profile routes.
//!
//! The transport adapter is intentionally deferred to the daemon host. This
//! module owns method/path dispatch, JSON DTO decoding, canonical response
//! bodies, and error/status mapping. The injected backend owns each atomic
//! catalog + runtime workflow.

use std::sync::Arc;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::Value;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::dto::error::ApiErrorResponse;
use uc_daemon_contract::api::dto::v2::spaces::{
    CreateSpaceProfileRequestDto, JoinSpaceProfileRequestDto, SetActiveSendSpaceRequestDto,
    SpaceProfileSummaryDto,
};

const SPACES_PATH: &str = "/v2/spaces";
const SPACES_JOIN_PATH: &str = "/v2/spaces/join";
const SPACES_ACTIVE_SEND_PATH: &str = "/v2/spaces/active-send";

/// Atomic multi-space workflows consumed by the Windows HTTP surface.
///
/// Implementations must coordinate catalog persistence and runtime lifecycle
/// as one operation. They must also record root-cause diagnostics before
/// returning [`SpacesBackendError::Internal`], whose diagnostic text is never
/// exposed or logged by this transport boundary.
#[async_trait]
pub trait SpacesHttpBackend: Send + Sync {
    async fn list_spaces(&self) -> Result<Vec<SpaceProfileSummaryDto>, SpacesBackendError>;

    async fn create_space(
        &self,
        request: CreateSpaceProfileRequestDto,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError>;

    async fn join_space(
        &self,
        request: JoinSpaceProfileRequestDto,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError>;

    async fn set_active_send(
        &self,
        request: SetActiveSendSpaceRequestDto,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError>;

    async fn remove_space(
        &self,
        profile_id: String,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError>;
}

/// Backend failure categories with stable client-facing codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpacesBackendError {
    BadRequest { code: String, message: String },
    NotFound { code: String, message: String },
    Conflict { code: String, message: String },
    RuntimeUnavailable { code: String, message: String },
    Internal { diagnostic: String },
}

impl SpacesBackendError {
    pub fn bad_request(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::BadRequest {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::NotFound {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn conflict(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Conflict {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn runtime_unavailable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::RuntimeUnavailable {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn internal(diagnostic: impl Into<String>) -> Self {
        Self::Internal {
            diagnostic: diagnostic.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpacesHttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpacesHttpRequest {
    pub method: SpacesHttpMethod,
    pub path: String,
    pub body: Option<Vec<u8>>,
}

impl SpacesHttpRequest {
    pub fn new(method: SpacesHttpMethod, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            body: None,
        }
    }

    pub fn with_body(method: SpacesHttpMethod, path: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            method,
            path: path.into(),
            body: Some(body),
        }
    }

    pub fn json(method: SpacesHttpMethod, path: impl Into<String>, body: &Value) -> Self {
        Self::with_body(method, path, body.to_string().into_bytes())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpacesHttpResponse {
    pub status: u16,
    pub body: Value,
}

#[derive(Clone)]
pub struct SpacesHttpService {
    backend: Arc<dyn SpacesHttpBackend>,
}

impl SpacesHttpService {
    pub fn new(backend: Arc<dyn SpacesHttpBackend>) -> Self {
        Self { backend }
    }

    pub async fn handle(&self, request: SpacesHttpRequest) -> SpacesHttpResponse {
        let route = resolve_route(&request.path);
        match (request.method, route) {
            (SpacesHttpMethod::Get, Route::Collection) => {
                backend_result("list", self.backend.list_spaces().await)
            }
            (SpacesHttpMethod::Post, Route::Collection) => {
                let request = match decode_body(request.body.as_deref()) {
                    Ok(request) => request,
                    Err(response) => return response,
                };
                backend_result("create", self.backend.create_space(request).await)
            }
            (SpacesHttpMethod::Post, Route::Join) => {
                let request = match decode_body(request.body.as_deref()) {
                    Ok(request) => request,
                    Err(response) => return response,
                };
                backend_result("join", self.backend.join_space(request).await)
            }
            (SpacesHttpMethod::Put, Route::ActiveSend) => {
                let request = match decode_body(request.body.as_deref()) {
                    Ok(request) => request,
                    Err(response) => return response,
                };
                backend_result(
                    "set_active_send",
                    self.backend.set_active_send(request).await,
                )
            }
            (SpacesHttpMethod::Delete, Route::Profile(profile_id)) => {
                backend_result("remove", self.backend.remove_space(profile_id).await)
            }
            (_, Route::UnsafeProfile) => {
                error_response(400, "bad_request", "profile path segment is invalid")
            }
            (_, Route::Unknown) => error_response(404, "not_found", "route was not found"),
            _ => error_response(
                405,
                "method_not_allowed",
                "HTTP method is not allowed for this route",
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Route {
    Collection,
    Join,
    ActiveSend,
    Profile(String),
    UnsafeProfile,
    Unknown,
}

fn resolve_route(path: &str) -> Route {
    match path {
        SPACES_PATH => Route::Collection,
        SPACES_JOIN_PATH => Route::Join,
        SPACES_ACTIVE_SEND_PATH => Route::ActiveSend,
        _ => {
            let Some(profile_id) = path.strip_prefix("/v2/spaces/") else {
                return Route::Unknown;
            };
            if profile_id.contains('/') {
                return Route::Unknown;
            }
            if is_safe_profile_segment(profile_id) {
                Route::Profile(profile_id.to_string())
            } else {
                Route::UnsafeProfile
            }
        }
    }
}

fn is_safe_profile_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn decode_body<T>(body: Option<&[u8]>) -> Result<T, SpacesHttpResponse>
where
    T: DeserializeOwned,
{
    let body = body.ok_or_else(|| error_response(400, "bad_request", "JSON body is required"))?;
    serde_json::from_slice(body)
        .map_err(|_| error_response(400, "bad_request", "JSON body is invalid"))
}

fn backend_result<T>(
    operation: &'static str,
    result: Result<T, SpacesBackendError>,
) -> SpacesHttpResponse
where
    T: serde::Serialize,
{
    match result {
        Ok(data) => success_response(data),
        Err(error) => backend_error_response(operation, error),
    }
}

fn success_response<T>(data: T) -> SpacesHttpResponse
where
    T: serde::Serialize,
{
    match serde_json::to_value(ApiEnvelope::now(data)) {
        Ok(body) => SpacesHttpResponse { status: 200, body },
        Err(error) => {
            tracing::error!(
                facade = "spaces_http",
                op = "serialize_success",
                error_variant = "serialization",
                status = 500_u16,
                error = %error,
                "failed to serialize multi-space success response"
            );
            error_response(500, "internal_error", "space operation failed")
        }
    }
}

fn backend_error_response(
    operation: &'static str,
    error: SpacesBackendError,
) -> SpacesHttpResponse {
    match error {
        SpacesBackendError::BadRequest { code, message } => error_response(400, code, message),
        SpacesBackendError::NotFound { code, message } => error_response(404, code, message),
        SpacesBackendError::Conflict { code, message } => error_response(409, code, message),
        SpacesBackendError::RuntimeUnavailable { code, message } => {
            error_response(503, code, message)
        }
        SpacesBackendError::Internal { diagnostic } => {
            tracing::error!(
                facade = "spaces_http",
                op = operation,
                error_variant = "internal",
                status = 500_u16,
                diagnostic_present = !diagnostic.is_empty(),
                "multi-space backend operation failed"
            );
            error_response(500, "internal_error", "space operation failed")
        }
    }
}

fn error_response(
    status: u16,
    code: impl Into<String>,
    message: impl Into<String>,
) -> SpacesHttpResponse {
    let error = ApiErrorResponse::new(code, message);
    let body = serde_json::to_value(error).unwrap_or_else(|_| {
        serde_json::json!({
            "code": "internal_error",
            "message": "space operation failed"
        })
    });
    SpacesHttpResponse { status, body }
}

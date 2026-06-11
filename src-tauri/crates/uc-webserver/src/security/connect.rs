//! POST /auth/connect - exchange bearer token for JWT session token.
//!
//! This endpoint is the entry point for the daemon's JWT authentication flow.
//! It accepts a bearer token (the daemon's local secret), validates it,
//! registers the client PID, and returns a short-lived JWT session token.
//!
//! Wire shape (ADR-008 §0.2): the success body is the canonical
//! `ApiEnvelope<SessionTokenResponse> { data: { sessionToken, expiresInSecs,
//! refreshAtSecs }, ts }`. This is a deliberate BREAKING change — the previous
//! shape was the flat `SessionTokenResponse` with no envelope. The native Rust
//! decoder (`uc-daemon-client/src/http/mod.rs`) is updated in lockstep (P3) to
//! unwrap `data`. `/auth/connect` is L1/PUBLIC (bootstrap: no session token yet)
//! and authenticates with `Authorization: Bearer <local-secret>`, so it is on
//! the `PUBLIC_PATHS` allowlist and does NOT carry the session security scheme.
//!
//! Errors use the canonical `ApiErrorResponse { code, message, details? }`
//! (§0.3). The native client only decodes the success body (it reads the error
//! body as opaque text), so normalising the error shape is safe.

use std::net::SocketAddr;

use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use std::collections::HashMap;
use uc_daemon_contract::api::dto::auth::{ConnectRequest, SessionTokenResponse};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use url::form_urlencoded;

use crate::api::auth::parse_bearer_token;
use crate::api::dto::error::ApiErrorResponse;
use crate::api::server::DaemonApiState;
use crate::security::claims::{SessionTokenClaims, LEVEL_L2, REFRESH_AT_SECS, TTL_SECS};
use crate::security::rate_limiter::{RateLimitDecision, PREAUTH_MAX_REQUESTS};

struct ParsedConnectRequest {
    pid: u32,
    client_type: String,
    token: Option<String>,
}

/// Router for auth-related routes.
///
/// POST /auth/connect is the only route - it accepts bearer token
/// (not session token) because it's the entry point for getting a session token.
pub fn router() -> Router<DaemonApiState> {
    // NOTE: cors_middleware is applied once at the outermost layer in
    // `build_router`; do not re-layer it here.
    Router::new().route(
        uc_daemon_contract::constants::auth_route::AUTH_CONNECT,
        post(connect_handler),
    )
}

/// POST /auth/connect.
///
/// Validates the bearer token in the Authorization header, registers the client
/// PID, and returns a JWT session token wrapped in the canonical envelope.
///
/// Rate limiting: This endpoint has no session token yet, so rate limiting
/// is applied by client IP address (from ConnectInfo). This is trustworthy
/// because it comes from the TCP stack, not caller-controlled input.
///
/// NOTE on ConnectInfo: ConnectInfo<SocketAddr> reads the socket address from
/// the TCP connection metadata, NOT from HTTP headers. In test contexts (using
/// tower::ServiceExt::oneshot without a real TCP listener), the socket address
/// will be a default value (typically 127.0.0.1:0 or ::1:0). The unit tests
/// for SlidingWindowRateLimiter cover the rate limiting logic independently.
/// IP-based rate limiting for /auth/connect works correctly in production.
///
/// IMPORTANT: ConnectInfo<SocketAddr> works ONLY when the server uses
/// `into_make_service_with_connect_info::<SocketAddr>()`.
/// In test contexts using tower::ServiceExt::oneshot, ConnectInfo may be absent.
/// The handler uses Option<ConnectInfo<SocketAddr>> so tests work correctly.
/// IP-based rate limiting is skipped when ConnectInfo is unavailable (test-only code path).
#[utoipa::path(
    post,
    path = "/auth/connect",
    operation_id = "authConnect",
    tag = "system",
    request_body = ConnectRequest,
    // L1/PUBLIC bootstrap endpoint: authenticates with `Authorization: Bearer
    // <local-secret>`, NOT a session token. It is on the `PUBLIC_PATHS`
    // allowlist in `openapi_meta::apply_metadata`, so the session security
    // schemes are intentionally NOT applied. No `security(...)` requirement is
    // declared here because the metadata module (assembly-owned) registers only
    // the two session schemes; declaring an undeclared `bearer_token` scheme
    // would emit a dangling reference.
    responses(
        (status = 200, description = "JWT session token issued", body = SessionTokenEnvelope),
        (status = 400, description = "Malformed connect request", body = ApiErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ApiErrorResponse),
        (status = 429, description = "Too many requests from this client IP", body = ApiErrorResponse),
        (status = 500, description = "Failed to sign the session token", body = ApiErrorResponse),
    )
)]
async fn connect_handler(
    State(state): State<DaemonApiState>,
    connect_info: Option<ConnectInfo<SocketAddr>>,
    headers: HeaderMap,
    request: Request<Body>,
) -> axum::response::Response {
    // Step 1: Apply rate limiting by client IP (pre-auth, no session token yet).
    // ConnectInfo is None in test contexts (no real TCP connection via oneshot).
    // In production (real TCP listener via into_make_service_with_connect_info), it is always Some.
    if let Some(ConnectInfo(client_ip)) = connect_info {
        let client_ip_str = client_ip.ip().to_string();
        if let RateLimitDecision::Limited { retry_after_secs } = state
            .security
            .rate_limiter
            .check(&client_ip_str, PREAUTH_MAX_REQUESTS)
            .await
        {
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limit_exceeded",
                &format!("too many connect requests; retry after {retry_after_secs} seconds"),
            );
        }
    }

    let parsed = match parse_connect_request(request).await {
        Some(parsed) => parsed,
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "invalid connect request",
            );
        }
    };

    // Step 2: Validate bearer token (same as existing daemon auth)
    let token = parsed.token.as_deref().or_else(|| {
        headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_bearer_token)
    });

    let Some(token) = token else {
        return unauthorized();
    };

    // Constant-time comparison: a naive `!=` short-circuits on the first
    // mismatching byte, leaking a timing side-channel that a local process on
    // the loopback interface could use to probe the token byte-by-byte.
    if !state.auth_token.verify(token) {
        return unauthorized();
    }

    // Step 3: Register PID in whitelist
    //
    // NOTE: The PID from the request body is trusted because:
    // 1. The bearer token (from daemon.token file) has already been validated above
    // 2. The frontend runs on the same machine as the daemon
    // 3. PID verification is defense-in-depth against local malware, not a hard security boundary
    // 4. The bearer token file has filesystem permissions (600)
    state.security.register_pid(parsed.pid).await;

    // Step 4: Build claims
    //
    // NOTE on L3/L4 (Phase 75 scope):
    // Phase 75 does NOT implement L3/L4 permission enforcement.
    // All clients receive L2 tokens (access_level = LEVEL_L2, encryption_ready = false).
    // The access_level field exists in the JWT for future use, but is not enforced by middleware.
    // Future phases (Phase 76+) will wire encryption state from `AppFacade::encryption`
    // to determine if the client should receive L3/L4 tokens based on session state.
    let encryption_ready = false; // Phase 75: always false
    let access_level = LEVEL_L2; // Phase 75: always L2

    let claims = SessionTokenClaims::new(
        parsed.pid,
        parsed.client_type,
        access_level,
        encryption_ready,
    );

    // Step 5: Sign JWT with HS256
    let token = match claims.sign(state.security.jwt_secret.as_ref()) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, "failed to sign session token");
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "failed to generate session token",
            );
        }
    };

    // Canonical envelope: `{ data: { sessionToken, expiresInSecs, refreshAtSecs }, ts }`.
    let payload = SessionTokenResponse {
        session_token: token,
        expires_in_secs: TTL_SECS,
        refresh_at_secs: REFRESH_AT_SECS,
    };

    (StatusCode::OK, Json(ApiEnvelope::now(payload))).into_response()
}

async fn parse_connect_request(request: Request<Body>) -> Option<ParsedConnectRequest> {
    let content_type = request
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value: &axum::http::HeaderValue| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 16 * 1024).await.ok()?;

    if content_type.starts_with("application/x-www-form-urlencoded") {
        let form: HashMap<String, String> = form_urlencoded::parse(bytes.as_ref())
            .into_owned()
            .collect();

        return Some(ParsedConnectRequest {
            pid: form.get("pid")?.parse().ok()?,
            client_type: form.get("clientType")?.clone(),
            token: form.get("token").cloned(),
        });
    }

    let req: ConnectRequest = serde_json::from_slice(&bytes).ok()?;
    let token = parts.uri.query().and_then(|query: &str| {
        form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == "token")
            .map(|(_, value)| value.into_owned())
    });

    Some(ParsedConnectRequest {
        pid: req.pid,
        client_type: req.client_type,
        token,
    })
}

/// Build a canonical `ApiErrorResponse` body with the given status + code/message.
fn error_response(status: StatusCode, code: &str, message: &str) -> axum::response::Response {
    let body = ApiErrorResponse::new(code, message);
    (status, Json(body)).into_response()
}

fn unauthorized() -> axum::response::Response {
    error_response(StatusCode::UNAUTHORIZED, "unauthorized", "unauthorized")
}

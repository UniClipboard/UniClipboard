//! Axum middleware functions for daemon HTTP API security.
//!
//! Phase 75 provides two middleware functions:
//! - `auth_extractor_middleware`: Extracts and validates JWT session tokens.
//! - `rate_limit_middleware`: Enforces per-client rate limits.
//!
//! L3/L4 permission enforcement is NOT implemented in Phase 75.
//! The `permission_middleware` and `RoutePermission` enum are deferred to future phases.

use std::sync::Arc;

use axum::{
    extract::Request,
    extract::State,
    http::{header::AUTHORIZATION, StatusCode},
    middleware::Next,
    response::IntoResponse,
};
use url::form_urlencoded;

use super::claims::SessionTokenClaims;
use super::rate_limiter::{RateLimitDecision, AUTHENTICATED_MAX_REQUESTS};

/// Marker type for storing the client_id (PID string) in request extensions.
/// This allows both auth_extractor_middleware and rate_limit_middleware
/// to share the same client identifier without type conflicts.
#[derive(Clone, Debug)]
pub struct ClientId(pub String);

/// Rate limiting middleware.
///
/// Reads the `ClientId` from request extensions (set by `auth_extractor_middleware`)
/// and checks the rate limiter. Returns 429 if the client has exceeded the configured limit.
///
/// For pre-auth routes, no `ClientId` will be present and the request passes through
/// without rate limiting (rate limiting for pre-auth routes uses a different mechanism
/// in future phases).
pub async fn rate_limit_middleware(
    State(state): State<Arc<super::super::api::server::DaemonApiState>>,
    request: Request,
    next: Next,
) -> axum::response::Response {
    // For authenticated routes, ClientId is set by auth_extractor_middleware
    // For pre-auth routes, this middleware is not applied (handled by future phases)
    let client_id = request
        .extensions()
        .get::<ClientId>()
        .map(|c| c.0.clone())
        .unwrap_or_else(|| "unknown".to_string());

    // Authenticated routes: a high per-PID backstop, not a fine-grained throttle.
    // Anything here has passed L2 (valid JWT + whitelisted PID), so it is a trusted
    // same-host process; the budget exists only to contain a runaway loop.
    if let RateLimitDecision::Limited { retry_after_secs } = state
        .security
        .rate_limiter
        .check(&client_id, AUTHENTICATED_MAX_REQUESTS)
        .await
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            axum::Json(serde_json::json!({
                "error": "rate_limit_exceeded",
                "retry_after_secs": retry_after_secs
            })),
        )
            .into_response();
    }

    next.run(request).await
}

/// JWT authentication middleware.
///
/// Extracts the `Authorization` header, parses the `Session <token>` prefix,
/// verifies the JWT using the daemon's secret, checks PID whitelist membership,
/// and stores the validated claims and client_id in request extensions for downstream handlers.
///
/// Returns:
/// - 401 if no Authorization header is present
/// - 401 if the token is invalid or expired
/// - 403 if the client PID is not in the whitelist
pub async fn auth_extractor_middleware(
    State(state): State<Arc<super::super::api::server::DaemonApiState>>,
    mut request: Request,
    next: Next,
) -> axum::response::Response {
    // Extract Authorization header first, then fall back to auth query param for browser fetches.
    let auth_header = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let auth_query = request.uri().query().and_then(|query| {
        form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == "auth")
            .map(|(_, value)| value.into_owned())
    });

    let auth_value = auth_header.map(str::to_owned).or(auth_query);

    let Some(auth_value) = auth_value else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "error": "missing_session_token"
            })),
        )
            .into_response();
    };

    // Parse "Session <token>" prefix — bare bearer tokens are explicitly rejected.
    let Some(token) = auth_value.strip_prefix("Session ") else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "error": "invalid_auth_scheme",
                "message": "expected 'Session ' prefix; use POST /auth/connect to obtain a session token"
            })),
        )
            .into_response();
    };
    let token = token.trim();
    if token.is_empty() {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "error": "missing_session_token"
            })),
        )
            .into_response();
    }

    // Verify JWT
    let claims = match SessionTokenClaims::verify(token, &state.security.jwt_secret) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "JWT verification failed");
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "error": "invalid_session_token"
                })),
            )
                .into_response();
        }
    };

    // Check PID whitelist
    if !state.security.is_pid_allowed(claims.pid).await {
        tracing::warn!(pid = claims.pid, "PID not in whitelist");
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "error": "pid_not_allowed"
            })),
        )
            .into_response();
    }

    // Store client_id for rate_limit_middleware before moving claims
    let client_id = ClientId(claims.pid.to_string());

    // Store claims and client_id in request extensions
    request.extensions_mut().insert(claims);
    request.extensions_mut().insert(client_id);

    next.run(request).await
}

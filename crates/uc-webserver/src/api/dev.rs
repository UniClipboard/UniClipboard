//! Dev-only HTTP endpoints for local development and debugging.
//!
//! **WARNING**: This module is only compiled in debug builds (`#[cfg(debug_assertions)]`).
//! It MUST NOT be present in release/production builds.
//!
//! ## Endpoints
//!
//! - `POST /auth/dev-token` — Obtain a JWT session token without bearer auth.
//!   Intended for Swagger UI, curl, or other local debugging tools.
//!
//! ## Security
//!
//! This module intentionally bypasses bearer token validation. It is guarded by:
//! 1. `#[cfg(debug_assertions)]` — ensures it never compiles into release builds.
//! 2. `UC_DEV_AUTH_BYPASS` env var must be set to `"true"` at runtime.
//!
//! The env var check provides an additional runtime gate so that even debug builds
//! require explicit opt-in when needed.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};

use crate::api::server::DaemonApiState;
use crate::security::claims::{SessionTokenClaims, LEVEL_L2, REFRESH_AT_SECS, TTL_SECS};

/// Check whether dev auth bypass is enabled at runtime.
///
/// Enabled when `UNICLIPBOARD_ENV=development` — this is set by the CLI
/// when `--dev` is passed, and propagated to the daemon process.
fn is_dev_bypass_enabled() -> bool {
    std::env::var("UNICLIPBOARD_ENV")
        .map(|v| v.trim().to_lowercase() == "development")
        .unwrap_or(false)
}

/// Request query params for GET /auth/dev-token
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevTokenQuery {
    /// Client process ID. Defaults to the current process ID.
    #[serde(default = "default_pid")]
    pub pid: u32,
    /// Client type. Defaults to "dev".
    #[serde(default = "default_client_type")]
    pub client_type: String,
}

fn default_pid() -> u32 {
    std::process::id()
}

fn default_client_type() -> String {
    "dev".to_string()
}

/// Response body for POST /auth/dev-token
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DevTokenResponse {
    /// HS256-signed JWT session token.
    pub session_token: String,
    /// Token time-to-live in seconds (5 minutes).
    pub expires_in_secs: i64,
    /// Recommended refresh time in seconds (4 minutes).
    pub refresh_at_secs: i64,
}

/// Build the dev-only auth router.
///
/// Routes are registered on the public L1 router — no auth middleware applied.
/// Compiled only in debug builds; empty router in release.
pub fn router(state: DaemonApiState) -> Router<DaemonApiState> {
    Router::new()
        .route("/auth/dev-token", post(dev_token_handler))
        .with_state(Arc::new(state))
}

/// POST /auth/dev-token
///
/// Obtain a JWT session token without bearer auth. Designed for local development
/// and debugging with tools like Swagger UI or curl.
///
/// Security: guarded by `#[cfg(debug_assertions)]` at compile time and
/// `UC_DEV_AUTH_BYPASS=true` env var at runtime.
///
/// ## Access
///
/// - OpenAPI: marked as dev-only
/// - Auth required: none (this IS the dev auth endpoint)
#[utoipa::path(
    post,
    path = "/auth/dev-token",
    tag = "dev",
    params(
        ("pid" = u32, Query, description = "Client process ID", example = 12345),
        ("client_type" = String, Query, description = "Client type", example = "gui")
    ),
    responses(
        (status = 200, description = "JWT session token issued", body = DevTokenResponse),
        (status = 403, description = "Dev bypass not enabled — set UC_DEV_AUTH_BYPASS=true")
    )
)]
pub async fn dev_token_handler(
    State(state): State<Arc<DaemonApiState>>,
    Query(query): Query<DevTokenQuery>,
) -> axum::response::Response {
    if !is_dev_bypass_enabled() {
        tracing::warn!(
            "dev-token endpoint hit but UC_DEV_AUTH_BYPASS is not set — denying request"
        );
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "dev_bypass_not_enabled",
                "hint": "pass --dev to the CLI to enable this endpoint"
            })),
        )
            .into_response();
    }

    tracing::debug!(
        pid = query.pid,
        client_type = %query.client_type,
        "issuing dev session token"
    );

    // Auto-register the dev token's PID in the whitelist so that the JWT
    // middleware's PID check passes without requiring a separate /auth/connect call.
    state.security.register_pid(query.pid).await;

    let claims = SessionTokenClaims::new(query.pid, query.client_type, LEVEL_L2, false);

    let token = match claims.sign(state.security.jwt_secret.as_ref()) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e, "failed to sign dev session token");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "token_generation_failed"})),
            )
                .into_response();
        }
    };

    (
        StatusCode::OK,
        Json(DevTokenResponse {
            session_token: token,
            expires_in_secs: TTL_SECS,
            refresh_at_secs: REFRESH_AT_SECS,
        }),
    )
        .into_response()
}

/// Dev-only OpenAPI spec. Exposes `POST /auth/dev-token` in debug builds.
///
/// This is defined HERE (inside the `dev` module) rather than in `openapi.rs`
/// so that utoipa generates the `__path_dev_token_handler` module relative to
/// this file. If defined in `openapi.rs`, utoipa would generate a path reference
/// to `crate::api::dev::__path_dev_token_handler`, which fails in release builds
/// because the `dev` module doesn't exist.
#[derive(OpenApi)]
#[openapi(
    paths(dev_token_handler),
    components(schemas(DevTokenResponse)),
    tags(
        (name = "dev", description = "Dev-only endpoints — not available in release builds")
    )
)]
pub struct ApiDocDev;

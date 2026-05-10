//! HTTP route handlers for the daemon API.
//!
//! Router is split into two tiers:
//! - L1 (router_l1): public endpoints requiring no authentication (health check)
//! - L2+ (router_l2_plus): protected endpoints behind auth_extractor + rate_limit middleware
//!
//! Middleware request order:
//!   cors_middleware runs FIRST and wraps all responses
//!   auth_extractor runs SECOND -> validates JWT + PID whitelist -> sets client_id
//!   rate_limit runs THIRD -> checks rate limit using client_id from extensions
//!
//! L3/L4 permission enforcement is NOT implemented in Phase 75 (deferred to future phases).

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use uc_application::facade::ClipboardRestoreError;
use uc_daemon_contract::constants::http_route;

use crate::api::server::DaemonApiState;
use crate::security::middleware::{auth_extractor_middleware, rate_limit_middleware};

/// Build the L1 (public) router - no auth required.
/// Contains only the health check endpoint.
///
/// Takes state to return Router<DaemonApiState> so it can be merged
/// with router_l2_plus without type mismatch.
pub fn router_l1(state: DaemonApiState) -> Router<DaemonApiState> {
    let mut router = Router::new()
        .route("/health", get(health))
        .with_state(state.clone());

    #[cfg(debug_assertions)]
    {
        router = router.merge(crate::api::dev::router(state));
    }

    // NOTE: cors_middleware is applied once at the outermost layer in
    // `build_router` so it wraps all merged sub-routers. Do not re-layer it
    // here or each request will traverse CORS twice.
    router
}

/// Build the L2+ (protected) router - requires valid session token.
/// All routes are behind auth_extractor -> rate_limit middleware layers.
/// CORS wrapping is applied once at the outermost level in `build_router`.
///
/// LAYER ORDER (FINDING-2): In Axum, the LAST `.layer()` call runs FIRST on
/// incoming requests and sees responses returned by inner layers. We want:
/// - auth_extractor to run before rate_limit
/// - rate_limit to run after auth_extractor has populated client_id
/// - CORS (applied outside this function) to wrap the whole chain so
///   auth/rate-limit rejections still include CORS headers
///
/// Therefore the order inside this function must be:
///   .layer(rate_limit_middleware)      // innermost -> runs THIRD
///   .layer(auth_extractor_middleware)  // outer of these two -> runs SECOND
///
/// The outer cors_middleware in `build_router` then runs FIRST on the merged
/// router, before either of these layers executes.
///
/// This means rate limiting applies to already-authenticated requests (by validated PID).
/// It is NOT a pre-auth gate - that is a deliberate design choice for Phase 75.
///
/// NOTE on L3/L4: Phase 75 does NOT implement L3/L4 permission enforcement.
/// The middleware chain enforces only L2 (valid JWT + PID whitelist).
/// L3/L4 checks (encryption_ready state) are reserved for future phases.
pub fn router_l2_plus(state: DaemonApiState) -> Router<DaemonApiState> {
    let router = Router::new()
        .merge(crate::api::clipboard::router())
        .merge(crate::api::search::router())
        .merge(crate::api::device::router())
        .merge(crate::api::member::router())
        .merge(crate::api::settings::router())
        .merge(crate::api::v2::router())
        .merge(crate::api::encryption::router())
        .merge(crate::api::storage::router())
        .merge(crate::api::pairing::router())
        .merge(crate::api::blob::router())
        .merge(crate::api::upgrade::router())
        .route("/status", get(status))
        .route("/peers", get(peers))
        .route("/paired-devices", get(paired_devices))
        .route("/presence/refresh", post(refresh_presence))
        .merge(crate::api::lifecycle::router())
        .route(
            &format!("{}/:entry_id", http_route::CLIPBOARD_RESTORE),
            post(restore_clipboard_entry_handler),
        )
        .with_state(state.clone());

    // Apply middleware layers.
    // NOTE: cors_middleware is NOT applied here; it is layered once at the
    // outermost level in `build_router` so it wraps every sub-router exactly
    // once. Browser clients still receive ACAO headers on auth/rate-limit
    // rejections because the outer cors layer wraps this entire chain.
    // auth_extractor runs before rate_limit and sets client_id in extensions.
    let state_for_middleware = Arc::new(state);
    router
        .layer(middleware::from_fn_with_state(
            state_for_middleware.clone(),
            rate_limit_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            state_for_middleware,
            auth_extractor_middleware,
        ))
}

async fn health(State(state): State<DaemonApiState>) -> impl IntoResponse {
    Json(state.health_response())
}

async fn restore_clipboard_entry_handler(
    State(state): State<DaemonApiState>,
    Path(entry_id): Path<String>,
) -> impl IntoResponse {
    let app = match state.app_facade_or_error() {
        Ok(app) => app,
        Err(error) => return error.into_response(),
    };

    tracing::info!(entry_id = %entry_id, "daemon restore request received");

    let restore_facade = match app.clipboard_restore.as_ref() {
        Some(facade) => facade,
        None => {
            return internal_error(anyhow::anyhow!(
                "clipboard_restore facade unavailable in this entry point"
            ))
            .into_response();
        }
    };

    match restore_facade.restore_entry(&entry_id).await {
        Ok(()) => {
            tracing::info!(entry_id = %entry_id, "daemon restore request succeeded");
            restore_success_response().into_response()
        }
        Err(error) => restore_error_to_response(error, &entry_id).into_response(),
    }
}

fn restore_success_response() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(json!({"success": true})))
}

/// Map `ClipboardRestoreError` to (status, JSON body).
///
/// Free function so the status-code contract is unit-testable without
/// spinning up an axum app or `DaemonApiState`. The handler above is a
/// thin wrapper around this.
fn restore_error_to_response(
    error: ClipboardRestoreError,
    entry_id: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    match error {
        ClipboardRestoreError::NotFound => {
            tracing::warn!(entry_id = %entry_id, "daemon restore: entry not found");
            (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"})))
        }
        ClipboardRestoreError::PayloadUnavailable {
            entry_id: e_id,
            rep_id,
            state,
        } => {
            // Known business outcome — content has logically vanished.
            // Use 410 Gone (resource is no longer available) and log at
            // warn level so this does NOT escalate to a Sentry error.
            tracing::warn!(
                entry_id = %e_id,
                rep_id = %rep_id,
                payload_state = %state,
                "daemon restore: payload unavailable (orphaned/lost)"
            );
            (
                StatusCode::GONE,
                Json(json!({
                    "error": "payload_unavailable",
                    "entry_id": e_id,
                    "rep_id": rep_id,
                    "state": state,
                })),
            )
        }
        ClipboardRestoreError::Internal(message) => {
            tracing::error!(
                entry_id = %entry_id,
                error = %message,
                "daemon restore failed (internal)"
            );
            internal_error(anyhow::anyhow!(message))
        }
    }
}

async fn status(State(state): State<DaemonApiState>) -> impl IntoResponse {
    Json(state.status_response()).into_response()
}

async fn peers(State(state): State<DaemonApiState>) -> impl IntoResponse {
    match state.peer_snapshots().await {
        Ok(response) => Json(response).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn paired_devices(State(state): State<DaemonApiState>) -> impl IntoResponse {
    match state.paired_devices().await {
        Ok(response) => Json(response).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn refresh_presence(State(state): State<DaemonApiState>) -> impl IntoResponse {
    match state.refresh_presence().await {
        Ok(response) => Json(response).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

/// NOTE: Individual API handlers now use `ApiError::unauthorized()` directly.
/// This helper is kept for backward compatibility with the security middleware layer.
pub(crate) fn internal_error(error: anyhow::Error) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!(error = %error, "daemon API request failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": "internal_error"})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_value(json: Json<serde_json::Value>) -> serde_json::Value {
        json.0
    }

    #[test]
    fn restore_success_returns_200_with_success_true() {
        let (status, body) = restore_success_response();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body_value(body), json!({"success": true}));
    }

    #[test]
    fn restore_not_found_returns_404() {
        let (status, body) = restore_error_to_response(ClipboardRestoreError::NotFound, "entry-1");
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body_value(body), json!({"error": "not_found"}));
    }

    #[test]
    fn restore_payload_unavailable_returns_410_with_full_context() {
        let (status, body) = restore_error_to_response(
            ClipboardRestoreError::PayloadUnavailable {
                entry_id: "entry-1".to_string(),
                rep_id: "rep-2".to_string(),
                state: "Lost".to_string(),
            },
            "entry-1",
        );
        // 410 Gone — known business outcome, never 500
        assert_eq!(status, StatusCode::GONE);
        assert_eq!(
            body_value(body),
            json!({
                "error": "payload_unavailable",
                "entry_id": "entry-1",
                "rep_id": "rep-2",
                "state": "Lost",
            })
        );
    }

    #[test]
    fn restore_payload_unavailable_with_orphaned_state_uses_state_string_verbatim() {
        let (status, body) = restore_error_to_response(
            ClipboardRestoreError::PayloadUnavailable {
                entry_id: "e".to_string(),
                rep_id: "r".to_string(),
                state: "Staged".to_string(),
            },
            "e",
        );
        assert_eq!(status, StatusCode::GONE);
        let value = body_value(body);
        assert_eq!(value["state"], "Staged");
    }

    #[test]
    fn restore_internal_returns_500_with_generic_body() {
        let (status, body) = restore_error_to_response(
            ClipboardRestoreError::Internal("write coordinator deadlocked".to_string()),
            "entry-3",
        );
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        // 内部错误细节不能泄漏到响应 body
        assert_eq!(body_value(body), json!({"error": "internal_error"}));
    }

    #[test]
    fn internal_error_returns_500_with_generic_body() {
        let (status, body) = internal_error(anyhow::anyhow!("boom"));
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body_value(body), json!({"error": "internal_error"}));
    }
}

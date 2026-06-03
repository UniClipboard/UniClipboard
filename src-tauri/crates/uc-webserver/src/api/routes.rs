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

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use uc_application::facade::ClipboardRestoreError;
use uc_daemon_contract::api::dto::clipboard_command::RestoreEntryResponse;
use uc_daemon_contract::api::dto::envelope::{
    ApiEnvelope, HealthEnvelope, PeerSnapshotListEnvelope, PresenceRefreshEnvelope,
    RestoreEntryEnvelope, SpaceMemberListEnvelope, StatusEnvelope,
};
use uc_daemon_contract::api::dto::error::ApiErrorResponse;
use uc_daemon_contract::constants::http_route;

use crate::api::dto::error::{log_facade_failure, ApiError};
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
        .merge(crate::api::mobile_sync::router())
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

/// GET /health
///
/// Public (L1) liveness probe. Returns the daemon status string plus version
/// metadata wrapped in the canonical `{ data, ts }` envelope (ADR-008 §0.2).
/// The previous bare `{ status, ... }` shape is retired.
#[utoipa::path(
    get,
    path = "/health",
    operation_id = "getHealth",
    tag = "system",
    responses(
        (status = 200, description = "Daemon is alive", body = HealthEnvelope)
    ),
    security(())
)]
async fn health(State(state): State<DaemonApiState>) -> Json<HealthEnvelope> {
    Json(ApiEnvelope::now(state.health_response()))
}

/// Restore endpoint 的可选 query 参数。
///
/// `plain=true` 时走「以纯文本形式恢复」路径——只把 `text/plain` 表示写入
/// 系统剪贴板，让目标应用别无选择地粘出纯文本（Markdown 源码 / HTML 标签 /
/// RTF 等富文本被剔除）。条目若没有 plain 表示，facade 静默降级为多格式恢复。
///
/// `plain=false` 或缺省时与历史行为完全一致：多格式恢复。
#[derive(Debug, Default, serde::Deserialize)]
struct RestoreQuery {
    #[serde(default)]
    plain: bool,
}

/// POST /clipboard/restore/{entry_id}
///
/// Re-apply a stored clipboard entry to the local system clipboard. Wrapped in
/// the canonical `{ data, ts }` envelope (ADR-008 §0.1/§0.2); errors use the
/// canonical `ApiErrorResponse`. The `payload_unavailable` (410) error carries
/// `{ entry_id, rep_id, state }` in `details` (§0.3); the `code`/`message`
/// strings are LOAD-BEARING and preserved.
#[utoipa::path(
    post,
    path = "/clipboard/restore/{entry_id}",
    operation_id = "restoreClipboardEntry",
    tag = "clipboard",
    params(
        ("entry_id" = String, Path, description = "Clipboard entry id to restore"),
        ("plain" = Option<bool>, Query, description = "Restore as plain text only (strip rich representations)")
    ),
    responses(
        (status = 200, description = "Entry restored to the system clipboard", body = RestoreEntryEnvelope),
        (status = 404, description = "Entry not found", body = ApiErrorResponse),
        (status = 410, description = "Entry payload is no longer available (orphaned/lost)", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
async fn restore_clipboard_entry_handler(
    State(state): State<DaemonApiState>,
    Path(entry_id): Path<String>,
    Query(query): Query<RestoreQuery>,
) -> impl IntoResponse {
    let app = match state.app_facade_or_error() {
        Ok(app) => app,
        Err(error) => return error.into_response(),
    };

    tracing::info!(
        entry_id = %entry_id,
        plain = query.plain,
        "daemon restore request received"
    );

    let restore_facade = match app.clipboard_restore.as_ref() {
        Some(facade) => facade,
        None => {
            return ApiError::internal(
                "clipboard_restore facade unavailable in this entry point".to_string(),
            )
            .into_response();
        }
    };

    let op: &'static str = if query.plain {
        "restore_entry_as_plain_text"
    } else {
        "restore_entry"
    };
    let result = if query.plain {
        restore_facade.restore_entry_as_plain_text(&entry_id).await
    } else {
        restore_facade.restore_entry(&entry_id).await
    };

    match result {
        Ok(()) => {
            tracing::info!(
                entry_id = %entry_id,
                plain = query.plain,
                "daemon restore request succeeded"
            );
            let (status, body) = restore_success_response();
            (status, body).into_response()
        }
        Err(error) => {
            let (status, body) = restore_error_to_response(op, error, &entry_id);
            (status, body).into_response()
        }
    }
}

/// Build the canonical 200 success body for a restore.
///
/// Free function so the status-code contract is unit-testable without
/// spinning up an axum app or `DaemonApiState`. The handler above is a thin
/// wrapper around this.
fn restore_success_response() -> (StatusCode, Json<RestoreEntryEnvelope>) {
    (
        StatusCode::OK,
        Json(ApiEnvelope::now(RestoreEntryResponse { success: true })),
    )
}

/// Map `ClipboardRestoreError` to (status, canonical `ApiErrorResponse` body).
///
/// Free function so the status-code + error-shape contract is unit-testable
/// without spinning up an axum app or `DaemonApiState`. `code`/`message` are
/// LOAD-BEARING (consumers substring-match them) and must not be reworded.
fn restore_error_to_response(
    op: &'static str,
    error: ClipboardRestoreError,
    entry_id: &str,
) -> (StatusCode, Json<ApiErrorResponse>) {
    use ClipboardRestoreError as E;
    match error {
        E::NotFound => {
            tracing::warn!(entry_id = %entry_id, "daemon restore: entry not found");
            (
                StatusCode::NOT_FOUND,
                Json(ApiErrorResponse::new(
                    "not_found",
                    "clipboard entry not found",
                )),
            )
        }
        E::PayloadUnavailable {
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
                Json(ApiErrorResponse::with_details(
                    "payload_unavailable",
                    "clipboard entry payload is no longer available",
                    json!({
                        "entry_id": e_id,
                        "rep_id": rep_id,
                        "state": state,
                    }),
                )),
            )
        }
        E::Internal(message) => {
            log_facade_failure(
                "clipboard_restore",
                op,
                "internal",
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("entry {entry_id} restore failed: {message}"),
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiErrorResponse::new("internal_error", "internal error")),
            )
        }
    }
}

/// GET /status
///
/// Diagnostic snapshot: version metadata, uptime, and worker health. Wrapped
/// in the canonical `{ data, ts }` envelope (ADR-008 §0.2); the previous bare
/// object shape is retired.
#[utoipa::path(
    get,
    path = "/status",
    operation_id = "getStatus",
    tag = "system",
    responses(
        (status = 200, description = "Daemon status snapshot", body = StatusEnvelope)
    )
)]
async fn status(State(state): State<DaemonApiState>) -> Json<StatusEnvelope> {
    Json(ApiEnvelope::now(state.status_response()))
}

/// GET /peers
///
/// List discovered peer snapshots (topology view). Wrapped in the canonical
/// `{ data, ts }` envelope (ADR-008 §0.2); the previous bare top-level array
/// is retired.
#[utoipa::path(
    get,
    path = "/peers",
    operation_id = "listPeers",
    tag = "system",
    responses(
        (status = 200, description = "Peer snapshot list", body = PeerSnapshotListEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
async fn peers(
    State(state): State<DaemonApiState>,
) -> Result<Json<PeerSnapshotListEnvelope>, ApiError> {
    let response = state
        .peer_snapshots()
        .await
        .map_err(|error| diagnostics_internal_error("peers", error))?;
    Ok(Json(ApiEnvelope::now(response)))
}

/// GET /paired-devices
///
/// List paired space members with presence. Wrapped in the canonical
/// `{ data, ts }` envelope (ADR-008 §0.2); the previous bare top-level array
/// is retired.
#[utoipa::path(
    get,
    path = "/paired-devices",
    operation_id = "listPairedDevices",
    tag = "system",
    responses(
        (status = 200, description = "Paired space-member list", body = SpaceMemberListEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
async fn paired_devices(
    State(state): State<DaemonApiState>,
) -> Result<Json<SpaceMemberListEnvelope>, ApiError> {
    let response = state
        .paired_devices()
        .await
        .map_err(|error| diagnostics_internal_error("paired_devices", error))?;
    Ok(Json(ApiEnvelope::now(response)))
}

/// POST /presence/refresh
///
/// Actively probe paired peers' reachability and return the round's counters.
/// Wrapped in the canonical `{ data, ts }` envelope (ADR-008 §0.2); the
/// previous bare object (counters at top level) is retired.
#[utoipa::path(
    post,
    path = "/presence/refresh",
    operation_id = "refreshPresence",
    tag = "system",
    responses(
        (status = 200, description = "Presence refresh round completed", body = PresenceRefreshEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
async fn refresh_presence(
    State(state): State<DaemonApiState>,
) -> Result<Json<PresenceRefreshEnvelope>, ApiError> {
    let response = state
        .refresh_presence()
        .await
        .map_err(|error| diagnostics_internal_error("refresh_presence", error))?;
    Ok(Json(ApiEnvelope::now(response)))
}

/// Map a diagnostics-handler `anyhow::Error` onto a canonical 500 `ApiError`,
/// preserving the structured `facade / op` Sentry signal previously emitted by
/// the legacy `internal_error` helper. Used by the `system` topology handlers
/// (peers / paired_devices / refresh_presence) now that they return
/// `ApiErrorResponse` instead of the ad-hoc `{ "error": "internal_error" }`
/// body.
fn diagnostics_internal_error(op: &'static str, error: anyhow::Error) -> ApiError {
    log_facade_failure(
        "daemon_api",
        op,
        "internal",
        StatusCode::INTERNAL_SERVER_ERROR,
        &error.to_string(),
    );
    ApiError::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_success_returns_200_with_enveloped_success_true() {
        let (status, body) = restore_success_response();
        assert_eq!(status, StatusCode::OK);
        // Canonical `{ data: { success: true }, ts }` envelope (ADR-008 §0.1).
        assert!(body.0.data.success);
    }

    #[test]
    fn restore_not_found_returns_404_with_not_found_code() {
        let (status, body) =
            restore_error_to_response("restore_entry", ClipboardRestoreError::NotFound, "entry-1");
        assert_eq!(status, StatusCode::NOT_FOUND);
        // `code` token is LOAD-BEARING and preserved.
        assert_eq!(body.0.code, "not_found");
        assert!(body.0.details.is_none());
    }

    #[test]
    fn restore_payload_unavailable_returns_410_with_full_context_in_details() {
        let (status, body) = restore_error_to_response(
            "restore_entry",
            ClipboardRestoreError::PayloadUnavailable {
                entry_id: "entry-1".to_string(),
                rep_id: "rep-2".to_string(),
                state: "Lost".to_string(),
            },
            "entry-1",
        );
        // 410 Gone — known business outcome, never 500
        assert_eq!(status, StatusCode::GONE);
        assert_eq!(body.0.code, "payload_unavailable");
        // The 410 context moves into `details` (ADR-008 §0.3).
        let details = body.0.details.expect("410 must carry structured details");
        assert_eq!(details["entry_id"], "entry-1");
        assert_eq!(details["rep_id"], "rep-2");
        assert_eq!(details["state"], "Lost");
    }

    #[test]
    fn restore_payload_unavailable_with_orphaned_state_uses_state_string_verbatim() {
        let (status, body) = restore_error_to_response(
            "restore_entry",
            ClipboardRestoreError::PayloadUnavailable {
                entry_id: "e".to_string(),
                rep_id: "r".to_string(),
                state: "Staged".to_string(),
            },
            "e",
        );
        assert_eq!(status, StatusCode::GONE);
        let details = body.0.details.expect("410 must carry structured details");
        assert_eq!(details["state"], "Staged");
    }

    #[test]
    fn restore_internal_returns_500_with_generic_body() {
        let (status, body) = restore_error_to_response(
            "restore_entry",
            ClipboardRestoreError::Internal("write coordinator deadlocked".to_string()),
            "entry-3",
        );
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        // 内部错误细节不能泄漏到响应 body — only the generic code/message.
        assert_eq!(body.0.code, "internal_error");
        assert_eq!(body.0.message, "internal error");
    }
}

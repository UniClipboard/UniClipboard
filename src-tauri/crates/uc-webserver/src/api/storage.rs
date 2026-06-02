//! HTTP route handlers for storage management endpoints.
//!
//! Provides GET /storage/stats and POST /storage/clear-cache.

use axum::extract::{rejection::JsonRejection, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use serde_json::json;

// Storage DTOs relocated to the contract crate (ADR-008 §C.4). The handlers keep
// their current JSON shape; `StorageStatsResponse` is an alias for the moved
// `StorageStatsDto` so the handler bodies stay unchanged.
use uc_daemon_contract::api::dto::storage::{
    ClearCacheRequest, ClearCacheResponse, StorageStatsDto as StorageStatsResponse,
};

use crate::api::routes::internal_error;
use crate::api::server::DaemonApiState;

/// Error response for POST /storage/clear-cache when confirmed is false or absent.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearCacheErrorResponse {
    pub code: String,
    pub message: String,
}

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/storage/stats", get(get_storage_stats_handler))
        .route("/storage/clear-cache", post(clear_cache_handler))
}

/// GET /storage/stats
/// Returns storage statistics across database, cache, and spool directories.
/// Includes blob_count derived from the total number of clipboard entries.
async fn get_storage_stats_handler(State(state): State<DaemonApiState>) -> impl IntoResponse {
    let app = match state.app_facade_or_error() {
        Ok(app) => app,
        Err(error) => return error.into_response(),
    };

    let result = match app.storage.stats().await {
        Ok(r) => r,
        Err(e) => {
            return internal_error("storage_stats", anyhow::anyhow!("{}", e)).into_response();
        }
    };

    let response = StorageStatsResponse {
        total_bytes: result.total_bytes,
        database_bytes: result.database_bytes,
        vault_bytes: result.vault_bytes,
        cache_bytes: result.cache_bytes,
        logs_bytes: result.logs_bytes,
    };

    let ts = chrono::Utc::now().timestamp_millis();
    Json(json!({ "data": response, "ts": ts })).into_response()
}

/// POST /storage/clear-cache
/// Clears the cache directory contents. Requires `confirmed: true` in the request body.
/// Returns 400 if confirmation is missing or false.
async fn clear_cache_handler(
    State(state): State<DaemonApiState>,
    body: Result<Json<ClearCacheRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Json(req) = match body {
        Ok(b) => b,
        Err(_) => {
            let ts = chrono::Utc::now().timestamp_millis();
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": ClearCacheErrorResponse {
                        code: "confirmation_required".to_string(),
                        message: "confirmed field must be set to true".to_string(),
                    },
                    "ts": ts,
                })),
            )
                .into_response();
        }
    };

    if !req.confirmed {
        let ts = chrono::Utc::now().timestamp_millis();
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": ClearCacheErrorResponse {
                    code: "confirmation_required".to_string(),
                    message: "confirmed field must be set to true".to_string(),
                },
                "ts": ts,
            })),
        )
            .into_response();
    }

    let app = match state.app_facade_or_error() {
        Ok(app) => app,
        Err(error) => return error.into_response(),
    };

    match app.storage.clear_cache().await {
        Ok(result) => {
            tracing::info!(
                freed_bytes = result.freed_bytes,
                "Cache cleared via HTTP API"
            );
            let ts = chrono::Utc::now().timestamp_millis();
            Json(
                json!({ "data": ClearCacheResponse { freed_bytes: result.freed_bytes }, "ts": ts }),
            )
            .into_response()
        }
        Err(e) => internal_error("storage_clear_cache", anyhow::anyhow!("{}", e)).into_response(),
    }
}

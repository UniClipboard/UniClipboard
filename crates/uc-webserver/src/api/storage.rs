//! HTTP route handlers for storage management endpoints.
//!
//! Provides GET /storage/stats and POST /storage/clear-cache.
//!
//! All responses use the canonical `ApiEnvelope<T> { data, ts }` success
//! envelope (ADR-008 §0.1) and `ApiErrorResponse { code, message, details? }`
//! for errors (§0.3). Storage DTOs live in the contract crate (§C.4).

use axum::extract::{rejection::JsonRejection, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_engine::internal::storage::{
    CLEAR_STORAGE_CACHE_FAILED_CODE, QUERY_STORAGE_STATS_FAILED_CODE,
};
use uc_engine::{EngineError, Operation, OperationResult};

// Storage DTOs relocated to the contract crate (ADR-008 §C.4). The handlers keep
// their current JSON shape; both endpoints are non-breaking (`{ data, ts }`).
use uc_daemon_contract::api::dto::storage::{
    ClearCacheRequest, ClearCacheResponse, StorageStatsDto,
};

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::server::DaemonApiState;

fn map_storage_engine_err(op: &'static str, error: EngineError) -> ApiError {
    let variant = match error.code() {
        QUERY_STORAGE_STATS_FAILED_CODE => "stats",
        CLEAR_STORAGE_CACHE_FAILED_CODE => "clear_cache",
        _ => "unexpected_engine_error",
    };
    let api = ApiError::internal("storage operation failed");
    log_facade_failure("storage", op, variant, api.status, &api.message);
    api
}

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/storage/stats", get(get_storage_stats_handler))
        .route("/storage/clear-cache", post(clear_cache_handler))
}

/// GET /storage/stats
/// Returns storage statistics across database, cache, and spool directories.
/// Includes blob_count derived from the total number of clipboard entries.
#[utoipa::path(
    get,
    path = "/storage/stats",
    operation_id = "getStorageStats",
    tag = "storage",
    responses(
        (status = 200, description = "Storage statistics retrieved", body = StorageStatsEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_storage_stats_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<StorageStatsDto>>, ApiError> {
    let result = state
        .execute(Operation::QueryStorageStats)
        .await
        .map_err(|error| map_storage_engine_err("storage_stats", error))?;
    let OperationResult::StorageStats(result) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected storage stats result",
        ));
    };

    Ok(Json(ApiEnvelope::now(StorageStatsDto {
        total_bytes: result.total_bytes,
        database_bytes: result.database_bytes,
        vault_bytes: result.vault_bytes,
        cache_bytes: result.cache_bytes,
        logs_bytes: result.logs_bytes,
    })))
}

/// POST /storage/clear-cache
/// Clears the cache directory contents. Requires `confirmed: true` in the request body.
/// Returns 400 if confirmation is missing or false.
#[utoipa::path(
    post,
    path = "/storage/clear-cache",
    operation_id = "clearStorageCache",
    tag = "storage",
    request_body = ClearCacheRequest,
    responses(
        (status = 200, description = "Cache cleared", body = ClearCacheEnvelope),
        (status = 400, description = "Confirmation missing or false", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn clear_cache_handler(
    State(state): State<DaemonApiState>,
    body: Result<Json<ClearCacheRequest>, JsonRejection>,
) -> Result<Json<ApiEnvelope<ClearCacheResponse>>, ApiError> {
    let req = match body {
        Ok(Json(req)) if req.confirmed => req,
        // Missing/invalid body OR `confirmed` not set to true → 400 with the
        // canonical error body. Preserve the exact `code`/`message` strings.
        _ => {
            return Err(ApiError {
                status: StatusCode::BAD_REQUEST,
                code: "confirmation_required".to_string(),
                message: "confirmed field must be set to true".to_string(),
                details: None,
            });
        }
    };

    debug_assert!(req.confirmed);

    let result = state
        .execute(Operation::ClearStorageCache)
        .await
        .map_err(|error| map_storage_engine_err("storage_clear_cache", error))?;
    let OperationResult::StorageCacheCleared { freed_bytes } = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected clear-cache result",
        ));
    };

    tracing::info!(freed_bytes, "Cache cleared via HTTP API");

    Ok(Json(ApiEnvelope::now(ClearCacheResponse { freed_bytes })))
}

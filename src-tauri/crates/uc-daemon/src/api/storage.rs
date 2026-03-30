//! HTTP route handlers for storage management endpoints.
//!
//! Provides GET /storage/stats and POST /storage/clear-cache.

use axum::extract::{rejection::JsonRejection, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use uc_app::usecases::CoreUseCases;

use crate::api::server::DaemonApiState;
use crate::api::routes::internal_error;

/// Response payload for GET /storage/stats.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatsResponse {
    pub total_size_bytes: u64,
    pub blob_count: usize,
    pub database_size_bytes: u64,
    pub cache_size_bytes: u64,
    pub spool_size_bytes: u64,
}

/// Request payload for POST /storage/clear-cache.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearCacheRequest {
    pub confirmed: bool,
}

/// Response payload for POST /storage/clear-cache on success.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearCacheResponse {
    pub freed_bytes: u64,
}

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
async fn get_storage_stats_handler(
    State(state): State<DaemonApiState>,
) -> impl IntoResponse {
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let usecases = CoreUseCases::new(runtime.as_ref());
    let spool_dir = runtime.storage_paths().spool_dir.clone();

    // Run three independent async operations concurrently:
    // 1. Storage stats (db, vault, cache, logs sizes)
    // 2. Clipboard entries list for blob_count
    // 3. Spool directory size
    let storage_result = match usecases.get_storage_stats().execute().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "Failed to compute storage stats");
            return internal_error(anyhow::anyhow!("{}", e)).into_response();
        }
    };

    let entries = match usecases.list_clipboard_entries().execute(10_000, 0).await {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, "Failed to list clipboard entries for blob count");
            return internal_error(anyhow::anyhow!("{}", e)).into_response();
        }
    };

    let spool_size = compute_dir_size(&spool_dir).await.unwrap_or(0);

    let blob_count = entries.len();

    let response = StorageStatsResponse {
        total_size_bytes: storage_result.total_bytes,
        blob_count,
        database_size_bytes: storage_result.database_bytes,
        cache_size_bytes: storage_result.cache_bytes,
        spool_size_bytes: spool_size,
    };

    let ts = chrono::Utc::now().timestamp_millis();
    Json(json!({ "data": response, "ts": ts })).into_response()
}

/// Compute the size of a directory and its contents using tokio::fs.
/// Returns 0 if the path does not exist.
async fn compute_dir_size(path: &Path) -> anyhow::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }

    if path.is_file() {
        let meta = tokio::fs::metadata(path).await?;
        return Ok(meta.len());
    }

    let mut total: u64 = 0;
    let mut entries = tokio::fs::read_dir(path).await?;
    while let Some(entry) = entries.next_entry().await? {
        let entry_path = entry.path();
        if entry_path.is_dir() {
            total += Box::pin(compute_dir_size(&entry_path)).await?;
        } else {
            let meta = tokio::fs::metadata(&entry_path).await?;
            total += meta.len();
        }
    }
    Ok(total)
}

/// POST /storage/clear-cache
/// Clears the cache directory contents. Requires `confirmed: true` in the request body.
/// Returns 400 if confirmation is missing or false.
async fn clear_cache_handler(
    State(state): State<DaemonApiState>,
    body: Result<Json<ClearCacheRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

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

    let usecases = CoreUseCases::new(runtime.as_ref());
    match usecases.clear_cache().execute().await {
        Ok(freed_bytes) => {
            tracing::info!(freed_bytes, "Cache cleared via HTTP API");
            let ts = chrono::Utc::now().timestamp_millis();
            Json(
                json!({ "data": ClearCacheResponse { freed_bytes }, "ts": ts }),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to clear cache");
            internal_error(anyhow::anyhow!("{}", e)).into_response()
        }
    }
}

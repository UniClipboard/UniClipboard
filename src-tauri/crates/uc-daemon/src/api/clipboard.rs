//! HTTP route handlers for clipboard CRUD endpoints.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use uc_app::usecases::clipboard::compute_clipboard_stats;
use uc_app::usecases::CoreUseCases;
use uc_core::clipboard::link_utils::extract_domain;
use uc_core::ids::EntryId;

use crate::api::routes::{internal_error, unauthorized};
use crate::api::server::DaemonApiState;

#[derive(Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    50
}

fn clamp_limit(limit: usize) -> usize {
    // Prevent unbounded queries — cap at 1000 entries per request
    limit.min(1000)
}

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/clipboard/entries", get(list_entries))
        .route("/clipboard/entries/:id", get(get_entry))
        .route("/clipboard/entries/:id", delete(delete_entry))
        .route("/clipboard/entries/:id/favorite", post(toggle_favorite))
        .route("/clipboard/stats", get(get_stats))
        .route("/clipboard/entries/:id/resource", get(get_entry_resource))
}

/// GET /clipboard/entries?limit=50&offset=0
/// Lists clipboard entries with pagination. Returns full EntryProjectionDto array.
/// Populates link_domains from link_urls (per Tauri command pattern).
/// limit is clamped to 1000 to prevent unbounded queries.
async fn list_entries(
    State(state): State<DaemonApiState>,
    headers: HeaderMap,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    if !state.is_authorized(&headers) {
        return unauthorized().into_response();
    }
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let limit = clamp_limit(params.limit);
    let usecases = CoreUseCases::new(runtime.as_ref());
    match usecases
        .list_entry_projections()
        .execute(limit, params.offset)
        .await
    {
        Ok(mut entries) => {
            // Populate link_domains from link_urls
            for dto in &mut entries {
                dto.link_domains = dto
                    .link_urls
                    .as_ref()
                    .map(|urls| urls.iter().filter_map(|u| extract_domain(u)).collect());
            }
            let ts = chrono::Utc::now().timestamp_millis();
            Json(json!({ "data": entries, "ts": ts })).into_response()
        }
        Err(e) => internal_error(anyhow::anyhow!("{}", e)).into_response(),
    }
}

/// GET /clipboard/entries/:id
/// Returns entry detail (full content) or 404 if not found.
/// Uses GetEntryDetailUseCase to return actual text content, not just projection fields.
/// NOTE: GetEntryDetailUseCase only returns entries that are text content (mime starts with "text/"
/// or contains json/xml/javascript). Non-text entries return an error.
async fn get_entry(
    State(state): State<DaemonApiState>,
    headers: HeaderMap,
    Path(entry_id): Path<String>,
) -> impl IntoResponse {
    if !state.is_authorized(&headers) {
        return unauthorized().into_response();
    }
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let parsed_id = EntryId::from(entry_id.clone());
    let usecases = CoreUseCases::new(runtime.as_ref());
    match usecases.get_entry_detail().execute(&parsed_id).await {
        Ok(detail) => {
            let ts = chrono::Utc::now().timestamp_millis();
            Json(json!({ "data": detail, "ts": ts })).into_response()
        }
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            // GetEntryDetailUseCase returns "not found" or "not text content"
            if msg.contains("not found") {
                let ts = chrono::Utc::now().timestamp_millis();
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    Json(json!({ "error": { "code": "not_found", "message": "entry not found" }, "ts": ts })),
                )
                    .into_response();
            }
            // Non-text content returns an error — return 422 Unprocessable Entity
            if msg.contains("not text content") || msg.contains("not text") {
                let ts = chrono::Utc::now().timestamp_millis();
                return (
                    axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({ "error": { "code": "unsupported_content", "message": "entry is not text content" }, "ts": ts })),
                )
                    .into_response();
            }
            internal_error(anyhow::anyhow!("{}", e)).into_response()
        }
    }
}

/// DELETE /clipboard/entries/:id
/// Deletes an entry. Returns 204 on success, 404 if not found.
async fn delete_entry(
    State(state): State<DaemonApiState>,
    headers: HeaderMap,
    Path(entry_id): Path<String>,
) -> impl IntoResponse {
    if !state.is_authorized(&headers) {
        return unauthorized().into_response();
    }
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let parsed_id = EntryId::from(entry_id.clone());
    let usecases = CoreUseCases::new(runtime.as_ref());
    match usecases.delete_clipboard_entry().execute(&parsed_id).await {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            if msg.contains("not found") {
                let ts = chrono::Utc::now().timestamp_millis();
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    Json(json!({ "error": { "code": "not_found", "message": "entry not found" }, "ts": ts })),
                )
                    .into_response();
            }
            internal_error(e).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct ToggleFavoriteBody {
    pub is_favorited: bool,
}

/// POST /clipboard/entries/:id/favorite
/// Body: { "is_favorited": bool }
/// Validates entry existence and acknowledges the favorite toggle request.
/// NOTE: The domain model (ClipboardEntry) does not yet persist an `is_favorited` flag.
/// This endpoint validates entry existence and returns success for known entries.
/// Actual persistence will land when the schema is extended with an `is_favorited` column.
/// Returns 200 on success, 404 if not found.
async fn toggle_favorite(
    State(state): State<DaemonApiState>,
    headers: HeaderMap,
    Path(entry_id): Path<String>,
    body: Result<Json<ToggleFavoriteBody>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    if !state.is_authorized(&headers) {
        return unauthorized().into_response();
    }
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let Json(body) = match body {
        Ok(b) => b,
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({ "error": { "code": "bad_request", "message": "missing is_favorited field" } })),
            )
                .into_response();
        }
    };

    let parsed_id = EntryId::from(entry_id.clone());
    let usecases = CoreUseCases::new(runtime.as_ref());
    match usecases
        .toggle_favorite_clipboard_entry()
        .execute(&parsed_id, body.is_favorited)
        .await
    {
        Ok(true) => {
            let ts = chrono::Utc::now().timestamp_millis();
            Json(json!({ "data": { "success": true }, "ts": ts })).into_response()
        }
        Ok(false) => {
            let ts = chrono::Utc::now().timestamp_millis();
            (
                axum::http::StatusCode::NOT_FOUND,
                Json(json!({ "error": { "code": "not_found", "message": "entry not found" }, "ts": ts })),
            )
                .into_response()
        }
        Err(e) => internal_error(anyhow::anyhow!("{}", e)).into_response(),
    }
}

/// GET /clipboard/stats
/// Returns { total_items, total_size }.
async fn get_stats(
    State(state): State<DaemonApiState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !state.is_authorized(&headers) {
        return unauthorized().into_response();
    }
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let usecases = CoreUseCases::new(runtime.as_ref());
    // Use a large limit to get all entries for stats computation (matching Tauri command pattern)
    match usecases.list_entry_projections().execute(10_000, 0).await {
        Ok(entries) => {
            let stats = compute_clipboard_stats(&entries);
            let ts = chrono::Utc::now().timestamp_millis();
            Json(json!({ "data": stats, "ts": ts })).into_response()
        }
        Err(e) => internal_error(anyhow::anyhow!("{}", e)).into_response(),
    }
}

/// GET /clipboard/entries/:id/resource
/// Returns resource metadata (URL for blob/thumbnail, or content_type + inline metadata).
async fn get_entry_resource(
    State(state): State<DaemonApiState>,
    headers: HeaderMap,
    Path(entry_id): Path<String>,
) -> impl IntoResponse {
    if !state.is_authorized(&headers) {
        return unauthorized().into_response();
    }
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let parsed_id = EntryId::from(entry_id.clone());
    let usecases = CoreUseCases::new(runtime.as_ref());
    match usecases.get_entry_resource().execute(&parsed_id).await {
        Ok(resource) => {
            let ts = chrono::Utc::now().timestamp_millis();
            // EntryResourceResult already derives serde::Serialize
            match serde_json::to_value(&resource) {
                Ok(data) => Json(json!({ "data": data, "ts": ts })).into_response(),
                Err(e) => internal_error(anyhow::anyhow!("failed to serialize resource: {}", e)).into_response(),
            }
        }
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            if msg.contains("not found") {
                let ts = chrono::Utc::now().timestamp_millis();
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    Json(json!({ "error": { "code": "not_found", "message": "entry not found" }, "ts": ts })),
                )
                    .into_response();
            }
            internal_error(e).into_response()
        }
    }
}

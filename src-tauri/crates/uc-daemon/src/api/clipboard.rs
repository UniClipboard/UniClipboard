//! HTTP route handlers for clipboard CRUD endpoints.
//!
//! All routes are protected by the auth_extractor + rate_limit middleware chain
//! applied at the router level (see routes::router_l2_plus).

use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use uc_app::usecases::clipboard::compute_clipboard_stats;
use uc_app::usecases::CoreUseCases;
use uc_core::clipboard::link_utils::extract_domain;
use uc_core::ids::EntryId;

use crate::api::conversion::IntoApiDto;
use crate::api::dto::clipboard::{
    ClearHistoryResponse, EntryProjectionResponseDto, GetClipboardStatsResponse,
    GetEntryDetailResponse, GetEntryResourceResponse, ListEntriesResponse, ToggleFavoriteRequest,
    ToggleFavoriteResponse, ToggleFavoriteResultDto,
};
use crate::api::dto::error::ApiError;
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
        .route("/clipboard/entries/clear", post(clear_history))
        .route("/clipboard/entries/:id", get(get_entry))
        .route("/clipboard/entries/:id", delete(delete_entry))
        .route("/clipboard/entries/:id/favorite", post(toggle_favorite))
        .route("/clipboard/stats", get(get_stats))
        .route("/clipboard/entries/:id/resource", get(get_entry_resource))
}

/// GET /clipboard/entries?limit=50&offset=0
///
/// Lists clipboard entries with pagination. Returns camelCase entry projections.
/// Populates `linkDomains` from `linkUrls`. Limit is clamped to 1000.
#[utoipa::path(
    get,
    path = "/clipboard/entries",
    tag = "clipboard",
    params(
        ("limit" = Option<usize>, Query, description = "Maximum entries to return (default 50, max 1000)"),
        ("offset" = Option<usize>, Query, description = "Number of entries to skip"),
    ),
    responses(
        (status = 200, body = ListEntriesResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse),
    )
)]
async fn list_entries(
    State(state): State<DaemonApiState>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ListEntriesResponse>, ApiError> {
    let runtime = state.runtime_or_error()?;
    let limit = clamp_limit(params.limit);
    let usecases = CoreUseCases::new(runtime.as_ref());

    let mut entries = usecases
        .list_entry_projections()
        .execute(limit, params.offset)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    // Populate link_domains from link_urls
    for dto in &mut entries {
        dto.link_domains = dto
            .link_urls
            .as_ref()
            .map(|urls| urls.iter().filter_map(|u| extract_domain(u)).collect());
    }

    let response_entries: Vec<EntryProjectionResponseDto> =
        entries.into_iter().map(IntoApiDto::into_api_dto).collect();

    Ok(Json(ListEntriesResponse {
        data: response_entries,
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// GET /clipboard/entries/:id
///
/// Returns entry detail (full text content). Returns 404 if not found,
/// 422 if entry is not text content.
#[utoipa::path(
    get,
    path = "/clipboard/entries/{id}",
    tag = "clipboard",
    params(
        ("id" = String, Path, description = "Entry ID"),
    ),
    responses(
        (status = 200, body = GetEntryDetailResponse),
        (status = 404, description = "Entry not found", body = crate::api::dto::error::ApiErrorResponse),
        (status = 422, description = "Entry is not text content", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse),
    )
)]
async fn get_entry(
    State(state): State<DaemonApiState>,
    Path(entry_id): Path<String>,
) -> Result<Json<GetEntryDetailResponse>, ApiError> {
    let runtime = state.runtime_or_error()?;
    let parsed_id = EntryId::from(entry_id);
    let usecases = CoreUseCases::new(runtime.as_ref());

    let detail = usecases
        .get_entry_detail()
        .execute(&parsed_id)
        .await
        .map_err(|e| {
            let msg = e.to_string().to_lowercase();
            if msg.contains("not found") {
                return ApiError::not_found("entry not found");
            }
            if msg.contains("not text content") || msg.contains("not text") {
                return ApiError {
                    status: axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                    code: "unsupported_content".to_string(),
                    message: "entry is not text content".to_string(),
                };
            }
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(GetEntryDetailResponse {
        data: detail.into_api_dto(),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// DELETE /clipboard/entries/:id
///
/// Deletes an entry. Returns 204 on success, 404 if not found.
#[utoipa::path(
    delete,
    path = "/clipboard/entries/{id}",
    tag = "clipboard",
    params(
        ("id" = String, Path, description = "Entry ID"),
    ),
    responses(
        (status = 204, description = "Entry deleted"),
        (status = 404, description = "Entry not found", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse),
    )
)]
async fn delete_entry(
    State(state): State<DaemonApiState>,
    Path(entry_id): Path<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    let runtime = state.runtime_or_error()?;
    let parsed_id = EntryId::from(entry_id);
    let usecases = CoreUseCases::new(runtime.as_ref());

    usecases
        .delete_clipboard_entry()
        .execute(&parsed_id)
        .await
        .map_err(|e| {
            let msg = e.to_string().to_lowercase();
            if msg.contains("not found") {
                return ApiError::not_found("entry not found");
            }
            ApiError::internal(e.to_string())
        })?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// POST /clipboard/entries/:id/favorite
///
/// Toggles favorite state for an entry. Returns 200 on success, 404 if not found.
#[utoipa::path(
    post,
    path = "/clipboard/entries/{id}/favorite",
    tag = "clipboard",
    params(
        ("id" = String, Path, description = "Entry ID"),
    ),
    request_body = ToggleFavoriteRequest,
    responses(
        (status = 200, body = ToggleFavoriteResponse),
        (status = 400, description = "Missing isFavorited field", body = crate::api::dto::error::ApiErrorResponse),
        (status = 404, description = "Entry not found", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse),
    )
)]
async fn toggle_favorite(
    State(state): State<DaemonApiState>,
    Path(entry_id): Path<String>,
    body: Result<Json<ToggleFavoriteRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<ToggleFavoriteResponse>, ApiError> {
    let runtime = state.runtime_or_error()?;

    let Json(body) = body.map_err(|_| ApiError::bad_request("missing isFavorited field"))?;

    let parsed_id = EntryId::from(entry_id);
    let usecases = CoreUseCases::new(runtime.as_ref());

    let found = usecases
        .toggle_favorite_clipboard_entry()
        .execute(&parsed_id, body.is_favorited)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    if !found {
        return Err(ApiError::not_found("entry not found"));
    }

    Ok(Json(ToggleFavoriteResponse {
        data: ToggleFavoriteResultDto { success: true },
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// GET /clipboard/stats
///
/// Returns aggregate clipboard statistics (total items and total size).
#[utoipa::path(
    get,
    path = "/clipboard/stats",
    tag = "clipboard",
    responses(
        (status = 200, body = GetClipboardStatsResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse),
    )
)]
async fn get_stats(
    State(state): State<DaemonApiState>,
) -> Result<Json<GetClipboardStatsResponse>, ApiError> {
    let runtime = state.runtime_or_error()?;
    let usecases = CoreUseCases::new(runtime.as_ref());

    let limit = clamp_limit(10_000);
    let entries = usecases
        .list_entry_projections()
        .execute(limit, 0)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let stats = compute_clipboard_stats(&entries);

    Ok(Json(GetClipboardStatsResponse {
        data: stats.into_api_dto(),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// GET /clipboard/entries/:id/resource
///
/// Returns resource metadata (blob URL or inline content).
#[utoipa::path(
    get,
    path = "/clipboard/entries/{id}/resource",
    tag = "clipboard",
    params(
        ("id" = String, Path, description = "Entry ID"),
    ),
    responses(
        (status = 200, body = GetEntryResourceResponse),
        (status = 404, description = "Entry not found", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse),
    )
)]
async fn get_entry_resource(
    State(state): State<DaemonApiState>,
    Path(entry_id): Path<String>,
) -> Result<Json<GetEntryResourceResponse>, ApiError> {
    let runtime = state.runtime_or_error()?;
    let parsed_id = EntryId::from(entry_id);
    let usecases = CoreUseCases::new(runtime.as_ref());

    let resource = usecases
        .get_entry_resource()
        .execute(&parsed_id)
        .await
        .map_err(|e| {
            let msg = e.to_string().to_lowercase();
            if msg.contains("not found") {
                return ApiError::not_found("entry not found");
            }
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(GetEntryResourceResponse {
        data: resource.into_api_dto(),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// POST /clipboard/entries/clear
///
/// Clears all clipboard history via bulk deletion.
/// Returns the number of entries deleted and any failures.
#[utoipa::path(
    post,
    path = "/clipboard/entries/clear",
    tag = "clipboard",
    responses(
        (status = 200, body = ClearHistoryResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse),
    )
)]
async fn clear_history(
    State(state): State<DaemonApiState>,
) -> Result<Json<ClearHistoryResponse>, ApiError> {
    let runtime = state.runtime_or_error()?;
    let usecases = CoreUseCases::new(runtime.as_ref());

    let result = usecases
        .clear_clipboard_history()
        .execute()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(ClearHistoryResponse {
        data: result.into_api_dto(),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

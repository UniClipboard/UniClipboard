//! HTTP route handlers for clipboard CRUD endpoints.
//!
//! All routes are protected by the auth_extractor + rate_limit middleware chain
//! applied at the router level (see routes::router_l2_plus).

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::Deserialize;
use uc_application::facade::{AppFacade, ResendEntryCommand};
use uc_application::facade::{
    ClipboardClearHistoryResultView, ClipboardHistoryError, ClipboardHistoryFacade,
    ClipboardListInput, ClipboardStatsView, EntryDetailView, EntryProjectionView,
    EntryResourceView,
};
use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::ports::DispatchAck;
use uc_core::{
    ClipboardChangeOrigin, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};

use uc_daemon_contract::api::dto::clipboard_command::{
    CancelTransferRequest, CancelTransferResponse, DispatchOutcomeResponse, DispatchTextRequest,
    PerTargetOutcomeDto, ResendRequest, ResendResponse,
};

use crate::api::dto::clipboard::{
    ClearHistoryResponse, ClearHistoryResultDto, ClipboardStatsDto, EntryDetailDto,
    EntryProjectionResponseDto, EntryResourceDto, GetClipboardStatsResponse,
    GetEntryDetailResponse, GetEntryResourceResponse, ListEntriesResponse, ToggleFavoriteRequest,
    ToggleFavoriteResponse, ToggleFavoriteResultDto,
};
use crate::api::dto::error::{log_facade_failure, ApiError};
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

fn require_facade(
    state: &DaemonApiState,
) -> Result<std::sync::Arc<ClipboardHistoryFacade>, ApiError> {
    Ok(state.app_facade_or_error()?.clipboard_history.clone())
}

pub fn router() -> Router<DaemonApiState> {
    use uc_daemon_contract::constants::http_route;
    Router::new()
        .route("/clipboard/entries", get(list_entries))
        .route("/clipboard/entries/clear", post(clear_history))
        .route("/clipboard/entries/:id", get(get_entry))
        .route("/clipboard/entries/:id", delete(delete_entry))
        .route("/clipboard/entries/:id/favorite", post(toggle_favorite))
        .route("/clipboard/stats", get(get_stats))
        .route("/clipboard/entries/:id/resource", get(get_entry_resource))
        .route(http_route::CLIPBOARD_DISPATCH, post(dispatch_text))
        .route(http_route::CLIPBOARD_RESEND, post(resend_entry))
        .route(
            &format!("{}/:transfer_id", http_route::CLIPBOARD_CANCEL_TRANSFER),
            post(cancel_transfer),
        )
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
    let facade = require_facade(&state)?;
    let limit = clamp_limit(params.limit);
    let entries = facade
        .list_entries(ClipboardListInput {
            limit,
            offset: params.offset,
        })
        .await
        .map_err(|e| map_clipboard_err("list_entries", e))?;

    let response_entries: Vec<EntryProjectionResponseDto> =
        entries.into_iter().map(entry_projection_to_dto).collect();

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
    let facade = require_facade(&state)?;
    let detail = facade
        .get_entry(&entry_id)
        .await
        .map_err(|e| map_clipboard_err("get_entry", e))?;

    Ok(Json(GetEntryDetailResponse {
        data: entry_detail_to_dto(detail),
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
    let facade = require_facade(&state)?;
    facade
        .delete_entry(&entry_id)
        .await
        .map_err(|e| map_clipboard_err("delete_entry", e))?;

    Ok(StatusCode::NO_CONTENT)
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
    let facade = require_facade(&state)?;

    let Json(body) = body.map_err(|_| ApiError::bad_request("missing isFavorited field"))?;

    let found = facade
        .toggle_favorite(&entry_id, body.is_favorited)
        .await
        .map_err(|e| map_clipboard_err("toggle_favorite", e))?;

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
    let facade = require_facade(&state)?;
    let stats = facade
        .stats()
        .await
        .map_err(|e| map_clipboard_err("get_stats", e))?;

    Ok(Json(GetClipboardStatsResponse {
        data: clipboard_stats_to_dto(stats),
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
    let facade = require_facade(&state)?;
    let resource = facade
        .get_entry_resource(&entry_id)
        .await
        .map_err(|e| map_clipboard_err("get_entry_resource", e))?;

    Ok(Json(GetEntryResourceResponse {
        data: entry_resource_to_dto(resource),
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
    let facade = require_facade(&state)?;
    let result = facade
        .clear_history()
        .await
        .map_err(|e| map_clipboard_err("clear_history", e))?;

    Ok(Json(ClearHistoryResponse {
        data: clear_history_to_dto(result),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

// ── Command endpoints (ADR-008 P2.5 / D7) ───────────────────────

fn require_app_facade(state: &DaemonApiState) -> Result<Arc<AppFacade>, ApiError> {
    state.app_facade_or_error()
}

async fn dispatch_text(
    State(state): State<DaemonApiState>,
    body: Result<Json<DispatchTextRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<DispatchOutcomeResponse>, ApiError> {
    let app = require_app_facade(&state)?;
    let Json(req) = body.map_err(|e| ApiError::bad_request(&e.to_string()))?;

    if req.text.is_empty() {
        return Err(ApiError::bad_request("text must not be empty"));
    }

    let target_filter: Option<Vec<DeviceId>> = req
        .peers
        .filter(|p| !p.is_empty())
        .map(|ids| ids.iter().map(DeviceId::new).collect());

    let snapshot = SystemClipboardSnapshot {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            req.text.into_bytes(),
        )],
    };

    let outcome = app
        .dispatch_clipboard_snapshot(snapshot, ClipboardChangeOrigin::LocalCapture, target_filter)
        .await
        .map_err(|e| {
            log_facade_failure(
                "clipboard_command",
                "dispatch_text",
                "dispatch_error",
                StatusCode::INTERNAL_SERVER_ERROR,
                &e.to_string(),
            );
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(dispatch_outcome_to_dto(outcome)))
}

async fn resend_entry(
    State(state): State<DaemonApiState>,
    body: Result<Json<ResendRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<ResendResponse>, ApiError> {
    let app = require_app_facade(&state)?;
    let Json(req) = body.map_err(|e| ApiError::bad_request(&e.to_string()))?;

    let target_filter: Option<Vec<DeviceId>> = req
        .peers
        .filter(|p| !p.is_empty())
        .map(|ids| ids.iter().map(DeviceId::new).collect());

    let cmd = ResendEntryCommand {
        entry_id: EntryId::from(req.entry_id.as_str()),
        target_filter,
    };

    let report = app.resend_entry(cmd).await.map_err(|e| {
        let status = match &e {
            uc_application::facade::ResendEntryError::EntryNotFound(_) => StatusCode::NOT_FOUND,
            uc_application::facade::ResendEntryError::NoEligibleTargets => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        log_facade_failure(
            "clipboard_command",
            "resend_entry",
            "resend_error",
            status,
            &e.to_string(),
        );
        ApiError {
            status,
            code: "resend_error".to_string(),
            message: e.to_string(),
        }
    })?;

    Ok(Json(ResendResponse {
        accepted: report.accepted,
        duplicate: report.duplicate,
        offline: report.offline,
        errored: report.errored,
        pending: report.pending,
    }))
}

async fn cancel_transfer(
    State(state): State<DaemonApiState>,
    Path(transfer_id): Path<String>,
    body: Result<Json<CancelTransferRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<CancelTransferResponse>, ApiError> {
    let app = require_app_facade(&state)?;
    let Json(req) = body.map_err(|e| ApiError::bad_request(&e.to_string()))?;

    let reason = match req.reason.as_str() {
        "local_user" => uc_core::FileTransferCancellationReason::LocalUser,
        "timeout" => uc_core::FileTransferCancellationReason::Timeout,
        other => {
            return Err(ApiError::bad_request(&format!(
                "unknown cancellation reason: {other}"
            )));
        }
    };

    let outcome = app
        .cancel_inbound_transfer(&transfer_id, reason)
        .await
        .map_err(|e| {
            log_facade_failure(
                "clipboard_command",
                "cancel_transfer",
                "cancel_error",
                StatusCode::INTERNAL_SERVER_ERROR,
                &e.to_string(),
            );
            ApiError::internal(e.to_string())
        })?;

    let outcome_str = match outcome {
        uc_application::facade::InboundCancelOutcome::Cancelled => "cancelled",
        uc_application::facade::InboundCancelOutcome::NotInflight => "not_inflight",
    };

    Ok(Json(CancelTransferResponse {
        outcome: outcome_str.to_string(),
    }))
}

fn dispatch_outcome_to_dto(
    o: uc_application::facade::DispatchEntryOutcome,
) -> DispatchOutcomeResponse {
    let per_target = o
        .per_target
        .iter()
        .map(|t| {
            let (outcome, error) = match &t.outcome {
                Ok(DispatchAck::Accepted) => ("accepted", None),
                Ok(DispatchAck::DuplicateIgnored) => ("duplicate", None),
                Err(msg) => ("error", Some(msg.clone())),
            };
            PerTargetOutcomeDto {
                device_id: t.device_id.as_str().to_string(),
                outcome: outcome.to_string(),
                error,
            }
        })
        .collect();

    DispatchOutcomeResponse {
        content_hash: o.content_hash,
        at_ms: o.at_ms,
        total_accepted: o.total_accepted,
        total_duplicate: o.total_duplicate,
        total_offline: o.total_offline,
        total_errored: o.total_errored,
        per_target,
    }
}

// ── Clipboard history helpers ────────────────────────────────────

fn map_clipboard_err(op: &'static str, err: ClipboardHistoryError) -> ApiError {
    use ClipboardHistoryError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::NotFound => ("not_found", ApiError::not_found("entry not found")),
        E::UnsupportedContent => (
            "unsupported_content",
            ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: "unsupported_content".to_string(),
                message: "entry is not text content".to_string(),
            },
        ),
        E::Internal(message) => ("internal", ApiError::internal(message)),
    };
    log_facade_failure("clipboard_history", op, variant, api.status, &api.message);
    api
}

fn entry_projection_to_dto(view: EntryProjectionView) -> EntryProjectionResponseDto {
    EntryProjectionResponseDto {
        id: view.id,
        preview: view.preview,
        has_detail: view.has_detail,
        size_bytes: view.size_bytes,
        captured_at: view.captured_at,
        content_type: view.content_type,
        thumbnail_url: view.thumbnail_url,
        is_encrypted: view.is_encrypted,
        is_favorited: view.is_favorited,
        updated_at: view.updated_at,
        active_time: view.active_time,
        file_transfer_status: view.file_transfer_status,
        file_transfer_reason: view.file_transfer_reason,
        link_urls: view.link_urls,
        link_domains: view.link_domains,
        file_sizes: view.file_sizes,
        image_width: view.image_width,
        image_height: view.image_height,
        payload_state: view.payload_state,
    }
}

fn entry_detail_to_dto(view: EntryDetailView) -> EntryDetailDto {
    EntryDetailDto {
        id: view.id,
        content: view.content,
        size_bytes: view.size_bytes,
        created_at_ms: view.created_at_ms,
        active_time_ms: view.active_time_ms,
        mime_type: view.mime_type,
    }
}

fn entry_resource_to_dto(view: EntryResourceView) -> EntryResourceDto {
    EntryResourceDto {
        blob_id: view.blob_id,
        mime_type: view.mime_type,
        size_bytes: view.size_bytes,
        url: view.url,
        inline_data: view.inline_data.map(|bytes| STANDARD.encode(bytes)),
    }
}

fn clipboard_stats_to_dto(view: ClipboardStatsView) -> ClipboardStatsDto {
    ClipboardStatsDto {
        total_items: view.total_items,
        total_size: view.total_size,
    }
}

fn clear_history_to_dto(view: ClipboardClearHistoryResultView) -> ClearHistoryResultDto {
    ClearHistoryResultDto {
        deleted_count: view.deleted_count,
        failed_entries: view.failed_entries,
    }
}

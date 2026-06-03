//! HTTP route handlers for clipboard CRUD endpoints.
//!
//! All routes are protected by the auth_extractor + rate_limit middleware chain
//! applied at the router level (see routes::router_l2_plus).

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::json;
use uc_application::facade::{
    AppFacade, NotResendableReason, ResendEntryCommand, ResendEntryError,
};
use uc_application::facade::{
    ClipboardClearHistoryResultView, ClipboardHistoryError, ClipboardHistoryFacade,
    ClipboardListInput, ClipboardStatsView, EntryDetailView, EntryProjectionView,
    EntryResourceView,
};
use uc_application::facade::{
    EntryDeliveryStatusView, EntryDeliveryTargetView, EntryDeliveryView, EntrySource,
    GetEntryDeliveryViewError,
};
use uc_core::clipboard::DeliveryFailureReason;
use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::ports::DispatchAck;
use uc_core::{
    ClipboardChangeOrigin, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};
use utoipa::IntoParams;

use uc_daemon_contract::api::dto::clipboard_command::{
    CancelTransferRequest, CancelTransferResponse, DispatchOutcomeResponse, DispatchTextRequest,
    PerTargetOutcomeDto, ResendRequest, ResendResponse,
};
use uc_daemon_contract::api::dto::clipboard_delivery::{
    DeliveryFailureReasonDto, EntryDeliveryStatusDto, EntryDeliveryTargetDto, EntryDeliveryViewDto,
    EntrySourceDto,
};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;

use crate::api::dto::clipboard::{
    ClearHistoryResultDto, ClipboardStatsDto, EntryDetailDto, EntryProjectionResponseDto,
    EntryResourceDto, ToggleFavoriteRequest, ToggleFavoriteResultDto,
};
use crate::api::dto::error::{log_facade_failure, ApiError, ApiErrorResponse};
use crate::api::server::DaemonApiState;

/// Query parameters for `GET /clipboard/entries`.
#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PaginationParams {
    /// Maximum entries to return (default 50, clamped to 1000).
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Number of entries to skip.
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
        .route(
            "/clipboard/entries/:id/delivery",
            get(get_entry_delivery_view_handler),
        )
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
    operation_id = "listClipboardEntries",
    tag = "clipboard",
    params(PaginationParams),
    responses(
        (status = 200, description = "Clipboard entries listed", body = ListEntriesEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn list_entries(
    State(state): State<DaemonApiState>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ApiEnvelope<Vec<EntryProjectionResponseDto>>>, ApiError> {
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

    Ok(Json(ApiEnvelope::now(response_entries)))
}

/// GET /clipboard/entries/:id
///
/// Returns entry detail (full text content). Returns 404 if not found,
/// 422 if entry is not text content.
#[utoipa::path(
    get,
    path = "/clipboard/entries/{id}",
    operation_id = "getClipboardEntry",
    tag = "clipboard",
    params(
        ("id" = String, Path, description = "Entry ID"),
    ),
    responses(
        (status = 200, description = "Entry detail retrieved", body = EntryDetailEnvelope),
        (status = 404, description = "Entry not found", body = ApiErrorResponse),
        (status = 422, description = "Entry is not text content", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_entry(
    State(state): State<DaemonApiState>,
    Path(entry_id): Path<String>,
) -> Result<Json<ApiEnvelope<EntryDetailDto>>, ApiError> {
    let facade = require_facade(&state)?;
    let detail = facade
        .get_entry(&entry_id)
        .await
        .map_err(|e| map_clipboard_err("get_entry", e))?;

    Ok(Json(ApiEnvelope::now(entry_detail_to_dto(detail))))
}

/// DELETE /clipboard/entries/:id
///
/// Deletes an entry. Returns 204 on success, 404 if not found.
#[utoipa::path(
    delete,
    path = "/clipboard/entries/{id}",
    operation_id = "deleteClipboardEntry",
    tag = "clipboard",
    params(
        ("id" = String, Path, description = "Entry ID"),
    ),
    responses(
        (status = 204, description = "Entry deleted"),
        (status = 404, description = "Entry not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
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
    operation_id = "toggleClipboardEntryFavorite",
    tag = "clipboard",
    params(
        ("id" = String, Path, description = "Entry ID"),
    ),
    request_body = ToggleFavoriteRequest,
    responses(
        (status = 200, description = "Favorite state toggled", body = ToggleFavoriteEnvelope),
        (status = 400, description = "Missing isFavorited field", body = ApiErrorResponse),
        (status = 404, description = "Entry not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn toggle_favorite(
    State(state): State<DaemonApiState>,
    Path(entry_id): Path<String>,
    body: Result<Json<ToggleFavoriteRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<ApiEnvelope<ToggleFavoriteResultDto>>, ApiError> {
    let facade = require_facade(&state)?;

    let Json(body) = body.map_err(|_| ApiError::bad_request("missing isFavorited field"))?;

    let found = facade
        .toggle_favorite(&entry_id, body.is_favorited)
        .await
        .map_err(|e| map_clipboard_err("toggle_favorite", e))?;

    if !found {
        return Err(ApiError::not_found("entry not found"));
    }

    Ok(Json(ApiEnvelope::now(ToggleFavoriteResultDto {
        success: true,
    })))
}

/// GET /clipboard/stats
///
/// Returns aggregate clipboard statistics (total items and total size).
#[utoipa::path(
    get,
    path = "/clipboard/stats",
    operation_id = "getClipboardStats",
    tag = "clipboard",
    responses(
        (status = 200, description = "Clipboard statistics retrieved", body = ClipboardStatsEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_stats(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<ClipboardStatsDto>>, ApiError> {
    let facade = require_facade(&state)?;
    let stats = facade
        .stats()
        .await
        .map_err(|e| map_clipboard_err("get_stats", e))?;

    Ok(Json(ApiEnvelope::now(clipboard_stats_to_dto(stats))))
}

/// GET /clipboard/entries/:id/resource
///
/// Returns resource metadata (blob URL or inline content).
#[utoipa::path(
    get,
    path = "/clipboard/entries/{id}/resource",
    operation_id = "getClipboardEntryResource",
    tag = "clipboard",
    params(
        ("id" = String, Path, description = "Entry ID"),
    ),
    responses(
        (status = 200, description = "Entry resource metadata retrieved", body = EntryResourceEnvelope),
        (status = 404, description = "Entry not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_entry_resource(
    State(state): State<DaemonApiState>,
    Path(entry_id): Path<String>,
) -> Result<Json<ApiEnvelope<EntryResourceDto>>, ApiError> {
    let facade = require_facade(&state)?;
    let resource = facade
        .get_entry_resource(&entry_id)
        .await
        .map_err(|e| map_clipboard_err("get_entry_resource", e))?;

    Ok(Json(ApiEnvelope::now(entry_resource_to_dto(resource))))
}

/// POST /clipboard/entries/clear
///
/// Clears all clipboard history via bulk deletion.
/// Returns the number of entries deleted and any failures.
#[utoipa::path(
    post,
    path = "/clipboard/entries/clear",
    operation_id = "clearClipboardHistory",
    tag = "clipboard",
    responses(
        (status = 200, description = "Clipboard history cleared", body = ClearHistoryEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn clear_history(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<ClearHistoryResultDto>>, ApiError> {
    let facade = require_facade(&state)?;
    let result = facade
        .clear_history()
        .await
        .map_err(|e| map_clipboard_err("clear_history", e))?;

    Ok(Json(ApiEnvelope::now(clear_history_to_dto(result))))
}

// ── Command endpoints (ADR-008 P2.5 / D7) ───────────────────────

fn require_app_facade(state: &DaemonApiState) -> Result<Arc<AppFacade>, ApiError> {
    state.app_facade_or_error()
}

/// POST /clipboard/dispatch
///
/// Wraps plaintext into a single `text/plain` snapshot and fans it out to
/// online peers. Returns the per-target delivery outcome.
#[utoipa::path(
    post,
    path = "/clipboard/dispatch",
    operation_id = "dispatchClipboardText",
    tag = "clipboard",
    request_body = DispatchTextRequest,
    responses(
        (status = 200, description = "Dispatch fan-out outcome", body = DispatchOutcomeEnvelope),
        (status = 400, description = "Empty or malformed request", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn dispatch_text(
    State(state): State<DaemonApiState>,
    body: Result<Json<DispatchTextRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<ApiEnvelope<DispatchOutcomeResponse>>, ApiError> {
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

    Ok(Json(ApiEnvelope::now(dispatch_outcome_to_dto(outcome))))
}

/// POST /clipboard/resend
///
/// Re-dispatches a previously captured entry to (optionally filtered) peers.
#[utoipa::path(
    post,
    path = "/clipboard/resend",
    operation_id = "resendClipboardEntry",
    tag = "clipboard",
    request_body = ResendRequest,
    responses(
        (status = 200, description = "Resend fan-out outcome", body = ResendEnvelope),
        (status = 400, description = "Malformed request", body = ApiErrorResponse),
        (status = 404, description = "Entry not found", body = ApiErrorResponse),
        (status = 409, description = "Entry not resendable / target not trusted / no eligible targets", body = ApiErrorResponse),
        (status = 500, description = "Storage or dispatch failure", body = ApiErrorResponse),
    )
)]
async fn resend_entry(
    State(state): State<DaemonApiState>,
    body: Result<Json<ResendRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<ApiEnvelope<ResendResponse>>, Response> {
    let app = require_app_facade(&state).map_err(IntoResponse::into_response)?;
    let Json(req) = body.map_err(|e| ApiError::bad_request(e.to_string()).into_response())?;

    let target_filter: Option<Vec<DeviceId>> = req
        .peers
        .filter(|p| !p.is_empty())
        .map(|ids| ids.iter().map(DeviceId::new).collect());

    let cmd = ResendEntryCommand {
        entry_id: EntryId::from(req.entry_id.as_str()),
        target_filter,
    };

    let report = app
        .resend_entry(cmd)
        .await
        .map_err(|e| resend_error_to_response(e).into_response())?;

    Ok(Json(ApiEnvelope::now(ResendResponse {
        accepted: report.accepted,
        duplicate: report.duplicate,
        offline: report.offline,
        errored: report.errored,
        pending: report.pending,
    })))
}

/// Map the typed [`ResendEntryError`] to (status, canonical `ApiErrorResponse`).
///
/// `code` is the SCREAMING_SNAKE tag the frontend `ResendEntryCommandError`
/// union switches on; the per-variant structured fields ride `details` so the
/// FE i18n placeholders (`entryId` / `deviceId` / `reason` / `message`) survive
/// the HTTP boundary (`callSdk` normalization exposes the body on
/// `DaemonApiError.details`, and the FE reconstructs `{ code, ...details }`).
/// The client-recoverable variants are 4xx (not 5xx) so they neither trip
/// `callSdk`'s 401 refresh-retry nor escalate to Sentry via `log_facade_failure`.
fn resend_error_to_response(err: ResendEntryError) -> (StatusCode, Json<ApiErrorResponse>) {
    use ResendEntryError as E;
    let (status, variant, code, message, details): (
        StatusCode,
        &'static str,
        &'static str,
        String,
        Option<serde_json::Value>,
    ) = match err {
        E::EntryNotFound(id) => (
            StatusCode::NOT_FOUND,
            "entry_not_found",
            "ENTRY_NOT_FOUND",
            format!("entry not found: {}", id.inner()),
            Some(json!({ "entryId": id.inner() })),
        ),
        E::EntryNotResendable { entry_id, reason } => {
            let reason_tag = match reason {
                NotResendableReason::RemoteOrigin => "remoteOrigin",
                NotResendableReason::PayloadLost => "payloadLost",
            };
            (
                StatusCode::CONFLICT,
                "entry_not_resendable",
                "ENTRY_NOT_RESENDABLE",
                format!("entry {} is not resendable", entry_id.inner()),
                Some(json!({ "entryId": entry_id.inner(), "reason": reason_tag })),
            )
        }
        E::TargetNotTrusted(device_id) => (
            StatusCode::CONFLICT,
            "target_not_trusted",
            "TARGET_NOT_TRUSTED",
            format!("target device {} is not a trusted peer", device_id.as_str()),
            Some(json!({ "deviceId": device_id.as_str() })),
        ),
        E::NoEligibleTargets => (
            StatusCode::CONFLICT,
            "no_eligible_targets",
            "NO_ELIGIBLE_TARGETS",
            "no eligible targets for resend".to_string(),
            None,
        ),
        E::Storage(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "storage",
            "STORAGE",
            msg.clone(),
            Some(json!({ "message": msg })),
        ),
        E::Dispatch(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "dispatch",
            "DISPATCH",
            msg.clone(),
            Some(json!({ "message": msg })),
        ),
    };
    log_facade_failure(
        "clipboard_command",
        "resend_entry",
        variant,
        status,
        &message,
    );
    let body = match details {
        Some(d) => ApiErrorResponse::with_details(code, message, d),
        None => ApiErrorResponse::new(code, message),
    };
    (status, Json(body))
}

/// GET /clipboard/entries/:id/delivery
///
/// Returns the entry's origin + per-trusted-peer delivery status for the detail
/// panel (ADR-008 P3-1 / D15; formerly the GUI-only
/// `clipboard_entry_delivery_view` Tauri command). Entry-not-found is a normal
/// degraded-render case for the frontend, so it maps to a plain 404.
#[utoipa::path(
    get,
    path = "/clipboard/entries/{id}/delivery",
    operation_id = "getClipboardEntryDelivery",
    tag = "clipboard",
    params(("id" = String, Path, description = "Entry id")),
    responses(
        (status = 200, description = "Entry delivery view", body = EntryDeliveryViewEnvelope),
        (status = 404, description = "Entry not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_entry_delivery_view_handler(
    State(state): State<DaemonApiState>,
    Path(id): Path<String>,
) -> Result<Json<ApiEnvelope<EntryDeliveryViewDto>>, ApiError> {
    let app = state.app_facade_or_error()?;
    let entry = EntryId::from_string(id);
    let view = app
        .get_entry_delivery_view(&entry)
        .await
        .map_err(map_delivery_view_err)?;
    Ok(Json(ApiEnvelope::now(entry_delivery_view_to_dto(view))))
}

fn map_delivery_view_err(err: GetEntryDeliveryViewError) -> ApiError {
    use GetEntryDeliveryViewError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::EntryNotFound(id) => (
            "entry_not_found",
            ApiError::not_found(format!("entry not found: {id}")),
        ),
        E::Storage(msg) => ("storage", ApiError::internal(msg)),
    };
    log_facade_failure(
        "clipboard",
        "get_entry_delivery_view",
        variant,
        api.status,
        &api.message,
    );
    api
}

fn entry_delivery_view_to_dto(view: EntryDeliveryView) -> EntryDeliveryViewDto {
    EntryDeliveryViewDto {
        entry_id: view.entry_id.as_str().to_string(),
        source: entry_source_to_dto(view.source),
        deliveries: view
            .deliveries
            .into_iter()
            .map(entry_delivery_target_to_dto)
            .collect(),
    }
}

fn entry_source_to_dto(source: EntrySource) -> EntrySourceDto {
    match source {
        EntrySource::Local => EntrySourceDto::Local,
        EntrySource::Remote {
            device_id,
            device_name,
        } => EntrySourceDto::Remote {
            device_id: device_id.as_str().to_string(),
            device_name,
        },
        EntrySource::Historical => EntrySourceDto::Historical,
    }
}

fn entry_delivery_target_to_dto(target: EntryDeliveryTargetView) -> EntryDeliveryTargetDto {
    EntryDeliveryTargetDto {
        target_device_id: target.target_device_id.as_str().to_string(),
        target_device_name: target.target_device_name,
        status: entry_delivery_status_to_dto(target.status),
        reason_detail: target.reason_detail,
        updated_at_ms: target.updated_at_ms,
    }
}

fn entry_delivery_status_to_dto(status: EntryDeliveryStatusView) -> EntryDeliveryStatusDto {
    match status {
        EntryDeliveryStatusView::Pending => EntryDeliveryStatusDto::Pending,
        EntryDeliveryStatusView::Delivered => EntryDeliveryStatusDto::Delivered,
        EntryDeliveryStatusView::Duplicate => EntryDeliveryStatusDto::Duplicate,
        EntryDeliveryStatusView::Failed { reason } => EntryDeliveryStatusDto::Failed {
            reason: delivery_failure_reason_to_dto(reason),
        },
    }
}

fn delivery_failure_reason_to_dto(reason: DeliveryFailureReason) -> DeliveryFailureReasonDto {
    match reason {
        DeliveryFailureReason::Offline => DeliveryFailureReasonDto::Offline,
        DeliveryFailureReason::LocalPolicy => DeliveryFailureReasonDto::LocalPolicy,
        DeliveryFailureReason::PeerRejected => DeliveryFailureReasonDto::PeerRejected,
        DeliveryFailureReason::Io => DeliveryFailureReasonDto::Io,
        DeliveryFailureReason::Internal => DeliveryFailureReasonDto::Internal,
    }
}

/// POST /clipboard/cancel-transfer/:transfer_id
///
/// Cancels an in-flight inbound file transfer. Returns the cancellation outcome.
#[utoipa::path(
    post,
    path = "/clipboard/cancel-transfer/{transfer_id}",
    operation_id = "cancelClipboardTransfer",
    tag = "clipboard",
    params(
        ("transfer_id" = String, Path, description = "Inbound transfer ID"),
    ),
    request_body = CancelTransferRequest,
    responses(
        (status = 200, description = "Transfer cancellation outcome", body = CancelTransferEnvelope),
        (status = 400, description = "Unknown cancellation reason", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn cancel_transfer(
    State(state): State<DaemonApiState>,
    Path(transfer_id): Path<String>,
    body: Result<Json<CancelTransferRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<ApiEnvelope<CancelTransferResponse>>, ApiError> {
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

    Ok(Json(ApiEnvelope::now(CancelTransferResponse {
        outcome: outcome_str.to_string(),
    })))
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
                details: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The FE `ResendEntryCommandError` union reconstructs `{ code, ...details }`
    /// off the normalized error body, so each variant must emit its SCREAMING_SNAKE
    /// `code` plus its structured fields in `details`, and the client-recoverable
    /// variants must be 4xx (not 5xx → no Sentry escalation, no 401 retry).
    #[test]
    fn resend_error_carries_typed_code_and_structured_details() {
        let (status, body) =
            resend_error_to_response(ResendEntryError::TargetNotTrusted(DeviceId::new("dev-x")));
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body.0.code, "TARGET_NOT_TRUSTED");
        assert_eq!(body.0.details.as_ref().unwrap()["deviceId"], "dev-x");

        let (status, body) = resend_error_to_response(ResendEntryError::EntryNotResendable {
            entry_id: EntryId::from("ent-1"),
            reason: NotResendableReason::PayloadLost,
        });
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body.0.code, "ENTRY_NOT_RESENDABLE");
        let details = body.0.details.as_ref().unwrap();
        assert_eq!(details["entryId"], "ent-1");
        assert_eq!(details["reason"], "payloadLost");

        let (status, body) = resend_error_to_response(ResendEntryError::EntryNotFound(
            EntryId::from("ent-missing"),
        ));
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body.0.code, "ENTRY_NOT_FOUND");
        assert_eq!(body.0.details.as_ref().unwrap()["entryId"], "ent-missing");

        let (status, body) = resend_error_to_response(ResendEntryError::Dispatch(
            "encrypt session locked".to_string(),
        ));
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body.0.code, "DISPATCH");
        assert_eq!(
            body.0.details.as_ref().unwrap()["message"],
            "encrypt session locked"
        );

        let (_, body) = resend_error_to_response(ResendEntryError::NoEligibleTargets);
        assert_eq!(body.0.code, "NO_ELIGIBLE_TARGETS");
        assert!(body.0.details.is_none());
    }

    /// Delivery-view: entry-not-found is a normal degraded-render case → plain 404.
    #[test]
    fn delivery_view_entry_not_found_is_404() {
        let api = map_delivery_view_err(GetEntryDeliveryViewError::EntryNotFound("ent-x".into()));
        assert_eq!(api.status, StatusCode::NOT_FOUND);
        assert_eq!(api.code, "not_found");
    }
}

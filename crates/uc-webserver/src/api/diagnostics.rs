//! HTTP route handlers for local diagnostics endpoints.

use axum::extract::State;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use tracing::{info, instrument};
use uc_daemon_contract::api::dto::diagnostics::{
    DebugStatusDto, LogExportRequestDto, LogExportResultDto, UpdateDebugModeRequestDto,
    UpdateDebugModeResultDto,
};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_engine::{
    EngineError, EngineErrorCategory, ExportDiagnosticLogsInput, Operation, OperationResult,
    UpdateDebugModeInput,
};
use utoipa;

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/diagnostics/debug", get(get_debug_status_handler))
        .route("/diagnostics/debug", put(update_debug_mode_handler))
        .route("/diagnostics/log-export", post(export_logs_handler))
}

#[utoipa::path(
    get,
    path = "/diagnostics/debug",
    tag = "system",
    operation_id = "getDebugStatus",
    responses(
        (status = 200, description = "Current persistent debug-mode status", body = DebugStatusEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.diagnostics.debug.get", level = "info", skip(state))]
pub async fn get_debug_status_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<DebugStatusDto>>, ApiError> {
    let result = state
        .execute(Operation::QueryDiagnostics)
        .await
        .map_err(|error| diagnostics_error_to_api("get_debug_status", error))?;
    let OperationResult::DiagnosticsStatus(status) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected diagnostics result",
        ));
    };
    Ok(Json(ApiEnvelope::now(DebugStatusDto {
        debug_mode: status.debug_mode,
        effective_log_profile: status.effective_log_profile,
        restart_required: status.restart_required,
    })))
}

#[utoipa::path(
    put,
    path = "/diagnostics/debug",
    tag = "system",
    operation_id = "updateDebugMode",
    request_body = UpdateDebugModeRequestDto,
    responses(
        (status = 200, description = "Debug mode persisted", body = UpdateDebugModeEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.diagnostics.debug.update", level = "info", skip(state, payload), fields(enabled = payload.enabled))]
pub async fn update_debug_mode_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<UpdateDebugModeRequestDto>,
) -> Result<Json<ApiEnvelope<UpdateDebugModeResultDto>>, ApiError> {
    let result = state
        .execute(Operation::UpdateDebugMode(UpdateDebugModeInput {
            enabled: payload.enabled,
        }))
        .await
        .map_err(|error| diagnostics_error_to_api("update_debug_mode", error))?;
    let OperationResult::DebugModeUpdated(result) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected debug-mode result",
        ));
    };
    info!(
        debug_mode = result.debug_mode,
        restart_required = result.restart_required,
        "debug mode updated"
    );
    Ok(Json(ApiEnvelope::now(UpdateDebugModeResultDto {
        debug_mode: result.debug_mode,
        restart_required: result.restart_required,
    })))
}

#[utoipa::path(
    post,
    path = "/diagnostics/log-export",
    tag = "system",
    operation_id = "exportLogs",
    request_body = LogExportRequestDto,
    responses(
        (status = 200, description = "Logs exported to Downloads", body = LogExportEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.diagnostics.log_export", level = "info", skip(state, payload), fields(since_hours = payload.since_hours))]
pub async fn export_logs_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<LogExportRequestDto>,
) -> Result<Json<ApiEnvelope<LogExportResultDto>>, ApiError> {
    let (destination, path) = state
        .file_handles
        .register_diagnostic_output()
        .map_err(|_| ApiError::internal("Downloads directory is unavailable"))?;
    let result = state
        .execute(Operation::ExportDiagnosticLogs(ExportDiagnosticLogsInput {
            since_hours: payload.since_hours,
            destination,
        }))
        .await
        .map_err(|error| diagnostics_error_to_api("export_logs", error))?;
    let OperationResult::DiagnosticLogsExported(result) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected log-export result",
        ));
    };
    let since = chrono::DateTime::from_timestamp_millis(result.since_unix_ms)
        .ok_or_else(|| ApiError::internal("engine returned an invalid log-export timestamp"))?;
    info!(
        included_files = result.included_files.len(),
        "logs exported"
    );
    Ok(Json(ApiEnvelope::now(LogExportResultDto {
        path,
        included_files: result.included_files,
        since,
    })))
}

fn diagnostics_error_to_api(op: &'static str, error: EngineError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match error.category() {
        EngineErrorCategory::InvalidInput => (
            "invalid_input",
            ApiError::bad_request("invalid diagnostic export destination"),
        ),
        EngineErrorCategory::Unauthorized => (
            "unauthorized",
            ApiError::unauthorized("diagnostic export destination is not writable"),
        ),
        EngineErrorCategory::Unavailable | EngineErrorCategory::DeadlineExceeded => (
            "unavailable",
            ApiError::service_unavailable("diagnostic export destination is unavailable"),
        ),
        EngineErrorCategory::InvalidState
        | EngineErrorCategory::NotFound
        | EngineErrorCategory::Conflict
        | EngineErrorCategory::Internal => (
            "operation_failed",
            ApiError::internal("diagnostics operation failed"),
        ),
    };
    log_facade_failure("diagnostics", op, variant, api.status, &api.message);
    api
}

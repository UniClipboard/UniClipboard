//! HTTP route handlers for local diagnostics endpoints.

use axum::extract::State;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use tracing::{info, instrument};
use uc_application::facade::DiagnosticsFacadeError;
use uc_daemon_contract::api::dto::diagnostics::{
    DebugStatusDto, LogExportRequestDto, LogExportResultDto, UpdateDebugModeRequestDto,
    UpdateDebugModeResultDto,
};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
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
    let app = state.app_facade_or_error()?;
    let status = app
        .diagnostics
        .debug_status()
        .await
        .map_err(|err| diagnostics_error_to_api("get_debug_status", err))?;
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
    let app = state.app_facade_or_error()?;
    let result = app
        .diagnostics
        .set_debug_mode(payload.enabled)
        .await
        .map_err(|err| diagnostics_error_to_api("update_debug_mode", err))?;
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
    let app = state.app_facade_or_error()?;
    let result = app
        .diagnostics
        .export_logs(payload.since_hours)
        .await
        .map_err(|err| diagnostics_error_to_api("export_logs", err))?;
    info!(
        path = %result.path,
        included_files = result.included_files.len(),
        "logs exported"
    );
    Ok(Json(ApiEnvelope::now(LogExportResultDto {
        path: result.path,
        included_files: result.included_files,
        since: result.since,
    })))
}

fn diagnostics_error_to_api(op: &'static str, err: DiagnosticsFacadeError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match err {
        DiagnosticsFacadeError::LoadSettings(msg) => (
            "load_settings",
            ApiError::internal(format!("failed to load settings: {msg}")),
        ),
        DiagnosticsFacadeError::SaveSettings(msg) => (
            "save_settings",
            ApiError::internal(format!("failed to save settings: {msg}")),
        ),
        DiagnosticsFacadeError::DownloadsUnavailable => (
            "downloads_unavailable",
            ApiError::internal("Downloads directory is unavailable"),
        ),
        DiagnosticsFacadeError::Export(msg) => (
            "export",
            ApiError::internal(format!("failed to export logs: {msg}")),
        ),
    };
    log_facade_failure("diagnostics", op, variant, api.status, &api.message);
    api
}

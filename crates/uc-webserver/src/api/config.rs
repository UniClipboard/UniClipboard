//! HTTP route handlers for whole-installation configuration migration
//! (export / import preview / import staging).
//!
//! * `POST /config/export` — pack the current installation into a
//!   password-protected `.ucbundle` (requires an unlocked, initialized session).
//! * `POST /config/import/preview` — read a bundle's non-secret manifest
//!   metadata for operator confirmation (read-only, ungated).
//! * `POST /config/import` — validate a bundle and stage it for the next
//!   restart to apply on boot (requires an uninitialized target + `confirmed`).
//!
//! Like `/encryption/*`, all three are session-JWT gated and carry their
//! password in the request body over the loopback API. Handlers MUST NOT log
//! the request body — there is intentionally no `?req` / password field on any
//! span or tracing event here. All responses use the canonical
//! `ApiEnvelope<T> { data, ts }` success envelope; errors use the shared
//! `ApiErrorResponse`.

use std::path::Path;

use axum::extract::{rejection::JsonRejection, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use tracing::info;
use uc_daemon_contract::api::dto::config::{
    ExportConfigRequest, ExportConfigResponse, ImportConfigRequest, ImportConfigResponse,
    PreviewImportRequest, PreviewImportResponse,
};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_engine::{
    ConfigExportOutcome, ConfigImportPreviewOutcome, ConfigImportPreviewSummary,
    ConfigImportStageOutcome, ConfigSourceModeSummary, EngineError, EngineErrorCategory,
    ExportConfigInput, Operation, OperationResult, PreviewConfigImportInput, SecretString,
    StageConfigImportInput,
};

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/config/export", post(export_config_handler))
        .route("/config/import/preview", post(preview_import_handler))
        .route("/config/import", post(import_config_handler))
}

fn map_config_engine_error(op: &'static str, error: EngineError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match error.category() {
        EngineErrorCategory::InvalidInput => (
            "invalid_input",
            ApiError::bad_request("configuration file is unavailable"),
        ),
        EngineErrorCategory::Unauthorized => (
            "unauthorized",
            ApiError::unauthorized("configuration file access was denied"),
        ),
        EngineErrorCategory::Unavailable | EngineErrorCategory::DeadlineExceeded => (
            "unavailable",
            ApiError::service_unavailable("configuration file service is unavailable"),
        ),
        EngineErrorCategory::InvalidState
        | EngineErrorCategory::NotFound
        | EngineErrorCategory::Conflict
        | EngineErrorCategory::Internal => (
            "operation_failed",
            ApiError::internal("configuration operation failed"),
        ),
    };
    log_facade_failure("config_migration", op, variant, api.status, &api.message);
    api
}

fn locked_error() -> ApiError {
    ApiError {
        status: StatusCode::LOCKED,
        code: "LOCKED".to_string(),
        message: "session is locked".to_string(),
        details: None,
    }
}

fn not_initialized_error() -> ApiError {
    ApiError {
        status: StatusCode::CONFLICT,
        code: "NOT_INITIALIZED".to_string(),
        message: "source installation is not initialized".to_string(),
        details: None,
    }
}

fn invalid_bundle_error() -> ApiError {
    ApiError {
        status: StatusCode::BAD_REQUEST,
        code: "INVALID_PASSWORD_OR_CORRUPT".to_string(),
        message: "invalid password or corrupt bundle".to_string(),
        details: None,
    }
}

fn incompatible_bundle_error(reason: String) -> ApiError {
    ApiError {
        status: StatusCode::UNPROCESSABLE_ENTITY,
        code: "INCOMPATIBLE_BUNDLE".to_string(),
        message: reason,
        details: None,
    }
}

/// Convert the domain `ConfigImportPreview` into the wire DTO. `source_mode`
/// becomes a stable lowercase token (`portable` / `installed`).
fn preview_to_dto(preview: ConfigImportPreviewSummary) -> PreviewImportResponse {
    let source_mode = match preview.source_mode {
        ConfigSourceModeSummary::Portable => "portable",
        ConfigSourceModeSummary::Installed => "installed",
    };
    PreviewImportResponse {
        app_version: preview.app_version,
        source_mode: source_mode.to_string(),
        created_at_unix_ms: preview.created_at_unix_ms,
        profile_id: preview.profile_id,
        device_fingerprint: preview.device_fingerprint,
    }
}

/// POST /config/export
///
/// Pack the current installation into an encrypted `.ucbundle` written to
/// `targetPath`, sealed with the installation's own key material (no export
/// password; opening it later requires the space passphrase). The facade
/// enforces the preconditions (initialized + unlocked) before any material is
/// read. D14: session-JWT gated; the handler MUST NOT log the request body (no
/// path on any span here).
#[utoipa::path(
    post,
    path = "/config/export",
    operation_id = "exportConfig",
    tag = "config",
    request_body = ExportConfigRequest,
    responses(
        (status = 200, description = "Configuration bundle written", body = ExportConfigEnvelope),
        (status = 409, description = "Source installation is not initialized", body = ApiErrorResponse),
        (status = 423, description = "Session is locked", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn export_config_handler(
    State(state): State<DaemonApiState>,
    Json(req): Json<ExportConfigRequest>,
) -> Result<Json<ApiEnvelope<ExportConfigResponse>>, ApiError> {
    info!("config export request received");
    let destination = state
        .file_handles
        .register_output(Path::new(&req.target_path))
        .map_err(|_| ApiError::bad_request("configuration export path is unavailable"))?;
    let result = state
        .execute(Operation::ExportConfig(ExportConfigInput { destination }))
        .await
        .map_err(|error| map_config_engine_error("export_config", error))?;
    match result {
        OperationResult::ConfigExport(ConfigExportOutcome::Exported) => {}
        OperationResult::ConfigExport(ConfigExportOutcome::Locked) => {
            return Err(locked_error());
        }
        OperationResult::ConfigExport(ConfigExportOutcome::NotInitialized) => {
            return Err(not_initialized_error());
        }
        _ => {
            return Err(ApiError::internal(
                "engine returned an unexpected config-export result",
            ));
        }
    }

    info!("config export completed");
    Ok(Json(ApiEnvelope::now(ExportConfigResponse {
        path: req.target_path,
    })))
}

/// POST /config/import/preview
///
/// Decrypt a bundle's manifest and return its non-secret descriptive metadata
/// so the UI can confirm before staging. Read-only and ungated. D14:
/// session-JWT gated; the handler MUST NOT log the request body.
#[utoipa::path(
    post,
    path = "/config/import/preview",
    operation_id = "previewConfigImport",
    tag = "config",
    request_body = PreviewImportRequest,
    responses(
        (status = 200, description = "Bundle preview metadata", body = PreviewImportEnvelope),
        (status = 400, description = "Invalid password or corrupt bundle", body = ApiErrorResponse),
        (status = 422, description = "Incompatible bundle", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn preview_import_handler(
    State(state): State<DaemonApiState>,
    Json(req): Json<PreviewImportRequest>,
) -> Result<Json<ApiEnvelope<PreviewImportResponse>>, ApiError> {
    info!("config import preview request received");
    let source = state
        .file_handles
        .register_input(Path::new(&req.source_path))
        .map_err(|_| ApiError::bad_request("configuration import file is unavailable"))?;
    let result = state
        .execute(Operation::PreviewConfigImport(PreviewConfigImportInput {
            source,
            password: SecretString::new(req.password),
        }))
        .await
        .map_err(|error| map_config_engine_error("preview_import", error))?;
    let preview = match result {
        OperationResult::ConfigImportPreview(ConfigImportPreviewOutcome::Ready(preview)) => {
            preview
        }
        OperationResult::ConfigImportPreview(
            ConfigImportPreviewOutcome::InvalidPasswordOrCorrupt,
        ) => return Err(invalid_bundle_error()),
        OperationResult::ConfigImportPreview(ConfigImportPreviewOutcome::Incompatible {
            reason,
        }) => return Err(incompatible_bundle_error(reason)),
        _ => {
            return Err(ApiError::internal(
                "engine returned an unexpected config-preview result",
            ));
        }
    };

    Ok(Json(ApiEnvelope::now(preview_to_dto(preview))))
}

/// POST /config/import
///
/// Validate a bundle and stage it for the next restart to apply on boot.
/// Applying on the next boot replaces whatever configuration the target
/// currently holds — there is no uninitialized precondition. `confirmed` must be
/// `true` (the import is a device-identity move that overwrites in place); a
/// missing/invalid body or `confirmed != true` is a 400. D14: session-JWT
/// gated; the handler MUST NOT log the request body.
#[utoipa::path(
    post,
    path = "/config/import",
    operation_id = "importConfig",
    tag = "config",
    request_body = ImportConfigRequest,
    responses(
        (status = 200, description = "Bundle staged for next restart", body = ImportConfigEnvelope),
        (status = 400, description = "Confirmation missing/false, or invalid password / corrupt bundle", body = ApiErrorResponse),
        (status = 422, description = "Incompatible bundle", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn import_config_handler(
    State(state): State<DaemonApiState>,
    body: Result<Json<ImportConfigRequest>, JsonRejection>,
) -> Result<Json<ApiEnvelope<ImportConfigResponse>>, ApiError> {
    // Confirmation gate (mirrors `/storage/clear-cache`): a missing/invalid body
    // OR `confirmed` not set to true → 400 with the canonical error body. This
    // runs before touching the facade so an unconfirmed import never stages.
    let req = match body {
        Ok(Json(req)) if req.confirmed => req,
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

    info!("config import (stage) request received");
    let source = state
        .file_handles
        .register_input(Path::new(&req.source_path))
        .map_err(|_| ApiError::bad_request("configuration import file is unavailable"))?;
    let result = state
        .execute(Operation::StageConfigImport(StageConfigImportInput {
            source,
            password: SecretString::new(req.password),
        }))
        .await
        .map_err(|error| map_config_engine_error("stage_import", error))?;
    let unlock_required_after_apply = match result {
        OperationResult::ConfigImportStaged(ConfigImportStageOutcome::Staged {
            unlock_required_after_apply,
        }) => unlock_required_after_apply,
        OperationResult::ConfigImportStaged(
            ConfigImportStageOutcome::InvalidPasswordOrCorrupt,
        ) => return Err(invalid_bundle_error()),
        OperationResult::ConfigImportStaged(ConfigImportStageOutcome::Incompatible {
            reason,
        }) => return Err(incompatible_bundle_error(reason)),
        _ => {
            return Err(ApiError::internal(
                "engine returned an unexpected config-import result",
            ));
        }
    };

    info!(
        unlock_required_after_apply,
        "config import staged"
    );
    Ok(Json(ApiEnvelope::now(ImportConfigResponse {
        staged_ok: true,
        unlock_required_after_apply,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_business_outcomes_keep_the_existing_statuses_and_codes() {
        let cases = [
            (locked_error(), StatusCode::LOCKED, "LOCKED"),
            (
                not_initialized_error(),
                StatusCode::CONFLICT,
                "NOT_INITIALIZED",
            ),
            (
                invalid_bundle_error(),
                StatusCode::BAD_REQUEST,
                "INVALID_PASSWORD_OR_CORRUPT",
            ),
            (
                incompatible_bundle_error("schema too new".to_string()),
                StatusCode::UNPROCESSABLE_ENTITY,
                "INCOMPATIBLE_BUNDLE",
            ),
        ];
        for (api, status, code) in cases {
            assert_eq!(api.status, status);
            assert_eq!(api.code, code);
        }
    }

    #[test]
    fn incompatible_bundle_preserves_reason_in_message() {
        let api = incompatible_bundle_error(
            "bundle archive schema 2 is newer than supported 1".to_string(),
        );
        assert_eq!(api.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            api.message,
            "bundle archive schema 2 is newer than supported 1"
        );
    }

    #[test]
    fn preview_to_dto_projects_source_mode_and_profile() {
        let dto = preview_to_dto(ConfigImportPreviewSummary {
            app_version: "0.16.0".to_string(),
            source_mode: ConfigSourceModeSummary::Installed,
            created_at_unix_ms: 1_700_000_000_000,
            profile_id: "default".to_string(),
            device_fingerprint: "AB-CD".to_string(),
        });
        assert_eq!(dto.source_mode, "installed");
        assert_eq!(dto.profile_id, "default");
        assert_eq!(dto.created_at_unix_ms, 1_700_000_000_000);

        let portable = preview_to_dto(ConfigImportPreviewSummary {
            app_version: "0.16.0".to_string(),
            source_mode: ConfigSourceModeSummary::Portable,
            created_at_unix_ms: 1,
            profile_id: "default".to_string(),
            device_fingerprint: "X".to_string(),
        });
        assert_eq!(portable.source_mode, "portable");
    }
}

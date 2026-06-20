//! HTTP route handlers for whole-installation configuration migration
//! (export / import preview / import staging).
//!
//! These endpoints expose `uc_application::facade::ConfigMigrationFacade`:
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
use uc_core::crypto::domain::Passphrase;
use uc_core::ports::config_migration::{
    ConfigImportPreview, ConfigMigrationError, ConfigSourceMode,
};
use uc_daemon_contract::api::dto::config::{
    ExportConfigRequest, ExportConfigResponse, ImportConfigRequest, ImportConfigResponse,
    PreviewImportRequest, PreviewImportResponse,
};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/config/export", post(export_config_handler))
        .route("/config/import/preview", post(preview_import_handler))
        .route("/config/import", post(import_config_handler))
}

/// Map the typed [`ConfigMigrationError`] onto an [`ApiError`].
///
/// Status mapping (design doc §8): `Locked` → 423, `NotInitialized` → 409,
/// `InvalidPasswordOrCorrupt` → 400, `IncompatibleBundle` → 422,
/// `Io` / `Internal` → 500. The `code` token is the SCREAMING_SNAKE semantic tag
/// the frontend error union switches on.
///
/// Messages here are the variant's own non-secret strings — they never carry a
/// password, plaintext, or a filesystem path (the facade/adapter keep those out
/// of the error; the adapter's `Io`/`Internal` details are generic). The 5xx
/// variants still log a root-cause ERROR via [`log_facade_failure`].
fn map_config_migration_err(op: &'static str, err: ConfigMigrationError) -> ApiError {
    use ConfigMigrationError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::Locked => (
            "locked",
            ApiError {
                status: StatusCode::LOCKED,
                code: "LOCKED".to_string(),
                message: "session is locked".to_string(),
                details: None,
            },
        ),
        E::NotInitialized => (
            "not_initialized",
            ApiError {
                status: StatusCode::CONFLICT,
                code: "NOT_INITIALIZED".to_string(),
                message: "source installation is not initialized".to_string(),
                details: None,
            },
        ),
        E::InvalidPasswordOrCorrupt => (
            "invalid_password_or_corrupt",
            ApiError {
                status: StatusCode::BAD_REQUEST,
                code: "INVALID_PASSWORD_OR_CORRUPT".to_string(),
                message: "invalid password or corrupt bundle".to_string(),
                details: None,
            },
        ),
        E::IncompatibleBundle { reason } => (
            "incompatible_bundle",
            ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: "INCOMPATIBLE_BUNDLE".to_string(),
                // `reason` is a stable, non-secret, operator-facing explanation.
                message: reason,
                details: None,
            },
        ),
        E::Io { details } => (
            "io",
            ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "IO".to_string(),
                // `details` is a generic, non-secret IO description.
                message: details,
                details: None,
            },
        ),
        E::Internal { details } => (
            "internal",
            ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "INTERNAL".to_string(),
                message: details,
                details: None,
            },
        ),
    };
    log_facade_failure("config_migration", op, variant, api.status, &api.message);
    api
}

/// Convert the domain `ConfigImportPreview` into the wire DTO. `source_mode`
/// becomes a stable lowercase token (`portable` / `installed`).
fn preview_to_dto(preview: ConfigImportPreview) -> PreviewImportResponse {
    let source_mode = match preview.source_mode {
        ConfigSourceMode::Portable => "portable",
        ConfigSourceMode::Installed => "installed",
    };
    PreviewImportResponse {
        app_version: preview.app_version,
        source_mode: source_mode.to_string(),
        created_at_unix_ms: preview.created_at_unix_ms,
        profile_id: preview.profile_id.inner().clone(),
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
    let app = state.app_facade_or_error()?;

    info!("config export request received");

    let path = app
        .config_migration
        .export_config(Path::new(&req.target_path))
        .await
        .map_err(|e| map_config_migration_err("export_config", e))?;

    info!("config export completed");
    Ok(Json(ApiEnvelope::now(ExportConfigResponse {
        path: path.to_string_lossy().into_owned(),
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
    let app = state.app_facade_or_error()?;

    info!("config import preview request received");

    let password = Passphrase::new(req.password);
    let preview = app
        .config_migration
        .preview_import(&password, Path::new(&req.source_path))
        .await
        .map_err(|e| map_config_migration_err("preview_import", e))?;

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

    let app = state.app_facade_or_error()?;

    info!("config import (stage) request received");

    let password = Passphrase::new(req.password);
    let staged = app
        .config_migration
        .stage_import(&password, Path::new(&req.source_path))
        .await
        .map_err(|e| map_config_migration_err("stage_import", e))?;

    info!(
        unlock_required_after_apply = staged.unlock_required_after_apply,
        "config import staged"
    );
    Ok(Json(ApiEnvelope::now(ImportConfigResponse {
        staged_ok: true,
        unlock_required_after_apply: staged.unlock_required_after_apply,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ids::ProfileId;

    /// Status mapping must match the design doc §8 contract: `Locked` → 423,
    /// `NotInitialized` → 409, `InvalidPasswordOrCorrupt` → 400,
    /// `IncompatibleBundle` → 422, `Io` / `Internal` → 500. The semantic
    /// `code` token is what the frontend error union switches on.
    #[test]
    fn map_config_migration_err_assigns_doc_statuses_and_codes() {
        let cases: Vec<(ConfigMigrationError, StatusCode, &str)> = vec![
            (ConfigMigrationError::Locked, StatusCode::LOCKED, "LOCKED"),
            (
                ConfigMigrationError::NotInitialized,
                StatusCode::CONFLICT,
                "NOT_INITIALIZED",
            ),
            (
                ConfigMigrationError::InvalidPasswordOrCorrupt,
                StatusCode::BAD_REQUEST,
                "INVALID_PASSWORD_OR_CORRUPT",
            ),
            (
                ConfigMigrationError::IncompatibleBundle {
                    reason: "schema too new".to_string(),
                },
                StatusCode::UNPROCESSABLE_ENTITY,
                "INCOMPATIBLE_BUNDLE",
            ),
            (
                ConfigMigrationError::Io {
                    details: "disk full".to_string(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                "IO",
            ),
            (
                ConfigMigrationError::Internal {
                    details: "boom".to_string(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL",
            ),
        ];
        for (err, status, code) in cases {
            let api = map_config_migration_err("export_config", err);
            assert_eq!(api.status, status);
            assert_eq!(api.code, code);
        }
    }

    /// `IncompatibleBundle` surfaces the (non-secret) reason verbatim so the
    /// operator sees why the bundle was rejected.
    #[test]
    fn incompatible_bundle_preserves_reason_in_message() {
        let api = map_config_migration_err(
            "preview_import",
            ConfigMigrationError::IncompatibleBundle {
                reason: "bundle archive schema 2 is newer than supported 1".to_string(),
            },
        );
        assert_eq!(api.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            api.message,
            "bundle archive schema 2 is newer than supported 1"
        );
    }

    /// `source_mode` is projected to a stable lowercase token and `profile_id`
    /// is flattened to its inner string for the wire DTO.
    #[test]
    fn preview_to_dto_projects_source_mode_and_profile() {
        let dto = preview_to_dto(ConfigImportPreview {
            app_version: "0.16.0".to_string(),
            source_mode: ConfigSourceMode::Installed,
            created_at_unix_ms: 1_700_000_000_000,
            profile_id: ProfileId::from("default"),
            device_fingerprint: "AB-CD".to_string(),
        });
        assert_eq!(dto.source_mode, "installed");
        assert_eq!(dto.profile_id, "default");
        assert_eq!(dto.created_at_unix_ms, 1_700_000_000_000);

        let portable = preview_to_dto(ConfigImportPreview {
            app_version: "0.16.0".to_string(),
            source_mode: ConfigSourceMode::Portable,
            created_at_unix_ms: 1,
            profile_id: ProfileId::from("default"),
            device_fingerprint: "X".to_string(),
        });
        assert_eq!(portable.source_mode, "portable");
    }
}

//! HTTP route handlers for the upgrade detection endpoints.
//!
//! Wires the `UpgradeFacade` (P1 thin upgrade detection module) into the
//! daemon REST API so the desktop frontend can decide whether to surface
//! the "re-pair after upgrade" notice on launch and acknowledge it.
//!
//! Endpoints:
//! - `GET /upgrade/status` — call `detect_on_startup` and return the
//!   discriminated status (FreshInstall / NoChange / Upgraded / Downgraded).
//! - `POST /upgrade/ack` — advance the version cursor to the running build.
//!
//! All responses use the canonical `ApiEnvelope<T> { data, ts }` success
//! envelope (ADR-008 §0.1) and `ApiErrorResponse { code, message, details? }`
//! for errors (§0.3). Both endpoints were already on `{ data, ts }`, so this
//! is NOT a wire change — only the bespoke wrapper structs are collapsed onto
//! the generic envelope.
//!
//! The running version is owned by the shared engine configuration.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::dto::upgrade::{AckUpgradePayload, UpgradeStatusDto};
use uc_engine::{EngineError, Operation, OperationResult};

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::projection::upgrade::upgrade_status_to_dto;
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/upgrade/status", get(get_upgrade_status_handler))
        .route("/upgrade/ack", post(ack_upgrade_handler))
}

/// GET /upgrade/status
/// Detect whether the running build is a fresh install / unchanged / upgraded
/// / downgraded relative to the stored version cursor.
#[utoipa::path(
    get,
    path = "/upgrade/status",
    operation_id = "getUpgradeStatus",
    tag = "upgrade",
    responses(
        (status = 200, description = "Upgrade status detected", body = UpgradeStatusEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_upgrade_status_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<UpgradeStatusDto>>, ApiError> {
    let result = state
        .execute(Operation::QueryUpgradeStatus)
        .await
        .map_err(detect_error_to_api)?;
    let OperationResult::UpgradeStatus(status) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected upgrade-status result",
        ));
    };

    Ok(Json(ApiEnvelope::now(upgrade_status_to_dto(status))))
}

/// POST /upgrade/ack
/// Advance the stored version cursor to the running build, clearing the
/// "re-pair after upgrade" notice.
#[utoipa::path(
    post,
    path = "/upgrade/ack",
    operation_id = "acknowledgeUpgrade",
    tag = "upgrade",
    responses(
        (status = 200, description = "Upgrade acknowledged", body = AckUpgradeEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn ack_upgrade_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<AckUpgradePayload>>, ApiError> {
    let result = state
        .execute(Operation::AcknowledgeUpgrade)
        .await
        .map_err(ack_error_to_api)?;
    let OperationResult::UpgradeAcknowledged { version } = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected upgrade-acknowledgement result",
        ));
    };

    Ok(Json(ApiEnvelope::now(AckUpgradePayload {
        acknowledged: version,
    })))
}

fn detect_error_to_api(error: EngineError) -> ApiError {
    tracing::error!(
        code = error.code(),
        category = %error.category(),
        "upgrade status query failed"
    );
    let variant = "query_failed";
    let api = ApiError::internal("failed to read upgrade status");
    log_facade_failure(
        "upgrade",
        "detect_on_startup",
        variant,
        api.status,
        &api.message,
    );
    api
}

fn ack_error_to_api(error: EngineError) -> ApiError {
    tracing::error!(
        code = error.code(),
        category = %error.category(),
        "upgrade acknowledgement failed"
    );
    let variant = "acknowledge_failed";
    let api = ApiError::internal("failed to acknowledge upgrade");
    log_facade_failure("upgrade", "acknowledge", variant, api.status, &api.message);
    api
}

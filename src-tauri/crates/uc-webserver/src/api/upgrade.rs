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
//! The version string fed to the facade is `env!("CARGO_PKG_VERSION")` of
//! `uc-webserver`, which is workspace-versioned alongside `uc-desktop`
//! (the daemon binary). Both crates resolve to the same value.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use uc_application::facade::{AcknowledgeUpgradeError, DetectUpgradeError, UpgradeStatus};
use uc_daemon_contract::api::dto::upgrade::{
    AckUpgradePayload, AckUpgradeResponse, GetUpgradeStatusResponse, UpgradeStatusDto,
};

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::server::DaemonApiState;

/// Build version reported to the upgrade facade. Workspace-versioned and
/// matches the `uc-desktop` daemon's own `DAEMON_VERSION`.
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/upgrade/status", get(get_upgrade_status_handler))
        .route("/upgrade/ack", post(ack_upgrade_handler))
}

#[utoipa::path(
    get,
    path = "/upgrade/status",
    tag = "upgrade",
    responses(
        (status = 200, body = GetUpgradeStatusResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn get_upgrade_status_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<GetUpgradeStatusResponse>, ApiError> {
    let app = state.app_facade_or_error()?;
    let status = app
        .upgrade
        .detect_on_startup(SERVER_VERSION)
        .await
        .map_err(detect_error_to_api)?;

    Ok(Json(GetUpgradeStatusResponse {
        data: status_to_dto(status),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

#[utoipa::path(
    post,
    path = "/upgrade/ack",
    tag = "upgrade",
    responses(
        (status = 200, body = AckUpgradeResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn ack_upgrade_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<AckUpgradeResponse>, ApiError> {
    let app = state.app_facade_or_error()?;
    app.upgrade
        .acknowledge(SERVER_VERSION)
        .await
        .map_err(ack_error_to_api)?;

    Ok(Json(AckUpgradeResponse {
        data: AckUpgradePayload {
            acknowledged: SERVER_VERSION.to_string(),
        },
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

fn status_to_dto(status: UpgradeStatus) -> UpgradeStatusDto {
    match status {
        UpgradeStatus::FreshInstall => UpgradeStatusDto::FreshInstall {
            current: SERVER_VERSION.to_string(),
        },
        UpgradeStatus::NoChange => UpgradeStatusDto::NoChange {
            current: SERVER_VERSION.to_string(),
        },
        UpgradeStatus::Upgraded { from, to } => UpgradeStatusDto::Upgraded {
            from: from.map(|v| v.to_string()),
            to: to.to_string(),
        },
        UpgradeStatus::Downgraded { from, to } => UpgradeStatusDto::Downgraded {
            from: from.to_string(),
            to: to.to_string(),
        },
    }
}

fn detect_error_to_api(err: DetectUpgradeError) -> ApiError {
    use DetectUpgradeError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::CurrentVersionMalformed(msg) => (
            "current_version_malformed",
            ApiError::internal(format!("current build version malformed: {msg}")),
        ),
        E::ReadCursor(msg) => (
            "read_cursor",
            ApiError::internal(format!("read app version cursor failed: {msg}")),
        ),
        E::ReadSetupStatus(msg) => (
            "read_setup_status",
            ApiError::internal(format!("read setup status failed: {msg}")),
        ),
    };
    log_facade_failure(
        "upgrade",
        "detect_on_startup",
        variant,
        api.status,
        &api.message,
    );
    api
}

fn ack_error_to_api(err: AcknowledgeUpgradeError) -> ApiError {
    use AcknowledgeUpgradeError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::CurrentVersionMalformed(msg) => (
            "current_version_malformed",
            ApiError::internal(format!("current build version malformed: {msg}")),
        ),
        E::WriteCursor(msg) => (
            "write_cursor",
            ApiError::internal(format!("write app version cursor failed: {msg}")),
        ),
    };
    log_facade_failure("upgrade", "acknowledge", variant, api.status, &api.message);
    api
}

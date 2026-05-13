//! HTTP route handlers for device info endpoints.
//!
//! Provides read-only access to local device identity (peer ID + device name).
//! 每成员同步设置已迁移至 `member::*`（phase 4b PR-2），本模块在 PR-4 后只保留
//! `GET /device/me`。

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use utoipa;

use crate::api::dto::device::GetLocalDeviceInfoResponse;
use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new().route("/device/me", get(get_local_device_info_handler))
}

/// GET /device/me
/// Returns the local device's peer ID and resolved device name.
#[utoipa::path(
    get,
    path = "/device/me",
    tag = "device",
    responses(
        (status = 200, body = GetLocalDeviceInfoResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn get_local_device_info_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<GetLocalDeviceInfoResponse>, ApiError> {
    let app = state.app_facade_or_error()?;
    let info = app.device.local_device_info().await.map_err(|e| {
        let api = ApiError::internal(e.to_string());
        log_facade_failure(
            "device",
            "local_device_info",
            "call_failed",
            api.status,
            &api.message,
        );
        api
    })?;

    Ok(Json(GetLocalDeviceInfoResponse {
        data: crate::api::dto::device::LocalDeviceInfoDto {
            peer_id: info.peer_id,
            device_name: info.device_name,
        },
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

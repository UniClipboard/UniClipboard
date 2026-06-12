//! HTTP route handlers for device info endpoints.
//!
//! Provides read-only access to local device identity (peer ID + device name).
//! 每成员同步设置已迁移至 `member::*`（phase 4b PR-2），本模块在 PR-4 后只保留
//! `GET /device/me`。
//!
//! Responses use the canonical `ApiEnvelope<T> { data, ts }` success envelope
//! (ADR-008 §0.1); `/device/me` was already on the `{data,ts}` wire, so this is
//! a no-op wire change (identical JSON, now produced by the shared envelope).

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use utoipa;

use uc_daemon_contract::api::dto::envelope::ApiEnvelope;

use crate::api::dto::device::LocalDeviceInfoDto;
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
    operation_id = "getLocalDeviceInfo",
    responses(
        (status = 200, description = "Local device identity", body = LocalDeviceInfoEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
async fn get_local_device_info_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<LocalDeviceInfoDto>>, ApiError> {
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

    Ok(Json(ApiEnvelope::now(LocalDeviceInfoDto {
        peer_id: info.peer_id,
        device_name: info.device_name,
    })))
}

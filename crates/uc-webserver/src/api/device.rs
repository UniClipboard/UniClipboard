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
use uc_engine::internal::device::{execute_query_local_device, QUERY_LOCAL_DEVICE_FAILED_CODE};
use uc_engine::{EngineError, OperationResult};

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
    let result = execute_query_local_device(app.as_ref())
        .await
        .map_err(map_local_device_engine_error)?;
    let OperationResult::LocalDevice(info) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected local-device result",
        ));
    };

    Ok(Json(ApiEnvelope::now(LocalDeviceInfoDto {
        peer_id: info.device_id,
        device_name: info.display_name,
    })))
}

fn map_local_device_engine_error(error: EngineError) -> ApiError {
    let variant = match error.code() {
        QUERY_LOCAL_DEVICE_FAILED_CODE => "query_failed",
        _ => "unexpected_engine_error",
    };
    let api = ApiError::internal("local device information is unavailable");
    log_facade_failure(
        "device",
        "local_device_info",
        variant,
        api.status,
        &api.message,
    );
    api
}

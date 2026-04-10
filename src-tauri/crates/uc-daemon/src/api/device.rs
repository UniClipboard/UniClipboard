//! HTTP route handlers for device info endpoints.
//!
//! Provides read-only access to local device identity (peer ID + device name)
//! and per-device sync settings management.

use axum::extract::{Path, State};
use axum::routing::{get, patch};
use axum::{Json, Router};
use utoipa;

use uc_app::usecases::CoreUseCases;
use uc_core::settings::model::SyncSettings as CoreSyncSettings;
use uc_core::PeerId;

use crate::api::conversion::IntoApiDto;
use crate::api::dto::device::{
    DeviceSyncSettingsPatchDto, GetDeviceSyncSettingsResponse, GetLocalDeviceInfoResponse,
    UpdateDeviceSyncSettingsResponse,
};
use crate::api::dto::error::ApiError;
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/device/me", get(get_local_device_info_handler))
        .route(
            "/device/:peer_id/sync-settings",
            get(get_device_sync_settings_handler),
        )
        .route(
            "/device/:peer_id/sync-settings",
            patch(update_device_sync_settings_handler),
        )
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
    let runtime = state.runtime_or_error()?;
    let usecases = CoreUseCases::new(runtime.as_ref());

    let info = usecases
        .get_local_device_info()
        .execute()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(GetLocalDeviceInfoResponse {
        data: info.into_api_dto(),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// GET /device/:peer_id/sync-settings
/// Returns the resolved sync settings for a paired device (per-device overrides
/// resolved against global defaults).
#[utoipa::path(
    get,
    path = "/device/{peer_id}/sync-settings",
    tag = "device",
    params(
        ("peer_id" = String, Path, description = "Paired device peer ID")
    ),
    responses(
        (status = 200, body = GetDeviceSyncSettingsResponse),
        (status = 404, description = "Device not found", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn get_device_sync_settings_handler(
    State(state): State<DaemonApiState>,
    Path(peer_id): Path<String>,
) -> Result<Json<GetDeviceSyncSettingsResponse>, ApiError> {
    let runtime = state.runtime_or_error()?;
    let usecases = CoreUseCases::new(runtime.as_ref());

    let settings = usecases
        .get_device_sync_settings()
        .execute(&PeerId::from(peer_id.as_str()))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, peer_id = %peer_id, "get_device_sync_settings failed");
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(GetDeviceSyncSettingsResponse {
        data: settings.into(),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// PATCH /device/:peer_id/sync-settings
/// Updates per-device sync settings. Passing `null` in the request body resets
/// to global defaults.
///
/// Partial update: only provided fields are changed.
#[utoipa::path(
    patch,
    path = "/device/{peer_id}/sync-settings",
    tag = "device",
    params(
        ("peer_id" = String, Path, description = "Paired device peer ID")
    ),
    request_body = DeviceSyncSettingsPatchDto,
    responses(
        (status = 200, body = UpdateDeviceSyncSettingsResponse),
        (status = 400, description = "Invalid request", body = crate::api::dto::error::ApiErrorResponse),
        (status = 404, description = "Device not found", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn update_device_sync_settings_handler(
    State(state): State<DaemonApiState>,
    Path(peer_id): Path<String>,
    Json(payload): Json<DeviceSyncSettingsPatchDto>,
) -> Result<Json<UpdateDeviceSyncSettingsResponse>, ApiError> {
    let runtime = state.runtime_or_error()?;
    let usecases = CoreUseCases::new(runtime.as_ref());

    let existing = usecases
        .get_device_sync_settings()
        .execute(&PeerId::from(peer_id.as_str()))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, peer_id = %peer_id, "failed to load existing device sync settings");
            ApiError::internal(e.to_string())
        })?;

    let merged = merge_device_sync_settings_patch(existing, payload)?;

    usecases
        .update_device_sync_settings()
        .execute(&PeerId::from(peer_id.as_str()), Some(merged))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, peer_id = %peer_id, "update_device_sync_settings failed");
            ApiError::internal(e.to_string())
        })?;

    let updated = usecases
        .get_device_sync_settings()
        .execute(&PeerId::from(peer_id.as_str()))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(UpdateDeviceSyncSettingsResponse {
        success: true,
        data: updated.into(),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// Merges a partial `DeviceSyncSettingsPatchDto` onto an existing `CoreSyncSettings`.
/// Only non-None fields from the patch are applied.
fn merge_device_sync_settings_patch(
    mut existing: CoreSyncSettings,
    patch: DeviceSyncSettingsPatchDto,
) -> Result<CoreSyncSettings, ApiError> {
    if let Some(auto_sync) = patch.auto_sync {
        existing.auto_sync = auto_sync;
    }

    if let Some(sync_frequency) = patch.sync_frequency {
        existing.sync_frequency = sync_frequency.into();
    }

    if let Some(content_types) = patch.content_types {
        if let Some(ct) = content_types.text {
            existing.content_types.text = ct;
        }
        if let Some(ct) = content_types.image {
            existing.content_types.image = ct;
        }
        if let Some(ct) = content_types.link {
            existing.content_types.link = ct;
        }
        if let Some(ct) = content_types.file {
            existing.content_types.file = ct;
        }
        if let Some(ct) = content_types.code_snippet {
            existing.content_types.code_snippet = ct;
        }
        if let Some(ct) = content_types.rich_text {
            existing.content_types.rich_text = ct;
        }
    }

    Ok(existing)
}

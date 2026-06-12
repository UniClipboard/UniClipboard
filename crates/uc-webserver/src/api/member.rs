//! HTTP handlers for per-member sync preferences (phase 4b).
//!
//! 读写 `SpaceMember.sync_preferences`（`MemberRepositoryPort`）；旧
//! `api::device::{get,update}_device_sync_settings_handler` 已在 PR-4 移除。

use axum::extract::{Path, State};
use axum::routing::{get, patch};
use axum::{Json, Router};
use tracing::{info, instrument};

use uc_application::facade::RosterError;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::dto::member::{MemberSyncPreferencesPatchDto, MemberSyncResultDto};
use crate::api::projection::{IntoApiDto, IntoDomain};
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route(
            "/member/:device_id/sync-preferences",
            get(get_member_sync_preferences_handler),
        )
        .route(
            "/member/:device_id/sync-preferences",
            patch(update_member_sync_preferences_handler),
        )
}

/// GET /member/:device_id/sync-preferences
/// 返回已接纳成员的同步偏好（双向 send/receive + 双套 content_types）。
#[utoipa::path(
    get,
    path = "/member/{device_id}/sync-preferences",
    tag = "member",
    operation_id = "getMemberSyncPreferences",
    params(
        ("device_id" = String, Path, description = "Space member's device ID (same string as peer_id, D5)")
    ),
    responses(
        (status = 200, body = MemberSyncPreferencesEnvelope),
        (status = 404, description = "Member not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(
    name = "api.member.get_sync_preferences",
    level = "info",
    skip(state),
    fields(device_id = %device_id)
)]
pub async fn get_member_sync_preferences_handler(
    State(state): State<DaemonApiState>,
    Path(device_id): Path<String>,
) -> Result<Json<ApiEnvelope<crate::api::dto::member::MemberSyncPreferencesDto>>, ApiError> {
    info!("get member sync preferences request received");
    let app = state.app_facade_or_error()?;
    let roster = app
        .member_roster
        .get()
        .cloned()
        .ok_or_else(|| ApiError::service_unavailable("member roster facade unavailable"))?;
    let prefs = roster
        .get_sync_preferences(&device_id)
        .await
        .map_err(|e| map_member_error(&device_id, "get_member_sync_preferences", e))?;

    info!(
        send_enabled = prefs.send_enabled,
        receive_enabled = prefs.receive_enabled,
        "get member sync preferences succeeded"
    );
    // Canonical `{ data, ts }` envelope (ADR-008 §0.1). Identical wire shape to
    // the legacy `GetMemberSyncPreferencesResponse` wrapper — not a wire change.
    Ok(Json(ApiEnvelope::now(prefs.into_api_dto())))
}

/// PATCH /member/:device_id/sync-preferences
/// 部分更新成员的同步偏好；未提供的字段保留当前值。
///
/// 内部 `get → merge → save`，保持与 `UpdateMemberSettingsUseCase` 的全量覆盖语义对齐。
#[utoipa::path(
    patch,
    path = "/member/{device_id}/sync-preferences",
    tag = "member",
    operation_id = "updateMemberSyncPreferences",
    params(
        ("device_id" = String, Path, description = "Space member's device ID")
    ),
    request_body = MemberSyncPreferencesPatchDto,
    responses(
        (status = 200, body = MemberSyncResultEnvelope),
        (status = 400, description = "Invalid request", body = ApiErrorResponse),
        (status = 404, description = "Member not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(
    name = "api.member.update_sync_preferences",
    level = "info",
    skip(state, payload),
    fields(
        device_id = %device_id,
        patch_send_enabled = ?payload.send_enabled,
        patch_receive_enabled = ?payload.receive_enabled,
        patch_send_content_types = payload.send_content_types.is_some(),
        patch_receive_content_types = payload.receive_content_types.is_some(),
    )
)]
pub async fn update_member_sync_preferences_handler(
    State(state): State<DaemonApiState>,
    Path(device_id): Path<String>,
    Json(payload): Json<MemberSyncPreferencesPatchDto>,
) -> Result<Json<ApiEnvelope<MemberSyncResultDto>>, ApiError> {
    info!("update member sync preferences request received");
    let app = state.app_facade_or_error()?;
    let roster = app
        .member_roster
        .get()
        .cloned()
        .ok_or_else(|| ApiError::service_unavailable("member roster facade unavailable"))?;
    let updated = roster
        .update_sync_preferences(&device_id, payload.into_domain())
        .await
        .map_err(|e| map_member_error(&device_id, "update_member_sync_preferences", e))?;

    info!(
        send_enabled = updated.send_enabled,
        receive_enabled = updated.receive_enabled,
        "update member sync preferences succeeded"
    );
    // BREAKING (ADR-008 §0.1): the legacy `{ success, data, ts }` shape collapses
    // into `ApiEnvelope<MemberSyncResultDto> = { data: { success }, ts }`. The
    // top-level `success` flag folds into the payload; the merged preferences
    // view is no longer echoed (consumers re-read via the GET endpoint).
    Ok(Json(ApiEnvelope::now(MemberSyncResultDto {
        success: true,
    })))
}

/// 把 `RosterError` 映射为 HTTP `ApiError`，并在 5xx 路径打根因日志。
///
/// `NotFound` → 404（业务可见层面，由请求日志兜底，本函数不再单独打 warn）；
/// `Unavailable` → 503；其余 repository / identity 故障 → 500。
/// 5xx 路径统一由 [`log_facade_failure`] 输出 `facade / op / error_variant` 结构化字段。
fn map_member_error(device_id: &str, op: &'static str, err: RosterError) -> ApiError {
    use RosterError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::NotFound(_) => (
            "not_found",
            ApiError::not_found(format!("member `{device_id}` not found")),
        ),
        E::MemberRepository(msg) => (
            "member_repository",
            ApiError::internal(format!("member repository failure: {msg}")),
        ),
        E::LocalIdentity(msg) => (
            "local_identity",
            ApiError::internal(format!("local identity failure: {msg}")),
        ),
        E::PeerAddressRepository(msg) => (
            "peer_address_repository",
            ApiError::internal(format!("peer address repository failure: {msg}")),
        ),
        E::TrustedPeerRepository(msg) => (
            "trusted_peer_repository",
            ApiError::internal(format!("trusted peer repository failure: {msg}")),
        ),
        E::Unavailable => (
            "unavailable",
            ApiError::service_unavailable("member roster facade unavailable"),
        ),
    };
    log_facade_failure("roster", op, variant, api.status, &api.message);
    api
}

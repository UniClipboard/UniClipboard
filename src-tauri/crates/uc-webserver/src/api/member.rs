//! HTTP handlers for per-member sync preferences (phase 4b).
//!
//! 读写 `SpaceMember.sync_preferences`（`MemberRepositoryPort`）；旧
//! `api::device::{get,update}_device_sync_settings_handler` 已在 PR-4 移除。

use axum::extract::{Path, State};
use axum::routing::{get, patch};
use axum::{Json, Router};
use tracing::{info, instrument};

use uc_application::facade::{
    ContentTypesPatch, MemberSyncPreferencesPatch, MemberSyncPreferencesView, RosterError,
};

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::dto::member::{
    GetMemberSyncPreferencesResponse, MemberSyncPreferencesPatchDto,
    UpdateMemberSyncPreferencesResponse,
};
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
    params(
        ("device_id" = String, Path, description = "Space member's device ID (same string as peer_id, D5)")
    ),
    responses(
        (status = 200, body = GetMemberSyncPreferencesResponse),
        (status = 404, description = "Member not found", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
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
) -> Result<Json<GetMemberSyncPreferencesResponse>, ApiError> {
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
    Ok(Json(GetMemberSyncPreferencesResponse {
        data: member_sync_preferences_to_dto(prefs),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// PATCH /member/:device_id/sync-preferences
/// 部分更新成员的同步偏好；未提供的字段保留当前值。
///
/// 内部 `get → merge → save`，保持与 `UpdateMemberSettingsUseCase` 的全量覆盖语义对齐。
#[utoipa::path(
    patch,
    path = "/member/{device_id}/sync-preferences",
    tag = "member",
    params(
        ("device_id" = String, Path, description = "Space member's device ID")
    ),
    request_body = MemberSyncPreferencesPatchDto,
    responses(
        (status = 200, body = UpdateMemberSyncPreferencesResponse),
        (status = 400, description = "Invalid request", body = crate::api::dto::error::ApiErrorResponse),
        (status = 404, description = "Member not found", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
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
) -> Result<Json<UpdateMemberSyncPreferencesResponse>, ApiError> {
    info!("update member sync preferences request received");
    let app = state.app_facade_or_error()?;
    let roster = app
        .member_roster
        .get()
        .cloned()
        .ok_or_else(|| ApiError::service_unavailable("member roster facade unavailable"))?;
    let updated = roster
        .update_sync_preferences(&device_id, member_sync_preferences_patch_from_dto(payload))
        .await
        .map_err(|e| map_member_error(&device_id, "update_member_sync_preferences", e))?;

    info!(
        send_enabled = updated.send_enabled,
        receive_enabled = updated.receive_enabled,
        "update member sync preferences succeeded"
    );
    Ok(Json(UpdateMemberSyncPreferencesResponse {
        success: true,
        data: member_sync_preferences_to_dto(updated),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

fn member_sync_preferences_patch_from_dto(
    patch: MemberSyncPreferencesPatchDto,
) -> MemberSyncPreferencesPatch {
    MemberSyncPreferencesPatch {
        send_enabled: patch.send_enabled,
        receive_enabled: patch.receive_enabled,
        send_content_types: patch.send_content_types.map(content_types_patch_from_dto),
        receive_content_types: patch
            .receive_content_types
            .map(content_types_patch_from_dto),
    }
}

fn content_types_patch_from_dto(
    patch: crate::api::dto::settings::ContentTypesPatchDto,
) -> ContentTypesPatch {
    ContentTypesPatch {
        text: patch.text,
        image: patch.image,
        link: patch.link,
        file: patch.file,
        code_snippet: patch.code_snippet,
        rich_text: patch.rich_text,
    }
}

fn member_sync_preferences_to_dto(
    value: MemberSyncPreferencesView,
) -> crate::api::dto::member::MemberSyncPreferencesDto {
    crate::api::dto::member::MemberSyncPreferencesDto {
        send_enabled: value.send_enabled,
        receive_enabled: value.receive_enabled,
        send_content_types: crate::api::dto::settings::ContentTypesDto {
            text: value.send_content_types.text,
            image: value.send_content_types.image,
            link: value.send_content_types.link,
            file: value.send_content_types.file,
            code_snippet: value.send_content_types.code_snippet,
            rich_text: value.send_content_types.rich_text,
        },
        receive_content_types: crate::api::dto::settings::ContentTypesDto {
            text: value.receive_content_types.text,
            image: value.receive_content_types.image,
            link: value.receive_content_types.link,
            file: value.receive_content_types.file,
            code_snippet: value.receive_content_types.code_snippet,
            rich_text: value.receive_content_types.rich_text,
        },
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_mapping_preserves_omitted_fields_as_none() {
        let patch = MemberSyncPreferencesPatchDto {
            send_enabled: Some(false),
            receive_enabled: None,
            send_content_types: None,
            receive_content_types: None,
        };
        let mapped = member_sync_preferences_patch_from_dto(patch);

        assert_eq!(mapped.send_enabled, Some(false));
        assert_eq!(mapped.receive_enabled, None);
        assert!(mapped.send_content_types.is_none());
        assert!(mapped.receive_content_types.is_none());
    }

    #[test]
    fn patch_mapping_keeps_partial_content_type_shape() {
        let patch = MemberSyncPreferencesPatchDto {
            send_enabled: None,
            receive_enabled: None,
            send_content_types: Some(crate::api::dto::settings::ContentTypesPatchDto {
                text: Some(true),
                image: None,
                link: None,
                file: None,
                code_snippet: None,
                rich_text: None,
            }),
            receive_content_types: None,
        };
        let mapped = member_sync_preferences_patch_from_dto(patch);
        let send = mapped.send_content_types.expect("send patch");
        assert_eq!(send.text, Some(true));
        assert_eq!(send.image, None);
    }
}

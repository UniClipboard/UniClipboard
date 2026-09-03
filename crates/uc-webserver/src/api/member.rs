//! HTTP handlers for per-member sync preferences.
//!
//! Storage access and partial-update semantics stay behind the engine interface.

use axum::extract::{Path, State};
use axum::routing::{get, patch};
use axum::{Json, Router};
use tracing::{info, instrument};

use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_engine::error_codes::{
    MEMBER_INVALID_INPUT_CODE, MEMBER_LEGACY_BOOTSTRAP_FAILED_CODE, MEMBER_NOT_FOUND_CODE,
    MEMBER_UNAVAILABLE_CODE, SPACE_PROTECTION_FAILED_CODE,
};
use uc_engine::{
    ChooseDeviceGroupInput, EngineError, EngineErrorCategory, Operation, OperationResult,
    QueryMemberSyncPreferencesInput, UpdateMemberSyncPreferencesInput,
};

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::dto::member::{
    ChooseDeviceGroupRequestDto, DeviceGroupChoiceResultDto, DeviceGroupChoicesDto,
    MemberSyncPreferencesPatchDto, MemberSyncResultDto, SpaceProtectionDto,
};
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
        .route("/member/protection", get(get_space_protection_handler))
        .route(
            "/member/device-group-choices",
            get(get_device_group_choices_handler).post(choose_device_group_handler),
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
    let result = state
        .execute(Operation::QueryMemberSyncPreferences(
            QueryMemberSyncPreferencesInput {
                device_id: device_id.clone(),
            },
        ))
        .await
        .map_err(|error| {
            map_member_engine_error(&device_id, "get_member_sync_preferences", error)
        })?;
    let OperationResult::MemberSyncPreferences(prefs) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected member-preferences result",
        ));
    };

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
    let result = state
        .execute(Operation::UpdateMemberSyncPreferences(
            UpdateMemberSyncPreferencesInput {
                device_id: device_id.clone(),
                patch: payload.into_domain(),
            },
        ))
        .await
        .map_err(|error| {
            map_member_engine_error(&device_id, "update_member_sync_preferences", error)
        })?;
    let OperationResult::MemberSyncPreferences(updated) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected member-preferences result",
        ));
    };

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

/// GET /member/protection
///
/// Projects the Engine's current protection authority. The daemon does not
/// infer or persist bootstrap progress; callers refresh this snapshot.
#[utoipa::path(
    get,
    path = "/member/protection",
    tag = "member",
    operation_id = "getSpaceProtection",
    responses(
        (status = 200, body = SpaceProtectionEnvelope),
        (status = 503, description = "Runtime unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.member.get_protection", level = "info", skip(state))]
pub async fn get_space_protection_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<SpaceProtectionDto>>, ApiError> {
    let result = state
        .execute(Operation::QuerySpaceProtection)
        .await
        .map_err(|error| map_member_engine_error("", "get_space_protection", error))?;
    let OperationResult::SpaceProtection(snapshot) = result else {
        tracing::error!(
            error_kind = "unexpected_operation_result",
            operation = "get_space_protection",
            "engine returned unexpected result"
        );
        return Err(ApiError::internal(
            "engine returned an unexpected space-protection result",
        ));
    };
    info!("get space protection succeeded");
    Ok(Json(ApiEnvelope::now(snapshot.into_api_dto())))
}

#[instrument(
    name = "api.member.get_device_group_choices",
    level = "info",
    skip(state)
)]
#[utoipa::path(
    get,
    path = "/member/device-group-choices",
    tag = "member",
    operation_id = "getDeviceGroupChoices",
    responses((status = 200, body = DeviceGroupChoicesEnvelope))
)]
pub async fn get_device_group_choices_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<DeviceGroupChoicesDto>>, ApiError> {
    let result = state
        .execute(Operation::QueryDeviceGroupChoices)
        .await
        .map_err(|error| map_member_engine_error("", "query_device_group_choices", error))?;
    let OperationResult::DeviceGroupChoices(choices) = result else {
        tracing::error!(
            error_kind = "unexpected_operation_result",
            operation = "query_device_group_choices",
            "engine returned an unexpected device group choices result"
        );
        return Err(ApiError::internal(
            "engine returned an unexpected device group choices result",
        ));
    };
    let revision = choices.revision;
    let issue_count = choices.issues.len();
    let dto = choices.into_api_dto();
    info!(
        revision,
        issue_count, "device group choices query completed"
    );
    Ok(Json(ApiEnvelope::now(dto)))
}

#[instrument(
    name = "api.member.choose_device_group",
    level = "info",
    skip(state, request),
    fields(expected_revision = request.expected_revision)
)]
#[utoipa::path(
    post,
    path = "/member/device-group-choices",
    tag = "member",
    operation_id = "chooseDeviceGroup",
    request_body = ChooseDeviceGroupRequestDto,
    responses((status = 200, body = DeviceGroupChoiceResultEnvelope))
)]
pub async fn choose_device_group_handler(
    State(state): State<DaemonApiState>,
    Json(request): Json<ChooseDeviceGroupRequestDto>,
) -> Result<Json<ApiEnvelope<DeviceGroupChoiceResultDto>>, ApiError> {
    let result = state
        .execute(Operation::ChooseDeviceGroup(ChooseDeviceGroupInput {
            issue_id: request.issue_id,
            choice_id: request.choice_id,
            expected_revision: request.expected_revision,
            confirm_local_removal: request.confirm_local_removal,
        }))
        .await
        .map_err(|error| map_member_engine_error("", "choose_device_group", error))?;
    let OperationResult::DeviceGroupChosen(choice) = result else {
        tracing::error!(
            error_kind = "unexpected_operation_result",
            operation = "choose_device_group",
            "engine returned an unexpected device group choice result"
        );
        return Err(ApiError::internal(
            "engine returned an unexpected device group choice result",
        ));
    };
    let outcome = choice.outcome;
    let dto = choice.into_api_dto();
    info!(?outcome, "device group choice completed");
    Ok(Json(ApiEnvelope::now(dto)))
}

pub(crate) fn map_member_engine_error(
    device_id: &str,
    op: &'static str,
    error: EngineError,
) -> ApiError {
    let (variant, api): (&'static str, ApiError) =
        if op == "choose_device_group" && error.category() == EngineErrorCategory::InvalidInput {
            (
                "invalid_device_group_choice",
                ApiError::bad_request("device group choice is invalid"),
            )
        } else if matches!(op, "query_device_group_choices" | "choose_device_group")
            && error.category() == EngineErrorCategory::Unavailable
        {
            (
                "device_group_choices_unavailable",
                ApiError::service_unavailable("device group choices are unavailable"),
            )
        } else {
            match error.code() {
                MEMBER_INVALID_INPUT_CODE => (
                    "invalid_input",
                    ApiError::bad_request("member device ID must not be empty"),
                ),
                MEMBER_NOT_FOUND_CODE => (
                    "not_found",
                    ApiError::not_found(format!("member `{device_id}` not found")),
                ),
                MEMBER_UNAVAILABLE_CODE => (
                    "unavailable",
                    ApiError::service_unavailable("member roster facade unavailable"),
                ),
                MEMBER_LEGACY_BOOTSTRAP_FAILED_CODE => (
                    "legacy_bootstrap_failed",
                    ApiError::internal("secure legacy member removal failed"),
                ),
                SPACE_PROTECTION_FAILED_CODE => (
                    "space_protection_failed",
                    ApiError::internal("space protection status is unavailable"),
                ),
                _ => ("internal", ApiError::internal("member operation failed")),
            }
        };
    log_facade_failure("roster", op, variant, api.status, &api.message);
    api
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use uc_engine::EngineErrorCategory;

    fn engine_error(code: u32) -> EngineError {
        EngineError::new(code, EngineErrorCategory::Internal, false)
    }

    #[test]
    fn invalid_device_group_choice_has_a_specific_message() {
        let api = map_member_engine_error(
            "",
            "choose_device_group",
            EngineError::new(1210, EngineErrorCategory::InvalidInput, false),
        );

        assert_eq!(api.status, StatusCode::BAD_REQUEST);
        assert_eq!(api.message, "device group choice is invalid");
    }

    #[test]
    fn secure_removal_and_protection_failures_do_not_expose_engine_detail() {
        for code in [
            MEMBER_LEGACY_BOOTSTRAP_FAILED_CODE,
            SPACE_PROTECTION_FAILED_CODE,
        ] {
            let api = map_member_engine_error("peer-1", "secure_remove_legacy", engine_error(code));

            assert_eq!(api.status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(api.code, "internal_error");
            assert!(api.details.is_none());
        }
    }

    #[test]
    fn member_removal_unknown_error_maps_to_internal() {
        let api = map_member_engine_error("peer-1", "get_member_removal", engine_error(9999));

        assert_eq!(api.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(api.code, "internal_error");
        assert!(api.details.is_none());
    }
}

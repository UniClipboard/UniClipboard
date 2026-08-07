//! HTTP handlers for per-member sync preferences.
//!
//! Storage access and partial-update semantics stay behind the engine interface.

use axum::extract::{Path, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use tracing::{info, instrument};

use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_engine::error_codes::{
    MEMBER_INVALID_INPUT_CODE, MEMBER_INVALID_PERMANENT_LOSS_SELECTION_CODE,
    MEMBER_LEGACY_BOOTSTRAP_FAILED_CODE, MEMBER_LEGACY_BOOTSTRAP_REQUIRED_CODE,
    MEMBER_NOT_FOUND_CODE, MEMBER_REMOVAL_IN_PROGRESS_CODE, MEMBER_REMOVAL_RECOVERY_REQUIRED_CODE,
    MEMBER_UNAVAILABLE_CODE, QUERY_MEMBERSHIP_CONVERGENCE_FAILED_CODE,
    QUERY_MEMBERSHIP_CONVERGENCE_UNAVAILABLE_CODE, SPACE_PROTECTION_FAILED_CODE,
};
use uc_engine::{
    ContinueMemberRevocationInput, EngineError, EngineErrorCategory, Operation, OperationResult,
    QueryMemberSyncPreferencesInput, QuerySharedDeviceRefreshInput, RemoveMemberInput,
    UpdateMemberSyncPreferencesInput,
};

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::dto::member::{
    ContinueMemberRemovalRequest, MemberRemovalDto, MemberSyncPreferencesPatchDto,
    MemberSyncResultDto, MembershipConvergenceDto, SecureLegacyRemovalDto, SharedDeviceRefreshDto,
    SharedDeviceRefreshStartedDto, SpaceProtectionDto,
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
            "/member/convergence",
            get(get_membership_convergence_handler),
        )
        .route(
            "/member/shared-device-refresh",
            post(start_shared_device_refresh_handler),
        )
        .route(
            "/member/shared-device-refresh/:request_id",
            get(get_shared_device_refresh_handler),
        )
        .route(
            "/member/:device_id/secure-remove",
            post(secure_remove_legacy_member_handler),
        )
        .route(
            "/member/removal/current",
            get(get_current_member_removal_handler),
        )
        .route(
            "/member/removal/continue",
            post(continue_member_removal_handler),
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

/// GET /member/convergence
///
/// Returns the Engine-owned connection state for the active space. The daemon
/// deliberately omits protocol counters because the product has no action for
/// them and must not mirror Engine retry state.
#[utoipa::path(
    get,
    path = "/member/convergence",
    tag = "member",
    operation_id = "getMembershipConvergence",
    responses(
        (status = 200, body = MembershipConvergenceEnvelope),
        (status = 503, description = "Runtime unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.member.get_convergence", level = "info", skip(state))]
pub async fn get_membership_convergence_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<MembershipConvergenceDto>>, ApiError> {
    let result = state
        .execute(Operation::QueryMembershipConvergence)
        .await
        .map_err(|error| map_member_engine_error("", "get_membership_convergence", error))?;
    let OperationResult::MembershipConvergence(snapshot) = result else {
        tracing::error!(
            error_kind = "unexpected_operation_result",
            operation = "get_membership_convergence",
            "engine returned unexpected result"
        );
        return Err(ApiError::internal(
            "engine returned an unexpected membership-convergence result",
        ));
    };
    info!(
        state = ?snapshot.state,
        "get membership convergence succeeded"
    );
    Ok(Json(ApiEnvelope::now(snapshot.into_api_dto())))
}

/// POST /member/shared-device-refresh
///
/// Starts one shared-device refresh round in the active Space. The Engine owns
/// deduplication while the first round is still in progress and returns the
/// same `requestId` for repeated starts.
#[utoipa::path(
    post,
    path = "/member/shared-device-refresh",
    tag = "member",
    operation_id = "startSharedDeviceRefresh",
    responses(
        (status = 200, body = SharedDeviceRefreshStartedEnvelope),
        (status = 503, description = "Runtime unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(
    name = "api.member.start_shared_device_refresh",
    level = "info",
    skip(state)
)]
pub async fn start_shared_device_refresh_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<SharedDeviceRefreshStartedDto>>, ApiError> {
    let result = state
        .execute(Operation::RefreshSharedDevices)
        .await
        .map_err(|error| map_shared_device_refresh_error("start", error))?;
    let OperationResult::SharedDeviceRefreshStarted(started) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected shared-device-refresh result",
        ));
    };
    info!(
        request_id = %started.request_id,
        "shared device refresh started"
    );
    Ok(Json(ApiEnvelope::now(started.into_api_dto())))
}

/// GET /member/shared-device-refresh/{request_id}
///
/// Returns the complete current snapshot for one refresh round. Unknown or
/// expired requests return a stable `shared_device_refresh_not_found` error.
#[utoipa::path(
    get,
    path = "/member/shared-device-refresh/{request_id}",
    tag = "member",
    operation_id = "getSharedDeviceRefresh",
    params(("request_id" = String, Path, description = "Shared-device refresh request ID")),
    responses(
        (status = 200, body = SharedDeviceRefreshEnvelope),
        (status = 404, description = "Request not found", body = ApiErrorResponse),
        (status = 503, description = "Runtime unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.member.get_shared_device_refresh", level = "info", skip(state), fields(request_id = %request_id))]
pub async fn get_shared_device_refresh_handler(
    State(state): State<DaemonApiState>,
    Path(request_id): Path<String>,
) -> Result<Json<ApiEnvelope<SharedDeviceRefreshDto>>, ApiError> {
    let result = state
        .execute(Operation::QuerySharedDeviceRefresh(
            QuerySharedDeviceRefreshInput {
                request_id: request_id.clone(),
            },
        ))
        .await
        .map_err(|error| map_shared_device_refresh_error("get", error))?;
    let OperationResult::SharedDeviceRefresh(Some(snapshot)) = result else {
        let api = ApiError::not_found("shared device refresh not found")
            .with_code("shared_device_refresh_not_found");
        tracing::info!(
            request_id,
            error_variant = "not_found",
            "shared device refresh query returned no active request"
        );
        return Err(api);
    };
    info!(
        request_id = %snapshot.request_id,
        phase = ?snapshot.phase,
        device_count = snapshot.devices.len(),
        "shared device refresh query succeeded"
    );
    Ok(Json(ApiEnvelope::now(snapshot.into_api_dto())))
}

/// POST /member/{device_id}/secure-remove
///
/// Starts the Engine-owned Legacy Space bootstrap that safely excludes a member
/// only after a replacement MLS state is activated. The returned summary is a
/// status snapshot, not a daemon-owned operation handle.
#[utoipa::path(
    post,
    path = "/member/{device_id}/secure-remove",
    tag = "member",
    operation_id = "secureRemoveLegacyMember",
    params(("device_id" = String, Path, description = "Legacy Space member to exclude")),
    responses(
        (status = 200, body = SecureLegacyRemovalEnvelope),
        (status = 400, description = "Invalid device ID", body = ApiErrorResponse),
        (status = 404, description = "Member not found", body = ApiErrorResponse),
        (status = 503, description = "Runtime unavailable", body = ApiErrorResponse),
        (status = 500, description = "Bootstrap failed", body = ApiErrorResponse)
    )
)]
#[instrument(
    name = "api.member.secure_remove_legacy",
    level = "info",
    skip(state),
    fields(device_id = %device_id)
)]
pub async fn secure_remove_legacy_member_handler(
    State(state): State<DaemonApiState>,
    Path(device_id): Path<String>,
) -> Result<Json<ApiEnvelope<SecureLegacyRemovalDto>>, ApiError> {
    let result = state
        .execute(Operation::SecureRemoveLegacyMember(RemoveMemberInput {
            device_id: device_id.clone(),
        }))
        .await
        .map_err(|error| map_member_engine_error(&device_id, "secure_remove_legacy", error))?;
    let OperationResult::LegacyMemberRemoval(bootstrap) = result else {
        tracing::error!(
            error_kind = "unexpected_operation_result",
            operation = "secure_remove_legacy",
            "engine returned unexpected result"
        );
        return Err(ApiError::internal(
            "engine returned an unexpected legacy-removal result",
        ));
    };
    info!("secure legacy member removal started");
    Ok(Json(ApiEnvelope::now(SecureLegacyRemovalDto {
        bootstrap: bootstrap.into_api_dto(),
    })))
}

#[utoipa::path(get, path = "/member/removal/current", tag = "member", operation_id = "getCurrentMemberRemoval", responses((status = 200, body = CurrentMemberRemovalEnvelope), (status = 503, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse)))]
#[instrument(name = "api.member.get_current_removal", level = "info", skip(state))]
pub async fn get_current_member_removal_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<Option<MemberRemovalDto>>>, ApiError> {
    let result = state
        .execute(Operation::QueryCurrentMemberRevocation)
        .await
        .map_err(|error| map_member_engine_error("current", "get_current_removal", error))?;
    let OperationResult::MemberRevocationStatus(removal) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected member-removal status",
        ));
    };
    info!(
        has_removal = removal.is_some(),
        "current member removal loaded"
    );
    Ok(Json(ApiEnvelope::now(
        removal.map(IntoApiDto::into_api_dto),
    )))
}

#[utoipa::path(post, path = "/member/removal/continue", tag = "member", operation_id = "continueMemberRemoval", request_body = ContinueMemberRemovalRequest, responses((status = 200, body = MemberRemovalEnvelope), (status = 400, body = ApiErrorResponse), (status = 409, body = ApiErrorResponse), (status = 503, body = ApiErrorResponse), (status = 500, body = ApiErrorResponse)))]
#[instrument(name = "api.member.continue_removal", level = "info", skip(state, payload), fields(revocation_id = %payload.revocation_id, permanently_lost_count = payload.permanently_lost_device_ids.len()))]
pub async fn continue_member_removal_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<ContinueMemberRemovalRequest>,
) -> Result<Json<ApiEnvelope<MemberRemovalDto>>, ApiError> {
    let revocation_id = payload.revocation_id.clone();
    let result = state
        .execute(Operation::ContinueMemberRevocation(
            ContinueMemberRevocationInput {
                revocation_id: revocation_id.clone(),
                permanently_lost_device_ids: payload.permanently_lost_device_ids,
            },
        ))
        .await
        .map_err(|error| map_member_engine_error(&revocation_id, "continue_removal", error))?;
    let OperationResult::MemberRevocationStatus(Some(removal)) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected member-removal continuation",
        ));
    };
    info!(
        revocation_id,
        "member removal continued after permanent-loss confirmation"
    );
    Ok(Json(ApiEnvelope::now(removal.into_api_dto())))
}

pub(crate) fn legacy_bootstrap_required_error() -> ApiError {
    ApiError::conflict("legacy Space member removal requires secure bootstrap")
        .with_code("legacy_bootstrap_required")
}

fn map_shared_device_refresh_error(op: &'static str, error: EngineError) -> ApiError {
    if error.category() == EngineErrorCategory::Unavailable {
        let api = ApiError::service_unavailable("shared device refresh is unavailable")
            .with_code("shared_device_refresh_unavailable");
        log_facade_failure("roster", op, "unavailable", api.status, &api.message);
        return api;
    }
    let api = ApiError::internal("shared device refresh failed");
    log_facade_failure("roster", op, "internal", api.status, &api.message);
    api
}

pub(crate) fn map_member_engine_error(
    device_id: &str,
    op: &'static str,
    error: EngineError,
) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match error.code() {
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
        MEMBER_LEGACY_BOOTSTRAP_REQUIRED_CODE => (
            "legacy_bootstrap_required",
            legacy_bootstrap_required_error(),
        ),
        MEMBER_LEGACY_BOOTSTRAP_FAILED_CODE => (
            "legacy_bootstrap_failed",
            ApiError::internal("secure legacy member removal failed"),
        ),
        MEMBER_REMOVAL_IN_PROGRESS_CODE => (
            "member_removal_in_progress",
            ApiError::conflict("another member removal is already in progress")
                .with_code("member_removal_in_progress"),
        ),
        MEMBER_REMOVAL_RECOVERY_REQUIRED_CODE => (
            "member_removal_recovery_required",
            ApiError::conflict("member removal requires permanent-loss recovery")
                .with_code("member_removal_recovery_required"),
        ),
        MEMBER_INVALID_PERMANENT_LOSS_SELECTION_CODE => (
            "invalid_permanent_loss_selection",
            ApiError::bad_request("selected device is not awaiting member-removal confirmation")
                .with_code("invalid_permanent_loss_selection"),
        ),
        SPACE_PROTECTION_FAILED_CODE => (
            "space_protection_failed",
            ApiError::internal("space protection status is unavailable"),
        ),
        QUERY_MEMBERSHIP_CONVERGENCE_UNAVAILABLE_CODE => (
            "membership_convergence_unavailable",
            ApiError::service_unavailable("space connection status is unavailable"),
        ),
        QUERY_MEMBERSHIP_CONVERGENCE_FAILED_CODE => (
            "membership_convergence_failed",
            ApiError::internal("failed to query space connection status"),
        ),
        _ => ("internal", ApiError::internal("member operation failed")),
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
    fn legacy_required_error_is_a_stable_conflict_for_the_legacy_client_path() {
        let api = map_member_engine_error(
            "peer-1",
            "get_member_sync_preferences",
            engine_error(MEMBER_LEGACY_BOOTSTRAP_REQUIRED_CODE),
        );

        assert_eq!(api.status, StatusCode::CONFLICT);
        assert_eq!(api.code, "legacy_bootstrap_required");
        assert_eq!(
            api.message,
            "legacy Space member removal requires secure bootstrap"
        );
    }

    #[test]
    fn legacy_bootstrap_required_error_preserves_the_shared_contract() {
        let api = legacy_bootstrap_required_error();

        assert_eq!(api.status, StatusCode::CONFLICT);
        assert_eq!(api.code, "legacy_bootstrap_required");
        assert_eq!(
            api.message,
            "legacy Space member removal requires secure bootstrap"
        );
    }

    #[test]
    fn shared_device_refresh_errors_use_stable_categories_without_leaking_details() {
        let unavailable = map_shared_device_refresh_error(
            "start",
            EngineError::new(1103, EngineErrorCategory::Unavailable, false),
        );
        assert_eq!(unavailable.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(unavailable.code, "shared_device_refresh_unavailable");
        assert!(!unavailable.message.contains("1103"));

        let failed = map_shared_device_refresh_error(
            "get",
            EngineError::new(1393, EngineErrorCategory::Internal, true),
        );
        assert_eq!(failed.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(failed.code, "internal_error");
        assert!(failed.details.is_none());
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
}

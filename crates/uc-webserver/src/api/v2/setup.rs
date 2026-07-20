//! Stateless v2 setup pairing HTTP handlers (Slice4 Phase 3 T3.2).
//!
//! Six endpoints under `/v2/setup/*`, each a thin adapter that translates a
//! stable engine operation or remaining `SpaceSetupFacade` call into the wire
//! DTOs declared in `uc_daemon_contract::api::dto::v2::setup`. Errors map onto
//! the daemon-wide `ApiError` surface (400 / 409 / 500 / 503).

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use uc_application::facade::space_setup::QueryMigrationProgressError;
use uc_application::facade::{
    CancelInvitationError, QuerySetupStateError, ResetSpaceError, SpaceSetupFacade,
};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::dto::v2::setup::{
    InitializeSpaceRequest, InitializeSpaceResponse, IssueInvitationResponse,
    MigrationProgressResponse, RedeemRequest, RedeemResponse, SetupStateResponse,
    SwitchSpaceRequest, SwitchSpaceResponse,
};
use uc_daemon_contract::constants::http_route_v2;
use uc_engine::internal::create_space::{
    execute_create_space, CREATE_SPACE_ALREADY_INITIALIZED_CODE, CREATE_SPACE_ALREADY_SETUP_CODE,
    CREATE_SPACE_DEVICE_NAME_REQUIRED_CODE, CREATE_SPACE_PASSPHRASE_MISMATCH_CODE,
};
use uc_engine::internal::invitation::execute_issue_invitation;
use uc_engine::internal::join_space::{
    execute_join_space, JoinSpaceMode, JOIN_SPACE_CONNECTION_LOST_CODE,
    JOIN_SPACE_CORRUPTED_KEY_CODE, JOIN_SPACE_DEVICE_NAME_REQUIRED_CODE,
    JOIN_SPACE_INVALID_CIPHERTEXT_CODE, JOIN_SPACE_INVITATION_EXPIRED_CODE,
    JOIN_SPACE_INVITATION_NOT_FOUND_CODE, JOIN_SPACE_NOT_SETUP_CODE, JOIN_SPACE_NOT_UNLOCKED_CODE,
    JOIN_SPACE_PASSPHRASE_MISMATCH_CODE, JOIN_SPACE_PENDING_MIGRATION_CODE,
    JOIN_SPACE_SERVICE_UNAVAILABLE_CODE, JOIN_SPACE_SPONSOR_DECLINED_CODE,
    JOIN_SPACE_SPONSOR_REJECTED_CODE, JOIN_SPACE_SPONSOR_TIMEOUT_CODE,
    JOIN_SPACE_SPONSOR_UNREACHABLE_CODE, JOIN_SPACE_STORAGE_CODE, JOIN_SPACE_TIMEOUT_CODE,
};
use uc_engine::{
    CreateSpaceInput, EngineError, EngineErrorCategory, JoinSpaceInput, OperationResult,
    SecretString,
};

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::projection::IntoApiDto;
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route(http_route_v2::SETUP_INITIALIZE, post(initialize))
        .route(
            http_route_v2::SETUP_ISSUE_INVITATION,
            post(issue_invitation),
        )
        .route(http_route_v2::SETUP_REDEEM, post(redeem))
        .route(http_route_v2::SETUP_CANCEL, post(cancel))
        .route(http_route_v2::SETUP_RESET, post(reset))
        .route(http_route_v2::SETUP_STATE, get(get_state))
        .route(http_route_v2::SETUP_SWITCH_SPACE, post(switch_space))
        .route(
            http_route_v2::SETUP_MIGRATION_PROGRESS,
            get(query_migration_progress),
        )
}

fn require_facade(state: &DaemonApiState) -> Result<std::sync::Arc<SpaceSetupFacade>, ApiError> {
    state
        .app_facade_or_error()?
        .space_setup
        .get()
        .cloned()
        .ok_or_else(|| ApiError::service_unavailable("space setup facade not assembled"))
}

// ---------------------------------------------------------------------------
// POST /v2/setup/initialize
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/v2/setup/initialize",
    tag = "setup-v2",
    operation_id = "setupV2Initialize",
    request_body = InitializeSpaceRequest,
    responses(
        (status = 200, description = "Space initialised", body = SetupInitializeEnvelope),
        (status = 400, description = "Passphrase mismatch or device name missing", body = ApiErrorResponse),
        (status = 409, description = "Setup already completed", body = ApiErrorResponse),
        (status = 503, description = "Facade not assembled", body = ApiErrorResponse),
        (status = 500, description = "Internal error", body = ApiErrorResponse),
    ),
)]
pub(crate) async fn initialize(
    State(state): State<DaemonApiState>,
    Json(req): Json<InitializeSpaceRequest>,
) -> Result<Json<ApiEnvelope<InitializeSpaceResponse>>, ApiError> {
    let app = state.app_facade_or_error()?;
    let result = execute_create_space(
        app.as_ref(),
        state.receive_readiness.as_ref(),
        CreateSpaceInput {
            passphrase: SecretString::new(req.passphrase),
            passphrase_confirmation: SecretString::new(req.passphrase_confirm),
            device_name: req.device_name,
        },
    )
    .await
    .map_err(map_create_engine_err)?;
    let OperationResult::SpaceCreated {
        space_id,
        self_device_id,
        identity_fingerprint,
    } = result
    else {
        return Err(ApiError::internal(
            "engine returned an unexpected create-space result",
        ));
    };
    Ok(Json(ApiEnvelope::now(InitializeSpaceResponse {
        space_id,
        self_device_id,
        fingerprint: identity_fingerprint,
    })))
}

fn map_create_engine_err(err: EngineError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match err.code() {
        CREATE_SPACE_PASSPHRASE_MISMATCH_CODE => (
            "passphrase_mismatch",
            ApiError::bad_request("passphrase and confirmation do not match"),
        ),
        CREATE_SPACE_DEVICE_NAME_REQUIRED_CODE => (
            "device_name_required",
            ApiError::bad_request("device name is required"),
        ),
        CREATE_SPACE_ALREADY_INITIALIZED_CODE => (
            "already_initialized",
            ApiError::conflict("space is already initialised"),
        ),
        CREATE_SPACE_ALREADY_SETUP_CODE => (
            "already_setup",
            ApiError::conflict("setup has already been completed on this device"),
        ),
        _ if err.category() == EngineErrorCategory::Unavailable => (
            "service_unavailable",
            ApiError::service_unavailable("create-space service unavailable"),
        ),
        _ => ("internal", ApiError::internal("failed to create space")),
    };
    log_facade_failure(
        "space_setup",
        "initialize_space",
        variant,
        api.status,
        &api.message,
    );
    api
}

// ---------------------------------------------------------------------------
// POST /v2/setup/issue-invitation
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/v2/setup/issue-invitation",
    tag = "setup-v2",
    operation_id = "setupV2IssueInvitation",
    responses(
        (status = 200, description = "Invitation issued", body = SetupIssueInvitationEnvelope),
        (status = 503, description = "Facade not assembled or network not started", body = ApiErrorResponse),
        (status = 500, description = "Internal error", body = ApiErrorResponse),
    ),
)]
pub(crate) async fn issue_invitation(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<IssueInvitationResponse>>, ApiError> {
    let app = state.app_facade_or_error()?;
    let result = execute_issue_invitation(app.as_ref())
        .await
        .map_err(map_issue_engine_err)?;
    let OperationResult::InvitationIssued {
        invitation_code,
        expires_at_ms,
    } = result
    else {
        return Err(ApiError::internal(
            "engine returned an unexpected invitation result",
        ));
    };
    Ok(Json(ApiEnvelope::now(IssueInvitationResponse {
        code: invitation_code,
        expires_at_ms,
    })))
}

fn map_issue_engine_err(err: EngineError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match err.category() {
        EngineErrorCategory::InvalidState => (
            "network_not_started",
            ApiError::service_unavailable("network is not started"),
        ),
        EngineErrorCategory::Unavailable => (
            "service_unavailable",
            ApiError::service_unavailable("pairing invitation service unavailable"),
        ),
        EngineErrorCategory::InvalidInput => (
            "invalid_input",
            ApiError::internal("unexpected invalid invitation input on default path"),
        ),
        _ => (
            "internal",
            ApiError::internal("failed to issue pairing invitation"),
        ),
    };
    log_facade_failure(
        "space_setup",
        "issue_pairing_invitation",
        variant,
        api.status,
        &api.message,
    );
    api
}

// ---------------------------------------------------------------------------
// POST /v2/setup/redeem
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/v2/setup/redeem",
    tag = "setup-v2",
    operation_id = "setupV2Redeem",
    request_body = RedeemRequest,
    responses(
        (status = 200, description = "Invitation redeemed", body = SetupRedeemEnvelope),
        (status = 400, description = "Invalid request", body = ApiErrorResponse),
        (status = 404, description = "Invitation not found / expired", body = ApiErrorResponse),
        (status = 503, description = "Sponsor unreachable / service unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal error", body = ApiErrorResponse),
    ),
)]
pub(crate) async fn redeem(
    State(state): State<DaemonApiState>,
    Json(req): Json<RedeemRequest>,
) -> Result<Json<ApiEnvelope<RedeemResponse>>, ApiError> {
    let app = state.app_facade_or_error()?;
    let result = execute_join_space(
        app.as_ref(),
        state.receive_readiness.as_ref(),
        JoinSpaceInput {
            invitation_code: req.code,
            device_name: None,
            passphrase: SecretString::new(req.passphrase),
        },
        JoinSpaceMode::Fresh,
    )
    .await
    .map_err(map_join_engine_err)?;
    let OperationResult::SpaceJoined {
        sponsor_device_id,
        sponsor_identity_fingerprint,
        space_id,
        self_device_id,
        self_identity_fingerprint,
        migrated_records: None,
    } = result
    else {
        return Err(ApiError::internal(
            "engine returned an unexpected fresh-join result",
        ));
    };
    Ok(Json(ApiEnvelope::now(RedeemResponse {
        sponsor_device_id,
        sponsor_identity_fingerprint,
        space_id,
        self_device_id,
        self_identity_fingerprint,
    })))
}

fn map_join_engine_err(err: EngineError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match err.code() {
        JOIN_SPACE_INVITATION_NOT_FOUND_CODE => (
            "invitation_not_found",
            ApiError::not_found("invitation not found"),
        ),
        JOIN_SPACE_INVITATION_EXPIRED_CODE => (
            "invitation_expired",
            ApiError::not_found("invitation has expired"),
        ),
        JOIN_SPACE_SPONSOR_UNREACHABLE_CODE => (
            "sponsor_unreachable",
            ApiError::service_unavailable("sponsor is not reachable"),
        ),
        JOIN_SPACE_SERVICE_UNAVAILABLE_CODE => (
            "service_unavailable",
            ApiError::service_unavailable("pairing invitation service unavailable"),
        ),
        JOIN_SPACE_PASSPHRASE_MISMATCH_CODE => (
            "passphrase_mismatch",
            ApiError::bad_request("wrong passphrase"),
        ),
        JOIN_SPACE_CORRUPTED_KEY_CODE => (
            "corrupted_key_material",
            ApiError::internal("space key material corrupted"),
        ),
        JOIN_SPACE_DEVICE_NAME_REQUIRED_CODE => (
            "device_name_required",
            ApiError::bad_request("device name is required"),
        ),
        JOIN_SPACE_SPONSOR_REJECTED_CODE => (
            "sponsor_rejected_invitation",
            ApiError::conflict("sponsor did not recognise the invitation code"),
        ),
        JOIN_SPACE_SPONSOR_DECLINED_CODE => (
            "sponsor_declined",
            ApiError::conflict("sponsor declined the pairing request"),
        ),
        JOIN_SPACE_SPONSOR_TIMEOUT_CODE => (
            "sponsor_timed_out",
            ApiError::service_unavailable("sponsor timed out the handshake"),
        ),
        JOIN_SPACE_TIMEOUT_CODE => (
            "timeout",
            ApiError::service_unavailable("pairing handshake timed out"),
        ),
        JOIN_SPACE_CONNECTION_LOST_CODE => (
            "connection_lost",
            ApiError::service_unavailable("connection lost mid-handshake"),
        ),
        _ if err.category() == EngineErrorCategory::Unavailable => (
            "service_unavailable",
            ApiError::service_unavailable("join-space service unavailable"),
        ),
        _ => ("internal", ApiError::internal("failed to join space")),
    };
    log_facade_failure(
        "space_setup",
        "redeem_pairing_invitation",
        variant,
        api.status,
        &api.message,
    );
    api
}

// ---------------------------------------------------------------------------
// POST /v2/setup/cancel
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/v2/setup/cancel",
    tag = "setup-v2",
    operation_id = "setupV2Cancel",
    responses(
        (status = 204, description = "Invitation cancelled"),
        (status = 409, description = "No in-flight invitation to cancel", body = ApiErrorResponse),
        (status = 503, description = "Facade not assembled", body = ApiErrorResponse),
        (status = 500, description = "Internal error", body = ApiErrorResponse),
    ),
)]
pub(crate) async fn cancel(State(state): State<DaemonApiState>) -> Result<StatusCode, ApiError> {
    let facade = require_facade(&state)?;
    facade.cancel_invitation().await.map_err(map_cancel_err)?;
    Ok(StatusCode::NO_CONTENT)
}

fn map_cancel_err(err: CancelInvitationError) -> ApiError {
    use CancelInvitationError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::NotIssued => (
            "not_issued",
            ApiError::conflict("no in-flight invitation to cancel"),
        ),
        E::Internal(msg) => ("internal", ApiError::internal(msg)),
    };
    log_facade_failure(
        "space_setup",
        "cancel_invitation",
        variant,
        api.status,
        &api.message,
    );
    api
}

// ---------------------------------------------------------------------------
// POST /v2/setup/reset
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/v2/setup/reset",
    tag = "setup-v2",
    operation_id = "setupV2Reset",
    responses(
        (status = 204, description = "Setup reset"),
        (status = 503, description = "Facade not assembled", body = ApiErrorResponse),
        (status = 500, description = "Storage failure", body = ApiErrorResponse),
    ),
)]
pub(crate) async fn reset(State(state): State<DaemonApiState>) -> Result<StatusCode, ApiError> {
    let facade = require_facade(&state)?;
    facade.reset().await.map_err(map_reset_err)?;
    Ok(StatusCode::NO_CONTENT)
}

fn map_reset_err(err: ResetSpaceError) -> ApiError {
    use ResetSpaceError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::StorageFailed(msg) => ("storage_failed", ApiError::internal(msg)),
        E::Internal(msg) => ("internal", ApiError::internal(msg)),
    };
    log_facade_failure("space_setup", "reset", variant, api.status, &api.message);
    api
}

// ---------------------------------------------------------------------------
// GET /v2/setup/state
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/v2/setup/state",
    tag = "setup-v2",
    operation_id = "setupV2GetState",
    responses(
        (status = 200, description = "Setup state snapshot", body = SetupStateEnvelope),
        (status = 503, description = "Facade not assembled", body = ApiErrorResponse),
        (status = 500, description = "Storage failure", body = ApiErrorResponse),
    ),
)]
pub(crate) async fn get_state(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<SetupStateResponse>>, ApiError> {
    let facade = require_facade(&state)?;
    let view = facade
        .query_setup_state()
        .await
        .map_err(map_query_setup_state_err)?;
    Ok(Json(ApiEnvelope::now(view.into_api_dto())))
}

fn map_query_setup_state_err(err: QuerySetupStateError) -> ApiError {
    use QuerySetupStateError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::StorageFailed(msg) => ("storage_failed", ApiError::internal(msg)),
        E::Internal(msg) => ("internal", ApiError::internal(msg)),
    };
    log_facade_failure(
        "space_setup",
        "query_setup_state",
        variant,
        api.status,
        &api.message,
    );
    api
}

// ---------------------------------------------------------------------------
// POST /v2/setup/switch-space
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/v2/setup/switch-space",
    tag = "setup-v2",
    operation_id = "setupV2SwitchSpace",
    request_body = SwitchSpaceRequest,
    responses(
        (status = 200, description = "Switched space", body = SetupSwitchSpaceEnvelope),
        (status = 400, description = "Wrong passphrase / device name missing", body = ApiErrorResponse),
        (status = 404, description = "Invitation not found / expired", body = ApiErrorResponse),
        (status = 409, description = "Not setup, pending migration, sponsor rejected, or session locked", body = ApiErrorResponse),
        (status = 503, description = "Sponsor unreachable / service unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal error / corrupted ciphertext / storage failure", body = ApiErrorResponse),
    ),
)]
pub(crate) async fn switch_space(
    State(state): State<DaemonApiState>,
    Json(req): Json<SwitchSpaceRequest>,
) -> Result<Json<ApiEnvelope<SwitchSpaceResponse>>, ApiError> {
    let app = state.app_facade_or_error()?;
    let result = execute_join_space(
        app.as_ref(),
        state.receive_readiness.as_ref(),
        JoinSpaceInput {
            invitation_code: req.code,
            device_name: None,
            passphrase: SecretString::new(req.new_passphrase),
        },
        JoinSpaceMode::Switch,
    )
    .await
    .map_err(map_switch_engine_err)?;
    let OperationResult::SpaceJoined {
        sponsor_device_id,
        sponsor_identity_fingerprint,
        space_id,
        self_device_id,
        self_identity_fingerprint,
        migrated_records: Some(migrated_records),
    } = result
    else {
        return Err(ApiError::internal(
            "engine returned an unexpected switch-space result",
        ));
    };
    Ok(Json(ApiEnvelope::now(SwitchSpaceResponse {
        sponsor_device_id,
        sponsor_identity_fingerprint,
        space_id,
        self_device_id,
        self_identity_fingerprint,
        migrated_records,
    })))
}

fn map_switch_engine_err(err: EngineError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match err.code() {
        JOIN_SPACE_NOT_SETUP_CODE => (
            "not_setup",
            ApiError::conflict(
                "this device has not completed first-time setup yet; use /v2/setup/initialize \
                 or /v2/setup/redeem first",
            ),
        ),
        JOIN_SPACE_PENDING_MIGRATION_CODE => (
            "pending_migration",
            ApiError::conflict(
                "a previous switch-space migration is still in flight; restart the daemon to \
                 auto-resume, or call /v2/setup/reset to abandon",
            ),
        ),
        JOIN_SPACE_NOT_UNLOCKED_CODE => (
            "not_unlocked",
            ApiError::conflict(
                "space session is locked; unlock the current space before switching",
            ),
        ),
        JOIN_SPACE_INVITATION_NOT_FOUND_CODE => (
            "invitation_not_found",
            ApiError::not_found("invitation not found"),
        ),
        JOIN_SPACE_INVITATION_EXPIRED_CODE => (
            "invitation_expired",
            ApiError::not_found("invitation has expired"),
        ),
        JOIN_SPACE_SPONSOR_UNREACHABLE_CODE => (
            "sponsor_unreachable",
            ApiError::service_unavailable("sponsor is not reachable"),
        ),
        JOIN_SPACE_SERVICE_UNAVAILABLE_CODE => (
            "service_unavailable",
            ApiError::service_unavailable("pairing invitation service unavailable"),
        ),
        JOIN_SPACE_PASSPHRASE_MISMATCH_CODE => (
            "passphrase_mismatch",
            ApiError::bad_request("wrong passphrase"),
        ),
        JOIN_SPACE_CORRUPTED_KEY_CODE => (
            "corrupted_key_material",
            ApiError::internal("space key material corrupted"),
        ),
        JOIN_SPACE_DEVICE_NAME_REQUIRED_CODE => (
            "device_name_required",
            ApiError::bad_request("device name is required"),
        ),
        JOIN_SPACE_SPONSOR_REJECTED_CODE => (
            "sponsor_rejected_invitation",
            ApiError::conflict("sponsor did not recognise the invitation code"),
        ),
        JOIN_SPACE_SPONSOR_DECLINED_CODE => (
            "sponsor_declined",
            ApiError::conflict("sponsor declined the pairing request"),
        ),
        JOIN_SPACE_TIMEOUT_CODE => (
            "timeout",
            ApiError::service_unavailable("handshake timed out"),
        ),
        JOIN_SPACE_CONNECTION_LOST_CODE => (
            "connection_lost",
            ApiError::service_unavailable("connection lost mid-handshake"),
        ),
        JOIN_SPACE_INVALID_CIPHERTEXT_CODE => (
            "invalid_ciphertext",
            ApiError::internal("backup record decryption failed (corrupted ciphertext)"),
        ),
        JOIN_SPACE_STORAGE_CODE => (
            "storage",
            ApiError::internal("storage failure while switching space"),
        ),
        _ if err.category() == EngineErrorCategory::Unavailable => (
            "service_unavailable",
            ApiError::service_unavailable("switch-space service unavailable"),
        ),
        _ => ("internal", ApiError::internal("failed to switch space")),
    };
    log_facade_failure(
        "space_setup",
        "switch_space",
        variant,
        api.status,
        &api.message,
    );
    api
}

// ---------------------------------------------------------------------------
// GET /v2/setup/migration-progress
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/v2/setup/migration-progress",
    tag = "setup-v2",
    operation_id = "setupV2GetMigrationProgress",
    responses(
        (status = 200, description = "Migration progress snapshot", body = SetupMigrationProgressEnvelope),
        (status = 503, description = "Facade not assembled", body = ApiErrorResponse),
        (status = 500, description = "Storage failure", body = ApiErrorResponse),
    ),
)]
pub(crate) async fn query_migration_progress(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<MigrationProgressResponse>>, ApiError> {
    let facade = require_facade(&state)?;
    let progress = facade
        .query_migration_progress()
        .await
        .map_err(map_query_migration_progress_err)?;
    Ok(Json(ApiEnvelope::now(progress.into_api_dto())))
}

fn map_query_migration_progress_err(err: QueryMigrationProgressError) -> ApiError {
    use QueryMigrationProgressError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::StorageFailed(msg) => ("storage_failed", ApiError::internal(msg)),
        E::Internal(msg) => ("internal", ApiError::internal(msg)),
    };
    log_facade_failure(
        "space_setup",
        "query_migration_progress",
        variant,
        api.status,
        &api.message,
    );
    api
}

#[cfg(test)]
mod tests {
    //! Handler-internal pure-function tests. End-to-end router /
    //! facade integration is covered downstream once T3.3 wires
    //! `SpaceSetupFacade` into the daemon assembly: building a real
    //! `DaemonApiState` requires the full bootstrap path, which is
    //! out of scope for T3.2.

    use super::*;

    #[test]
    fn map_create_engine_err_branches() {
        let err = map_create_engine_err(EngineError::new(
            CREATE_SPACE_PASSPHRASE_MISMATCH_CODE,
            EngineErrorCategory::InvalidInput,
            false,
        ));
        assert_eq!(err.status.as_u16(), 400);
        let err = map_create_engine_err(EngineError::new(
            CREATE_SPACE_ALREADY_INITIALIZED_CODE,
            EngineErrorCategory::Conflict,
            false,
        ));
        assert_eq!(err.status.as_u16(), 409);
        let err = map_create_engine_err(EngineError::new(
            CREATE_SPACE_ALREADY_SETUP_CODE,
            EngineErrorCategory::Conflict,
            false,
        ));
        assert_eq!(err.status.as_u16(), 409);
        let err = map_create_engine_err(EngineError::new(
            CREATE_SPACE_DEVICE_NAME_REQUIRED_CODE,
            EngineErrorCategory::InvalidInput,
            false,
        ));
        assert_eq!(err.status.as_u16(), 400);
        let err =
            map_create_engine_err(EngineError::new(1299, EngineErrorCategory::Internal, false));
        assert_eq!(err.status.as_u16(), 500);
        let err = map_create_engine_err(EngineError::new(
            1299,
            EngineErrorCategory::Unavailable,
            true,
        ));
        assert_eq!(err.status.as_u16(), 503);
    }

    #[test]
    fn map_join_engine_err_branches() {
        let cases = [
            (
                JOIN_SPACE_INVITATION_NOT_FOUND_CODE,
                EngineErrorCategory::NotFound,
                404,
            ),
            (
                JOIN_SPACE_INVITATION_EXPIRED_CODE,
                EngineErrorCategory::NotFound,
                404,
            ),
            (
                JOIN_SPACE_PASSPHRASE_MISMATCH_CODE,
                EngineErrorCategory::Unauthorized,
                400,
            ),
            (
                JOIN_SPACE_SPONSOR_REJECTED_CODE,
                EngineErrorCategory::Conflict,
                409,
            ),
            (
                JOIN_SPACE_SPONSOR_UNREACHABLE_CODE,
                EngineErrorCategory::Unavailable,
                503,
            ),
            (1299, EngineErrorCategory::Internal, 500),
        ];
        for (code, category, expected) in cases {
            assert_eq!(
                map_join_engine_err(EngineError::new(code, category, false))
                    .status
                    .as_u16(),
                expected
            );
        }
    }

    #[test]
    fn map_switch_engine_err_branches() {
        let cases = [
            (
                JOIN_SPACE_NOT_SETUP_CODE,
                EngineErrorCategory::InvalidState,
                409,
            ),
            (
                JOIN_SPACE_PENDING_MIGRATION_CODE,
                EngineErrorCategory::Conflict,
                409,
            ),
            (
                JOIN_SPACE_NOT_UNLOCKED_CODE,
                EngineErrorCategory::InvalidState,
                409,
            ),
            (
                JOIN_SPACE_SPONSOR_REJECTED_CODE,
                EngineErrorCategory::Conflict,
                409,
            ),
            (
                JOIN_SPACE_INVITATION_NOT_FOUND_CODE,
                EngineErrorCategory::NotFound,
                404,
            ),
            (
                JOIN_SPACE_PASSPHRASE_MISMATCH_CODE,
                EngineErrorCategory::Unauthorized,
                400,
            ),
            (
                JOIN_SPACE_DEVICE_NAME_REQUIRED_CODE,
                EngineErrorCategory::InvalidInput,
                400,
            ),
            (
                JOIN_SPACE_SERVICE_UNAVAILABLE_CODE,
                EngineErrorCategory::Unavailable,
                503,
            ),
            (
                JOIN_SPACE_TIMEOUT_CODE,
                EngineErrorCategory::DeadlineExceeded,
                503,
            ),
            (
                JOIN_SPACE_INVALID_CIPHERTEXT_CODE,
                EngineErrorCategory::Internal,
                500,
            ),
            (JOIN_SPACE_STORAGE_CODE, EngineErrorCategory::Internal, 500),
            (1299, EngineErrorCategory::Internal, 500),
        ];
        for (code, category, expected) in cases {
            assert_eq!(
                map_switch_engine_err(EngineError::new(code, category, false))
                    .status
                    .as_u16(),
                expected
            );
        }
    }
}

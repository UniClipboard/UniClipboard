//! Stateless v2 setup pairing HTTP handlers (Slice4 Phase 3 T3.2).
//!
//! Six endpoints under `/v2/setup/*`, each a thin adapter that
//! translates a `SpaceSetupFacade` call into the wire DTOs declared
//! in `uc_daemon_contract::api::dto::v2::setup`. Errors map onto the
//! daemon-wide `ApiError` surface (400 / 409 / 500 / 503).

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use uc_application::facade::space_setup::{
    QueryMigrationProgressError, SwitchSpaceError, SwitchSpaceInput,
};
use uc_application::facade::{
    CancelInvitationError, InitializeSpaceError, QuerySetupStateError,
    RedeemPairingInvitationError, ResetSpaceError, SpaceSetupFacade,
};
use uc_application::facade::{InitializeSpaceInput, RedeemPairingInvitationInput};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::dto::v2::setup::{
    InitializeSpaceRequest, InitializeSpaceResponse, IssueInvitationResponse,
    MigrationProgressResponse, RedeemRequest, RedeemResponse, SetupStateResponse,
    SwitchSpaceRequest, SwitchSpaceResponse,
};
use uc_daemon_contract::constants::http_route_v2;

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
    let facade = require_facade(&state)?;
    let input = InitializeSpaceInput {
        passphrase: req.passphrase,
        passphrase_confirm: req.passphrase_confirm,
        device_name: req.device_name,
    };
    let out = facade.initialize_space(input).await.map_err(map_init_err)?;
    Ok(Json(ApiEnvelope::now(out.into_api_dto())))
}

fn map_init_err(err: InitializeSpaceError) -> ApiError {
    use InitializeSpaceError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::PassphraseMismatch => (
            "passphrase_mismatch",
            ApiError::bad_request("passphrase and confirmation do not match"),
        ),
        E::DeviceNameRequired => (
            "device_name_required",
            ApiError::bad_request("device name is required"),
        ),
        E::AlreadyInitialized => (
            "already_initialized",
            ApiError::conflict("space is already initialised"),
        ),
        E::AlreadySetup => (
            "already_setup",
            ApiError::conflict("setup has already been completed on this device"),
        ),
        E::StorageFailed(msg) => ("storage_failed", ApiError::internal(msg)),
        E::Internal(msg) => ("internal", ApiError::internal(msg)),
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
    let facade = require_facade(&state)?;
    let out = facade
        .issue_pairing_invitation()
        .await
        .map_err(map_issue_err)?;
    Ok(Json(ApiEnvelope::now(out.into_api_dto())))
}

fn map_issue_err(err: uc_application::facade::IssuePairingInvitationError) -> ApiError {
    use uc_application::facade::IssuePairingInvitationError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::NetworkNotStarted => (
            "network_not_started",
            ApiError::service_unavailable("network is not started"),
        ),
        E::ServiceUnavailable => (
            "service_unavailable",
            ApiError::service_unavailable("pairing invitation service unavailable"),
        ),
        // `AddressNotAvailable` is only emitted by the dev-only
        // `issue_pairing_invitation_for_address` path. The webserver
        // never calls that path; collapse to Internal so a future
        // regression surfaces in logs instead of being silently
        // mapped to a misleading 400.
        E::AddressNotAvailable(ip) => (
            "address_not_available",
            ApiError::internal(format!(
                "unexpected AddressNotAvailable({ip}) on default path"
            )),
        ),
        E::Internal(msg) => ("internal", ApiError::internal(msg)),
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
    let facade = require_facade(&state)?;
    let input = RedeemPairingInvitationInput {
        code: req.code,
        passphrase: req.passphrase,
    };
    let out = facade
        .redeem_pairing_invitation(input)
        .await
        .map_err(map_redeem_err)?;
    Ok(Json(ApiEnvelope::now(out.into_api_dto())))
}

fn map_redeem_err(err: RedeemPairingInvitationError) -> ApiError {
    use RedeemPairingInvitationError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::InvitationNotFound => (
            "invitation_not_found",
            ApiError::not_found("invitation not found"),
        ),
        E::InvitationExpired => (
            "invitation_expired",
            ApiError::not_found("invitation has expired"),
        ),
        E::SponsorUnreachable => (
            "sponsor_unreachable",
            ApiError::service_unavailable("sponsor is not reachable"),
        ),
        E::ServiceUnavailable => (
            "service_unavailable",
            ApiError::service_unavailable("pairing invitation service unavailable"),
        ),
        E::PassphraseMismatch => (
            "passphrase_mismatch",
            ApiError::bad_request("wrong passphrase"),
        ),
        E::CorruptedKeyMaterial => (
            "corrupted_key_material",
            ApiError::internal("space key material corrupted"),
        ),
        E::DeviceNameRequired => (
            "device_name_required",
            ApiError::bad_request("device name is required"),
        ),
        E::SponsorRejectedInvitation => (
            "sponsor_rejected_invitation",
            ApiError::conflict("sponsor did not recognise the invitation code"),
        ),
        E::SponsorDeclined => (
            "sponsor_declined",
            ApiError::conflict("sponsor declined the pairing request"),
        ),
        E::SponsorTimedOut => (
            "sponsor_timed_out",
            ApiError::service_unavailable("sponsor timed out the handshake"),
        ),
        E::SponsorInternal(msg) => (
            "sponsor_internal",
            ApiError::internal(format!("sponsor internal error: {msg}")),
        ),
        E::Timeout => (
            "timeout",
            ApiError::service_unavailable("pairing handshake timed out"),
        ),
        E::ConnectionLost => (
            "connection_lost",
            ApiError::service_unavailable("connection lost mid-handshake"),
        ),
        E::Internal(msg) => ("internal", ApiError::internal(msg)),
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
    let facade = require_facade(&state)?;
    let input = SwitchSpaceInput {
        code: req.code,
        new_passphrase: req.new_passphrase,
    };
    let out = facade
        .switch_space(input)
        .await
        .map_err(map_switch_space_err)?;
    Ok(Json(ApiEnvelope::now(out.into_api_dto())))
}

fn map_switch_space_err(err: SwitchSpaceError) -> ApiError {
    use SwitchSpaceError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::NotSetup => (
            "not_setup",
            ApiError::conflict(
                "this device has not completed first-time setup yet; use /v2/setup/initialize \
                 or /v2/setup/redeem first",
            ),
        ),
        E::PendingMigration(_) => (
            "pending_migration",
            ApiError::conflict(
                "a previous switch-space migration is still in flight; restart the daemon to \
                 auto-resume, or call /v2/setup/reset to abandon",
            ),
        ),
        E::NotUnlocked => (
            "not_unlocked",
            ApiError::conflict(
                "space session is locked; unlock the current space before switching",
            ),
        ),
        E::InvitationNotFound => (
            "invitation_not_found",
            ApiError::not_found("invitation not found"),
        ),
        E::InvitationExpired => (
            "invitation_expired",
            ApiError::not_found("invitation has expired"),
        ),
        E::SponsorUnreachable => (
            "sponsor_unreachable",
            ApiError::service_unavailable("sponsor is not reachable"),
        ),
        E::ServiceUnavailable => (
            "service_unavailable",
            ApiError::service_unavailable("pairing invitation service unavailable"),
        ),
        E::PassphraseMismatch => (
            "passphrase_mismatch",
            ApiError::bad_request("wrong passphrase"),
        ),
        E::CorruptedKeyMaterial => (
            "corrupted_key_material",
            ApiError::internal("space key material corrupted"),
        ),
        E::DeviceNameRequired => (
            "device_name_required",
            ApiError::bad_request("device name is required"),
        ),
        E::SponsorRejectedInvitation => (
            "sponsor_rejected_invitation",
            ApiError::conflict("sponsor did not recognise the invitation code"),
        ),
        E::SponsorDeclined => (
            "sponsor_declined",
            ApiError::conflict("sponsor declined the pairing request"),
        ),
        E::Timeout => (
            "timeout",
            ApiError::service_unavailable("handshake timed out"),
        ),
        E::ConnectionLost => (
            "connection_lost",
            ApiError::service_unavailable("connection lost mid-handshake"),
        ),
        E::InvalidCiphertext => (
            "invalid_ciphertext",
            ApiError::internal("backup record decryption failed (corrupted ciphertext)"),
        ),
        E::Storage(msg) => (
            "storage",
            ApiError::internal(format!("storage failure: {msg}")),
        ),
        E::Internal(msg) => ("internal", ApiError::internal(msg)),
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
    fn map_init_err_branches() {
        let err = map_init_err(InitializeSpaceError::PassphraseMismatch);
        assert_eq!(err.status.as_u16(), 400);
        let err = map_init_err(InitializeSpaceError::AlreadyInitialized);
        assert_eq!(err.status.as_u16(), 409);
        let err = map_init_err(InitializeSpaceError::AlreadySetup);
        assert_eq!(err.status.as_u16(), 409);
        let err = map_init_err(InitializeSpaceError::DeviceNameRequired);
        assert_eq!(err.status.as_u16(), 400);
        let err = map_init_err(InitializeSpaceError::StorageFailed("disk full".into()));
        assert_eq!(err.status.as_u16(), 500);
    }

    #[test]
    fn map_redeem_err_branches() {
        assert_eq!(
            map_redeem_err(RedeemPairingInvitationError::InvitationNotFound)
                .status
                .as_u16(),
            404
        );
        assert_eq!(
            map_redeem_err(RedeemPairingInvitationError::InvitationExpired)
                .status
                .as_u16(),
            404
        );
        assert_eq!(
            map_redeem_err(RedeemPairingInvitationError::PassphraseMismatch)
                .status
                .as_u16(),
            400
        );
        assert_eq!(
            map_redeem_err(RedeemPairingInvitationError::SponsorRejectedInvitation)
                .status
                .as_u16(),
            409
        );
        assert_eq!(
            map_redeem_err(RedeemPairingInvitationError::SponsorUnreachable)
                .status
                .as_u16(),
            503
        );
        assert_eq!(
            map_redeem_err(RedeemPairingInvitationError::Internal("boom".into()))
                .status
                .as_u16(),
            500
        );
    }

    #[test]
    fn map_switch_space_err_branches() {
        // Conflict (409) — pre-flight + state-conflict variants.
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::NotSetup)
                .status
                .as_u16(),
            409
        );
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::PendingMigration(
                uc_core::setup::MigrationPhase::Prepared {
                    run_id: uc_core::setup::MigrationRunId::new("r")
                }
            ))
            .status
            .as_u16(),
            409
        );
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::NotUnlocked)
                .status
                .as_u16(),
            409
        );
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::SponsorRejectedInvitation)
                .status
                .as_u16(),
            409
        );
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::SponsorDeclined)
                .status
                .as_u16(),
            409
        );
        // 404 — invitation-shape errors.
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::InvitationNotFound)
                .status
                .as_u16(),
            404
        );
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::InvitationExpired)
                .status
                .as_u16(),
            404
        );
        // 400 — client-fixable input.
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::PassphraseMismatch)
                .status
                .as_u16(),
            400
        );
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::DeviceNameRequired)
                .status
                .as_u16(),
            400
        );
        // 503 — transient / retry-friendly.
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::SponsorUnreachable)
                .status
                .as_u16(),
            503
        );
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::ServiceUnavailable)
                .status
                .as_u16(),
            503
        );
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::Timeout)
                .status
                .as_u16(),
            503
        );
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::ConnectionLost)
                .status
                .as_u16(),
            503
        );
        // 500 — corrupted state / internal.
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::CorruptedKeyMaterial)
                .status
                .as_u16(),
            500
        );
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::InvalidCiphertext)
                .status
                .as_u16(),
            500
        );
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::Storage("boom".into()))
                .status
                .as_u16(),
            500
        );
        assert_eq!(
            map_switch_space_err(SwitchSpaceError::Internal("boom".into()))
                .status
                .as_u16(),
            500
        );
    }
}

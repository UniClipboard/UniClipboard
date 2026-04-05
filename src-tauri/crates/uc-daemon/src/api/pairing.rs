//! HTTP route handlers for pairing endpoints.
//!
//! Provides pairing lifecycle management: initiate, accept, reject, cancel, verify,
//! unpair, GUI lease, discoverability, and participant state.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use utoipa;

use uc_app::usecases::CoreUseCases;
use uc_core::network::daemon_api_strings::pairing_stage;

use crate::api::dto::error::ApiError;
use crate::api::dto::pairing::{
    AckedPairingCommandResponse, InitiatePairingRequest, InitiatePairingResponse,
    PairingGuiLeaseRequest, PairingSessionCommandRequest, PairingSessionSummaryDto,
    SetPairingDiscoverabilityRequest, SetPairingParticipantRequest, UnpairDeviceRequest,
    VerifyPairingRequest,
};
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/pairing/initiate", post(handle_initiate_pairing))
        .route("/pairing/accept", post(handle_accept_pairing))
        .route("/pairing/reject", post(handle_reject_pairing))
        .route("/pairing/cancel", post(handle_cancel_pairing))
        .route("/pairing/unpair", post(handle_unpair_device))
        .route("/pairing/gui/lease", post(handle_pairing_gui_lease))
        .route(
            "/pairing/discoverability/current",
            put(set_pairing_discoverability),
        )
        .route(
            "/pairing/participants/current",
            put(set_pairing_participant),
        )
        .route("/pairing/sessions", post(initiate_pairing))
        .route("/pairing/sessions/:session_id", get(pairing_session))
        .route("/pairing/sessions/:session_id/accept", post(accept_pairing))
        .route("/pairing/sessions/:session_id/reject", post(reject_pairing))
        .route("/pairing/sessions/:session_id/cancel", post(cancel_pairing))
        .route("/pairing/sessions/:session_id/verify", post(verify_pairing))
}

// ---------------------------------------------------------------------------
// Query / session status
// ---------------------------------------------------------------------------

/// GET /pairing/sessions/{session_id}
/// Returns the current state of a pairing session.
#[utoipa::path(
    get,
    path = "/pairing/sessions/{session_id}",
    tag = "pairing",
    params(
        ("session_id" = String, Path, description = "Pairing session ID")
    ),
    responses(
        (status = 200, description = "Session found"),
        (status = 404, description = "Session not found"),
        (status = 500, description = "Internal error"),
    )
)]
async fn pairing_session(
    State(state): State<DaemonApiState>,
    Path(session_id): Path<String>,
) -> Result<Json<PairingSessionSummaryDto>, ApiError> {
    match state.query_service.pairing_session(&session_id).await {
        Ok(Some(response)) => Ok(Json(response)),
        Ok(None) => Err(ApiError::not_found(format!(
            "pairing session not found: {session_id}"
        ))),
        Err(error) => {
            tracing::error!(error = %error, session_id = %session_id, "pairing_session query failed");
            Err(ApiError::internal(error.to_string()))
        }
    }
}

// ---------------------------------------------------------------------------
// Session-level command handlers (with :session_id path param)
// ---------------------------------------------------------------------------

/// POST /pairing/sessions/{session_id}/accept
#[utoipa::path(
    post,
    path = "/pairing/sessions/{session_id}/accept",
    tag = "pairing",
    params(
        ("session_id" = String, Path, description = "Pairing session ID")
    ),
    responses(
        (status = 202, description = "Accepted", body = AckedPairingCommandResponse),
        (status = 400, description = "Bad request"),
        (status = 404, description = "Session not found"),
        (status = 409, description = "Conflict"),
        (status = 500, description = "Internal error"),
    )
)]
async fn accept_pairing(
    State(state): State<DaemonApiState>,
    Path(session_id): Path<String>,
) -> Result<(StatusCode, Json<AckedPairingCommandResponse>), ApiError> {
    let pairing_host = state
        .pairing_host()
        .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;

    pairing_host
        .accept_pairing(&session_id)
        .await
        .map_err(ApiError::from_pairing_error)?;

    Ok((
        StatusCode::ACCEPTED,
        Json(AckedPairingCommandResponse {
            session_id,
            accepted: true,
            state: pairing_stage::VERIFYING.to_string(),
            error: None,
        }),
    ))
}

/// POST /pairing/sessions/{session_id}/reject
#[utoipa::path(
    post,
    path = "/pairing/sessions/{session_id}/reject",
    tag = "pairing",
    params(
        ("session_id" = String, Path, description = "Pairing session ID")
    ),
    responses(
        (status = 202, description = "Rejected", body = AckedPairingCommandResponse),
        (status = 400, description = "Bad request"),
        (status = 404, description = "Session not found"),
        (status = 409, description = "Conflict"),
        (status = 500, description = "Internal error"),
    )
)]
async fn reject_pairing(
    State(state): State<DaemonApiState>,
    Path(session_id): Path<String>,
) -> Result<(StatusCode, Json<AckedPairingCommandResponse>), ApiError> {
    let pairing_host = state
        .pairing_host()
        .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;

    pairing_host
        .reject_pairing(&session_id)
        .await
        .map_err(ApiError::from_pairing_error)?;

    Ok((
        StatusCode::ACCEPTED,
        Json(AckedPairingCommandResponse {
            session_id,
            accepted: true,
            state: pairing_stage::FAILED.to_string(),
            error: None,
        }),
    ))
}

/// POST /pairing/sessions/{session_id}/cancel
#[utoipa::path(
    post,
    path = "/pairing/sessions/{session_id}/cancel",
    tag = "pairing",
    params(
        ("session_id" = String, Path, description = "Pairing session ID")
    ),
    responses(
        (status = 202, description = "Cancelled", body = AckedPairingCommandResponse),
        (status = 400, description = "Bad request"),
        (status = 404, description = "Session not found"),
        (status = 409, description = "Conflict"),
        (status = 500, description = "Internal error"),
    )
)]
async fn cancel_pairing(
    State(state): State<DaemonApiState>,
    Path(session_id): Path<String>,
) -> Result<(StatusCode, Json<AckedPairingCommandResponse>), ApiError> {
    let pairing_host = state
        .pairing_host()
        .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;

    pairing_host
        .cancel_pairing(&session_id)
        .await
        .map_err(ApiError::from_pairing_error)?;

    Ok((
        StatusCode::ACCEPTED,
        Json(AckedPairingCommandResponse {
            session_id,
            accepted: true,
            state: pairing_stage::FAILED.to_string(),
            error: None,
        }),
    ))
}

/// POST /pairing/sessions/{session_id}/verify
#[utoipa::path(
    post,
    path = "/pairing/sessions/{session_id}/verify",
    tag = "pairing",
    params(
        ("session_id" = String, Path, description = "Pairing session ID")
    ),
    request_body = VerifyPairingRequest,
    responses(
        (status = 202, description = "Verification result", body = AckedPairingCommandResponse),
        (status = 400, description = "Bad request"),
        (status = 404, description = "Session not found"),
        (status = 409, description = "Conflict"),
        (status = 500, description = "Internal error"),
    )
)]
async fn verify_pairing(
    State(state): State<DaemonApiState>,
    Path(session_id): Path<String>,
    Json(payload): Json<VerifyPairingRequest>,
) -> Result<(StatusCode, Json<AckedPairingCommandResponse>), ApiError> {
    let pairing_host = state
        .pairing_host()
        .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;

    pairing_host
        .verify_pairing(&session_id, payload.pin_matches)
        .await
        .map_err(ApiError::from_pairing_error)?;

    let state_str = if payload.pin_matches {
        pairing_stage::VERIFYING
    } else {
        pairing_stage::FAILED
    };

    Ok((
        StatusCode::ACCEPTED,
        Json(AckedPairingCommandResponse {
            session_id,
            accepted: true,
            state: state_str.to_string(),
            error: None,
        }),
    ))
}

// ---------------------------------------------------------------------------
// Body-based command handlers
// ---------------------------------------------------------------------------

/// POST /pairing/sessions
#[utoipa::path(
    post,
    path = "/pairing/sessions",
    tag = "pairing",
    request_body = InitiatePairingRequest,
    responses(
        (status = 202, description = "Initiated", body = AckedPairingCommandResponse),
        (status = 400, description = "Bad request"),
        (status = 409, description = "Conflict"),
        (status = 503, description = "Pairing host unavailable"),
        (status = 500, description = "Internal error"),
    )
)]
async fn initiate_pairing(
    State(state): State<DaemonApiState>,
    Json(payload): Json<InitiatePairingRequest>,
) -> Result<(StatusCode, Json<AckedPairingCommandResponse>), ApiError> {
    let pairing_host = state
        .pairing_host()
        .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;

    let session_id = pairing_host
        .initiate_pairing(payload.peer_id)
        .await
        .map_err(ApiError::from_pairing_error)?;

    Ok((
        StatusCode::ACCEPTED,
        Json(AckedPairingCommandResponse {
            session_id,
            accepted: true,
            state: pairing_stage::REQUEST.to_string(),
            error: None,
        }),
    ))
}

/// POST /pairing/initiate
#[utoipa::path(
    post,
    path = "/pairing/initiate",
    tag = "pairing",
    request_body = InitiatePairingRequest,
    responses(
        (status = 200, body = InitiatePairingResponse),
        (status = 400, description = "Bad request"),
        (status = 409, description = "Conflict"),
        (status = 503, description = "Pairing host unavailable"),
        (status = 500, description = "Internal error"),
    )
)]
async fn handle_initiate_pairing(
    State(state): State<DaemonApiState>,
    Json(payload): Json<InitiatePairingRequest>,
) -> Result<Json<InitiatePairingResponse>, ApiError> {
    let pairing_host = state
        .pairing_host()
        .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;

    let session_id = pairing_host
        .initiate_pairing(payload.peer_id)
        .await
        .map_err(ApiError::from_pairing_error)?;

    Ok(Json(InitiatePairingResponse { session_id }))
}

/// POST /pairing/accept
#[utoipa::path(
    post,
    path = "/pairing/accept",
    tag = "pairing",
    request_body = PairingSessionCommandRequest,
    responses(
        (status = 204, description = "Accepted"),
        (status = 400, description = "Bad request"),
        (status = 404, description = "Session not found"),
        (status = 409, description = "Conflict"),
        (status = 503, description = "Pairing host unavailable"),
        (status = 500, description = "Internal error"),
    )
)]
async fn handle_accept_pairing(
    State(state): State<DaemonApiState>,
    Json(payload): Json<PairingSessionCommandRequest>,
) -> Result<StatusCode, ApiError> {
    let pairing_host = state
        .pairing_host()
        .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;

    pairing_host
        .accept_pairing(&payload.session_id)
        .await
        .map_err(ApiError::from_pairing_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// POST /pairing/reject
#[utoipa::path(
    post,
    path = "/pairing/reject",
    tag = "pairing",
    request_body = PairingSessionCommandRequest,
    responses(
        (status = 204, description = "Rejected"),
        (status = 400, description = "Bad request"),
        (status = 404, description = "Session not found"),
        (status = 409, description = "Conflict"),
        (status = 503, description = "Pairing host unavailable"),
        (status = 500, description = "Internal error"),
    )
)]
async fn handle_reject_pairing(
    State(state): State<DaemonApiState>,
    Json(payload): Json<PairingSessionCommandRequest>,
) -> Result<StatusCode, ApiError> {
    let pairing_host = state
        .pairing_host()
        .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;

    pairing_host
        .reject_pairing(&payload.session_id)
        .await
        .map_err(ApiError::from_pairing_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// POST /pairing/cancel
#[utoipa::path(
    post,
    path = "/pairing/cancel",
    tag = "pairing",
    request_body = PairingSessionCommandRequest,
    responses(
        (status = 204, description = "Cancelled"),
        (status = 400, description = "Bad request"),
        (status = 404, description = "Session not found"),
        (status = 409, description = "Conflict"),
        (status = 503, description = "Pairing host unavailable"),
        (status = 500, description = "Internal error"),
    )
)]
async fn handle_cancel_pairing(
    State(state): State<DaemonApiState>,
    Json(payload): Json<PairingSessionCommandRequest>,
) -> Result<StatusCode, ApiError> {
    let pairing_host = state
        .pairing_host()
        .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;

    pairing_host
        .cancel_pairing(&payload.session_id)
        .await
        .map_err(ApiError::from_pairing_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// POST /pairing/unpair
#[utoipa::path(
    post,
    path = "/pairing/unpair",
    tag = "pairing",
    request_body = UnpairDeviceRequest,
    responses(
        (status = 204, description = "Device unpaired"),
        (status = 400, description = "Bad request"),
        (status = 503, description = "Runtime unavailable"),
        (status = 500, description = "Internal error"),
    )
)]
async fn handle_unpair_device(
    State(state): State<DaemonApiState>,
    Json(payload): Json<UnpairDeviceRequest>,
) -> Result<StatusCode, ApiError> {
    let runtime = state.runtime_or_error()?;
    let usecases = CoreUseCases::new(runtime.as_ref());

    usecases
        .unpair_device()
        .execute(payload.peer_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon unpair failed");
            ApiError::internal(e.to_string())
        })?;

    Ok(StatusCode::NO_CONTENT)
}

/// POST /pairing/gui/lease
#[utoipa::path(
    post,
    path = "/pairing/gui/lease",
    tag = "pairing",
    request_body = PairingGuiLeaseRequest,
    responses(
        (status = 204, description = "Lease updated"),
        (status = 400, description = "Bad request"),
        (status = 503, description = "Pairing host unavailable"),
        (status = 500, description = "Internal error"),
    )
)]
async fn handle_pairing_gui_lease(
    State(state): State<DaemonApiState>,
    Json(payload): Json<PairingGuiLeaseRequest>,
) -> Result<StatusCode, ApiError> {
    let pairing_host = state
        .pairing_host()
        .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;

    pairing_host
        .register_gui_participant(payload.enabled, payload.lease_ttl_ms)
        .await
        .map_err(ApiError::from_pairing_error)?;

    Ok(StatusCode::NO_CONTENT)
}

/// PUT /pairing/discoverability/current
#[utoipa::path(
    put,
    path = "/pairing/discoverability/current",
    tag = "pairing",
    request_body = SetPairingDiscoverabilityRequest,
    responses(
        (status = 202, description = "Discoverability updated", body = AckedPairingCommandResponse),
        (status = 400, description = "Bad request"),
        (status = 503, description = "Pairing host unavailable"),
    )
)]
async fn set_pairing_discoverability(
    State(state): State<DaemonApiState>,
    Json(payload): Json<SetPairingDiscoverabilityRequest>,
) -> Result<(StatusCode, Json<AckedPairingCommandResponse>), ApiError> {
    let pairing_host = state
        .pairing_host()
        .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;

    pairing_host
        .set_discoverability(
            payload.client_kind,
            payload.discoverable,
            payload.lease_ttl_ms,
        )
        .await;

    Ok((
        StatusCode::ACCEPTED,
        Json(AckedPairingCommandResponse {
            session_id: "current".to_string(),
            accepted: payload.discoverable,
            state: "discoverability_updated".to_string(),
            error: None,
        }),
    ))
}

/// PUT /pairing/participants/current
#[utoipa::path(
    put,
    path = "/pairing/participants/current",
    tag = "pairing",
    request_body = SetPairingParticipantRequest,
    responses(
        (status = 202, description = "Participant updated", body = AckedPairingCommandResponse),
        (status = 400, description = "Bad request"),
        (status = 503, description = "Pairing host unavailable"),
    )
)]
async fn set_pairing_participant(
    State(state): State<DaemonApiState>,
    Json(payload): Json<SetPairingParticipantRequest>,
) -> Result<(StatusCode, Json<AckedPairingCommandResponse>), ApiError> {
    let pairing_host = state
        .pairing_host()
        .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;

    pairing_host
        .set_participant_ready(payload.client_kind, payload.ready, payload.lease_ttl_ms)
        .await;

    Ok((
        StatusCode::ACCEPTED,
        Json(AckedPairingCommandResponse {
            session_id: "current".to_string(),
            accepted: payload.ready,
            state: "participant_updated".to_string(),
            error: None,
        }),
    ))
}

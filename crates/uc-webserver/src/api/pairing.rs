//! HTTP route handlers for pairing endpoints.
//!
//! Only local member removal remains after the retired pairing protocol. The
//! engine owns the member, trust, and peer-address cleanup sequence.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use utoipa;

use uc_engine::{Operation, OperationResult, RemoveMemberInput};

use crate::api::dto::error::ApiError;
use crate::api::dto::member::DeviceTrustSnapshotDto;
use crate::api::dto::pairing::UnpairDeviceRequest;
use crate::api::member::map_member_engine_error;
use crate::api::projection::IntoApiDto;
use crate::api::server::DaemonApiState;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;

pub fn router() -> Router<DaemonApiState> {
    Router::new().route("/pairing/unpair", post(handle_unpair_device))
}

/// POST /pairing/unpair
///
/// Revokes the local member record for the given peer and returns the
/// current Engine-owned device relationship state. Errors flow through the shared `ApiError`
/// carrier and therefore serialize to `ApiErrorResponse { code, message,
/// details? }` on the wire.
#[utoipa::path(
    post,
    path = "/pairing/unpair",
    tag = "pairing",
    operation_id = "unpairDevice",
    request_body = UnpairDeviceRequest,
    responses(
        (status = 200, body = DeviceTrustEnvelope),
        (status = 404, description = "Member not found", body = ApiErrorResponse),
        (status = 503, description = "Runtime unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
pub(crate) async fn handle_unpair_device(
    State(state): State<DaemonApiState>,
    Json(payload): Json<UnpairDeviceRequest>,
) -> Result<Json<ApiEnvelope<DeviceTrustSnapshotDto>>, ApiError> {
    let peer_id = payload.peer_id;

    // Removing a peer records a local membership decision. Engine owns the
    // durable membership, trust, and peer-address cleanup sequence.
    let result = state
        .execute(Operation::RemoveMember(RemoveMemberInput {
            device_id: peer_id.to_string(),
        }))
        .await
        .map_err(|error| map_member_engine_error(peer_id.as_str(), "unpair_device", error))?;
    let OperationResult::DeviceTrust(device_trust) = result else {
        tracing::error!(
            operation = "unpair_device",
            error_kind = ?result,
            "engine returned an unexpected result"
        );
        return Err(ApiError::internal(
            "engine returned an unexpected device trust result",
        ));
    };

    Ok(Json(ApiEnvelope::now(device_trust.into_api_dto())))
}

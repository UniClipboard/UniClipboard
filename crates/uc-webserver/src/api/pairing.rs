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
use crate::api::dto::member::WorkspaceConvergenceDto;
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
/// Engine-owned workspace convergence state. Errors flow through the shared `ApiError`
/// carrier and therefore serialize to `ApiErrorResponse { code, message,
/// details? }` on the wire.
#[utoipa::path(
    post,
    path = "/pairing/unpair",
    tag = "pairing",
    operation_id = "unpairDevice",
    request_body = UnpairDeviceRequest,
    responses(
        (status = 200, body = WorkspaceConvergenceEnvelope),
        (status = 404, description = "Member not found", body = ApiErrorResponse),
        (status = 503, description = "Runtime unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
pub(crate) async fn handle_unpair_device(
    State(state): State<DaemonApiState>,
    Json(payload): Json<UnpairDeviceRequest>,
) -> Result<Json<ApiEnvelope<WorkspaceConvergenceDto>>, ApiError> {
    let peer_id = payload.peer_id;

    // Slice 4 P5a-1: 取消配对 = 删除本机成员记录。libp2p 时代的
    // `PairingTransportPort::unpair_device` 通知对端的能力随 libp2p 一同下线；
    // 本地自治模型下不再广播给对端。对端残留的 member/trust 记录不影响
    // 重新配对——admit/trust use case 在重配时显式替换旧记录（#1023）。
    let result = state
        .execute(Operation::RemoveMember(RemoveMemberInput {
            device_id: peer_id.to_string(),
        }))
        .await
        .map_err(|error| map_member_engine_error(peer_id.as_str(), "unpair_device", error))?;
    let OperationResult::WorkspaceConvergence(convergence) = result else {
        tracing::error!(
            operation = "unpair_device",
            error_kind = ?result,
            "engine returned an unexpected result"
        );
        return Err(ApiError::internal(
            "engine returned an unexpected workspace-convergence result",
        ));
    };

    Ok(Json(ApiEnvelope::now(convergence.into_api_dto())))
}

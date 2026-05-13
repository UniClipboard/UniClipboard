//! HTTP route handlers for pairing endpoints.
//!
//! Slice 4 P5a-4: 旧 pairing 协议（initiate/accept/reject/verify/sessions/...）
//! 随 libp2p 一起下线。本文件仅保留 `/pairing/unpair`，前端 DevicesPage 通过
//! `MemberRepositoryPort` 走本地撤销路径。

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use utoipa;

use uc_application::facade::RosterError;

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::dto::pairing::UnpairDeviceRequest;
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new().route("/pairing/unpair", post(handle_unpair_device))
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
pub(crate) async fn handle_unpair_device(
    State(state): State<DaemonApiState>,
    Json(payload): Json<UnpairDeviceRequest>,
) -> Result<StatusCode, ApiError> {
    let app = state.app_facade_or_error()?;
    let roster = app
        .member_roster
        .get()
        .cloned()
        .ok_or_else(|| ApiError::service_unavailable("member roster facade unavailable"))?;
    let peer_id = payload.peer_id;

    // Slice 4 P5a-1: 取消配对 = 删除本机成员记录。libp2p 时代的
    // `PairingTransportPort::unpair_device` 通知对端的能力随 libp2p 一同下线；
    // 本地自治模型下不再广播给对端（对端发现后会自行清理）。
    roster
        .revoke_member(peer_id.as_str())
        .await
        .map_err(|e| map_unpair_err(e, peer_id.as_str()))?;

    Ok(StatusCode::NO_CONTENT)
}

fn map_unpair_err(err: RosterError, peer_id: &str) -> ApiError {
    use RosterError as E;
    let (variant, api): (&'static str, ApiError) = match err {
        E::NotFound(_) => (
            "not_found",
            ApiError::not_found(format!("member `{peer_id}` not found")),
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
    log_facade_failure("roster", "unpair_device", variant, api.status, &api.message);
    api
}

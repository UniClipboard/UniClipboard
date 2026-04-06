//! HTTP route handlers for setup endpoints.
//!
//! Handles device setup flow: creating/joining a space, device selection,
//! passphrase submission, and setup reset.
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use uc_core::setup::SetupState;
use utoipa;

use crate::api::dto::error::ApiError;
use crate::api::dto::setup::{
    GetSetupStateResponse, SetupActionResponse, SetupResetResponse, SetupSelectPeerRequest,
    SetupStateResponseDto, SetupSubmitPassphraseRequest,
};
use crate::api::server::DaemonApiState;
use crate::pairing::host::DaemonPairingHostError;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/setup/state", get(get_setup_state))
        .route("/setup/new", post(start_new))
        .route("/setup/join", post(start_join))
        .route("/setup/select-peer", post(select_peer))
        .route("/setup/confirm-peer", post(confirm_peer))
        .route("/setup/submit-passphrase", post(submit_passphrase))
        .route("/setup/verify-passphrase", post(verify_passphrase))
        .route("/setup/cancel", post(cancel))
        .route("/setup/clear-transient", post(clear_transient))
        .route("/setup/complete-space-access", post(complete_space_access))
        .route("/setup/reset", post(reset))
}

/// GET /setup/state
/// Returns the current setup state.
#[utoipa::path(
    get,
    path = "/setup/state",
    tag = "setup",
    responses(
        (status = 200, body = GetSetupStateResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn get_setup_state(
    State(state): State<DaemonApiState>,
) -> Result<Json<GetSetupStateResponse>, ApiError> {
    let orchestrator = state.setup_orchestrator().ok_or_else(|| {
        tracing::error!("setup orchestrator unavailable");
        ApiError::internal("setup orchestrator unavailable")
    })?;

    let inner = state
        .query_service
        .setup_state(orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon setup API request failed");
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(GetSetupStateResponse {
        data: SetupStateResponseDto::from(inner),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// POST /setup/new
/// Initiates a new space creation flow.
#[utoipa::path(
    post,
    path = "/setup/new",
    tag = "setup",
    responses(
        (status = 200, body = SetupActionResponse),
        (status = 409, description = "Current setup state does not allow this action", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn start_new(
    State(state): State<DaemonApiState>,
) -> Result<Json<SetupActionResponse>, ApiError> {
    let orchestrator = state
        .setup_orchestrator()
        .ok_or_else(|| ApiError::internal("setup orchestrator unavailable"))?;

    if !matches!(orchestrator.get_state().await, SetupState::Welcome) {
        return Err(ApiError::conflict(
            "current setup state does not allow this action",
        ));
    }

    orchestrator.new_space().await.map_err(|e| {
        tracing::error!(error = %e, "setup host action failed");
        ApiError::internal(format!("setup action failed: {e}"))
    })?;

    let inner = state
        .query_service
        .setup_state(orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon setup API request failed");
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(SetupActionResponse {
        data: SetupStateResponseDto::from(inner),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// POST /setup/join
/// Initiates a space join flow.
#[utoipa::path(
    post,
    path = "/setup/join",
    tag = "setup",
    responses(
        (status = 200, body = SetupActionResponse),
        (status = 409, description = "Current setup state does not allow this action", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn start_join(
    State(state): State<DaemonApiState>,
) -> Result<Json<SetupActionResponse>, ApiError> {
    let orchestrator = state
        .setup_orchestrator()
        .ok_or_else(|| ApiError::internal("setup orchestrator unavailable"))?;

    if !matches!(orchestrator.get_state().await, SetupState::Welcome) {
        return Err(ApiError::conflict(
            "current setup state does not allow this action",
        ));
    }

    orchestrator.join_space().await.map_err(|e| {
        tracing::error!(error = %e, "setup join action failed");
        ApiError::internal(format!("setup action failed: {e}"))
    })?;

    let inner = state
        .query_service
        .setup_state(orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon setup API request failed");
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(SetupActionResponse {
        data: SetupStateResponseDto::from(inner),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// POST /setup/select-peer
/// Selects a peer device during the join flow.
#[utoipa::path(
    post,
    path = "/setup/select-peer",
    tag = "setup",
    request_body = SetupSelectPeerRequest,
    responses(
        (status = 200, body = SetupActionResponse),
        (status = 409, description = "Current setup state does not allow this action", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn select_peer(
    State(state): State<DaemonApiState>,
    Json(payload): Json<SetupSelectPeerRequest>,
) -> Result<Json<SetupActionResponse>, ApiError> {
    let orchestrator = state
        .setup_orchestrator()
        .ok_or_else(|| ApiError::internal("setup orchestrator unavailable"))?;

    if !matches!(
        orchestrator.get_state().await,
        SetupState::JoinSpaceSelectDevice { .. }
    ) {
        return Err(ApiError::conflict(
            "current setup state does not allow selecting a peer",
        ));
    }

    orchestrator
        .select_device(payload.peer_id)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "setup select peer failed");
            ApiError::internal(format!("setup select peer failed: {e}"))
        })?;

    let inner = state
        .query_service
        .setup_state(orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon setup API request failed");
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(SetupActionResponse {
        data: SetupStateResponseDto::from(inner),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// POST /setup/confirm-peer
/// Confirms trust for a peer device during the join flow.
#[utoipa::path(
    post,
    path = "/setup/confirm-peer",
    tag = "setup",
    responses(
        (status = 200, body = SetupActionResponse),
        (status = 409, description = "Current setup state does not allow this action", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn confirm_peer(
    State(state): State<DaemonApiState>,
) -> Result<Json<SetupActionResponse>, ApiError> {
    let orchestrator = state
        .setup_orchestrator()
        .ok_or_else(|| ApiError::internal("setup orchestrator unavailable"))?;

    let inner = state
        .query_service
        .setup_state(orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon setup API request failed");
            ApiError::internal(e.to_string())
        })?;

    let current_state = orchestrator.get_state().await;
    let hint = &inner.next_step_hint;

    let is_join_confirm = matches!(current_state, SetupState::JoinSpaceConfirmPeer { .. });
    let is_host_delegate =
        matches!(current_state, SetupState::Completed) && hint == "host-confirm-peer";

    if !is_join_confirm && !is_host_delegate {
        return Err(ApiError::conflict(
            "current setup state does not allow this action",
        ));
    }

    if is_host_delegate {
        let pairing_host = state
            .pairing_host()
            .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;
        let session_id = pairing_host
            .active_session_id()
            .await
            .ok_or_else(|| ApiError::conflict("no active pairing session to confirm"))?;

        pairing_host
            .accept_pairing(&session_id)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "pairing accept failed");
                ApiError::from(e)
            })?;
    } else {
        orchestrator.confirm_peer_trust().await.map_err(|e| {
            tracing::error!(error = %e, "setup confirm peer failed");
            ApiError::internal(format!("setup action failed: {e}"))
        })?;
    }

    let inner = state
        .query_service
        .setup_state(orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon setup API request failed");
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(SetupActionResponse {
        data: SetupStateResponseDto::from(inner),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// POST /setup/submit-passphrase
/// Submits the encryption passphrase during setup.
#[utoipa::path(
    post,
    path = "/setup/submit-passphrase",
    tag = "setup",
    request_body = SetupSubmitPassphraseRequest,
    responses(
        (status = 200, body = SetupActionResponse),
        (status = 409, description = "Current setup state does not allow this action", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn submit_passphrase(
    State(state): State<DaemonApiState>,
    Json(payload): Json<SetupSubmitPassphraseRequest>,
) -> Result<Json<SetupActionResponse>, ApiError> {
    let orchestrator = state
        .setup_orchestrator()
        .ok_or_else(|| ApiError::internal("setup orchestrator unavailable"))?;

    if !matches!(
        orchestrator.get_state().await,
        SetupState::CreateSpaceInputPassphrase { .. }
    ) {
        return Err(ApiError::conflict(
            "current setup state does not allow submitting a passphrase",
        ));
    }

    let result = orchestrator
        .submit_passphrase(payload.passphrase.clone(), payload.passphrase)
        .await;

    result.map_err(|e| {
        tracing::error!(error = %e, "setup submit passphrase failed");
        ApiError::internal(format!("setup submit passphrase failed: {e}"))
    })?;

    let inner = state
        .query_service
        .setup_state(orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon setup API request failed");
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(SetupActionResponse {
        data: SetupStateResponseDto::from(inner),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// POST /setup/verify-passphrase
/// Verifies the encryption passphrase during join setup.
#[utoipa::path(
    post,
    path = "/setup/verify-passphrase",
    tag = "setup",
    request_body = SetupSubmitPassphraseRequest,
    responses(
        (status = 200, body = SetupActionResponse),
        (status = 409, description = "Current setup state does not allow this action", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn verify_passphrase(
    State(state): State<DaemonApiState>,
    Json(payload): Json<SetupSubmitPassphraseRequest>,
) -> Result<Json<SetupActionResponse>, ApiError> {
    let orchestrator = state
        .setup_orchestrator()
        .ok_or_else(|| ApiError::internal("setup orchestrator unavailable"))?;

    if !matches!(
        orchestrator.get_state().await,
        SetupState::JoinSpaceInputPassphrase { .. }
    ) {
        return Err(ApiError::conflict(
            "current setup state does not allow verifying a passphrase",
        ));
    }

    orchestrator
        .verify_passphrase(payload.passphrase)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "setup verify passphrase failed");
            ApiError::internal(format!("setup verify passphrase failed: {e}"))
        })?;

    let inner = state
        .query_service
        .setup_state(orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon setup API request failed");
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(SetupActionResponse {
        data: SetupStateResponseDto::from(inner),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// POST /setup/complete-space-access
/// Called by the frontend when the daemon emits `setup.spaceAccessCompleted` via
/// the WebSocket bridge. Transitions the setup orchestrator to `Completed`.
///
/// For the sponsor (already Completed), returns the current state without
/// dispatching any transition.
#[utoipa::path(
    post,
    path = "/setup/complete-space-access",
    tag = "setup",
    responses(
        (status = 200, body = SetupActionResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn complete_space_access(
    State(state): State<DaemonApiState>,
) -> Result<Json<SetupActionResponse>, ApiError> {
    let orchestrator = state
        .setup_orchestrator()
        .ok_or_else(|| ApiError::internal("setup orchestrator unavailable"))?;

    // If setup is already completed (sponsor role), return current state
    // without dispatching any transition.
    let current_state = orchestrator.get_state().await;
    if !matches!(current_state, SetupState::Completed) {
        orchestrator.complete_join_space().await.map_err(|e| {
            tracing::warn!(error = %e, "complete_space_access: join space succeeded event not applicable in current state");
            ApiError::internal(format!("complete space access failed: {e}"))
        })?;
    }

    let inner = state
        .query_service
        .setup_state(orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon setup API request failed");
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(SetupActionResponse {
        data: SetupStateResponseDto::from(inner),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// POST /setup/cancel
/// Cancels the current setup flow.
#[utoipa::path(
    post,
    path = "/setup/cancel",
    tag = "setup",
    responses(
        (status = 200, body = SetupActionResponse),
        (status = 409, description = "No active pairing session to cancel", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn cancel(
    State(state): State<DaemonApiState>,
) -> Result<Json<SetupActionResponse>, ApiError> {
    let orchestrator = state
        .setup_orchestrator()
        .ok_or_else(|| ApiError::internal("setup orchestrator unavailable"))?;

    let inner = state
        .query_service
        .setup_state(orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon setup API request failed");
            ApiError::internal(e.to_string())
        })?;

    let current_state = orchestrator.get_state().await;
    let hint = &inner.next_step_hint;
    let is_host_delegate =
        matches!(current_state, SetupState::Completed) && hint == "host-confirm-peer";

    if is_host_delegate {
        let pairing_host = state
            .pairing_host()
            .ok_or_else(|| ApiError::service_unavailable("pairing host unavailable"))?;
        let session_id = pairing_host
            .active_session_id()
            .await
            .ok_or_else(|| ApiError::conflict("no active pairing session to cancel"))?;

        pairing_host
            .reject_pairing(&session_id)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "pairing reject failed");
                ApiError::from(e)
            })?;
    } else {
        orchestrator.cancel_setup().await.map_err(|e| {
            tracing::error!(error = %e, "setup cancel failed");
            ApiError::internal(format!("setup cancel failed: {e}"))
        })?;
    }

    let inner = state
        .query_service
        .setup_state(orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon setup API request failed");
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(SetupActionResponse {
        data: SetupStateResponseDto::from(inner),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// POST /setup/clear-transient
/// Clears the in-memory setup session and any active pairing session while
/// preserving whether the device has already completed setup.
#[utoipa::path(
    post,
    path = "/setup/clear-transient",
    tag = "setup",
    responses(
        (status = 200, body = SetupActionResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn clear_transient(
    State(state): State<DaemonApiState>,
) -> Result<Json<SetupActionResponse>, ApiError> {
    let orchestrator = state
        .setup_orchestrator()
        .ok_or_else(|| ApiError::internal("setup orchestrator unavailable"))?;

    if let Some(pairing_host) = state.pairing_host() {
        pairing_host.reset_setup_state().await;
    }

    orchestrator.clear_transient_state().await.map_err(|e| {
        tracing::error!(error = %e, "setup transient clear failed");
        ApiError::internal(format!("setup transient clear failed: {e}"))
    })?;

    let inner = state
        .query_service
        .setup_state(orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "daemon setup API request failed");
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(SetupActionResponse {
        data: SetupStateResponseDto::from(inner),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// POST /setup/reset
/// Resets the setup state, removing all paired devices and encryption keys.
#[utoipa::path(
    post,
    path = "/setup/reset",
    tag = "setup",
    responses(
        (status = 200, body = SetupResetResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn reset(State(state): State<DaemonApiState>) -> Result<Json<SetupResetResponse>, ApiError> {
    let orchestrator = state
        .setup_orchestrator()
        .ok_or_else(|| ApiError::internal("setup orchestrator unavailable"))?;
    let runtime = state
        .runtime
        .clone()
        .ok_or_else(|| ApiError::internal("daemon runtime unavailable"))?;

    if let Some(pairing_host) = state.pairing_host() {
        pairing_host.reset_setup_state().await;
    }

    orchestrator.reset().await.map_err(|e| {
        tracing::error!(error = %e, "setup reset failed");
        ApiError::internal(format!("setup reset failed: {e}"))
    })?;

    let deps = runtime.wiring_deps();

    for device in deps
        .device
        .paired_device_repo
        .list_all()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "setup reset failed");
            ApiError::internal(format!("setup reset failed: {e}"))
        })?
    {
        if let Err(e) = deps.device.paired_device_repo.delete(&device.peer_id).await {
            if !matches!(e, uc_core::ports::PairedDeviceRepositoryError::NotFound) {
                tracing::error!(error = %e, "setup reset failed");
                return Err(ApiError::internal(format!("setup reset failed: {e}")));
            }
        }
    }

    let scope = deps.security.key_scope.current_scope().await.map_err(|e| {
        tracing::error!(error = %e, "setup reset failed");
        ApiError::internal(format!("setup reset failed: {e}"))
    })?;

    if let Err(e) = deps.security.key_material.delete_keyslot(&scope).await {
        if !matches!(e, uc_core::security::model::EncryptionError::KeyNotFound) {
            tracing::error!(error = %e, "setup reset failed");
            return Err(ApiError::internal(format!("setup reset failed: {e}")));
        }
    }
    if let Err(e) = deps.security.key_material.delete_kek(&scope).await {
        if !matches!(e, uc_core::security::model::EncryptionError::KeyNotFound) {
            tracing::error!(error = %e, "setup reset failed");
            return Err(ApiError::internal(format!("setup reset failed: {e}")));
        }
    }
    deps.security
        .encryption_state
        .clear_initialized()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "setup reset failed");
            ApiError::internal(format!("setup reset failed: {e}"))
        })?;
    if let Err(e) = deps.security.encryption_session.clear().await {
        if !matches!(
            e,
            uc_core::security::model::EncryptionError::KeyNotFound
                | uc_core::security::model::EncryptionError::NotInitialized
        ) {
            tracing::error!(error = %e, "setup reset failed");
            return Err(ApiError::internal(format!("setup reset failed: {e}")));
        }
    }

    Ok(Json(SetupResetResponse {
        profile: std::env::var("UC_PROFILE")
            .ok()
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|| "default".to_string()),
        daemon_kept_running: true,
    }))
}

// ---------------------------------------------------------------------------
// Error conversions
// ---------------------------------------------------------------------------

impl From<DaemonPairingHostError> for ApiError {
    fn from(error: DaemonPairingHostError) -> Self {
        let (status, body) = crate::api::server::map_daemon_pairing_error(error);
        ApiError {
            status,
            code: body.code,
            message: body.message,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use uc_core::setup::SetupState;

    #[test]
    fn confirm_peer_transition_accepts_completed_state_when_hint_requests_host_confirmation() {
        let state = SetupState::Completed;
        let hint = "host-confirm-peer";
        let is_join_confirm = matches!(state, SetupState::JoinSpaceConfirmPeer { .. });
        let is_host_delegate =
            matches!(state, SetupState::Completed) && hint == "host-confirm-peer";
        assert!(is_host_delegate);
        assert!(!is_join_confirm);

        // Different hint - should not be allowed
        let hint = "completed";
        let is_host_delegate =
            matches!(state, SetupState::Completed) && hint == "host-confirm-peer";
        assert!(!is_host_delegate);
    }
}

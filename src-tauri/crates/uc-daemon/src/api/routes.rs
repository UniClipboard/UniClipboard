//! HTTP route handlers for the daemon API.
//!
//! Router is split into two tiers:
//! - L1 (router_l1): public endpoints requiring no authentication (health check)
//! - L2+ (router_l2_plus): protected endpoints behind auth_extractor + rate_limit middleware
//!
//! Auth middleware chain (layer order):
//!   auth_extractor (innermost) runs FIRST -> validates JWT + PID whitelist -> sets client_id
//!   rate_limit (outermost) runs SECOND -> checks rate limit using client_id from extensions
//!
//! L3/L4 permission enforcement is NOT implemented in Phase 75 (deferred to future phases).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde_json::json;
use uc_app::usecases::CoreUseCases;
use uc_core::network::daemon_api_strings::{http_route, pairing_error_code, pairing_stage};
use uc_core::security::model::EncryptionError;
use uc_core::setup::SetupState;

use crate::api::pairing::{
    AckedPairingCommandResponse, InitiatePairingRequest, InitiatePairingResponse,
    PairingApiErrorResponse, PairingGuiLeaseRequest, PairingSessionCommandRequest,
    SetPairingDiscoverabilityRequest, SetPairingParticipantRequest, UnpairDeviceRequest,
    VerifyPairingRequest,
};
use crate::api::server::{map_daemon_pairing_error, DaemonApiState};
use crate::api::types::{SetupResetResponse, SetupSelectPeerRequest, SetupSubmitPassphraseRequest};
use crate::pairing::host::{DaemonPairingHost, DaemonPairingHostError};
use crate::security::middleware::{auth_extractor_middleware, rate_limit_middleware};

/// Build the L1 (public) router - no auth required.
/// Contains only the health check endpoint.
///
/// Takes state to return Router<DaemonApiState> so it can be merged
/// with router_l2_plus without type mismatch.
pub fn router_l1(state: DaemonApiState) -> Router<DaemonApiState> {
    Router::new()
        .route("/health", get(health))
        .with_state(state)
}

/// Build the L2+ (protected) router - requires valid session token.
/// All routes are behind auth_extractor -> rate_limit middleware layers.
///
/// LAYER ORDER (FINDING-2): In Axum, the LAST `.layer()` call wraps closest to the
/// handler and runs FIRST on incoming requests. We want auth_extractor to run FIRST
/// (to validate the JWT and set client_id in extensions), then rate_limit to run SECOND
/// (to check the rate limit using the client_id from extensions).
///
/// Therefore, auth_extractor_middleware must be the LAST .layer() call:
///   .layer(rate_limit_middleware)      // first in code -> outer -> runs SECOND
///   .layer(auth_extractor_middleware)  // last in code -> inner -> runs FIRST
///
/// This means rate limiting applies to already-authenticated requests (by validated PID).
/// It is NOT a pre-auth gate - that is a deliberate design choice for Phase 75.
///
/// NOTE on L3/L4: Phase 75 does NOT implement L3/L4 permission enforcement.
/// The middleware chain enforces only L2 (valid JWT + PID whitelist).
/// L3/L4 checks (encryption_ready state) are reserved for future phases.
pub fn router_l2_plus(state: DaemonApiState) -> Router<DaemonApiState> {
    let router = Router::new()
        .merge(crate::api::clipboard::router())
        .merge(crate::api::settings::router())
        .merge(crate::api::encryption::router())
        .merge(crate::api::storage::router())
        .route("/status", get(status))
        .route("/peers", get(peers))
        .route("/paired-devices", get(paired_devices))
        .route("/space-access/state", get(space_access_state_handler))
        .route("/setup/state", get(setup_state))
        .route("/setup/host", post(setup_host))
        .route("/setup/join", post(setup_join))
        .route("/setup/select-peer", post(setup_select_peer))
        .route("/setup/confirm-peer", post(setup_confirm_peer))
        .route("/setup/submit-passphrase", post(setup_submit_passphrase))
        .route("/setup/cancel", post(setup_cancel))
        .route("/setup/reset", post(setup_reset))
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
        .route("/lifecycle/ready", post(lifecycle_ready))
        .route(
            &format!("{}/:entry_id", http_route::CLIPBOARD_RESTORE),
            post(restore_clipboard_entry_handler),
        )
        .with_state(state.clone());

    // Apply middleware layers.
    // auth_extractor (innermost, runs first) sets client_id in extensions.
    // rate_limit (outermost, runs second) checks the rate limit using client_id.
    // See detailed comment above for layer order explanation.
    let state_for_middleware = Arc::new(state);
    router.layer(middleware::from_fn_with_state(
        state_for_middleware.clone(),
        rate_limit_middleware,
    )).layer(middleware::from_fn_with_state(
        state_for_middleware,
        auth_extractor_middleware,
    ))
}

async fn health(State(state): State<DaemonApiState>) -> impl IntoResponse {
    Json(state.query_service.health().await)
}

/// Signal that the GUI has unlocked and clipboard capture can begin.
///
/// In `--gui-managed` mode, clipboard capture is gated until the GUI
/// explicitly signals readiness (after the user unlocks the app).
/// This endpoint opens that gate.
async fn lifecycle_ready(
    State(state): State<DaemonApiState>,
) -> impl IntoResponse {
    if let Some(gate) = &state.clipboard_capture_gate {
        let was_closed = !gate.swap(true, Ordering::SeqCst);
        if was_closed {
            tracing::info!("Clipboard capture gate opened by GUI lifecycle/ready signal");
        } else {
            tracing::debug!("Clipboard capture gate already open (duplicate lifecycle/ready call)");
        }
    }

    // Trigger deferred services start (clipboard-watcher, inbound-clipboard-sync, etc.)
    if let Some(notify) = &state.deferred_ready_notify {
        notify.notify_one();
    }

    StatusCode::NO_CONTENT.into_response()
}

async fn restore_clipboard_entry_handler(
    State(state): State<DaemonApiState>,
    Path(entry_id): Path<String>,
) -> impl IntoResponse {
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let parsed_id = uc_core::ids::EntryId::from(entry_id.clone());
    let usecases = CoreUseCases::new(runtime.as_ref());

    tracing::info!(entry_id = %entry_id, "daemon restore request received");

    // Restore to OS clipboard first - this calls set_next_origin(LocalRestore) in-process.
    // The daemon's ClipboardWatcherWorker will detect the write, but CaptureClipboardUseCase
    // skips capture for LocalRestore origin - no duplicate DB entry, no outbound sync.
    // This is correct behavior: restored content is already in DB and was previously synced.
    // Do NOT call SyncOutboundClipboardUseCase here - it would cause unwanted duplicate sync.
    let restore_uc = match usecases.restore_clipboard_selection() {
        Ok(uc) => uc,
        Err(e) => {
            tracing::warn!(entry_id = %entry_id, error = %e, "clipboard_write_coordinator unavailable for restore");
            return internal_error(e).into_response();
        }
    };
    match restore_uc.execute(&parsed_id).await {
        Ok(()) => {
            tracing::info!(entry_id = %entry_id, "daemon restore request succeeded");
        }
        Err(e) => {
            // Map "entry not found" errors to 404 (not 500).
            // RestoreClipboardSelectionUseCase returns anyhow error with "not found" text
            // when entry or representations are missing.
            let msg = e.to_string().to_lowercase();
            tracing::warn!(entry_id = %entry_id, error = %e, "daemon restore request failed");
            if msg.contains("not found") {
                return (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"})))
                    .into_response();
            }
            return internal_error(e).into_response();
        }
    }

    // Touch after successful restore to bump active_time (avoids stale timestamp on failed restore)
    match usecases.touch_clipboard_entry().execute(&parsed_id).await {
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(error = %e, entry_id = %entry_id, "touch_clipboard_entry failed after restore");
        }
    }

    (StatusCode::OK, Json(json!({"success": true}))).into_response()
}

async fn status(State(state): State<DaemonApiState>) -> impl IntoResponse {
    match state.query_service.status().await {
        Ok(response) => Json(response).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn peers(State(state): State<DaemonApiState>) -> impl IntoResponse {
    match state.query_service.peers().await {
        Ok(response) => Json(response).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn paired_devices(
    State(state): State<DaemonApiState>,
) -> impl IntoResponse {
    match state.query_service.paired_devices().await {
        Ok(response) => Json(response).into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn space_access_state_handler(
    State(state): State<DaemonApiState>,
) -> impl IntoResponse {
    Json(
        state
            .query_service
            .space_access_state(state.space_access_orchestrator().as_deref())
            .await,
    )
    .into_response()
}

async fn setup_state(
    State(state): State<DaemonApiState>,
) -> axum::response::Response {
    let Some(setup_orchestrator) = setup_orchestrator(&state).into_response_ok() else {
        return setup_failed("setup orchestrator unavailable").into_response();
    };

    match state
        .query_service
        .setup_state(setup_orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => setup_internal_error(error).into_response(),
    }
}

async fn setup_host(
    State(state): State<DaemonApiState>,
) -> axum::response::Response {
    setup_action_without_body(
        state,
        SetupRouteAction::Host,
        |setup_orchestrator| async move { setup_orchestrator.new_space().await },
    )
    .await
}

async fn setup_join(
    State(state): State<DaemonApiState>,
) -> axum::response::Response {
    setup_action_without_body(
        state,
        SetupRouteAction::Join,
        |setup_orchestrator| async move { setup_orchestrator.join_space().await },
    )
    .await
}

async fn setup_select_peer(
    State(state): State<DaemonApiState>,
    payload: Result<Json<SetupSelectPeerRequest>, JsonRejection>,
) -> axum::response::Response {
    let Some(setup_orchestrator) = setup_orchestrator(&state).into_response_ok() else {
        return setup_failed("setup orchestrator unavailable").into_response();
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(_) => return setup_bad_request("malformed request body").into_response(),
    };

    let current_state = setup_orchestrator.get_state().await;
    if !is_transition_allowed(SetupRouteAction::SelectPeer, &current_state) {
        return invalid_setup_transition("current setup state does not allow selecting a peer")
            .into_response();
    }

    match setup_orchestrator.select_device(payload.peer_id).await {
        Ok(_) => setup_action_ack_response(&state, setup_orchestrator.as_ref()).await,
        Err(error) => setup_failed(format!("setup select peer failed: {error}")).into_response(),
    }
}

async fn setup_confirm_peer(
    State(state): State<DaemonApiState>,
) -> axum::response::Response {
    let Some(setup_orchestrator) = setup_orchestrator(&state).into_response_ok() else {
        return setup_failed("setup orchestrator unavailable").into_response();
    };

    let current_state = setup_orchestrator.get_state().await;
    let current_hint = match state
        .query_service
        .setup_state(setup_orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
    {
        Ok(response) => response.next_step_hint,
        Err(error) => return setup_internal_error(error).into_response(),
    };

    if !is_confirm_peer_transition_allowed(&current_state, &current_hint) {
        return invalid_setup_transition("current setup state does not allow this action")
            .into_response();
    }

    if should_delegate_host_confirmation_to_pairing_host(&current_state, &current_hint) {
        let Some(pairing_host) = pairing_host(&state).into_response_ok() else {
            return daemon_pairing_unavailable().into_response();
        };
        let Some(session_id) = pairing_host.active_session_id().await else {
            return invalid_setup_transition("no active pairing session to confirm")
                .into_response();
        };

        return match pairing_host.accept_pairing(&session_id).await {
            Ok(()) => setup_action_ack_response(&state, setup_orchestrator.as_ref()).await,
            Err(error) => pairing_command_error(error).into_response(),
        };
    }

    match setup_orchestrator.confirm_peer_trust().await {
        Ok(_) => setup_action_ack_response(&state, setup_orchestrator.as_ref()).await,
        Err(error) => setup_failed(format!("setup action failed: {error}")).into_response(),
    }
}

async fn setup_submit_passphrase(
    State(state): State<DaemonApiState>,
    payload: Result<Json<SetupSubmitPassphraseRequest>, JsonRejection>,
) -> axum::response::Response {
    let Some(setup_orchestrator) = setup_orchestrator(&state).into_response_ok() else {
        return setup_failed("setup orchestrator unavailable").into_response();
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(_) => return setup_bad_request("malformed request body").into_response(),
    };

    let current_state = setup_orchestrator.get_state().await;
    let result = match current_state {
        SetupState::CreateSpaceInputPassphrase { .. } => {
            setup_orchestrator
                .submit_passphrase(payload.passphrase.clone(), payload.passphrase)
                .await
        }
        SetupState::JoinSpaceInputPassphrase { .. } => {
            setup_orchestrator
                .verify_passphrase(payload.passphrase)
                .await
        }
        _ => {
            return invalid_setup_transition(
                "current setup state does not allow submitting a passphrase",
            )
            .into_response();
        }
    };

    match result {
        Ok(_) => setup_action_ack_response(&state, setup_orchestrator.as_ref()).await,
        Err(error) => {
            setup_failed(format!("setup submit passphrase failed: {error}")).into_response()
        }
    }
}

async fn setup_cancel(
    State(state): State<DaemonApiState>,
) -> axum::response::Response {
    let Some(setup_orchestrator) = setup_orchestrator(&state).into_response_ok() else {
        return setup_failed("setup orchestrator unavailable").into_response();
    };
    let current_state = setup_orchestrator.get_state().await;
    let current_hint = match state
        .query_service
        .setup_state(setup_orchestrator.as_ref(), state.pairing_host().as_deref())
        .await
    {
        Ok(response) => response.next_step_hint,
        Err(error) => return setup_internal_error(error).into_response(),
    };

    if should_delegate_host_confirmation_to_pairing_host(&current_state, &current_hint) {
        let Some(pairing_host) = pairing_host(&state).into_response_ok() else {
            return daemon_pairing_unavailable().into_response();
        };
        let Some(session_id) = pairing_host.active_session_id().await else {
            return invalid_setup_transition("no active pairing session to cancel").into_response();
        };

        return match pairing_host.reject_pairing(&session_id).await {
            Ok(()) => setup_action_ack_response(&state, setup_orchestrator.as_ref()).await,
            Err(error) => pairing_command_error(error).into_response(),
        };
    }

    match setup_orchestrator.cancel_setup().await {
        Ok(_) => setup_action_ack_response(&state, setup_orchestrator.as_ref()).await,
        Err(error) => setup_failed(format!("setup cancel failed: {error}")).into_response(),
    }
}

async fn setup_reset(
    State(state): State<DaemonApiState>,
) -> axum::response::Response {
    let Some(setup_orchestrator) = setup_orchestrator(&state).into_response_ok() else {
        return setup_failed("setup orchestrator unavailable").into_response();
    };
    let Some(runtime) = state.runtime.clone() else {
        return setup_failed("daemon runtime unavailable").into_response();
    };

    if let Some(pairing_host) = state.pairing_host() {
        pairing_host.reset_setup_state().await;
    }

    if let Err(error) = setup_orchestrator.reset().await {
        return setup_failed(format!("setup reset failed: {error}")).into_response();
    }

    let deps = runtime.wiring_deps();
    let paired_devices = match deps.device.paired_device_repo.list_all().await {
        Ok(devices) => devices,
        Err(error) => {
            return setup_failed(format!("setup reset failed: {error}")).into_response();
        }
    };
    for device in paired_devices {
        if let Err(error) = deps.device.paired_device_repo.delete(&device.peer_id).await {
            if !matches!(error, uc_core::ports::PairedDeviceRepositoryError::NotFound) {
                return setup_failed(format!("setup reset failed: {error}")).into_response();
            }
        }
    }

    let scope = match deps.security.key_scope.current_scope().await {
        Ok(scope) => scope,
        Err(error) => {
            return setup_failed(format!("setup reset failed: {error}")).into_response();
        }
    };

    if let Err(error) = deps.security.key_material.delete_keyslot(&scope).await {
        if !matches!(error, EncryptionError::KeyNotFound) {
            return setup_failed(format!("setup reset failed: {error}")).into_response();
        }
    }
    if let Err(error) = deps.security.key_material.delete_kek(&scope).await {
        if !matches!(error, EncryptionError::KeyNotFound) {
            return setup_failed(format!("setup reset failed: {error}")).into_response();
        }
    }
    if let Err(error) = deps.security.encryption_state.clear_initialized().await {
        return setup_failed(format!("setup reset failed: {error}")).into_response();
    }
    if let Err(error) = deps.security.encryption_session.clear().await {
        if !matches!(
            error,
            EncryptionError::KeyNotFound | EncryptionError::NotInitialized
        ) {
            return setup_failed(format!("setup reset failed: {error}")).into_response();
        }
    }

    Json(SetupResetResponse {
        profile: std::env::var("UC_PROFILE")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "default".to_string()),
        daemon_kept_running: true,
    })
    .into_response()
}

async fn pairing_session(
    State(state): State<DaemonApiState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.query_service.pairing_session(&session_id).await {
        Ok(Some(response)) => Json(response).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "not_found", "sessionId": session_id})),
        )
            .into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}

async fn set_pairing_discoverability(
    State(state): State<DaemonApiState>,
    payload: Result<Json<SetPairingDiscoverabilityRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Some(pairing_host) = pairing_host(&state).into_response_ok() else {
        return daemon_pairing_unavailable().into_response();
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(_) => return bad_request("malformed_request_body").into_response(),
    };

    pairing_host
        .set_discoverability(
            payload.client_kind,
            payload.discoverable,
            payload.lease_ttl_ms,
        )
        .await;

    acknowledged(
        "current".to_string(),
        payload.discoverable,
        "discoverability_updated",
    )
    .into_response()
}

async fn set_pairing_participant(
    State(state): State<DaemonApiState>,
    payload: Result<Json<SetPairingParticipantRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Some(pairing_host) = pairing_host(&state).into_response_ok() else {
        return daemon_pairing_unavailable().into_response();
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(_) => return bad_request("malformed_request_body").into_response(),
    };

    pairing_host
        .set_participant_ready(payload.client_kind, payload.ready, payload.lease_ttl_ms)
        .await;

    acknowledged("current".to_string(), payload.ready, "participant_updated").into_response()
}

async fn initiate_pairing(
    State(state): State<DaemonApiState>,
    payload: Result<Json<InitiatePairingRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Some(pairing_host) = pairing_host(&state).into_response_ok() else {
        return daemon_pairing_unavailable().into_response();
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(_) => return bad_request("malformed_request_body").into_response(),
    };

    match pairing_host.initiate_pairing(payload.peer_id).await {
        Ok(session_id) => acknowledged(session_id, true, pairing_stage::REQUEST).into_response(),
        Err(error) => pairing_command_error(error).into_response(),
    }
}

async fn handle_initiate_pairing(
    State(state): State<DaemonApiState>,
    payload: Result<Json<InitiatePairingRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Some(pairing_host) = pairing_host(&state).into_response_ok() else {
        return daemon_pairing_unavailable().into_response();
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return pairing_api_error(
                StatusCode::BAD_REQUEST,
                pairing_error_code::BAD_REQUEST,
                "malformed request body",
            )
            .into_response();
        }
    };

    match pairing_host.initiate_pairing(payload.peer_id).await {
        Ok(session_id) => {
            (StatusCode::OK, Json(InitiatePairingResponse { session_id })).into_response()
        }
        Err(error) => pairing_http_error(error).into_response(),
    }
}

async fn accept_pairing(
    State(state): State<DaemonApiState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let Some(pairing_host) = pairing_host(&state).into_response_ok() else {
        return daemon_pairing_unavailable().into_response();
    };

    match pairing_host.accept_pairing(&session_id).await {
        Ok(()) => acknowledged(session_id, true, pairing_stage::VERIFYING).into_response(),
        Err(error) => pairing_command_error(error).into_response(),
    }
}

async fn handle_accept_pairing(
    State(state): State<DaemonApiState>,
    payload: Result<Json<PairingSessionCommandRequest>, JsonRejection>,
) -> impl IntoResponse {
    handle_session_command(
        state,
        payload,
        |pairing_host, session_id| async move { pairing_host.accept_pairing(&session_id).await },
    )
    .await
}

async fn reject_pairing(
    State(state): State<DaemonApiState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let Some(pairing_host) = pairing_host(&state).into_response_ok() else {
        return daemon_pairing_unavailable().into_response();
    };

    match pairing_host.reject_pairing(&session_id).await {
        Ok(()) => acknowledged(session_id, true, pairing_stage::FAILED).into_response(),
        Err(error) => pairing_command_error(error).into_response(),
    }
}

async fn handle_reject_pairing(
    State(state): State<DaemonApiState>,
    payload: Result<Json<PairingSessionCommandRequest>, JsonRejection>,
) -> impl IntoResponse {
    handle_session_command(
        state,
        payload,
        |pairing_host, session_id| async move { pairing_host.reject_pairing(&session_id).await },
    )
    .await
}

async fn cancel_pairing(
    State(state): State<DaemonApiState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let Some(pairing_host) = pairing_host(&state).into_response_ok() else {
        return daemon_pairing_unavailable().into_response();
    };

    match pairing_host.cancel_pairing(&session_id).await {
        Ok(()) => acknowledged(session_id, true, pairing_stage::FAILED).into_response(),
        Err(error) => pairing_command_error(error).into_response(),
    }
}

async fn handle_cancel_pairing(
    State(state): State<DaemonApiState>,
    payload: Result<Json<PairingSessionCommandRequest>, JsonRejection>,
) -> impl IntoResponse {
    handle_session_command(
        state,
        payload,
        |pairing_host, session_id| async move { pairing_host.cancel_pairing(&session_id).await },
    )
    .await
}

async fn handle_unpair_device(
    State(state): State<DaemonApiState>,
    payload: Result<Json<UnpairDeviceRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Some(runtime) = state.runtime.clone() else {
        return pairing_api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            pairing_error_code::RUNTIME_UNAVAILABLE,
            "daemon runtime unavailable",
        )
        .into_response();
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return pairing_api_error(
                StatusCode::BAD_REQUEST,
                pairing_error_code::BAD_REQUEST,
                "malformed request body",
            )
            .into_response();
        }
    };

    let usecases = CoreUseCases::new(runtime.as_ref());
    match usecases.unpair_device().execute(payload.peer_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            let message = error.to_string();
            tracing::error!(error = %error, "daemon unpair command failed");
            pairing_api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                pairing_error_code::INTERNAL,
                &message,
            )
            .into_response()
        }
    }
}

async fn verify_pairing(
    State(state): State<DaemonApiState>,
    Path(session_id): Path<String>,
    payload: Result<Json<VerifyPairingRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Some(pairing_host) = pairing_host(&state).into_response_ok() else {
        return daemon_pairing_unavailable().into_response();
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(_) => return bad_request("malformed_request_body").into_response(),
    };

    match pairing_host
        .verify_pairing(&session_id, payload.pin_matches)
        .await
    {
        Ok(()) => acknowledged(
            session_id,
            true,
            if payload.pin_matches {
                pairing_stage::VERIFYING
            } else {
                pairing_stage::FAILED
            },
        )
        .into_response(),
        Err(error) => pairing_command_error(error).into_response(),
    }
}

async fn handle_pairing_gui_lease(
    State(state): State<DaemonApiState>,
    payload: Result<Json<PairingGuiLeaseRequest>, JsonRejection>,
) -> impl IntoResponse {
    let Some(pairing_host) = pairing_host(&state).into_response_ok() else {
        return daemon_pairing_unavailable().into_response();
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return pairing_api_error(
                StatusCode::BAD_REQUEST,
                pairing_error_code::BAD_REQUEST,
                "malformed request body",
            )
            .into_response();
        }
    };

    match pairing_host
        .register_gui_participant(payload.enabled, payload.lease_ttl_ms)
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => pairing_http_error(error).into_response(),
    }
}

fn pairing_host(state: &DaemonApiState) -> Result<Arc<DaemonPairingHost>, ()> {
    state.pairing_host().ok_or(())
}

fn setup_orchestrator(
    state: &DaemonApiState,
) -> Result<Arc<uc_app::usecases::SetupOrchestrator>, ()> {
    state.setup_orchestrator().ok_or(())
}

async fn handle_session_command<F, Fut>(
    state: DaemonApiState,
    payload: Result<Json<PairingSessionCommandRequest>, JsonRejection>,
    handler: F,
) -> axum::response::Response
where
    F: FnOnce(Arc<DaemonPairingHost>, String) -> Fut,
    Fut: std::future::Future<Output = Result<(), DaemonPairingHostError>>,
{
    let Some(pairing_host) = pairing_host(&state).into_response_ok() else {
        return daemon_pairing_unavailable().into_response();
    };
    let Json(payload) = match payload {
        Ok(payload) => payload,
        Err(_) => {
            return pairing_api_error(
                StatusCode::BAD_REQUEST,
                pairing_error_code::BAD_REQUEST,
                "malformed request body",
            )
            .into_response();
        }
    };

    match handler(pairing_host, payload.session_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => pairing_http_error(error).into_response(),
    }
}

/// NOTE: The `unauthorized()` helper is kept for backward compatibility with modules
/// that may still reference it. The middleware layer now handles authentication
/// before requests reach handlers, so individual handlers no longer call this.
pub(crate) fn unauthorized() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "unauthorized"})),
    )
}

fn setup_bad_request(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "code": "bad_request",
            "message": message,
        })),
    )
}

fn invalid_setup_transition(message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::CONFLICT,
        Json(json!({
            "code": "invalid_setup_transition",
            "message": message,
        })),
    )
}

fn setup_failed(message: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "code": "setup_failed",
            "message": message.into(),
        })),
    )
}

fn setup_internal_error(error: anyhow::Error) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!(error = %error, "daemon setup API request failed");
    setup_failed(error.to_string())
}

fn daemon_pairing_unavailable() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error": "pairing_host_unavailable"})),
    )
}

fn bad_request(error: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": error })))
}

pub(crate) fn internal_error(error: anyhow::Error) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!(error = %error, "daemon API request failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": "internal_error"})),
    )
}

fn pairing_command_error(error: DaemonPairingHostError) -> (StatusCode, Json<serde_json::Value>) {
    let (status, body) = map_daemon_pairing_error(error);
    (
        status,
        Json(json!({
            "code": body.code,
            "message": body.message,
        })),
    )
}

fn pairing_http_error(
    error: DaemonPairingHostError,
) -> (StatusCode, Json<PairingApiErrorResponse>) {
    let (status, body) = map_daemon_pairing_error(error);
    (status, Json(body))
}

fn pairing_api_error(
    status: StatusCode,
    code: &str,
    message: &str,
) -> (StatusCode, Json<PairingApiErrorResponse>) {
    (
        status,
        Json(PairingApiErrorResponse {
            code: code.to_string(),
            message: message.to_string(),
        }),
    )
}

fn acknowledged(
    session_id: String,
    accepted: bool,
    state: &'static str,
) -> (StatusCode, Json<AckedPairingCommandResponse>) {
    (
        StatusCode::ACCEPTED,
        Json(AckedPairingCommandResponse {
            session_id,
            accepted,
            state: state.to_string(),
            error: None,
        }),
    )
}

trait IntoResponseOk<T> {
    fn into_response_ok(self) -> Option<T>;
}

impl<T, E> IntoResponseOk<T> for Result<T, E> {
    fn into_response_ok(self) -> Option<T> {
        self.ok()
    }
}

#[derive(Clone, Copy)]
enum SetupRouteAction {
    Host,
    Join,
    SelectPeer,
}

fn is_transition_allowed(action: SetupRouteAction, state: &SetupState) -> bool {
    match action {
        SetupRouteAction::Host | SetupRouteAction::Join => matches!(state, SetupState::Welcome),
        SetupRouteAction::SelectPeer => matches!(state, SetupState::JoinSpaceSelectDevice { .. }),
    }
}

fn is_confirm_peer_transition_allowed(state: &SetupState, next_step_hint: &str) -> bool {
    matches!(state, SetupState::JoinSpaceConfirmPeer { .. })
        || matches!(state, SetupState::Completed) && next_step_hint == "host-confirm-peer"
}

fn should_delegate_host_confirmation_to_pairing_host(
    state: &SetupState,
    next_step_hint: &str,
) -> bool {
    matches!(state, SetupState::Completed) && next_step_hint == "host-confirm-peer"
}

async fn setup_action_without_body<F, Fut>(
    state: DaemonApiState,
    action: SetupRouteAction,
    handler: F,
) -> axum::response::Response
where
    F: FnOnce(Arc<uc_app::usecases::SetupOrchestrator>) -> Fut,
    Fut: std::future::Future<Output = Result<SetupState, uc_app::usecases::SetupError>>,
{
    let Some(setup_orchestrator) = setup_orchestrator(&state).into_response_ok() else {
        return setup_failed("setup orchestrator unavailable").into_response();
    };

    let current_state = setup_orchestrator.get_state().await;
    if !is_transition_allowed(action, &current_state) {
        return invalid_setup_transition("current setup state does not allow this action")
            .into_response();
    }

    match handler(setup_orchestrator.clone()).await {
        Ok(_) => setup_action_ack_response(&state, setup_orchestrator.as_ref()).await,
        Err(error) => setup_failed(format!("setup action failed: {error}")).into_response(),
    }
}

async fn setup_action_ack_response(
    state: &DaemonApiState,
    setup_orchestrator: &uc_app::usecases::SetupOrchestrator,
) -> axum::response::Response {
    match state
        .query_service
        .setup_action_ack(setup_orchestrator, state.pairing_host().as_deref())
        .await
    {
        Ok(response) => Json(response).into_response(),
        Err(error) => setup_internal_error(error).into_response(),
    }
}

#[allow(dead_code)]
fn _route_markers() -> [&'static str; 22] {
    [
        "/space-access/state",
        "/setup/state",
        "/setup/host",
        "/setup/join",
        "/setup/select-peer",
        "/setup/confirm-peer",
        "/setup/submit-passphrase",
        "/setup/cancel",
        "/setup/reset",
        "/pairing/discoverability/current",
        "/pairing/participants/current",
        "/pairing/unpair",
        "/pairing/sessions",
        "/pairing/sessions/:session_id/accept",
        "/pairing/sessions/:session_id/reject",
        "/pairing/sessions/:session_id/cancel",
        "/pairing/sessions/:session_id/verify",
        pairing_error_code::ACTIVE_SESSION_EXISTS,
        pairing_error_code::NO_LOCAL_PARTICIPANT,
        pairing_error_code::HOST_NOT_DISCOVERABLE,
        "invalid_setup_transition",
        "/clipboard/restore/:entry_id",
    ]
}

#[allow(dead_code)]
fn _response_marker(_: AckedPairingCommandResponse) -> StatusCode {
    StatusCode::ACCEPTED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_peer_transition_accepts_completed_state_when_hint_requests_host_confirmation() {
        assert!(is_confirm_peer_transition_allowed(
            &SetupState::Completed,
            "host-confirm-peer"
        ));
        assert!(!is_confirm_peer_transition_allowed(
            &SetupState::Completed,
            "completed"
        ));
    }
}

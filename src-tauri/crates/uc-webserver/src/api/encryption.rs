//! HTTP route handlers for encryption state and session management endpoints.

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::sync::broadcast::error::SendError;
use tracing::{info, warn};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::constants::{ws_event, ws_topic};
use utoipa;

use crate::api::dto::encryption::{
    EncryptionActionResponse, EncryptionSessionReadyPayload, EncryptionStateResponse,
    KeychainAccessResponse,
};
use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::server::DaemonApiState;
use crate::api::types::DaemonWsEvent;

/// 把 encryption facade 的 anyhow 错误映射为 500 + 根因日志。
///
/// 与 `RosterError` / `SearchFacadeError` 不同，encryption facade 当前向 webserver
/// 暴露 `anyhow::Error` 而非 enum，所以 `error_variant` 退化为 `"call_failed"`，
/// 分桶由 `op` 维度承担（state / unlock / lock / verify_keychain_access）。
fn map_encryption_internal(op: &'static str, message: String) -> ApiError {
    let api = ApiError::internal(message);
    log_facade_failure("encryption", op, "call_failed", api.status, &api.message);
    api
}

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/encryption/state", get(get_encryption_state_handler))
        .route("/encryption/unlock", post(unlock_handler))
        .route("/encryption/lock", post(lock_handler))
        .route(
            "/encryption/keychain-access",
            get(verify_keychain_access_handler),
        )
}

/// GET /encryption/state
/// Returns the current encryption state and session readiness.
#[utoipa::path(
    get,
    path = "/encryption/state",
    operation_id = "getEncryptionState",
    tag = "encryption",
    responses(
        (status = 200, description = "Encryption state retrieved", body = EncryptionStateEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_encryption_state_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<EncryptionStateResponse>>, ApiError> {
    let app = state.app_facade_or_error()?;
    let view = app
        .encryption
        .state()
        .await
        .map_err(|e| map_encryption_internal("encryption_state", e.to_string()))?;

    Ok(Json(ApiEnvelope::now(EncryptionStateResponse {
        initialized: view.initialized,
        session_ready: view.session_ready,
    })))
}

/// POST /encryption/unlock
/// Attempts to auto-unlock the encryption session using keyring-stored KEK.
/// No passphrase is required — credentials are retrieved from the OS keychain.
/// On success, broadcasts the `encryption.session_ready` WebSocket event.
#[utoipa::path(
    post,
    path = "/encryption/unlock",
    operation_id = "unlockEncryptionSession",
    tag = "encryption",
    responses(
        (status = 200, description = "Encryption session unlocked (or already ready)", body = EncryptionActionEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn unlock_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<EncryptionActionResponse>>, ApiError> {
    let app = state.app_facade_or_error()?;

    // Route to `space_setup.try_resume_session()` rather than the bare
    // `encryption.unlock()`. The thin variant only resumes the session;
    // the SpaceSetupFacade variant also runs the switch-space migration
    // recovery hook (`resume_pending`) so a pending HandshakeDone replay
    // gets advanced the moment the session unlocks. Without that, a
    // device that crashed mid-`switch_space` would silently land here,
    // get a new master_key, leave the main table inline_data encrypted
    // with the previous key, and surface as "no history" in the UI —
    // exactly the wedged state we just dug out of on fedora dev.
    match app.try_resume_session().await {
        Ok(true) => {
            info!("encryption session auto-unlocked via keyring");

            let ts = chrono::Utc::now().timestamp_millis();
            let event_payload = EncryptionSessionReadyPayload { ts };
            let event = DaemonWsEvent {
                topic: ws_topic::ENCRYPTION.to_string(),
                event_type: ws_event::ENCRYPTION_SESSION_READY.to_string(),
                session_id: None,
                ts,
                payload: serde_json::to_value(&event_payload).unwrap_or(serde_json::Value::Null),
            };
            if let Err(SendError(_)) = state.event_tx.send(event) {
                warn!("failed to broadcast encryption.session_ready event — no active subscribers");
            }

            Ok(Json(ApiEnvelope::now(EncryptionActionResponse {
                success: true,
            })))
        }
        Ok(false) => {
            info!("encryption not initialized, skipping auto-unlock");
            Ok(Json(ApiEnvelope::now(EncryptionActionResponse {
                success: false,
            })))
        }
        Err(e) => Err(map_encryption_internal(
            "encryption_unlock",
            format!("auto-unlock failed: {e}"),
        )),
    }
}

/// POST /encryption/lock
/// Locks the encryption session by clearing the master key.
#[utoipa::path(
    post,
    path = "/encryption/lock",
    operation_id = "lockEncryptionSession",
    tag = "encryption",
    responses(
        (status = 200, description = "Encryption session locked", body = EncryptionActionEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn lock_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<EncryptionActionResponse>>, ApiError> {
    let app = state.app_facade_or_error()?;
    app.encryption.lock().await.map_err(|e| {
        map_encryption_internal("encryption_lock", format!("failed to lock encryption: {e}"))
    })?;

    info!("encryption session cleared (locked)");
    Ok(Json(ApiEnvelope::now(EncryptionActionResponse {
        success: true,
    })))
}

/// GET /encryption/keychain-access
/// Verifies macOS Keychain "Always Allow" permission for this app.
/// Returns `granted: true` if Keychain access succeeds silently, `false` if permission denied.
#[utoipa::path(
    get,
    path = "/encryption/keychain-access",
    operation_id = "verifyKeychainAccess",
    tag = "encryption",
    responses(
        (status = 200, description = "Keychain access verified", body = KeychainAccessEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn verify_keychain_access_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<KeychainAccessResponse>>, ApiError> {
    let app = state.app_facade_or_error()?;
    let granted = app.encryption.verify_keychain_access().await.map_err(|e| {
        map_encryption_internal(
            "verify_keychain_access",
            format!("keychain access check failed: {e}"),
        )
    })?;

    Ok(Json(ApiEnvelope::now(KeychainAccessResponse { granted })))
}

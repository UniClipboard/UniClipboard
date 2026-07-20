//! HTTP route handlers for encryption state and session management endpoints.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::sync::broadcast::error::SendError;
use tracing::{debug, info};
use uc_application::facade::FactoryResetError;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::constants::{ws_event, ws_topic};
use uc_engine::internal::session_recovery::{
    execute_recover_session, RECOVER_SESSION_RECEIVE_UNAVAILABLE_CODE,
};
use uc_engine::internal::unlock::{
    execute_unlock_space, UNLOCK_SPACE_CORRUPTED_CODE, UNLOCK_SPACE_NOT_INITIALIZED_CODE,
    UNLOCK_SPACE_SETUP_NOT_COMPLETED_CODE, UNLOCK_SPACE_UNAUTHORIZED_CODE,
};
use uc_engine::{
    EngineError, EngineErrorCategory, OperationResult, RecoverSessionInput, SecretString,
    UnlockSpaceInput,
};
use utoipa;

use crate::api::dto::encryption::{
    EncryptionActionResponse, EncryptionSessionReadyPayload, EncryptionStateResponse,
    KeychainAccessResponse, UnlockSpaceRequest, UnlockSpaceResponse,
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
        .route(
            "/encryption/unlock-with-passphrase",
            post(unlock_with_passphrase_handler),
        )
        .route("/encryption/lock", post(lock_handler))
        .route("/encryption/factory-reset", post(factory_reset_handler))
        .route(
            "/encryption/keychain-access",
            get(verify_keychain_access_handler),
        )
}

/// Map the typed [`FactoryResetError`] onto an [`ApiError`] whose `code` is the
/// SCREAMING_SNAKE tag the frontend `FactoryResetCommandError` union switches on
/// (read off `DaemonApiError.details.code`). All variants are infra failures →
/// 500 with the cause string preserved in `message` (never a secret).
fn map_factory_reset_err(err: FactoryResetError) -> ApiError {
    use FactoryResetError as E;
    let (variant, code, message) = match err {
        E::KeyMaterialWipeFailed(msg) => {
            ("key_material_wipe_failed", "KEY_MATERIAL_WIPE_FAILED", msg)
        }
        E::StorageFailed(msg) => ("storage_failed", "STORAGE_FAILED", msg),
        E::Internal(msg) => ("internal", "INTERNAL", msg),
    };
    let api = ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        code: code.to_string(),
        message,
        details: None,
    };
    log_facade_failure(
        "space_setup",
        "factory_reset",
        variant,
        api.status,
        &api.message,
    );
    api
}

/// Map stable engine unlock failures onto the existing HTTP error contract.
fn map_unlock_engine_err(err: EngineError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match err.code() {
        UNLOCK_SPACE_SETUP_NOT_COMPLETED_CODE => (
            "setup_not_completed",
            ApiError {
                status: StatusCode::CONFLICT,
                code: "SETUP_NOT_COMPLETED".to_string(),
                message: "setup has not been completed".to_string(),
                details: None,
            },
        ),
        UNLOCK_SPACE_NOT_INITIALIZED_CODE => (
            "space_not_initialized",
            ApiError {
                status: StatusCode::CONFLICT,
                code: "SPACE_NOT_INITIALIZED".to_string(),
                message: "space is not initialized on this device".to_string(),
                details: None,
            },
        ),
        UNLOCK_SPACE_UNAUTHORIZED_CODE => (
            "wrong_passphrase",
            ApiError {
                status: StatusCode::FORBIDDEN,
                code: "WRONG_PASSPHRASE".to_string(),
                message: "wrong passphrase".to_string(),
                details: None,
            },
        ),
        UNLOCK_SPACE_CORRUPTED_CODE => (
            "corrupted_key_material",
            ApiError {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                code: "CORRUPTED_KEY_MATERIAL".to_string(),
                message: "space key material is corrupted".to_string(),
                details: None,
            },
        ),
        _ if err.category() == EngineErrorCategory::Unavailable => (
            "service_unavailable",
            ApiError::service_unavailable("receive recovery failed after space unlock"),
        ),
        _ => (
            "internal",
            ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "INTERNAL".to_string(),
                message: "failed to unlock space".to_string(),
                details: None,
            },
        ),
    };
    log_facade_failure(
        "space_setup",
        "unlock_space",
        variant,
        api.status,
        &api.message,
    );
    api
}

fn map_recover_engine_err(err: EngineError) -> ApiError {
    let (variant, api) = if err.code() == RECOVER_SESSION_RECEIVE_UNAVAILABLE_CODE {
        (
            "receive_unavailable",
            ApiError::service_unavailable("receive recovery failed after session recovery"),
        )
    } else {
        (
            "recovery_failed",
            ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "INTERNAL".to_string(),
                message: "auto-unlock failed".to_string(),
                details: None,
            },
        )
    };
    log_facade_failure(
        "space_setup",
        "recover_session",
        variant,
        api.status,
        &api.message,
    );
    api
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
    let result = execute_recover_session(
        app.as_ref(),
        state.receive_readiness.as_ref(),
        RecoverSessionInput {
            allow_secure_storage_unlock: true,
        },
    )
    .await
    .map_err(map_recover_engine_err)?;

    match result {
        OperationResult::SessionRecovered { unlocked: true, .. } => {
            info!("encryption session auto-unlocked via keyring");
            broadcast_session_ready(&state);
            Ok(Json(ApiEnvelope::now(EncryptionActionResponse {
                success: true,
            })))
        }
        OperationResult::SessionRecovered {
            unlocked: false, ..
        } => {
            info!("encryption not initialized, skipping auto-unlock");
            Ok(Json(ApiEnvelope::now(EncryptionActionResponse {
                success: false,
            })))
        }
        _ => Err(ApiError::internal(
            "engine returned an unexpected recovery result",
        )),
    }
}

/// POST /encryption/unlock-with-passphrase
/// Unlocks the space with a user-supplied plaintext passphrase (ADR-008 D15).
///
/// Routes through the engine unlock operation, which also runs switch-space,
/// search, receive, clipboard gate, and deferred-service recovery. On success
/// the HTTP layer only broadcasts `encryption.session_ready`.
///
/// D14: this endpoint is session-JWT gated (it is NOT in `PUBLIC_PATHS`) and
/// the handler MUST NOT log the request body — there is intentionally no
/// `?req` / passphrase field on any span or tracing event here.
#[utoipa::path(
    post,
    path = "/encryption/unlock-with-passphrase",
    operation_id = "unlockSpaceWithPassphrase",
    tag = "encryption",
    request_body = UnlockSpaceRequest,
    responses(
        (status = 200, description = "Space unlocked", body = UnlockSpaceEnvelope),
        (status = 403, description = "Wrong passphrase", body = ApiErrorResponse),
        (status = 409, description = "Setup not completed / space not initialized", body = ApiErrorResponse),
        (status = 422, description = "Space key material corrupted", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn unlock_with_passphrase_handler(
    State(state): State<DaemonApiState>,
    Json(req): Json<UnlockSpaceRequest>,
) -> Result<Json<ApiEnvelope<UnlockSpaceResponse>>, ApiError> {
    let app = state.app_facade_or_error()?;
    let result = execute_unlock_space(
        app.as_ref(),
        state.receive_readiness.as_ref(),
        UnlockSpaceInput {
            passphrase: SecretString::new(req.passphrase),
        },
    )
    .await
    .map_err(map_unlock_engine_err)?;
    let OperationResult::SpaceUnlocked { space_id } = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected unlock result",
        ));
    };

    info!("space unlocked via passphrase");
    broadcast_session_ready(&state);

    Ok(Json(ApiEnvelope::now(UnlockSpaceResponse { space_id })))
}

fn broadcast_session_ready(state: &DaemonApiState) {
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
        debug!("failed to broadcast encryption.session_ready event — no active subscribers");
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
    state.receive_readiness.close_receive_gate();

    info!("encryption session cleared (locked)");
    Ok(Json(ApiEnvelope::now(EncryptionActionResponse {
        success: true,
    })))
}

/// POST /encryption/factory-reset
/// Wipes key material + clears setup status + cancels pending invitations
/// (ADR-008 P3-1 / D15). Routes through `SpaceSetupFacade::factory_reset`,
/// mirroring the former in-process `factory_reset_space` Tauri command.
#[utoipa::path(
    post,
    path = "/encryption/factory-reset",
    operation_id = "factoryResetSpace",
    tag = "encryption",
    responses(
        (status = 200, description = "Space reset to factory state", body = EncryptionActionEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn factory_reset_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<EncryptionActionResponse>>, ApiError> {
    let app = state.app_facade_or_error()?;
    let facade = app
        .space_setup
        .get()
        .cloned()
        .ok_or_else(|| ApiError::service_unavailable("space setup facade not assembled"))?;

    facade
        .factory_reset()
        .await
        .map_err(map_factory_reset_err)?;

    info!("space factory-reset completed");
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend `UnlockSpaceCommandError` union switches on the SCREAMING_SNAKE
    /// `code` (read off `DaemonApiError.details.code` after `callSdk` normalization),
    /// and `callSdk` would fire a spurious session refresh + retry on a `401`. So
    /// every user-recoverable variant must carry its semantic code and a non-401
    /// status.
    #[test]
    fn map_engine_unlock_internal_is_500_and_redacted() {
        let api = map_unlock_engine_err(EngineError::new(
            uc_engine::internal::unlock::UNLOCK_SPACE_FAILED_CODE,
            EngineErrorCategory::Internal,
            false,
        ));
        assert_eq!(api.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(api.code, "INTERNAL");
        assert_eq!(api.message, "failed to unlock space");
    }

    #[test]
    fn map_engine_unlock_errors_preserves_http_contract() {
        use uc_engine::internal::unlock::{
            UNLOCK_SPACE_CORRUPTED_CODE, UNLOCK_SPACE_FAILED_CODE,
            UNLOCK_SPACE_NOT_INITIALIZED_CODE, UNLOCK_SPACE_SETUP_NOT_COMPLETED_CODE,
            UNLOCK_SPACE_UNAUTHORIZED_CODE,
        };
        use uc_engine::{EngineError, EngineErrorCategory};

        let cases = [
            (
                EngineError::new(
                    UNLOCK_SPACE_SETUP_NOT_COMPLETED_CODE,
                    EngineErrorCategory::InvalidState,
                    false,
                ),
                StatusCode::CONFLICT,
                "SETUP_NOT_COMPLETED",
            ),
            (
                EngineError::new(
                    UNLOCK_SPACE_NOT_INITIALIZED_CODE,
                    EngineErrorCategory::InvalidState,
                    false,
                ),
                StatusCode::CONFLICT,
                "SPACE_NOT_INITIALIZED",
            ),
            (
                EngineError::new(
                    UNLOCK_SPACE_UNAUTHORIZED_CODE,
                    EngineErrorCategory::Unauthorized,
                    false,
                ),
                StatusCode::FORBIDDEN,
                "WRONG_PASSPHRASE",
            ),
            (
                EngineError::new(
                    UNLOCK_SPACE_CORRUPTED_CODE,
                    EngineErrorCategory::Internal,
                    false,
                ),
                StatusCode::UNPROCESSABLE_ENTITY,
                "CORRUPTED_KEY_MATERIAL",
            ),
            (
                EngineError::new(
                    UNLOCK_SPACE_FAILED_CODE,
                    EngineErrorCategory::Internal,
                    false,
                ),
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL",
            ),
            (
                EngineError::new(
                    UNLOCK_SPACE_FAILED_CODE,
                    EngineErrorCategory::Unavailable,
                    true,
                ),
                StatusCode::SERVICE_UNAVAILABLE,
                "runtime_unavailable",
            ),
        ];

        for (error, status, code) in cases {
            let api = map_unlock_engine_err(error);
            assert_eq!(api.status, status);
            assert_eq!(api.code, code);
        }
    }

    #[test]
    fn map_engine_recovery_errors_preserves_http_statuses() {
        use uc_engine::internal::session_recovery::{
            RECOVER_SESSION_RECEIVE_UNAVAILABLE_CODE, RECOVER_SESSION_UNAVAILABLE_CODE,
        };

        let internal = map_recover_engine_err(EngineError::new(
            RECOVER_SESSION_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        ));
        assert_eq!(internal.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(internal.code, "INTERNAL");
        assert_eq!(internal.message, "auto-unlock failed");

        let receive = map_recover_engine_err(EngineError::new(
            RECOVER_SESSION_RECEIVE_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        ));
        assert_eq!(receive.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(receive.code, "runtime_unavailable");
    }

    /// Factory-reset variants keep the `FactoryResetCommandError` semantic codes
    /// (frontend reads `DaemonApiError.details.code`) and preserve the infra
    /// cause string in `message`.
    #[test]
    fn map_factory_reset_err_keeps_semantic_codes_and_message() {
        let cases = [
            (
                FactoryResetError::KeyMaterialWipeFailed("disk i/o".to_string()),
                "KEY_MATERIAL_WIPE_FAILED",
                "disk i/o",
            ),
            (
                FactoryResetError::StorageFailed("settings db locked".to_string()),
                "STORAGE_FAILED",
                "settings db locked",
            ),
            (
                FactoryResetError::Internal("oops".to_string()),
                "INTERNAL",
                "oops",
            ),
        ];
        for (err, code, message) in cases {
            let api = map_factory_reset_err(err);
            assert_eq!(api.status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(api.code, code);
            assert_eq!(api.message, message);
        }
    }
}

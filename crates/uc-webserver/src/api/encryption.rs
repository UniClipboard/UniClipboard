//! HTTP route handlers for encryption state and session management endpoints.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio::sync::broadcast::error::SendError;
use tracing::{debug, info};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::constants::{ws_event, ws_topic};
use uc_engine::internal::encryption::{
    execute_lock_encryption, execute_query_encryption_state, execute_verify_secure_storage_access,
    LOCK_ENCRYPTION_FAILED_CODE, QUERY_ENCRYPTION_STATE_FAILED_CODE,
    VERIFY_SECURE_STORAGE_ACCESS_FAILED_CODE,
};
use uc_engine::internal::factory_reset::{
    execute_factory_reset_space, FACTORY_RESET_FAILED_CODE, FACTORY_RESET_KEY_MATERIAL_FAILED_CODE,
    FACTORY_RESET_STORAGE_FAILED_CODE, FACTORY_RESET_UNAVAILABLE_CODE,
};
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

fn map_encryption_engine_error(
    op: &'static str,
    expected_code: u32,
    public_message: &'static str,
    error: EngineError,
) -> ApiError {
    let variant = if error.code() == expected_code {
        "operation_failed"
    } else {
        "unexpected_engine_error"
    };
    let api = ApiError::internal(public_message);
    log_facade_failure("encryption", op, variant, api.status, &api.message);
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

fn map_factory_reset_engine_err(error: EngineError) -> ApiError {
    let (variant, api) = match error.code() {
        FACTORY_RESET_UNAVAILABLE_CODE => (
            "unavailable",
            ApiError::service_unavailable("space setup facade not assembled"),
        ),
        FACTORY_RESET_KEY_MATERIAL_FAILED_CODE => (
            "key_material_wipe_failed",
            ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "KEY_MATERIAL_WIPE_FAILED".to_string(),
                message: "failed to wipe key material".to_string(),
                details: None,
            },
        ),
        FACTORY_RESET_STORAGE_FAILED_CODE => (
            "storage_failed",
            ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "STORAGE_FAILED".to_string(),
                message: "failed to clear setup status".to_string(),
                details: None,
            },
        ),
        FACTORY_RESET_FAILED_CODE => (
            "internal",
            ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "INTERNAL".to_string(),
                message: "factory reset failed".to_string(),
                details: None,
            },
        ),
        _ => (
            "unexpected_engine_error",
            ApiError::internal("factory reset failed"),
        ),
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
    let result = execute_query_encryption_state(app.as_ref())
        .await
        .map_err(|error| {
            map_encryption_engine_error(
                "encryption_state",
                QUERY_ENCRYPTION_STATE_FAILED_CODE,
                "failed to get encryption state",
                error,
            )
        })?;
    let OperationResult::EncryptionState(view) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected encryption-state result",
        ));
    };

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
    let result = execute_lock_encryption(app.as_ref(), state.receive_readiness.as_ref())
        .await
        .map_err(|error| {
            map_encryption_engine_error(
                "encryption_lock",
                LOCK_ENCRYPTION_FAILED_CODE,
                "failed to lock encryption",
                error,
            )
        })?;
    if !matches!(result, OperationResult::EncryptionLocked) {
        return Err(ApiError::internal(
            "engine returned an unexpected encryption-lock result",
        ));
    }

    info!("encryption session cleared (locked)");
    Ok(Json(ApiEnvelope::now(EncryptionActionResponse {
        success: true,
    })))
}

/// POST /encryption/factory-reset
/// Wipes key material + clears setup status + cancels pending invitations
/// (ADR-008 P3-1 / D15). Routes through the engine's factory-reset operation.
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
    let result = execute_factory_reset_space(app.as_ref(), state.receive_readiness.as_ref())
        .await
        .map_err(map_factory_reset_engine_err)?;
    if !matches!(result, OperationResult::SpaceFactoryReset) {
        return Err(ApiError::internal(
            "engine returned an unexpected factory-reset result",
        ));
    }

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
    let result = execute_verify_secure_storage_access(app.as_ref())
        .await
        .map_err(|error| {
            map_encryption_engine_error(
                "verify_keychain_access",
                VERIFY_SECURE_STORAGE_ACCESS_FAILED_CODE,
                "secure storage access check failed",
                error,
            )
        })?;
    let OperationResult::SecureStorageAccess { granted } = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected secure-storage result",
        ));
    };

    Ok(Json(ApiEnvelope::now(KeychainAccessResponse { granted })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_encryption_operation_errors_keeps_stable_public_messages() {
        let cases = [
            (
                QUERY_ENCRYPTION_STATE_FAILED_CODE,
                "encryption_state",
                "failed to get encryption state",
            ),
            (
                LOCK_ENCRYPTION_FAILED_CODE,
                "encryption_lock",
                "failed to lock encryption",
            ),
            (
                VERIFY_SECURE_STORAGE_ACCESS_FAILED_CODE,
                "verify_keychain_access",
                "secure storage access check failed",
            ),
        ];

        for (code, op, public_message) in cases {
            let api = map_encryption_engine_error(
                op,
                code,
                public_message,
                EngineError::new(code, EngineErrorCategory::Internal, false),
            );

            assert_eq!(api.status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(api.code, "internal_error");
            assert_eq!(api.message, public_message);
            assert!(api.details.is_none());
        }
    }

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

    /// Factory-reset variants keep the frontend semantic codes while redacting
    /// infrastructure details from the public message.
    #[test]
    fn map_factory_reset_engine_err_keeps_semantic_codes_and_redacts_details() {
        let cases = [
            (
                FACTORY_RESET_KEY_MATERIAL_FAILED_CODE,
                "KEY_MATERIAL_WIPE_FAILED",
                "failed to wipe key material",
            ),
            (
                FACTORY_RESET_STORAGE_FAILED_CODE,
                "STORAGE_FAILED",
                "failed to clear setup status",
            ),
            (
                FACTORY_RESET_FAILED_CODE,
                "INTERNAL",
                "factory reset failed",
            ),
        ];
        for (engine_code, api_code, message) in cases {
            let api = map_factory_reset_engine_err(EngineError::new(
                engine_code,
                EngineErrorCategory::Internal,
                false,
            ));
            assert_eq!(api.status, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(api.code, api_code);
            assert_eq!(api.message, message);
            assert!(api.details.is_none());
        }
    }
}

//! Shared encryption state, lock, and secure-storage access operations.

use crate::error_codes::*;

use tracing::error;
use uc_application::facade::AppFacade;
use uc_application::receive_reconciliation::EnsureReceiveReadyPort;

use crate::{EncryptionStateSummary, EngineError, EngineErrorCategory, OperationResult};

pub async fn execute_query_encryption_state(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let state = facade.encryption.state().await.map_err(|error| {
        error!(error = %error, "query encryption state failed");
        stable_internal_error(QUERY_ENCRYPTION_STATE_FAILED_CODE)
    })?;

    Ok(OperationResult::EncryptionState(EncryptionStateSummary {
        initialized: state.initialized,
        session_ready: state.session_ready,
    }))
}

pub async fn execute_lock_encryption(
    facade: &AppFacade,
    receive_readiness: &dyn EnsureReceiveReadyPort,
) -> Result<OperationResult, EngineError> {
    facade.encryption.lock().await.map_err(|error| {
        error!(error = %error, "lock encryption session failed");
        stable_internal_error(LOCK_ENCRYPTION_FAILED_CODE)
    })?;
    receive_readiness.close_receive_gate();

    Ok(OperationResult::EncryptionLocked)
}

pub async fn execute_verify_secure_storage_access(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let granted = facade
        .encryption
        .verify_keychain_access()
        .await
        .map_err(|error| {
            error!(error = %error, "verify secure storage access failed");
            stable_internal_error(VERIFY_SECURE_STORAGE_ACCESS_FAILED_CODE)
        })?;

    Ok(OperationResult::SecureStorageAccess { granted })
}

fn stable_internal_error(code: u32) -> EngineError {
    EngineError::new(code, EngineErrorCategory::Internal, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encryption_operations_keep_distinct_stable_codes() {
        assert_ne!(
            QUERY_ENCRYPTION_STATE_FAILED_CODE,
            LOCK_ENCRYPTION_FAILED_CODE
        );
        assert_ne!(
            LOCK_ENCRYPTION_FAILED_CODE,
            VERIFY_SECURE_STORAGE_ACCESS_FAILED_CODE
        );
        assert_ne!(
            QUERY_ENCRYPTION_STATE_FAILED_CODE,
            VERIFY_SECURE_STORAGE_ACCESS_FAILED_CODE
        );
    }
}

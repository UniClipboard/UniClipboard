//! Shared passphrase unlock implementation.
//!
//! The daemon uses this internal seam only while its remaining callers migrate
//! to `Engine`. Do not re-export it from the crate root.

use tracing::error;
use uc_application::facade::{
    AppFacade, UnlockSpaceError, UnlockSpaceInput as AppUnlockSpaceInput,
};
use uc_application::receive_reconciliation::EnsureReceiveReadyPort;

use crate::{EngineError, EngineErrorCategory, OperationResult, UnlockSpaceInput};

pub const UNLOCK_SPACE_SETUP_NOT_COMPLETED_CODE: u32 = 1211;
pub const UNLOCK_SPACE_UNAUTHORIZED_CODE: u32 = 1212;
pub const UNLOCK_SPACE_FAILED_CODE: u32 = 1213;
pub const UNLOCK_SPACE_NOT_INITIALIZED_CODE: u32 = 1215;
pub const UNLOCK_SPACE_CORRUPTED_CODE: u32 = 1216;

pub async fn execute_unlock_space(
    facade: &AppFacade,
    receive_readiness: &dyn EnsureReceiveReadyPort,
    input: UnlockSpaceInput,
) -> Result<OperationResult, EngineError> {
    let result = facade
        .unlock_space(AppUnlockSpaceInput {
            passphrase: input.passphrase.expose().to_owned(),
        })
        .await
        .map_err(map_unlock_space_error)?;
    facade.search.on_session_ready().await;
    receive_readiness
        .ensure_receive_ready()
        .await
        .map_err(|error| {
            error!(error = %error, "receive recovery failed after space unlock");
            EngineError::new(
                UNLOCK_SPACE_FAILED_CODE,
                EngineErrorCategory::Unavailable,
                true,
            )
        })?;

    Ok(OperationResult::SpaceUnlocked {
        space_id: result.space_id.as_ref().to_string(),
    })
}

fn map_unlock_space_error(error: UnlockSpaceError) -> EngineError {
    match error {
        UnlockSpaceError::SetupNotCompleted => EngineError::new(
            UNLOCK_SPACE_SETUP_NOT_COMPLETED_CODE,
            EngineErrorCategory::InvalidState,
            false,
        ),
        UnlockSpaceError::SpaceNotInitialized => EngineError::new(
            UNLOCK_SPACE_NOT_INITIALIZED_CODE,
            EngineErrorCategory::InvalidState,
            false,
        ),
        UnlockSpaceError::WrongPassphrase => EngineError::new(
            UNLOCK_SPACE_UNAUTHORIZED_CODE,
            EngineErrorCategory::Unauthorized,
            false,
        ),
        UnlockSpaceError::CorruptedKeyMaterial => EngineError::new(
            UNLOCK_SPACE_CORRUPTED_CODE,
            EngineErrorCategory::Internal,
            false,
        ),
        UnlockSpaceError::Internal(_) => {
            error!(error = %error, "unlock space failed");
            EngineError::new(
                UNLOCK_SPACE_FAILED_CODE,
                EngineErrorCategory::Internal,
                false,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlock_state_failures_keep_distinct_stable_codes() {
        let setup_incomplete = map_unlock_space_error(UnlockSpaceError::SetupNotCompleted);
        let space_missing = map_unlock_space_error(UnlockSpaceError::SpaceNotInitialized);

        assert_eq!(
            setup_incomplete.category(),
            EngineErrorCategory::InvalidState
        );
        assert_eq!(space_missing.category(), EngineErrorCategory::InvalidState);
        assert_ne!(setup_incomplete.code(), space_missing.code());
    }

    #[test]
    fn corrupted_unlock_material_is_distinct_from_internal_failure() {
        let corrupted = map_unlock_space_error(UnlockSpaceError::CorruptedKeyMaterial);
        let internal =
            map_unlock_space_error(UnlockSpaceError::Internal("migration failed".into()));

        assert_ne!(corrupted.code(), internal.code());
    }
}

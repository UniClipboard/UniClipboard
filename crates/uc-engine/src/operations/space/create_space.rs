//! Shared create-space implementation.
//!
//! The daemon uses this internal seam only while its remaining callers migrate
//! to `Engine`. Do not re-export it from the crate root.

use crate::error_codes::*;

use tracing::error;
use uc_application::facade::{
    AppFacade, InitializeSpaceError, InitializeSpaceInput as AppInitializeSpaceInput,
};
use uc_application::receive_reconciliation::EnsureReceiveReadyPort;

use crate::{CreateSpaceInput, EngineError, EngineErrorCategory, OperationResult};

pub async fn execute_create_space(
    facade: &AppFacade,
    receive_readiness: &dyn EnsureReceiveReadyPort,
    input: CreateSpaceInput,
) -> Result<OperationResult, EngineError> {
    let result = facade
        .initialize_space(AppInitializeSpaceInput {
            passphrase: input.passphrase.expose().to_owned(),
            passphrase_confirm: input.passphrase_confirmation.expose().to_owned(),
            device_name: input.device_name,
        })
        .await
        .map_err(map_create_space_error)?;

    receive_readiness
        .ensure_receive_ready()
        .await
        .map_err(|error| {
            error!(error = %error, "receive recovery failed after space creation");
            EngineError::new(
                CREATE_SPACE_FAILED_CODE,
                EngineErrorCategory::Unavailable,
                true,
            )
        })?;

    Ok(OperationResult::SpaceCreated {
        space_id: result.space_id.as_ref().to_string(),
        self_device_id: result.self_device_id.to_string(),
        identity_fingerprint: result.fingerprint.as_display().to_string(),
    })
}

fn map_create_space_error(error: InitializeSpaceError) -> EngineError {
    match error {
        InitializeSpaceError::PassphraseMismatch => EngineError::new(
            CREATE_SPACE_PASSPHRASE_MISMATCH_CODE,
            EngineErrorCategory::InvalidInput,
            false,
        ),
        InitializeSpaceError::DeviceNameRequired => EngineError::new(
            CREATE_SPACE_DEVICE_NAME_REQUIRED_CODE,
            EngineErrorCategory::InvalidInput,
            false,
        ),
        InitializeSpaceError::AlreadyInitialized => EngineError::new(
            CREATE_SPACE_ALREADY_INITIALIZED_CODE,
            EngineErrorCategory::Conflict,
            false,
        ),
        InitializeSpaceError::AlreadySetup => EngineError::new(
            CREATE_SPACE_ALREADY_SETUP_CODE,
            EngineErrorCategory::Conflict,
            false,
        ),
        InitializeSpaceError::StorageFailed(_) | InitializeSpaceError::Internal(_) => {
            error!(error = %error, "create space failed");
            EngineError::new(
                CREATE_SPACE_FAILED_CODE,
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
    fn input_and_conflict_failures_keep_distinct_stable_codes() {
        let passphrase = map_create_space_error(InitializeSpaceError::PassphraseMismatch);
        let device_name = map_create_space_error(InitializeSpaceError::DeviceNameRequired);
        let initialized = map_create_space_error(InitializeSpaceError::AlreadyInitialized);
        let setup = map_create_space_error(InitializeSpaceError::AlreadySetup);

        assert_ne!(passphrase.code(), device_name.code());
        assert_ne!(initialized.code(), setup.code());
    }
}

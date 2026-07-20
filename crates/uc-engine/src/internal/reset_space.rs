//! Shared space reset implementation.

use tracing::error;
use uc_application::facade::{AppFacade, ResetSpaceError};

use crate::{EngineError, EngineErrorCategory, OperationResult};

pub const RESET_SPACE_UNAVAILABLE_CODE: u32 = 1311;
pub const RESET_SPACE_FAILED_CODE: u32 = 1312;

pub async fn execute_reset_space(facade: &AppFacade) -> Result<OperationResult, EngineError> {
    let setup = facade.space_setup.get().ok_or_else(|| {
        EngineError::new(
            RESET_SPACE_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        )
    })?;
    setup.reset().await.map_err(|error| match error {
        ResetSpaceError::StorageFailed(_) | ResetSpaceError::Internal(_) => {
            error!(error = %error, "reset space failed");
            EngineError::new(
                RESET_SPACE_FAILED_CODE,
                EngineErrorCategory::Internal,
                false,
            )
        }
    })?;
    Ok(OperationResult::SpaceReset)
}

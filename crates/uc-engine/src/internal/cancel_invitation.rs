//! Shared invitation cancellation implementation.

use tracing::error;
use uc_application::facade::{AppFacade, CancelInvitationError};

use crate::{EngineError, EngineErrorCategory, OperationResult};

pub const CANCEL_INVITATION_NOT_ISSUED_CODE: u32 = 1301;
pub const CANCEL_INVITATION_FAILED_CODE: u32 = 1302;
pub const CANCEL_INVITATION_UNAVAILABLE_CODE: u32 = 1303;

pub async fn execute_cancel_invitation(facade: &AppFacade) -> Result<OperationResult, EngineError> {
    let setup = facade.space_setup.get().ok_or_else(|| {
        EngineError::new(
            CANCEL_INVITATION_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        )
    })?;
    setup
        .cancel_invitation()
        .await
        .map_err(|error| match error {
            CancelInvitationError::NotIssued => EngineError::new(
                CANCEL_INVITATION_NOT_ISSUED_CODE,
                EngineErrorCategory::Conflict,
                false,
            ),
            CancelInvitationError::Internal(_) => {
                error!(error = %error, "cancel invitation failed");
                EngineError::new(
                    CANCEL_INVITATION_FAILED_CODE,
                    EngineErrorCategory::Internal,
                    false,
                )
            }
        })?;
    Ok(OperationResult::InvitationCancelled)
}

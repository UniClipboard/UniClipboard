//! Shared pairing invitation implementation.
//!
//! The daemon uses this internal seam only while its remaining callers migrate
//! to `Engine`. Do not re-export it from the crate root.

use tracing::error;
use uc_application::facade::{AppFacade, IssuePairingInvitationError};

use crate::{EngineError, EngineErrorCategory, OperationResult};

const INVITATION_INVALID_STATE_CODE: u32 = 1221;
const INVITATION_INVALID_INPUT_CODE: u32 = 1222;
const INVITATION_UNAVAILABLE_CODE: u32 = 1223;
const INVITATION_FAILED_CODE: u32 = 1224;

pub async fn execute_issue_invitation(facade: &AppFacade) -> Result<OperationResult, EngineError> {
    let invitation = facade
        .issue_pairing_invitation()
        .await
        .map_err(map_issue_invitation_error)?;
    Ok(OperationResult::InvitationIssued {
        invitation_code: invitation.code.as_str().to_string(),
        expires_at_ms: invitation.expires_at.timestamp_millis(),
    })
}

fn map_issue_invitation_error(error: IssuePairingInvitationError) -> EngineError {
    match error {
        IssuePairingInvitationError::NetworkNotStarted => EngineError::new(
            INVITATION_INVALID_STATE_CODE,
            EngineErrorCategory::InvalidState,
            true,
        ),
        IssuePairingInvitationError::AddressNotAvailable(_) => EngineError::new(
            INVITATION_INVALID_INPUT_CODE,
            EngineErrorCategory::InvalidInput,
            false,
        ),
        IssuePairingInvitationError::ServiceUnavailable => EngineError::new(
            INVITATION_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        ),
        IssuePairingInvitationError::Internal(_) => {
            error!(error = %error, "issue invitation failed");
            EngineError::new(INVITATION_FAILED_CODE, EngineErrorCategory::Internal, false)
        }
    }
}

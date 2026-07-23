//! Shared setup-state query implementation.

use crate::error_codes::*;

use tracing::error;
use uc_application::facade::{AppFacade, QuerySetupStateError};

use crate::{
    EngineError, EngineErrorCategory, OperationResult, SetupInvitationSummary, SetupStateSummary,
};

pub async fn execute_query_setup_state(facade: &AppFacade) -> Result<OperationResult, EngineError> {
    let setup = facade.space_setup.get().ok_or_else(|| {
        EngineError::new(
            QUERY_SETUP_STATE_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        )
    })?;
    let state = setup
        .query_setup_state()
        .await
        .map_err(|error| match error {
            QuerySetupStateError::StorageFailed(_) | QuerySetupStateError::Internal(_) => {
                error!(error = %error, "query setup state failed");
                EngineError::new(
                    QUERY_SETUP_STATE_FAILED_CODE,
                    EngineErrorCategory::Internal,
                    false,
                )
            }
        })?;

    Ok(OperationResult::SetupState(SetupStateSummary {
        has_completed: state.has_completed,
        current_invitation: state
            .current_invitation
            .map(|invitation| SetupInvitationSummary {
                invitation_code: invitation.code.as_str().to_string(),
                expires_at_ms: invitation.expires_at.timestamp_millis(),
            }),
        device_name: state.device_name,
    }))
}

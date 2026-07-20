//! Shared session recovery implementation.
//!
//! The daemon uses this internal seam only while its remaining callers migrate
//! to `Engine`. Do not re-export it from the crate root.

use tracing::{error, warn};
use uc_application::facade::AppFacade;
use uc_application::receive_reconciliation::EnsureReceiveReadyPort;

use crate::{EngineError, EngineErrorCategory, OperationResult, RecoverSessionInput};

pub const RECOVER_SESSION_UNAVAILABLE_CODE: u32 = 1214;
pub const RECOVER_SESSION_RECEIVE_UNAVAILABLE_CODE: u32 = 1217;

pub async fn execute_recover_session(
    facade: &AppFacade,
    receive_readiness: &dyn EnsureReceiveReadyPort,
    input: RecoverSessionInput,
) -> Result<OperationResult, EngineError> {
    if !input.allow_secure_storage_unlock {
        return Ok(OperationResult::SessionRecovered {
            unlocked: false,
            resumed: false,
        });
    }

    let unlocked = facade
        .encryption
        .unlock()
        .await
        .map_err(|error| recover_session_error("unlock encryption session", error))?;
    if !unlocked {
        return Ok(OperationResult::SessionRecovered {
            unlocked: false,
            resumed: false,
        });
    }

    let resumed = facade
        .try_resume_session()
        .await
        .map_err(|error| recover_session_error("resume space session", error))?;
    if resumed {
        if let Err(error) = facade.refresh_presence().await {
            warn!(error = %error, "presence refresh failed after session recovery");
        }
    }
    facade.search.on_session_ready().await;

    receive_readiness
        .ensure_receive_ready()
        .await
        .map_err(|error| {
            error!(error = %error, "receive recovery failed after space access");
            EngineError::new(
                RECOVER_SESSION_RECEIVE_UNAVAILABLE_CODE,
                EngineErrorCategory::Unavailable,
                true,
            )
        })?;

    Ok(OperationResult::SessionRecovered {
        unlocked: true,
        resumed,
    })
}

fn recover_session_error(context: &'static str, error: impl std::fmt::Display) -> EngineError {
    error!(context, error = %error, "engine session recovery failed");
    EngineError::new(
        RECOVER_SESSION_UNAVAILABLE_CODE,
        EngineErrorCategory::Unavailable,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receive_recovery_failure_has_a_distinct_stable_code() {
        assert_ne!(
            RECOVER_SESSION_UNAVAILABLE_CODE,
            RECOVER_SESSION_RECEIVE_UNAVAILABLE_CODE
        );
    }
}

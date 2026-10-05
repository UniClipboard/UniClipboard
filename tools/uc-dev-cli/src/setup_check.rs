//! Query setup through the daemon, the owner of the active Engine storage layout.

use std::time::Duration;

use anyhow::Result;
use uc_daemon_client::{DaemonClientContext, DaemonRequestError};
use uc_daemon_contract::constants::daemon_error_code;

const SETUP_READY_TIMEOUT: Duration = Duration::from_secs(10);
const SETUP_READY_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Call only after connecting to a compatible daemon; private vault files are
/// not a setup contract and may belong to an inactive storage generation.
pub async fn is_setup_complete() -> Result<bool> {
    let context = DaemonClientContext::from_env()?;
    Ok(context.setup_v2_client().get_state().await?.has_completed)
}

/// Wait for the Engine-backed setup service after the daemon transport becomes
/// healthy. Startup publishes the HTTP endpoint before profile recovery has
/// necessarily made every business service available.
pub async fn wait_for_setup_complete() -> Result<bool> {
    let deadline = tokio::time::Instant::now() + SETUP_READY_TIMEOUT;
    loop {
        match is_setup_complete().await {
            Ok(complete) => return Ok(complete),
            Err(error)
                if is_runtime_unavailable(&error) && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(SETUP_READY_POLL_INTERVAL).await;
            }
            Err(error) => return Err(error),
        }
    }
}

fn is_runtime_unavailable(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<DaemonRequestError>()
        .and_then(DaemonRequestError::code)
        == Some(daemon_error_code::RUNTIME_UNAVAILABLE)
}

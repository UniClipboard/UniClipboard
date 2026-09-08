//! Query setup through the daemon, the owner of the active Engine storage layout.

use anyhow::Result;
use uc_daemon_client::DaemonClientContext;

/// Call only after connecting to a compatible daemon; private vault files are
/// not a setup contract and may belong to an inactive storage generation.
pub async fn is_setup_complete() -> Result<bool> {
    let context = DaemonClientContext::from_env()?;
    Ok(context.setup_v2_client().get_state().await?.has_completed)
}

//! Desktop startup coordination: progress, readiness, and cold-launch actions.

mod actions;
mod progress;

pub use actions::{
    run_cold_launch_actions, wait_for_daemon_connection, wait_for_encryption_session_ready,
    ColdLaunchOutcome, StartupWindowAction, DEFAULT_READY_POLL, DEFAULT_READY_TIMEOUT,
};
pub use progress::read_startup_status;
pub(crate) use progress::wait_for_ready;

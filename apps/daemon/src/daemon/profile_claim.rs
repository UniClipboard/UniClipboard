//! Claiming the profile before anything else is opened.
//!
//! The instance lock is the first thing a daemon takes. A daemon that loses it
//! must not install tracing, because the Engine diagnostic sink opens the shared
//! `engine.*.jsonl` files: a waiting daemon would interleave its records with
//! the running one's. Failures at this stage therefore cannot reach the log
//! files either, and are recorded as the bounded startup failure instead.

use std::path::Path;
use std::time::Duration;

use uc_daemon_local::instance_lock::{self, DaemonInstanceLock, InstanceLockError};
use uc_observability::startup_failure::{
    write_startup_failure, StartupFailure, StartupFailureClass, StartupStep,
};

/// Record that `step` of this start failed with `class`.
///
/// Best effort by necessity: tracing is not installed yet, so a record that
/// cannot be written has nowhere to be reported, and the start failure being
/// recorded is returned to the caller either way.
pub(super) fn record_startup_failure(
    logs_dir: &Path,
    step: StartupStep,
    class: StartupFailureClass,
) {
    if let Err(error) = write_startup_failure(logs_dir, &StartupFailure::now(step, class)) {
        tracing::warn!(error_kind = ?error.kind(), "could not record the startup failure");
    }
}

/// Take the profile's instance lock, waiting at most `deadline` for a holder
/// to release it. On failure the startup failure record is written to
/// `logs_dir` and the original error is returned.
pub async fn claim_instance_lock(
    app_data_root: &Path,
    logs_dir: &Path,
    deadline: Duration,
) -> anyhow::Result<DaemonInstanceLock> {
    instance_lock::acquire_with_deadline(app_data_root, deadline)
        .await
        .map_err(|error| {
            let class = match &error {
                InstanceLockError::AlreadyRunning { .. } => StartupFailureClass::LockTimeout,
                InstanceLockError::Io(_) => StartupFailureClass::Io,
            };
            record_startup_failure(logs_dir, StartupStep::InstanceLock, class);
            anyhow::Error::new(error)
        })
}

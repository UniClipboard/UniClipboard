//! Daemon host entry points — re-exported from [`uc_daemon`] (ADR-008 P2).
//!
//! The implementation migrated to `uc-daemon::daemon::host` in Slice 2b.
//! This module preserves `uc_desktop::daemon::host::*` import paths for
//! backward compatibility.

pub use uc_daemon::daemon::host::{
    run, run_standalone_from_env, start_in_process, ProcessRuntimeHandles, RUN_MODE_ENV,
    RUN_MODE_SERVER,
};

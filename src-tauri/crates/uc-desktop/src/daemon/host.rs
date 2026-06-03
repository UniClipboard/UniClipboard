//! Daemon host entry points — re-exported from [`uc_daemon`] (ADR-008 P2).
//!
//! The implementation migrated to `uc-daemon::daemon::host` in Slice 2b.
//! This module preserves `uc_desktop::daemon::host::*` import paths for
//! backward compatibility.

// ADR-008 P3-3 (B2'-3): `start_in_process` + `ProcessRuntimeHandles` re-exports
// dropped — the GUI no longer runs an in-process daemon. Consumers of the daemon
// body reach these via `uc_daemon::daemon::host` directly.
pub use uc_daemon::daemon::host::{run, run_standalone_from_env, RUN_MODE_ENV, RUN_MODE_SERVER};

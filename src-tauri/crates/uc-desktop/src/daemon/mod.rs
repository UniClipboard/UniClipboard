//! Daemon host glue — re-exports from [`uc_daemon`] (ADR-008 P2).
//!
//! GUI-agnostic runtime has fully migrated to `uc-daemon`; this module
//! preserves the `uc_desktop::daemon::*` public API surface via re-exports.

pub mod host;

// ── re-exports from uc-daemon (preserve uc_desktop::daemon::* public surface) ──
pub use uc_daemon::daemon::run_mode;
pub use uc_daemon::{DaemonHandle, DaemonOwnership};

pub use host::{run, run_standalone_from_env, RUN_MODE_ENV, RUN_MODE_SERVER};

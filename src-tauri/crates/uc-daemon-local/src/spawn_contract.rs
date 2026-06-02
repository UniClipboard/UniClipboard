//! Spawn contract between CLI and daemon binary.
//!
//! The CLI `start [--server]` sets these environment variables before
//! detached-spawning the daemon binary. The daemon reads them at startup
//! to resolve its run mode.

/// Environment variable carrying the daemon run mode from the CLI spawner
/// to the daemon binary.
pub const RUN_MODE_ENV: &str = "UC_DAEMON_RUN_MODE";

/// Value of [`RUN_MODE_ENV`] that selects headless server mode.
pub const RUN_MODE_SERVER: &str = "server";

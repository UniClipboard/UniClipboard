//! Shared daemon transport contracts.

/// Local client <-> daemon control-plane revision. Bump by one whenever a change
/// is not compatible with an already running daemon; clients replace a daemon
/// that reports a different value.
pub const DAEMON_API_REVISION: &str = "1";

pub mod api;
pub mod constants;
pub mod probe;
pub mod startup;

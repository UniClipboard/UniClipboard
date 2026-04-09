//! Daemon transport auth contracts.

/// Connection details for loopback daemon clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConnectionInfo {
    pub base_url: String,
    pub ws_url: String,
    /// Raw bearer token (used only to exchange for session JWT).
    pub token: String,
    /// PID of the client process (used for daemon JWT PID whitelist verification).
    pub pid: u32,
}

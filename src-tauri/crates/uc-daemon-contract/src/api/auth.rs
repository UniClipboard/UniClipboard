//! Daemon transport auth contracts.

use std::fmt;

/// Connection details for loopback daemon clients.
#[derive(Clone, PartialEq, Eq)]
pub struct DaemonConnectionInfo {
    pub base_url: String,
    pub ws_url: String,
    /// Raw bearer token (used only to exchange for session JWT).
    pub token: String,
    /// PID of the client process (used for daemon JWT PID whitelist verification).
    pub pid: u32,
}

impl fmt::Debug for DaemonConnectionInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DaemonConnectionInfo")
            .field("base_url", &self.base_url)
            .field("ws_url", &self.ws_url)
            .field("token", &"<redacted>")
            .field("pid", &self.pid)
            .finish()
    }
}
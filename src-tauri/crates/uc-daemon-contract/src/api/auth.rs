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
    /// Formats a `DaemonConnectionInfo` for `Debug`, redacting the `token` field.
    ///
    /// The debug output includes `base_url`, `ws_url`, and `pid` while replacing the
    /// `token` value with the literal `"<redacted>"`.
    ///
    /// # Examples
    ///
    /// ```
    /// let info = DaemonConnectionInfo {
    ///     base_url: "http://localhost".into(),
    ///     ws_url: "ws://localhost".into(),
    ///     token: "secret-token".into(),
    ///     pid: 1234,
    /// };
    /// let s = format!("{:?}", info);
    /// assert!(s.contains("base_url"));
    /// assert!(s.contains("ws_url"));
    /// assert!(s.contains("pid"));
    /// assert!(s.contains("<redacted>"));
    /// ```
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DaemonConnectionInfo")
            .field("base_url", &self.base_url)
            .field("ws_url", &self.ws_url)
            .field("token", &"<redacted>")
            .field("pid", &self.pid)
            .finish()
    }
}

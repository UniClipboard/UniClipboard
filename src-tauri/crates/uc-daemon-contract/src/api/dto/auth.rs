//! DTOs for `POST /auth/connect` (ADR-008 §C.6).
//!
//! Deduplicates the three inline copies of the connect request/response shape
//! (webserver `security/connect.rs`, webserver dev token, native
//! `uc-daemon-client/src/http/mod.rs`). The wire field names mirror the exact
//! current flat connect-response shape; consumers are migrated to these types in
//! a later phase.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Request body for `POST /auth/connect`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectRequest {
    /// Client process ID. Used for PID whitelist verification in JWT middleware.
    pub pid: u32,
    /// Client type: `"gui"`, `"cli"`, or `"other"`.
    pub client_type: String,
}

/// Response body for `POST /auth/connect`.
///
/// Mirrors the current flat shape returned by the webserver `ConnectResponse`
/// and decoded by the native client's local `ConnectResponse`:
/// `{ "sessionToken", "expiresInSecs", "refreshAtSecs" }`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokenResponse {
    /// HS256-signed JWT session token.
    pub session_token: String,
    /// Token time-to-live in seconds.
    pub expires_in_secs: i64,
    /// Recommended refresh time in seconds.
    pub refresh_at_secs: i64,
}

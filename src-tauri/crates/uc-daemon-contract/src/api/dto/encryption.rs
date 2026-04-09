use serde::Serialize;
use utoipa::ToSchema;

/// Response payload for GET /encryption/state.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionStateResponse {
    pub initialized: bool,
    pub session_ready: bool,
}

/// Internal event payload for the encryption.session_ready WS event.
/// Serialized as part of DaemonWsEvent payload.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionSessionReadyPayload {
    pub ts: i64,
}

/// Response payload for GET /encryption/keychain-access.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeychainAccessResponse {
    /// Whether Keychain access is granted (Always Allow permission).
    pub granted: bool,
}

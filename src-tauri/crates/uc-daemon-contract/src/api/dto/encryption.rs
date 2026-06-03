use serde::{Deserialize, Serialize};
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

/// Shared response payload for `POST /encryption/unlock` and
/// `POST /encryption/lock`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionActionResponse {
    pub success: bool,
}

/// Request body for `POST /encryption/unlock-with-passphrase` (ADR-008 D15).
///
/// Carries the user's plaintext passphrase over the loopback API. Per D14 the
/// endpoint is session-JWT gated (not in `PUBLIC_PATHS`) and the handler MUST
/// never log this body — see the rule in `uc-webserver` `api/encryption.rs`.
/// This formally retires the historical "passphrase 不出进程" invariant: under
/// the "same UID = trusted" model (D14) an attacker who can sniff loopback can
/// already dump the master key from daemon memory, so loopback transport adds
/// zero incremental exposure.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnlockSpaceRequest {
    pub passphrase: String,
}

/// Response payload for `POST /encryption/unlock-with-passphrase`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnlockSpaceResponse {
    pub space_id: String,
}

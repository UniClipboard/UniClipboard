//! DTOs for the setup API endpoints.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Debug;
use utoipa::ToSchema;

/// Inner setup state type returned by the query service.
/// Exposed as `SetupStateResponse` in the `types.rs` module for internal use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStateResponse {
    pub state: Value,
    pub session_id: Option<String>,
    pub next_step_hint: String,
    pub profile: String,
    pub clipboard_mode: String,
    pub device_name: String,
    pub peer_id: String,
    pub selected_peer_id: Option<String>,
    pub selected_peer_name: Option<String>,
    pub has_completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupSelectPeerRequest {
    pub peer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupSubmitPassphraseRequest {
    pub passphrase: String,
}

/// Response wrapper for setup state read endpoints.
/// Includes a server-side timestamp for frontend cache invalidation.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetSetupStateResponse {
    pub data: SetupStateResponseDto,
    pub ts: i64,
}

/// Response wrapper for setup action endpoints (host, join, select_peer, etc.).
/// Includes a server-side timestamp for frontend cache invalidation.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupActionResponse {
    pub data: SetupStateResponseDto,
    pub ts: i64,
}

/// Inner setup state returned by all setup endpoints.
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupStateResponseDto {
    pub state: Value,
    pub session_id: Option<String>,
    pub next_step_hint: String,
    pub profile: String,
    pub clipboard_mode: String,
    pub device_name: String,
    pub peer_id: String,
    pub selected_peer_id: Option<String>,
    pub selected_peer_name: Option<String>,
    pub has_completed: bool,
}

impl Debug for SetupStateResponseDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let variant = match &self.state {
            Value::String(s) => s.as_str(),
            Value::Object(map) if map.len() == 1 => {
                map.keys().next().map(String::as_str).unwrap_or("<none>")
            }
            _ => "<complex>",
        };
        f.debug_struct("SetupStateResponseDto")
            .field("hint", &self.next_step_hint)
            .field(
                "sid",
                &self.session_id.as_deref().map(|s| &s[..8.min(s.len())]),
            )
            .field("done", &self.has_completed)
            .field("variant", &variant)
            .finish()
    }
}

impl From<SetupStateResponse> for SetupStateResponseDto {
    fn from(value: SetupStateResponse) -> Self {
        Self {
            state: value.state,
            session_id: value.session_id,
            next_step_hint: value.next_step_hint,
            profile: value.profile,
            clipboard_mode: value.clipboard_mode,
            device_name: value.device_name,
            peer_id: value.peer_id,
            selected_peer_id: value.selected_peer_id,
            selected_peer_name: value.selected_peer_name,
            has_completed: value.has_completed,
        }
    }
}

impl From<SetupStateResponseDto> for SetupStateResponse {
    fn from(value: SetupStateResponseDto) -> Self {
        Self {
            state: value.state,
            session_id: value.session_id,
            next_step_hint: value.next_step_hint,
            profile: value.profile,
            clipboard_mode: value.clipboard_mode,
            device_name: value.device_name,
            peer_id: value.peer_id,
            selected_peer_id: value.selected_peer_id,
            selected_peer_name: value.selected_peer_name,
            has_completed: value.has_completed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupResetResponse {
    pub profile: String,
    pub daemon_kept_running: bool,
}

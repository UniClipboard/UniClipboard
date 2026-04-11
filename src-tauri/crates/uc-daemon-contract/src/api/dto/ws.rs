//! WebSocket DTOs for the daemon HTTP API.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use uc_core::network::daemon_api_strings::ws_topic;

/// Request body sent by a client to subscribe to daemon event topics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WsSubscribeRequest {
    pub action: String,
    pub topics: Vec<String>,
}

/// Acknowledgement sent back to the client after a successful subscribe action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WsSubscribeResponse {
    /// The action that was processed (e.g. `"subscribe"`).
    pub action: String,
    /// Topics that were accepted and subscribed.
    pub topics: Vec<String>,
    /// Server-side timestamp (ms since epoch).
    pub ts: i64,
}

/// Error response sent via HTTP status + JSON body when the WebSocket upgrade fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WsErrorResponse {
    pub error: String,
    /// Only present for rate-limit errors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u32>,
}

/// All topics supported by the daemon WebSocket server.
/// Used by clients and OpenAPI documentation.
pub const WS_SUPPORTED_TOPICS: &[&str] = &[
    ws_topic::STATUS,
    ws_topic::PEERS,
    ws_topic::PAIRED_DEVICES,
    ws_topic::PAIRING,
    ws_topic::SETUP,
    ws_topic::SPACE_ACCESS,
    ws_topic::CLIPBOARD,
    ws_topic::FILE_TRANSFER,
    ws_topic::ENCRYPTION,
    ws_topic::SEARCH,
];

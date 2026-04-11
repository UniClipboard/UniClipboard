//! HTTP and WebSocket DTOs for the daemon transport layer.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uc_core::ports::transfer_progress::TransferDirection;
use uc_core::security::space_access::state::SpaceAccessState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
    pub package_version: String,
    pub api_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub package_version: String,
    pub api_revision: String,
    pub uptime_seconds: u64,
    pub workers: Vec<WorkerStatusDto>,
    pub connected_peers: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerStatusDto {
    pub name: String,
    pub health: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerSnapshotDto {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub addresses: Vec<String>,
    pub is_paired: bool,
    pub connected: bool,
    pub pairing_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDeviceDto {
    pub peer_id: String,
    pub device_name: String,
    pub pairing_state: String,
    pub last_seen_at_ms: Option<i64>,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupActionAckResponse {
    pub state: Value,
    pub session_id: Option<String>,
    pub next_step_hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStateChangedPayload {
    pub session_id: Option<String>,
    pub state: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupSpaceAccessCompletedPayload {
    pub session_id: String,
    pub peer_id: String,
    pub success: bool,
    pub reason: Option<String>,
    pub ts: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTransferProgressPayload {
    pub transfer_id: String,
    pub entry_id: Option<String>,
    pub peer_id: String,
    pub direction: TransferDirection,
    pub chunks_completed: u32,
    pub total_chunks: u32,
    pub bytes_transferred: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingSessionChangedPayload {
    pub session_id: String,
    pub state: String,
    pub stage: String,
    pub peer_id: Option<String>,
    pub device_name: Option<String>,
    pub updated_at_ms: i64,
    pub ts: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingVerificationPayload {
    pub session_id: String,
    pub kind: String,
    pub peer_id: Option<String>,
    pub device_name: Option<String>,
    pub code: Option<String>,
    pub error: Option<String>,
    pub local_fingerprint: Option<String>,
    pub peer_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingFailurePayload {
    pub session_id: String,
    pub peer_id: Option<String>,
    pub error: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerChangedPayload {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub addresses: Vec<String>,
    pub discovered: bool,
    pub connected: bool,
}

/// Full-snapshot payload for `peers.changed` events.
/// Carries the complete current peer list so the frontend can replace its state atomically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeersChangedFullPayload {
    pub peers: Vec<PeerSnapshotDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerNameUpdatedPayload {
    pub peer_id: String,
    pub device_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerConnectionChangedPayload {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedDevicesChangedPayload {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceAccessStateChangedPayload {
    pub state: SpaceAccessState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceAccessStateResponse {
    pub state: SpaceAccessState,
}

/// Response payload for GET /lifecycle/status.
/// Mirrors the frontend LifecycleStatusDto shape so the HTTP endpoint
/// can replace the Tauri get_lifecycle_status command without frontend type changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleStatusResponse {
    /// Current lifecycle state.
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonWsSubscribeRequest {
    pub action: String,
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonWsEvent {
    pub topic: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub session_id: Option<String>,
    pub ts: i64,
    pub payload: Value,
}

pub use crate::api::dto::device::LocalDeviceInfoDto;
pub use crate::api::dto::pairing::PairingSessionSummaryDto;

// Re-export setup DTOs for backward compatibility with internal consumers.
pub use crate::api::dto::setup::{
    GetSetupStateResponse, SetupActionResponse, SetupResetResponse, SetupSelectPeerRequest,
    SetupStateResponse, SetupStateResponseDto, SetupSubmitPassphraseRequest,
};

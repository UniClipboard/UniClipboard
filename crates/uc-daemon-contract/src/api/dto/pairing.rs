//! Pairing DTOs for the daemon HTTP API.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InitiatePairingRequest {
    pub peer_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VerifyPairingRequest {
    pub pin_matches: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PairingSessionCommandRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnpairDeviceRequest {
    pub peer_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetPairingDiscoverabilityRequest {
    pub client_kind: String,
    pub discoverable: bool,
    pub lease_ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetPairingParticipantRequest {
    pub client_kind: String,
    pub ready: bool,
    pub lease_ttl_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AckedPairingCommandResponse {
    pub session_id: String,
    pub accepted: bool,
    pub state: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InitiatePairingResponse {
    pub session_id: String,
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PairingApiErrorResponse {
    pub code: String,
    pub message: String,
}

/// Response DTO for GET /pairing/sessions/{session_id}.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PairingSessionSummaryDto {
    pub session_id: String,
    pub peer_id: Option<String>,
    pub device_name: Option<String>,
    pub state: String,
    pub updated_at_ms: i64,
}

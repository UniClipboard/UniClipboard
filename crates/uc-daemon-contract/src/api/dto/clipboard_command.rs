//! DTOs for clipboard command endpoints (ADR-008 P2.5 / D7).
//!
//! These types are shared between `uc-webserver` (server) and
//! `uc-daemon-client` (consumer). All response payloads use `camelCase`
//! field names to match frontend/CLI conventions.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ── POST /clipboard/dispatch ─────────────────────────────────────

/// Request body for `POST /clipboard/dispatch`.
///
/// The daemon wraps the text into a single `text/plain`
/// `SystemClipboardSnapshot` and fans it out to online peers.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchTextRequest {
    /// Plaintext to dispatch.
    pub text: String,
    /// Optional target device IDs. Empty or absent = full fan-out.
    #[serde(default)]
    pub peers: Option<Vec<String>>,
}

/// Per-target delivery outcome in the dispatch response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PerTargetOutcomeDto {
    pub device_id: String,
    /// `"accepted"` | `"duplicate"` | `"error"`.
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Response body for `POST /clipboard/dispatch`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchOutcomeResponse {
    pub snapshot_hash: String,
    pub at_ms: i64,
    pub total_accepted: usize,
    pub total_duplicate: usize,
    pub total_offline: usize,
    pub total_errored: usize,
    pub per_target: Vec<PerTargetOutcomeDto>,
}

// ── POST /clipboard/resend ───────────────────────────────────────

/// Request body for `POST /clipboard/resend`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResendRequest {
    /// ID of the previously captured entry to resend.
    pub entry_id: String,
    /// Optional target device IDs.
    #[serde(default)]
    pub peers: Option<Vec<String>>,
}

/// Response body for `POST /clipboard/resend`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResendResponse {
    pub accepted: usize,
    pub duplicate: usize,
    pub offline: usize,
    pub errored: usize,
    pub pending: usize,
}

// ── POST /clipboard/cancel-transfer/:transfer_id ─────────────────

/// Request body for `POST /clipboard/cancel-transfer/:transfer_id`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelTransferRequest {
    /// Cancellation reason: `"local_user"` | `"timeout"` etc.
    pub reason: String,
}

/// Response body for `POST /clipboard/cancel-transfer/:transfer_id`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelTransferResponse {
    /// `"cancelled"` | `"not_inflight"`.
    pub outcome: String,
}

// ── POST /clipboard/restore/:entry_id ────────────────────────────

/// Response body for `POST /clipboard/restore/:entry_id`.
///
/// Wrapped in `ApiEnvelope` per §0.1. The success body was previously the
/// ad-hoc `{ "success": true }` shape; consumers ignore it today.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestoreEntryResponse {
    pub success: bool,
}

// ── WS clipboard.inbound_notice payload ──────────────────────────

/// Payload for the `clipboard.inbound_notice` WebSocket event.
///
/// Carries the full V3 envelope as base64 so CLI `watch` can decode
/// content without an extra HTTP round-trip.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InboundNoticeEvent {
    pub from_device: String,
    pub snapshot_hash: String,
    /// Base64-encoded V3 envelope bytes.
    pub plaintext_base64: String,
    /// `"new_entry"` | `"duplicate_ignored"`.
    pub action: String,
    pub at_ms: i64,
}

// ── WS clipboard.new_content payload ─────────────────────────────

/// Payload for the `clipboard.new_content` WebSocket event (ADR-008 P5-1b).
///
/// Emitted after an inbound clipboard entry has been fully applied —
/// including free-file materialization — so the `entry_id` is the
/// **receiver-side** id and the materialized file (if any) is ready to be
/// fetched via `GET /clipboard/entries/{id}/file`. The same event also fires
/// for local clipboard captures; consumers that only want remote arrivals
/// must filter on `origin == "remote"`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InboundEntryEvent {
    pub entry_id: String,
    pub preview: String,
    /// `"remote"` for inbound peer pushes, otherwise a local-capture origin.
    pub origin: String,
    /// Sending device id for remote pushes; empty for local-capture events.
    /// `#[serde(default)]` keeps deserialization resilient if a future
    /// `new_content` emitter omits the field.
    #[serde(default)]
    pub from_device: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_request_round_trip() {
        let json = r#"{"text":"hello","peers":["device-1"]}"#;
        let req: DispatchTextRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.text, "hello");
        assert_eq!(req.peers.as_deref(), Some(&["device-1".to_string()][..]));
    }

    #[test]
    fn dispatch_request_without_peers() {
        let json = r#"{"text":"hello"}"#;
        let req: DispatchTextRequest = serde_json::from_str(json).unwrap();
        assert!(req.peers.is_none());
    }

    #[test]
    fn dispatch_outcome_response_camel_case() {
        let resp = DispatchOutcomeResponse {
            snapshot_hash: "abc".into(),
            at_ms: 1000,
            total_accepted: 1,
            total_duplicate: 0,
            total_offline: 0,
            total_errored: 0,
            per_target: vec![PerTargetOutcomeDto {
                device_id: "d1".into(),
                outcome: "accepted".into(),
                error: None,
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json.get("snapshotHash").is_some());
        assert!(json.get("atMs").is_some());
        assert!(json.get("totalAccepted").is_some());
        assert!(json.get("perTarget").is_some());
    }

    #[test]
    fn resend_request_round_trip() {
        let json = r#"{"entryId":"e1","peers":null}"#;
        let req: ResendRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.entry_id, "e1");
        assert!(req.peers.is_none());
    }

    #[test]
    fn inbound_entry_event_camel_case() {
        let json = r#"{"entryId":"e1","preview":"p","origin":"remote"}"#;
        let evt: InboundEntryEvent = serde_json::from_str(json).unwrap();
        assert_eq!(evt.entry_id, "e1");
        assert_eq!(evt.origin, "remote");
        let value = serde_json::to_value(&evt).unwrap();
        assert!(value.get("entryId").is_some());
    }

    #[test]
    fn inbound_notice_event_camel_case() {
        let evt = InboundNoticeEvent {
            from_device: "d1".into(),
            snapshot_hash: "h".into(),
            plaintext_base64: "base64data".into(),
            action: "new_entry".into(),
            at_ms: 123,
        };
        let json = serde_json::to_value(&evt).unwrap();
        assert!(json.get("fromDevice").is_some());
        assert!(json.get("snapshotHash").is_some());
        assert!(json.get("plaintextBase64").is_some());
    }
}

//! DTOs for clipboard command endpoints (ADR-008 P2.5 / D7).
//!
//! These types are shared between `uc-webserver` (server) and
//! `uc-daemon-client` (consumer). All response payloads use `camelCase`
//! field names to match frontend/CLI conventions.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::clipboard_outbound::ResendTargetProgressDto;

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

/// Request body for `POST /clipboard/dispatch-file`.
///
/// The path is consumed only by the authenticated local daemon and must never
/// be logged or persisted as clipboard metadata.
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchFileRequest {
    pub source_path: String,
    #[serde(default)]
    pub peers: Option<Vec<String>>,
}

/// Initial file dispatch result. Delivery may still be pending while peers
/// fetch the bytes; callers use `entry_id` to query the delivery view.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DispatchFileOutcomeResponse {
    pub entry_id: String,
    pub snapshot_hash: String,
    pub at_ms: i64,
    pub total_accepted: usize,
    pub total_duplicate: usize,
    pub total_offline: usize,
    pub total_errored: usize,
    pub total_pending: usize,
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
    /// Explicit user confirmation to start a new transfer when the existing
    /// task cannot be continued or adopted. Defaults to `false`: reconcile
    /// the existing task only, never re-download silently.
    #[serde(default)]
    pub confirm_new_transfer: bool,
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
    /// Per-target reconciliation of existing tasks. The counters above only
    /// count real deliveries; use this to tell reused, adopted and refused targets apart.
    #[serde(default)]
    pub reconciled: Vec<ResendTargetProgressDto>,
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

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelEntryReceiveRequest {
    pub attempt_id: String,
}

/// Response body for `POST /clipboard/entries/{id}/receive/cancel`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelEntryReceiveResponse {
    /// `"cancelled"` | `"cancellation_incomplete"` | `"not_receiving"` | `"too_late"` |
    /// `"already_terminal"` | `"superseded"`. Only `"cancelled"` confirms the
    /// receive settled; `"cancellation_incomplete"` means output or cleanup is
    /// still being released and the final state arrives later.
    pub outcome: String,
}

/// One selectable result of the current receive attempt.
///
/// `item_id` is opaque and bound to the attempt that was queried; callers pass
/// it back to the retry route unchanged and never build it from file names.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReceiveItemResponse {
    pub item_id: String,
    /// `"unknown"` | `"file"` | `"representation"` | `"directory_unit"`.
    pub kind: String,
    /// `"pending"` | `"transferring"` | `"completed"` | `"failed"` | `"cancelled"`.
    /// `"completed"` means the item was saved, not only downloaded.
    pub state: String,
    /// `"network_unavailable"` | `"timed_out"` | `"access_denied"` |
    /// `"storage_unavailable"` | `"integrity_check_failed"` | `"interrupted"` |
    /// `"unknown"`. Fixed categories only, never raw error text.
    #[serde(default)]
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryReceiveProgressResponse {
    pub entry_id: String,
    pub attempt_id: String,
    pub state: String,
    pub total_bytes: i64,
    /// Bytes of items that were saved; downloaded-but-unsaved bytes do not count.
    pub completed_bytes: i64,
    /// Member count of the attempt. A directory counts its members here but is
    /// one entry of `items`.
    pub items_total: u32,
    pub items_completed: u32,
    /// Per-item results. An aggregate `failed` or `cancelled` attempt can still
    /// contain `completed` items. Empty from daemons that predate per-item results.
    #[serde(default)]
    pub items: Vec<ReceiveItemResponse>,
}

/// Request body for `POST /clipboard/entries/{id}/receive/retry`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RetryEntryReceiveRequest {
    /// The attempt the caller is looking at; a newer attempt answers `superseded`.
    pub expected_attempt_id: String,
    /// Item ids taken from the latest receive query. Must not be empty.
    pub item_ids: Vec<String>,
}

/// Response body for `POST /clipboard/entries/{id}/receive/retry`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RetryEntryReceiveResponse {
    /// `"accepted"` | `"already_receiving"` | `"superseded"` |
    /// `"cancellation_incomplete"` | `"unavailable"`. `"accepted"` only means a
    /// new attempt was admitted; items are saved when a later query says so.
    pub outcome: String,
    /// The new attempt, present only for `"accepted"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    /// `"source_unavailable"` | `"access_denied"`, present only for `"unavailable"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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

// ── POST /clipboard/capture-current ──────────────────────────────

/// Response body for `POST /clipboard/capture-current`.
///
/// `entry_id` is `None` when there was nothing on the OS clipboard to
/// capture.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CaptureCurrentClipboardResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_id: Option<String>,
}

// ── WS clipboard.inbound_notice payload ──────────────────────────

/// Payload for the `clipboard.inbound_notice` WebSocket event.
///
/// Carries structured display summaries without duplicating the full clipboard
/// payload that has already travelled through the sync pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InboundNoticeEvent {
    pub from_device: String,
    pub snapshot_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub representations: Vec<InboundRepresentationSummaryDto>,
    /// `"new_entry"` | `"duplicate_ignored"`.
    pub action: String,
    pub at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InboundRepresentationSummaryDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub size_bytes: i64,
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
    fn file_dispatch_request_and_response_use_camel_case() {
        let request: DispatchFileRequest = serde_json::from_value(serde_json::json!({
            "sourcePath": "/tmp/report.pdf",
            "peers": ["device-1"]
        }))
        .unwrap();
        assert_eq!(request.source_path, "/tmp/report.pdf");

        let response = DispatchFileOutcomeResponse {
            entry_id: "entry-1".into(),
            snapshot_hash: "hash".into(),
            at_ms: 1,
            total_accepted: 1,
            total_duplicate: 0,
            total_offline: 0,
            total_errored: 0,
            total_pending: 1,
            per_target: Vec::new(),
        };
        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["entryId"], "entry-1");
        assert_eq!(json["totalPending"], 1);
        assert!(json.get("entry_id").is_none());
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
            text_preview: Some("hello".into()),
            representations: vec![InboundRepresentationSummaryDto {
                mime_type: Some("text/plain".into()),
                size_bytes: 5,
            }],
            action: "new_entry".into(),
            at_ms: 123,
        };
        let json = serde_json::to_value(&evt).unwrap();
        assert!(json.get("fromDevice").is_some());
        assert!(json.get("snapshotHash").is_some());
        assert!(json.get("plaintextBase64").is_none());
        assert!(json.get("textPreview").is_some());
        assert!(json.get("representations").is_some());
    }

    #[test]
    fn receive_progress_items_use_camel_case_wire_keys() {
        let response = EntryReceiveProgressResponse {
            entry_id: "e".into(),
            attempt_id: "a".into(),
            state: "failed".into(),
            total_bytes: 2,
            completed_bytes: 1,
            items_total: 2,
            items_completed: 1,
            items: vec![ReceiveItemResponse {
                item_id: "i".into(),
                kind: "file".into(),
                state: "failed".into(),
                failure_reason: Some("timed_out".into()),
            }],
        };
        let json = serde_json::to_value(&response).unwrap();
        let item = &json["items"][0];
        assert_eq!(item["itemId"], "i");
        assert_eq!(item["failureReason"], "timed_out");
        assert!(item.get("item_id").is_none());
        assert!(item.get("failure_reason").is_none());
        // Aggregate fields stay for existing consumers.
        assert_eq!(json["itemsTotal"], 2);
        assert_eq!(json["itemsCompleted"], 1);
    }

    #[test]
    fn receive_progress_from_an_older_daemon_has_no_items() {
        let json = r#"{"entryId":"e","attemptId":"a","state":"receiving","totalBytes":2,
            "completedBytes":1,"itemsTotal":1,"itemsCompleted":0}"#;
        let response: EntryReceiveProgressResponse = serde_json::from_str(json).unwrap();
        assert!(response.items.is_empty());
    }

    #[test]
    fn retry_request_requires_the_expected_attempt_and_camel_case_keys() {
        let request: RetryEntryReceiveRequest =
            serde_json::from_str(r#"{"expectedAttemptId":"a","itemIds":["x","y"]}"#).unwrap();
        assert_eq!(request.expected_attempt_id, "a");
        assert_eq!(request.item_ids, vec!["x", "y"]);

        assert!(serde_json::from_str::<RetryEntryReceiveRequest>(r#"{"itemIds":["x"]}"#).is_err());
        assert!(serde_json::from_str::<RetryEntryReceiveRequest>(
            r#"{"expected_attempt_id":"a","item_ids":["x"]}"#
        )
        .is_err());
    }

    #[test]
    fn retry_response_serializes_attempt_and_reason_in_camel_case() {
        let json = serde_json::to_value(RetryEntryReceiveResponse {
            outcome: "accepted".into(),
            attempt_id: Some("a2".into()),
            reason: None,
        })
        .unwrap();
        assert_eq!(json["outcome"], "accepted");
        assert_eq!(json["attemptId"], "a2");
        assert!(json.get("attempt_id").is_none());
    }
}

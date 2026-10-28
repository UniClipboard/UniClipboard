//! Sender-side outbound progress DTOs.
//!
//! Mirrors the Engine's `OutboundProgressSummary` snapshot per
//! (source entry, target device). The snapshot is the single source of truth:
//! `file-transfer.outbound_progress_changed` is only an invalidation hint, and
//! clients re-read the full snapshot instead of patching a local cache.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One item (file, representation or directory unit) of an outbound attempt.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutboundProgressItemDto {
    /// `file` | `representation` | `directory_unit` | `unknown`.
    pub kind: String,
    pub index: u16,
    /// `waiting` | `transferring` | `processing` | `retrying` | `completed` |
    /// `failed` | `cancelled` | `unconfirmed`.
    pub state: String,
    /// Last phase observed before the item left the running states.
    pub last_known_phase: Option<String>,
    pub round: Option<u32>,
    /// `None` means unknown, not zero. Reaching `total_bytes` does not imply completion.
    pub completed_bytes: Option<u64>,
    /// `None` means the total is unknown, not zero.
    pub total_bytes: Option<u64>,
    pub failure_reason: Option<String>,
    pub cancellation_reason: Option<String>,
}

/// Full outbound snapshot for one (entry, target device) pair.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutboundProgressResponse {
    pub entry_id: String,
    pub target_device_id: String,
    /// Current attempt; `None` when no attempt is known.
    pub attempt_id: Option<String>,
    /// Monotonic per (entry, target); older revisions must not overwrite newer ones.
    pub revision: u64,
    /// `waiting` | `live` | `unconfirmed` | `settled`.
    pub freshness: String,
    /// `false` when the Engine has no freshness evidence (for example after a restart).
    pub freshness_available: bool,
    /// `no_items` | `waiting` | `in_progress` | `unconfirmed` | `partially_unconfirmed` |
    /// `completed` | `failed` | `partially_failed` | `cancelled` | `partially_cancelled`.
    pub state: String,
    pub items_total: u32,
    pub items_waiting: u32,
    pub items_running: u32,
    pub items_completed: u32,
    pub items_failed: u32,
    pub items_cancelled: u32,
    pub items_unconfirmed: u32,
    #[serde(default)]
    pub items: Vec<OutboundProgressItemDto>,
}

/// Request body for `POST /clipboard/entries/{id}/outbound/{deviceId}/reconcile`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileOutboundProgressRequest {
    /// The attempt the caller is looking at. A newer attempt answers `superseded`.
    #[serde(default)]
    pub expected_attempt_id: Option<String>,
}

/// Response body for `POST /clipboard/entries/{id}/outbound/{deviceId}/reconcile`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileOutboundProgressResponse {
    /// `running` | `settled` | `superseded` | `not_retained` | `unsupported` |
    /// `unreachable` | `busy`.
    pub state: String,
}

/// Per-target reconciliation result of a resend request.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResendTargetProgressDto {
    pub target_device_id: String,
    /// Same values as [`ReconcileOutboundProgressResponse::state`].
    pub state: String,
    /// Snapshot of the task the result refers to, when one exists.
    pub current: Option<OutboundProgressResponse>,
    /// A new transfer was actually requested for this target.
    pub new_transfer_requested: bool,
    /// The new transfer may leave duplicate files on the receiver.
    pub duplicate_files_possible: bool,
}

#[cfg(test)]
mod wire_tests {
    use super::*;

    fn snapshot() -> OutboundProgressResponse {
        OutboundProgressResponse {
            entry_id: "e".into(),
            target_device_id: "d".into(),
            attempt_id: None,
            revision: 1,
            freshness: "live".into(),
            freshness_available: true,
            state: "in_progress".into(),
            items_total: 1,
            items_waiting: 0,
            items_running: 1,
            items_completed: 0,
            items_failed: 0,
            items_cancelled: 0,
            items_unconfirmed: 0,
            items: vec![OutboundProgressItemDto {
                kind: "file".into(),
                index: 0,
                state: "transferring".into(),
                last_known_phase: Some("transferring".into()),
                round: Some(1),
                completed_bytes: Some(1),
                total_bytes: None,
                failure_reason: None,
                cancellation_reason: None,
            }],
        }
    }

    #[test]
    fn snapshot_uses_camel_case_and_keeps_unknown_total_null() {
        let value = serde_json::to_value(snapshot()).expect("serialize");
        for key in [
            "entryId",
            "targetDeviceId",
            "freshnessAvailable",
            "itemsRunning",
        ] {
            assert!(value.get(key).is_some(), "missing {key}");
        }
        assert!(value.get("entry_id").is_none());
        assert!(value.get("freshness_available").is_none());
        let item = &value["items"][0];
        assert!(item.get("lastKnownPhase").is_some());
        assert!(item.get("last_known_phase").is_none());
        // Unknown total is null, never 0.
        assert!(item["totalBytes"].is_null());
    }

    #[test]
    fn reconcile_request_defaults_to_no_expected_attempt() {
        let req: ReconcileOutboundProgressRequest =
            serde_json::from_str("{}").expect("empty body is valid");
        assert!(req.expected_attempt_id.is_none());
        let req: ReconcileOutboundProgressRequest =
            serde_json::from_str(r#"{"expectedAttemptId":"a"}"#).expect("camelCase");
        assert_eq!(req.expected_attempt_id.as_deref(), Some("a"));
    }

    #[test]
    fn resend_request_defaults_to_reconcile_only() {
        use crate::api::dto::clipboard_command::ResendRequest;
        let req: ResendRequest = serde_json::from_str(r#"{"entryId":"e"}"#).expect("parse");
        assert!(!req.confirm_new_transfer);
        let req: ResendRequest =
            serde_json::from_str(r#"{"entryId":"e","confirmNewTransfer":true}"#).expect("parse");
        assert!(req.confirm_new_transfer);
    }
}

use uc_daemon_contract::api::dto::clipboard_command::{
    InboundNoticeEvent as InboundNoticeDto, InboundRepresentationSummaryDto,
};
use uc_daemon_contract::constants::{ws_event, ws_topic};
use uc_engine::{
    ClipboardOriginSummary, EngineEvent, InboundNoticeActionSummary, NetworkRecoveryPhaseSummary,
    TransferDirectionSummary,
};

use crate::api::projection::IntoApiDto;
use crate::api::types::DaemonWsEvent;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn engine_event_to_ws(event: EngineEvent) -> Option<DaemonWsEvent> {
    let (topic, event_type, ts, payload) = match event {
        EngineEvent::InboundNotice(event) => (
            ws_topic::CLIPBOARD,
            ws_event::CLIPBOARD_INBOUND_NOTICE,
            event.at_ms,
            serde_json::to_value(InboundNoticeDto {
                from_device: event.from_device,
                snapshot_hash: event.snapshot_hash,
                text_preview: event.text_preview,
                representations: event
                    .representations
                    .into_iter()
                    .map(|representation| InboundRepresentationSummaryDto {
                        mime_type: representation.mime_type,
                        size_bytes: representation.size_bytes,
                    })
                    .collect(),
                action: match event.action {
                    InboundNoticeActionSummary::NewEntry => "new_entry",
                    InboundNoticeActionSummary::DuplicateIgnored => "duplicate_ignored",
                }
                .to_string(),
                at_ms: event.at_ms,
            })
            .ok()?,
        ),
        EngineEvent::IncomingEntry(event) => (
            ws_topic::CLIPBOARD,
            ws_event::CLIPBOARD_NEW_CONTENT,
            now_ms(),
            serde_json::json!({
                "entryId": event.entry_id,
                "attemptId": event.attempt_id,
                "preview": event.preview,
                "origin": match event.origin {
                    ClipboardOriginSummary::Local => "local",
                    ClipboardOriginSummary::Remote => "remote",
                },
                "fromDevice": "",
                "contentType": null,
            }),
        ),
        EngineEvent::IncomingPending(event) => (
            ws_topic::CLIPBOARD,
            ws_event::CLIPBOARD_INCOMING_PENDING,
            now_ms(),
            serde_json::json!({
                "entryId": event.entry_id,
                "attemptId": event.attempt_id,
                "fromDevice": event.from_device,
                "totalBytes": event.total_bytes,
                "filenames": event.filenames,
            }),
        ),
        EngineEvent::ReceiveAttemptStateChanged(event) => (
            ws_topic::CLIPBOARD,
            ws_event::CLIPBOARD_RECEIVE_ATTEMPT_STATE_CHANGED,
            now_ms(),
            serde_json::json!({
                "entryId": event.entry_id,
                "attemptId": event.attempt_id,
                "state": event.state,
            }),
        ),
        EngineEvent::TransferProgress(event) => (
            ws_topic::FILE_TRANSFER,
            ws_event::FILE_TRANSFER_PROGRESS,
            now_ms(),
            serde_json::json!({
                "transferId": event.transfer_id,
                "entryId": event.entry_id,
                "attemptId": event.attempt_id,
                "peerId": event.peer_id,
                "direction": match event.direction {
                    TransferDirectionSummary::Sending => "sending",
                    TransferDirectionSummary::Receiving => "receiving",
                },
                "bytesTransferred": event.completed_bytes,
                "totalBytes": event.total_bytes,
            }),
        ),
        EngineEvent::TransferStatusChanged(event) => (
            ws_topic::FILE_TRANSFER,
            ws_event::FILE_TRANSFER_STATUS_CHANGED,
            now_ms(),
            serde_json::json!({
                "transferId": event.transfer_id,
                "entryId": event.entry_id,
                "attemptId": event.attempt_id,
                "status": event.status,
                "reason": event.reason,
            }),
        ),
        EngineEvent::DeliveryStatusChanged(event) => (
            ws_topic::CLIPBOARD,
            ws_event::CLIPBOARD_DELIVERY_STATUS_CHANGED,
            now_ms(),
            serde_json::json!({
                "entryId": event.entry_id,
                "targetDeviceId": event.target_device_id,
            }),
        ),
        EngineEvent::WorkspaceConvergenceChanged(summary) => {
            let updated_at_ms = summary.updated_at_ms;
            let dto = summary.into_api_dto();
            (
                ws_topic::WORKSPACE_CONVERGENCE,
                ws_event::WORKSPACE_CONVERGENCE_CHANGED,
                updated_at_ms,
                serde_json::to_value(dto).ok()?,
            )
        }
        EngineEvent::NetworkRecoveryChanged(status) => (
            ws_topic::NETWORK_RECOVERY,
            ws_event::NETWORK_RECOVERY_CHANGED,
            now_ms(),
            serde_json::json!({
                "phase": match status.phase {
                    NetworkRecoveryPhaseSummary::Idle => "idle",
                    NetworkRecoveryPhaseSummary::Recovering => "recovering",
                    NetworkRecoveryPhaseSummary::RetryScheduled => "retryScheduled",
                    NetworkRecoveryPhaseSummary::Failed => "failed",
                },
                "retryable": status.retryable,
                "nextRetryInMs": status.next_retry_in_ms,
            }),
        ),
        EngineEvent::RefreshRequired { .. } => return Some(refresh_required_ws_event()),
        EngineEvent::StateChanged { .. }
        | EngineEvent::PeerPresenceChanged(_)
        | EngineEvent::ActiveClipboardChanged(_)
        | EngineEvent::MobileLanSettingsChanged(_)
        | EngineEvent::OperationFinished { .. }
        | EngineEvent::LifecycleFailed { .. }
        | EngineEvent::Fatal { .. } => return None,
    };

    Some(DaemonWsEvent {
        topic: topic.to_string(),
        event_type: event_type.to_string(),
        session_id: None,
        ts,
        payload,
    })
}

/// Builds the process-wide notification used at every incremental-event lag boundary.
pub fn refresh_required_ws_event() -> DaemonWsEvent {
    DaemonWsEvent {
        topic: ws_topic::SYSTEM.to_string(),
        event_type: ws_event::SYSTEM_REFRESH_REQUIRED.to_string(),
        session_id: None,
        ts: now_ms(),
        payload: serde_json::json!({}),
    }
}

#[cfg(test)]
mod engine_event_tests {
    use super::*;
    use serde::Serialize;
    use uc_engine::{
        EngineError, EngineErrorCategory, InboundNoticeEvent, LifecycleAction,
        NetworkRecoveryStatusSummary, RefreshReason, TransferProgress,
        WorkspaceConvergenceFailureCategorySummary, WorkspaceConvergencePhaseSummary,
        WorkspaceConvergenceSummary,
    };

    #[derive(Serialize)]
    struct InboundNoticeFixture {
        from_device: &'static str,
        snapshot_hash: &'static str,
        plaintext: Vec<u8>,
        text_preview: Option<&'static str>,
        representations: Vec<serde_json::Value>,
        action: &'static str,
        at_ms: i64,
    }

    fn inbound_notice_with_payload(
        payload_size: usize,
        action: &'static str,
    ) -> InboundNoticeEvent {
        let encoded = serde_json::to_vec(&InboundNoticeFixture {
            from_device: "peer-1",
            snapshot_hash: "hash-1",
            plaintext: vec![0; payload_size],
            text_preview: None,
            representations: Vec::new(),
            action,
            at_ms: 42,
        })
        .expect("serialize inbound notice fixture");
        serde_json::from_slice(&encoded).expect("deserialize inbound notice fixture")
    }

    #[test]
    fn lifecycle_failures_stay_on_the_host_event_stream() {
        let event = EngineEvent::LifecycleFailed {
            action: LifecycleAction::Suspend,
            error: EngineError::new(1214, EngineErrorCategory::Unavailable, true),
        };

        assert!(engine_event_to_ws(event).is_none());
    }

    #[test]
    fn refresh_required_becomes_a_global_resync_notification() {
        let event = engine_event_to_ws(EngineEvent::RefreshRequired {
            reason: RefreshReason::ConsumerLagged,
        })
        .expect("global refresh-required websocket event");

        assert_eq!(event.topic, ws_topic::SYSTEM);
        assert_eq!(event.event_type, ws_event::SYSTEM_REFRESH_REQUIRED);
        assert_eq!(event.payload, serde_json::json!({}));
    }

    #[test]
    fn inbound_notice_keeps_the_existing_watch_wire_shape() {
        let event = engine_event_to_ws(EngineEvent::InboundNotice(inbound_notice_with_payload(
            3,
            "duplicate_ignored",
        )))
        .expect("inbound notice websocket event");

        assert_eq!(event.topic, ws_topic::CLIPBOARD);
        assert_eq!(event.event_type, ws_event::CLIPBOARD_INBOUND_NOTICE);
        assert_eq!(event.payload["fromDevice"], "peer-1");
        assert_eq!(event.payload["snapshotHash"], "hash-1");
        assert!(event.payload.get("plaintextBase64").is_none());
        assert_eq!(event.payload["action"], "duplicate_ignored");
        assert_eq!(event.payload["atMs"], 42);
    }

    #[test]
    fn inbound_notice_omits_full_clipboard_payload() {
        let event = engine_event_to_ws(EngineEvent::InboundNotice(inbound_notice_with_payload(
            20 * 1024 * 1024,
            "new_entry",
        )))
        .expect("inbound notice websocket event");

        assert!(event.payload.get("plaintextBase64").is_none());
    }

    #[test]
    fn inbound_notice_wire_size_does_not_scale_with_clipboard_payload() {
        let event = engine_event_to_ws(EngineEvent::InboundNotice(inbound_notice_with_payload(
            20 * 1024 * 1024,
            "new_entry",
        )))
        .expect("inbound notice websocket event");

        let encoded = serde_json::to_vec(&event).expect("serialize inbound notice websocket event");
        assert!(
            encoded.len() < 64 * 1024,
            "inbound notice unexpectedly contains {} bytes",
            encoded.len()
        );
    }

    #[test]
    fn transfer_progress_keeps_every_existing_wire_field() {
        let event = engine_event_to_ws(EngineEvent::TransferProgress(TransferProgress {
            transfer_id: "transfer-1".into(),
            entry_id: Some("entry-1".into()),
            attempt_id: Some("attempt-1".into()),
            peer_id: "peer-1".into(),
            direction: TransferDirectionSummary::Receiving,
            completed_bytes: 64,
            total_bytes: Some(128),
        }))
        .expect("transfer progress websocket event");

        assert_eq!(event.topic, ws_topic::FILE_TRANSFER);
        assert_eq!(event.event_type, ws_event::FILE_TRANSFER_PROGRESS);
        assert_eq!(event.payload["transferId"], "transfer-1");
        assert_eq!(event.payload["entryId"], "entry-1");
        assert_eq!(event.payload["attemptId"], "attempt-1");
        assert_eq!(event.payload["peerId"], "peer-1");
        assert_eq!(event.payload["direction"], "receiving");
        assert_eq!(event.payload["bytesTransferred"], 64);
        assert_eq!(event.payload["totalBytes"], 128);
    }

    #[test]
    fn network_recovery_event_uses_the_stable_camel_case_wire_shape() {
        let event = engine_event_to_ws(EngineEvent::NetworkRecoveryChanged(
            NetworkRecoveryStatusSummary {
                phase: NetworkRecoveryPhaseSummary::RetryScheduled,
                retryable: true,
                next_retry_in_ms: Some(2_000),
            },
        ))
        .expect("network recovery websocket event");

        assert_eq!(event.topic, ws_topic::NETWORK_RECOVERY);
        assert_eq!(event.event_type, ws_event::NETWORK_RECOVERY_CHANGED);
        assert_eq!(event.payload["phase"], "retryScheduled");
        assert_eq!(event.payload["nextRetryInMs"], 2_000);
        assert!(event.payload.get("next_retry_in_ms").is_none());
    }

    #[test]
    fn workspace_convergence_changes_include_the_complete_engine_state() {
        let event = engine_event_to_ws(EngineEvent::WorkspaceConvergenceChanged(
            WorkspaceConvergenceSummary {
                phase: WorkspaceConvergencePhaseSummary::WaitingForOfflineMember,
                revision: 4,
                change_count: 2,
                removal_intent_count: 1,
                effective_member_count: 3,
                confirmed_member_count: 2,
                waiting_member_device_ids: vec!["device-b".into()],
                waiting_member_count: 1,
                convergence_digest: Some("digest-1".into()),
                updated_at_ms: 42,
                removed: false,
                failure_category: Some(WorkspaceConvergenceFailureCategorySummary::Storage),
            },
        ))
        .expect("workspace convergence websocket event");

        assert_eq!(event.topic, "workspace-convergence");
        assert_eq!(event.event_type, "workspace-convergence.changed");
        assert_eq!(event.payload["phase"], "waiting_for_offline_member");
        assert_eq!(event.payload["revision"], 4);
        assert_eq!(event.payload["changeCount"], 2);
        assert_eq!(event.payload["removalIntentCount"], 1);
        assert_eq!(event.payload["effectiveMemberCount"], 3);
        assert_eq!(event.payload["confirmedMemberCount"], 2);
        assert_eq!(
            event.payload["waitingMemberDeviceIds"],
            serde_json::json!(["device-b"])
        );
        assert_eq!(event.payload["waitingMemberCount"], 1);
        assert_eq!(event.payload["convergenceDigest"], "digest-1");
        assert_eq!(event.payload["updatedAtMs"], 42);
        assert_eq!(event.payload["removed"], false);
        assert_eq!(event.payload["failureCategory"], "storage");
    }
}

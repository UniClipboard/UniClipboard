use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::Serialize;
use tokio::sync::broadcast;
use uc_application::facade::{
    ClipboardHostEvent, DeliveryHostEvent, EmitError, HostEvent, HostEventEmitterPort,
    TransferHostEvent,
};
// `HostEvent::Delivery` is forwarded on the `clipboard` topic (ADR-008 P3-3
// GAP-WS-1). It used to be GUI-only via the in-process `TauriHostEventEmitter`;
// once the GUI becomes a pure client (B2'-3) the in-process path is gone, so the
// delivery refetch signal must travel over WS like every other host event. LAN
// WS clients (which have no "entry I'm viewing" notion) simply don't subscribe
// to it; the GUI filters by entry_id client-side, so the extra type is cheap.
use uc_daemon_contract::api::dto::clipboard_command::InboundNoticeEvent as InboundNoticeDto;
use uc_daemon_contract::constants::{ws_event, ws_topic};
use uc_engine::{
    ClipboardOriginSummary, EngineEvent, InboundNoticeActionSummary, TransferDirectionSummary,
};

use crate::api::types::{DaemonWsEvent, FileTransferProgressPayload};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileTransferStatusChangedPayload {
    transfer_id: String,
    entry_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt_id: Option<String>,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardNewContentPayload {
    entry_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt_id: Option<String>,
    preview: String,
    origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardDeliveryStatusChangedPayload {
    entry_id: String,
    target_device_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardIncomingPendingPayload {
    entry_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt_id: Option<String>,
    from_device: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_bytes: Option<u64>,
    /// 文件名列表(顺序与 envelope 的 blob_refs 一致)。空列表表示
    /// 该入站事件没有可显示的文件名(纯文本 / 仅图像等)。
    filenames: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardReceiveAttemptStatePayload {
    entry_id: String,
    attempt_id: String,
    state: String,
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
                plaintext_base64: STANDARD.encode(event.plaintext),
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
            DaemonApiEventEmitter::now_ms(),
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
            DaemonApiEventEmitter::now_ms(),
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
            DaemonApiEventEmitter::now_ms(),
            serde_json::json!({
                "entryId": event.entry_id,
                "attemptId": event.attempt_id,
                "state": event.state,
            }),
        ),
        EngineEvent::TransferProgress(event) => (
            ws_topic::FILE_TRANSFER,
            ws_event::FILE_TRANSFER_PROGRESS,
            DaemonApiEventEmitter::now_ms(),
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
            DaemonApiEventEmitter::now_ms(),
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
            DaemonApiEventEmitter::now_ms(),
            serde_json::json!({
                "entryId": event.entry_id,
                "targetDeviceId": event.target_device_id,
            }),
        ),
        EngineEvent::StateChanged { .. }
        | EngineEvent::PeerPresenceChanged(_)
        | EngineEvent::ActiveClipboardChanged(_)
        | EngineEvent::MobileLanSettingsChanged(_)
        | EngineEvent::RefreshRequired { .. }
        | EngineEvent::OperationFinished { .. }
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

pub struct DaemonApiEventEmitter {
    event_tx: broadcast::Sender<DaemonWsEvent>,
}

impl DaemonApiEventEmitter {
    pub fn new(event_tx: broadcast::Sender<DaemonWsEvent>) -> Self {
        Self { event_tx }
    }

    fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    fn emit_ws_event<T: serde::Serialize>(
        &self,
        event_type: &str,
        topic: &str,
        session_id: Option<String>,
        ts: i64,
        payload: T,
    ) {
        let payload = match serde_json::to_value(payload) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::warn!(error = %error, event_type, "failed to serialize daemon api event");
                return;
            }
        };

        let _ = self.event_tx.send(DaemonWsEvent {
            topic: topic.to_string(),
            event_type: event_type.to_string(),
            session_id,
            ts,
            payload,
        });
    }
}

impl HostEventEmitterPort for DaemonApiEventEmitter {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        match event {
            HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id,
                entry_id,
                attempt_id,
                status,
                reason,
            }) => {
                self.emit_ws_event(
                    ws_event::FILE_TRANSFER_STATUS_CHANGED,
                    ws_topic::FILE_TRANSFER,
                    None,
                    Self::now_ms(),
                    FileTransferStatusChangedPayload {
                        transfer_id,
                        entry_id,
                        attempt_id,
                        status,
                        reason,
                    },
                );
            }
            HostEvent::Transfer(TransferHostEvent::Progress {
                transfer_id,
                entry_id,
                attempt_id,
                peer_id,
                direction,
                bytes_transferred,
                total_bytes,
            }) => {
                self.emit_ws_event(
                    ws_event::FILE_TRANSFER_PROGRESS,
                    ws_topic::FILE_TRANSFER,
                    None,
                    Self::now_ms(),
                    FileTransferProgressPayload {
                        transfer_id,
                        entry_id,
                        attempt_id,
                        peer_id,
                        direction,
                        bytes_transferred,
                        total_bytes,
                    },
                );
            }
            HostEvent::Clipboard(ClipboardHostEvent::NewContent {
                entry_id,
                attempt_id,
                preview,
                origin,
            }) => {
                self.emit_ws_event(
                    ws_event::CLIPBOARD_NEW_CONTENT,
                    ws_topic::CLIPBOARD,
                    None,
                    Self::now_ms(),
                    ClipboardNewContentPayload {
                        entry_id,
                        attempt_id,
                        preview,
                        origin: format!("{:?}", origin).to_lowercase(),
                        content_type: None,
                    },
                );
            }
            HostEvent::Clipboard(ClipboardHostEvent::IncomingPending {
                entry_id,
                attempt_id,
                from_device,
                total_bytes,
                filenames,
            }) => {
                self.emit_ws_event(
                    ws_event::CLIPBOARD_INCOMING_PENDING,
                    ws_topic::CLIPBOARD,
                    None,
                    Self::now_ms(),
                    ClipboardIncomingPendingPayload {
                        entry_id,
                        attempt_id,
                        from_device,
                        total_bytes,
                        filenames,
                    },
                );
            }
            HostEvent::Clipboard(ClipboardHostEvent::ReceiveAttemptStateChanged {
                entry_id,
                attempt_id,
                state,
            }) => {
                self.emit_ws_event(
                    ws_event::CLIPBOARD_RECEIVE_ATTEMPT_STATE_CHANGED,
                    ws_topic::CLIPBOARD,
                    None,
                    Self::now_ms(),
                    ClipboardReceiveAttemptStatePayload {
                        entry_id,
                        attempt_id,
                        state,
                    },
                );
            }
            HostEvent::Delivery(DeliveryHostEvent::StatusChanged {
                entry_id,
                target_device_id,
            }) => {
                self.emit_ws_event(
                    ws_event::CLIPBOARD_DELIVERY_STATUS_CHANGED,
                    ws_topic::CLIPBOARD,
                    None,
                    Self::now_ms(),
                    ClipboardDeliveryStatusChangedPayload {
                        entry_id,
                        target_device_id,
                    },
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod engine_event_tests {
    use super::*;
    use uc_engine::{InboundNoticeEvent, TransferProgress};

    #[test]
    fn inbound_notice_keeps_the_existing_watch_wire_shape() {
        let event = engine_event_to_ws(EngineEvent::InboundNotice(InboundNoticeEvent {
            from_device: "peer-1".into(),
            snapshot_hash: "hash-1".into(),
            plaintext: vec![1, 2, 3],
            action: InboundNoticeActionSummary::DuplicateIgnored,
            at_ms: 42,
        }))
        .expect("inbound notice websocket event");

        assert_eq!(event.topic, ws_topic::CLIPBOARD);
        assert_eq!(event.event_type, ws_event::CLIPBOARD_INBOUND_NOTICE);
        assert_eq!(event.payload["fromDevice"], "peer-1");
        assert_eq!(event.payload["snapshotHash"], "hash-1");
        assert_eq!(event.payload["plaintextBase64"], "AQID");
        assert_eq!(event.payload["action"], "duplicate_ignored");
        assert_eq!(event.payload["atMs"], 42);
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
}

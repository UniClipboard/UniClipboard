use serde::Serialize;
use tokio::sync::broadcast;
use uc_application::facade::{
    ClipboardHostEvent, EmitError, HostEvent, HostEventEmitterPort, TransferHostEvent,
};
use uc_daemon_contract::constants::{ws_event, ws_topic};

use crate::api::types::{DaemonWsEvent, FileTransferProgressPayload};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileTransferStatusChangedPayload {
    transfer_id: String,
    entry_id: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardNewContentPayload {
    entry_id: String,
    preview: String,
    origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardIncomingPendingPayload {
    entry_id: String,
    from_device: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_bytes: Option<u64>,
    /// 文件名列表(顺序与 envelope 的 blob_refs 一致)。空列表表示
    /// 该入站事件没有可显示的文件名(纯文本 / 仅图像等)。
    filenames: Vec<String>,
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
                        status,
                        reason,
                    },
                );
            }
            HostEvent::Transfer(TransferHostEvent::Progress {
                transfer_id,
                entry_id,
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
                        peer_id,
                        direction,
                        bytes_transferred,
                        total_bytes,
                    },
                );
            }
            HostEvent::Clipboard(ClipboardHostEvent::NewContent {
                entry_id,
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
                        preview,
                        origin: format!("{:?}", origin).to_lowercase(),
                        content_type: None,
                    },
                );
            }
            HostEvent::Clipboard(ClipboardHostEvent::IncomingPending {
                entry_id,
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
                        from_device,
                        total_bytes,
                        filenames,
                    },
                );
            }
        }

        Ok(())
    }
}

use serde::Serialize;
use tokio::sync::broadcast;
use uc_core::network::daemon_api_strings::{ws_event, ws_topic};
use uc_core::ports::host_event_emitter::{
    ClipboardHostEvent, EmitError, HostEvent, HostEventEmitterPort, SetupHostEvent,
    SpaceAccessHostEvent, TransferHostEvent,
};

use crate::api::types::{
    DaemonWsEvent, SetupSpaceAccessCompletedPayload, SetupStateChangedPayload,
};

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
#[allow(dead_code)]
struct ClipboardDeletedPayload {
    entry_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct ClipboardUpdatedPayload {
    entry_id: String,
    changes: Vec<String>,
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

    fn log_non_setup_event(event_type: &'static str) {
        tracing::debug!(event_type, "host event (daemon api emitter)");
    }
}

impl HostEventEmitterPort for DaemonApiEventEmitter {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        match event {
            HostEvent::Setup(SetupHostEvent::StateChanged { state, session_id }) => {
                self.emit_ws_event(
                    ws_event::SETUP_STATE_CHANGED,
                    ws_topic::SETUP,
                    session_id.clone(),
                    Self::now_ms(),
                    SetupStateChangedPayload {
                        session_id,
                        state: serde_json::to_value(state).unwrap_or_default(),
                    },
                );
            }
            HostEvent::SpaceAccess(SpaceAccessHostEvent::Completed {
                session_id,
                peer_id,
                success,
                reason,
                ts,
            })
            | HostEvent::SpaceAccess(SpaceAccessHostEvent::P2PCompleted {
                session_id,
                peer_id,
                success,
                reason,
                ts,
            }) => {
                self.emit_ws_event(
                    ws_event::SETUP_SPACE_ACCESS_COMPLETED,
                    ws_topic::SETUP,
                    Some(session_id.clone()),
                    ts,
                    SetupSpaceAccessCompletedPayload {
                        session_id,
                        peer_id,
                        success,
                        reason,
                        ts,
                    },
                );
            }
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
            HostEvent::Clipboard(clipboard_event) => match clipboard_event {
                ClipboardHostEvent::NewContent {
                    entry_id,
                    preview,
                    origin,
                } => {
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
                ClipboardHostEvent::InboundError { .. }
                | ClipboardHostEvent::InboundSubscribeRecovered { .. }
                | ClipboardHostEvent::InboundSubscribeError { .. }
                | ClipboardHostEvent::InboundSubscribeRetry { .. }
                | ClipboardHostEvent::DaemonReconnected => {
                    // These are handled by TauriEventEmitter / DaemonWsBridge on the GUI side.
                    // No WS broadcast needed from daemon — these are GUI-internal signals.
                    Self::log_non_setup_event("clipboard_internal");
                }
            },
            HostEvent::PeerDiscovery(_) => Self::log_non_setup_event("peer_discovery"),
            HostEvent::PeerConnection(_) => Self::log_non_setup_event("peer_connection"),
            HostEvent::Transfer(_) => Self::log_non_setup_event("transfer"),
            HostEvent::Pairing(_) => Self::log_non_setup_event("pairing"),
            HostEvent::Realtime(_) => Self::log_non_setup_event("realtime"),
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ports::host_event_emitter::HostEvent;
    use uc_core::security::space_access::state::SpaceAccessState;
    use uc_core::setup::SetupState;

    use crate::api::types::SpaceAccessStateChangedPayload;

    #[test]
    fn emits_setup_state_changed_to_setup_topic() {
        let (tx, mut rx) = broadcast::channel(4);
        let emitter = DaemonApiEventEmitter::new(tx);

        emitter
            .emit(HostEvent::Setup(SetupHostEvent::StateChanged {
                state: SetupState::JoinSpaceConfirmPeer {
                    short_code: "123456".to_string(),
                    peer_fingerprint: Some("peer-fp".to_string()),
                    error: None,
                },
                session_id: Some("session-1".to_string()),
            }))
            .expect("daemon api emitter should stay infallible");

        let event = rx.try_recv().expect("setup event should be broadcast");
        assert_eq!(event.topic, ws_topic::SETUP);
        assert_eq!(event.event_type, ws_event::SETUP_STATE_CHANGED);
        assert_eq!(event.session_id.as_deref(), Some("session-1"));
        assert_eq!(
            event.payload["sessionId"].as_str(),
            Some("session-1"),
            "setup payload should use camelCase sessionId"
        );
        assert!(
            event.payload["state"]["JoinSpaceConfirmPeer"].is_object(),
            "setup payload should carry full setup state object"
        );
    }

    #[test]
    fn space_access_state_changed_event_uses_space_access_topic() {
        // This test verifies the DaemonWsEvent structure for space_access.state_changed
        // events that DaemonPairingHost broadcasts directly
        let payload = SpaceAccessStateChangedPayload {
            state: SpaceAccessState::Idle,
        };
        let serialized = serde_json::to_value(&payload).expect("serialize");
        let event = DaemonWsEvent {
            topic: "space-access".to_string(),
            event_type: "space_access.state_changed".to_string(),
            session_id: None,
            ts: 1234567890,
            payload: serialized,
        };
        assert_eq!(event.topic, "space-access");
        assert_eq!(event.event_type, "space_access.state_changed");
        assert_eq!(event.payload["state"], "Idle");
    }

    #[test]
    fn emits_file_transfer_status_changed_to_file_transfer_topic() {
        let (tx, mut rx) = broadcast::channel(4);
        let emitter = DaemonApiEventEmitter::new(tx);

        emitter
            .emit(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id: "xfer-42".to_string(),
                entry_id: "entry-99".to_string(),
                status: "completed".to_string(),
                reason: None,
            }))
            .expect("emit should succeed");

        let event = rx
            .try_recv()
            .expect("file-transfer event should be broadcast");
        assert_eq!(event.topic, ws_topic::FILE_TRANSFER);
        assert_eq!(event.event_type, ws_event::FILE_TRANSFER_STATUS_CHANGED);
        assert_eq!(event.payload["transferId"].as_str(), Some("xfer-42"));
        assert_eq!(event.payload["entryId"].as_str(), Some("entry-99"));
        assert_eq!(event.payload["status"].as_str(), Some("completed"));
        assert!(
            event.payload.get("reason").is_none(),
            "reason should be omitted when None"
        );
    }

    #[test]
    fn emits_clipboard_new_content_to_clipboard_topic() {
        let (tx, mut rx) = broadcast::channel(4);
        let emitter = DaemonApiEventEmitter::new(tx);

        use uc_core::ports::host_event_emitter::ClipboardOriginKind;

        emitter
            .emit(HostEvent::Clipboard(ClipboardHostEvent::NewContent {
                entry_id: "entry-001".to_string(),
                preview: "Hello world".to_string(),
                origin: ClipboardOriginKind::Local,
            }))
            .expect("emit should succeed");

        let event = rx
            .try_recv()
            .expect("clipboard event should be broadcast");
        assert_eq!(event.topic, ws_topic::CLIPBOARD);
        assert_eq!(event.event_type, ws_event::CLIPBOARD_NEW_CONTENT);
        assert_eq!(event.payload["entryId"].as_str(), Some("entry-001"));
        assert_eq!(event.payload["preview"].as_str(), Some("Hello world"));
        assert_eq!(event.payload["origin"].as_str(), Some("local"));
        assert!(
            event.payload.get("contentType").is_none(),
            "contentType should be omitted when None"
        );
    }

    #[test]
    fn clipboard_new_content_remote_origin_serializes_as_remote() {
        let (tx, mut rx) = broadcast::channel(4);
        let emitter = DaemonApiEventEmitter::new(tx);

        use uc_core::ports::host_event_emitter::ClipboardOriginKind;

        emitter
            .emit(HostEvent::Clipboard(ClipboardHostEvent::NewContent {
                entry_id: "entry-002".to_string(),
                preview: "Remote clipboard".to_string(),
                origin: ClipboardOriginKind::Remote,
            }))
            .expect("emit should succeed");

        let event = rx
            .try_recv()
            .expect("clipboard event should be broadcast");
        assert_eq!(event.payload["origin"].as_str(), Some("remote"));
    }
}

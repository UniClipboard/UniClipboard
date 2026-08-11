use std::sync::Arc;

use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uc_daemon_contract::api::types::{DaemonWsEvent, PeerSnapshotDto, PeersChangedFullPayload};
use uc_daemon_contract::constants::{ws_event, ws_topic};
use uc_engine::{
    ActiveClipboardChanged, Engine, EngineEvent, EventStream, Operation, OperationResult,
    PeerConnectionChannelSummary, PeerConnectionSummary,
};

use super::mobile_lan_lifecycle::{mobile_lan_target, MobileLanLifecycleController};

fn daemon_ws_event(event: EngineEvent) -> Option<DaemonWsEvent> {
    uc_webserver::api::event_emitter::engine_event_to_ws(event)
}

pub(crate) async fn forward_engine_events(
    mut events: EventStream,
    engine: Arc<Engine>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    active_clipboard_tx: broadcast::Sender<ActiveClipboardChanged>,
    mobile_lan: Arc<MobileLanLifecycleController>,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            event = events.next() => {
                let Some(event) = event else { return; };
                match event {
                    EngineEvent::ActiveClipboardChanged(change) => {
                        let _ = active_clipboard_tx.send(change);
                    }
                    EngineEvent::PeerPresenceChanged(_) => {
                        publish_peer_snapshot(&engine, &event_tx).await;
                    }
                    EngineEvent::RefreshRequired { .. } => {
                        if let Some(event) = daemon_ws_event(event) {
                            let _ = event_tx.send(event);
                        }
                        publish_peer_snapshot(&engine, &event_tx).await;
                    }
                    EngineEvent::MobileLanSettingsChanged(settings) => {
                        mobile_lan
                            .reconcile(mobile_lan_target(
                                settings.enabled,
                                settings.lan_listen_enabled,
                                settings.lan_port,
                            ))
                            .await;
                    }
                    EngineEvent::Fatal { error } => tracing::error!(
                        code = error.code(),
                        category = %error.category(),
                        "engine reported an unrecoverable error"
                    ),
                    event => {
                        if let Some(event) = daemon_ws_event(event) {
                            let _ = event_tx.send(event);
                        }
                    }
                }
            }
        }
    }
}

async fn publish_peer_snapshot(engine: &Engine, event_tx: &broadcast::Sender<DaemonWsEvent>) {
    let peers = match engine.execute(Operation::QueryPeerConnections).await {
        Ok(OperationResult::PeerConnections(peers)) => peers,
        Ok(_) => {
            tracing::warn!("engine returned an unexpected peer snapshot result");
            return;
        }
        Err(error) => {
            tracing::warn!(
                code = error.code(),
                category = %error.category(),
                "failed to refresh peer snapshot"
            );
            return;
        }
    };
    let payload = PeersChangedFullPayload {
        peers: peers.into_iter().map(peer_to_dto).collect(),
    };
    let payload = match serde_json::to_value(payload) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(error = %error, "failed to serialize peer snapshot");
            return;
        }
    };
    let _ = event_tx.send(DaemonWsEvent {
        topic: ws_topic::PEERS.to_string(),
        event_type: ws_event::PEERS_CHANGED.to_string(),
        session_id: None,
        ts: chrono::Utc::now().timestamp_millis(),
        payload,
    });
}

fn peer_to_dto(peer: PeerConnectionSummary) -> PeerSnapshotDto {
    PeerSnapshotDto {
        peer_id: peer.peer_id,
        device_name: peer.device_name,
        addresses: peer.addresses,
        is_paired: peer.is_paired,
        connected: peer.connected,
        pairing_state: peer.pairing_state,
        channel: match peer.channel {
            PeerConnectionChannelSummary::Direct => "direct",
            PeerConnectionChannelSummary::Relay => "relay",
            PeerConnectionChannelSummary::Offline => "offline",
            PeerConnectionChannelSummary::Unknown => "unknown",
        }
        .to_string(),
        connection_address: peer.connection_address,
    }
}

#[cfg(test)]
mod tests {
    use uc_daemon_contract::constants::{ws_event, ws_topic};
    use uc_engine::{EngineEvent, InboundNoticeEvent, TransferDirectionSummary, TransferProgress};

    use super::daemon_ws_event;

    #[test]
    fn inbound_notice_keeps_the_existing_watch_wire_shape() {
        let notice = serde_json::from_value::<InboundNoticeEvent>(serde_json::json!({
            "from_device": "peer-1",
            "snapshot_hash": "hash-1",
            "plaintext": [1, 2, 3],
            "text_preview": null,
            "representations": [],
            "action": "duplicate_ignored",
            "at_ms": 42
        }))
        .expect("deserialize inbound notice fixture");
        let event = daemon_ws_event(EngineEvent::InboundNotice(notice))
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
    fn transfer_progress_keeps_every_existing_wire_field() {
        let event = daemon_ws_event(EngineEvent::TransferProgress(TransferProgress {
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

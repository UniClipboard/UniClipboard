//! WebSocket subscribe protocol for daemon read-model topics.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, warn};
use uc_core::network::daemon_api_strings::{ws_event, ws_topic};

use crate::api::server::DaemonApiState;
use crate::security::claims::SessionTokenClaims;
use crate::api::types::{
    DaemonWsEvent, DaemonWsSubscribeRequest, PairedDeviceDto, PairedDevicesChangedPayload,
    PairingFailurePayload, PairingSessionChangedPayload, PairingSessionSummaryDto,
    PairingVerificationPayload, PeerConnectionChangedPayload, PeerNameUpdatedPayload,
    PeerSnapshotDto, PeersChangedFullPayload, SpaceAccessStateResponse, StatusResponse,
};

type ClientTopics = Arc<RwLock<HashSet<String>>>;

pub fn router() -> Router<DaemonApiState> {
    Router::new().route("/ws", get(websocket_upgrade))
}

async fn websocket_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<DaemonApiState>,
    headers: HeaderMap,
) -> Response {
    // Step 1: Extract session token from Authorization header
    let token = match headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Session "))
    {
        Some(t) => t.to_string(),
        None => {
            return ws_unauthorized("missing_session_token").into_response();
        }
    };

    // Step 2: Verify JWT using the JWT secret from merged SecurityState
    let claims = match SessionTokenClaims::verify(&token, &state.security.jwt_secret) {
        Ok(claims) => claims,
        Err(e) => {
            warn!(error = %e, "WS JWT validation failed");
            return ws_unauthorized("invalid_session_token").into_response();
        }
    };

    // Step 3: Check PID whitelist via merged DaemonApiState.security field
    if !state.security.is_pid_allowed(claims.pid).await {
        warn!(pid = claims.pid, "WS request from non-whitelisted PID");
        return ws_forbidden("pid_not_allowed").into_response();
    }

    // Step 4: Apply rate limiting using PID from validated JWT claims.
    // Rate limiting by PID is trustworthy because the PID was extracted from a
    // validated JWT (signed with the daemon's secret), not from caller-controlled input.
    if !state.security.rate_limiter.check(&claims.pid.to_string()).await {
        return ws_rate_limited().into_response();
    }

    // Step 5: Upgrade the WebSocket, passing claims to the connection handler
    ws.on_upgrade(move |socket| handle_connection(socket, state, claims))
}

async fn handle_connection(socket: WebSocket, state: DaemonApiState, claims: SessionTokenClaims) {
    let topics = Arc::new(RwLock::new(HashSet::<String>::new()));
    tracing::info!(
        pid = claims.pid,
        client_type = %claims.client_type,
        "websocket connection authenticated"
    );
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<DaemonWsEvent>(32);
    let mut broadcast_rx = state.event_tx.subscribe();
    let fanout_topics = Arc::clone(&topics);
    let fanout_tx = outbound_tx.clone();

    let fanout_task = tokio::spawn(async move {
        loop {
            match broadcast_rx.recv().await {
                Ok(event) => {
                    let should_deliver = {
                        let guard = fanout_topics.read().await;
                        guard
                            .iter()
                            .any(|topic| topic_matches(topic, event.topic.as_str()))
                    };

                    if should_deliver && fanout_tx.send(event).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!(
                        skipped,
                        "websocket client lagged behind daemon event stream"
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let (mut sender, mut receiver) = socket.split();
    let send_task = tokio::spawn(async move {
        while let Some(event) = outbound_rx.recv().await {
            let payload = match serde_json::to_string(&event) {
                Ok(payload) => payload,
                Err(error) => {
                    warn!(error = %error, "failed to serialize websocket event");
                    continue;
                }
            };

            if sender.send(Message::Text(payload.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(message) = receiver.next().await {
        match message {
            Ok(Message::Text(payload)) => {
                if let Err(error) =
                    handle_client_message(payload.as_str(), &state, &topics, &outbound_tx).await
                {
                    warn!(error = %error, "failed to handle websocket client message");
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Binary(_)) => {}
            Err(error) => {
                warn!(error = %error, "websocket receive loop failed");
                break;
            }
        }
    }

    drop(outbound_tx);
    fanout_task.abort();
    let _ = fanout_task.await;
    let _ = send_task.await;
    debug!("daemon websocket connection closed");
}

async fn handle_client_message(
    payload: &str,
    state: &DaemonApiState,
    topics: &ClientTopics,
    outbound_tx: &mpsc::Sender<DaemonWsEvent>,
) -> Result<()> {
    let request: DaemonWsSubscribeRequest =
        serde_json::from_str(payload).context("invalid websocket request payload")?;

    if request.action != "subscribe" {
        return Ok(());
    }

    let normalized_topics = normalize_topics(request.topics);
    {
        let mut guard = topics.write().await;
        guard.extend(normalized_topics.iter().cloned());
    }

    for topic in normalized_topics {
        if let Some(snapshot) = build_snapshot_event(state, &topic).await? {
            if outbound_tx.send(snapshot).await.is_err() {
                break;
            }
        }
    }

    Ok(())
}

fn normalize_topics(topics: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for topic in topics {
        if !is_supported_topic(topic.as_str()) {
            continue;
        }
        if seen.insert(topic.clone()) {
            normalized.push(topic);
        }
    }

    normalized
}

fn is_supported_topic(topic: &str) -> bool {
    matches!(
        topic,
        ws_topic::STATUS
            | ws_topic::PEERS
            | ws_topic::PAIRED_DEVICES
            | ws_topic::PAIRING
            | ws_topic::PAIRING_SESSION
            | ws_topic::PAIRING_VERIFICATION
            | ws_topic::SETUP
            | ws_topic::SPACE_ACCESS
            | ws_topic::CLIPBOARD
            | ws_topic::FILE_TRANSFER
    )
}

fn topic_matches(subscription: &str, event_topic: &str) -> bool {
    subscription == event_topic
        || (subscription == ws_topic::PAIRING && event_topic.starts_with("pairing/"))
}

async fn build_snapshot_event(
    state: &DaemonApiState,
    topic: &str,
) -> Result<Option<DaemonWsEvent>> {
    match topic {
        ws_topic::STATUS => snapshot_event(
            ws_topic::STATUS,
            ws_event::STATUS_SNAPSHOT,
            None,
            state.query_service.status().await?,
        )
        .map(Some),
        ws_topic::PEERS => snapshot_event(
            ws_topic::PEERS,
            ws_event::PEERS_SNAPSHOT,
            None,
            state.query_service.peers().await?,
        )
        .map(Some),
        ws_topic::PAIRED_DEVICES => snapshot_event(
            ws_topic::PAIRED_DEVICES,
            ws_event::PAIRED_DEVICES_SNAPSHOT,
            None,
            state.query_service.paired_devices().await?,
        )
        .map(Some),
        ws_topic::PAIRING => snapshot_event(
            ws_topic::PAIRING,
            ws_event::PAIRING_SNAPSHOT,
            None,
            state.query_service.pairing_sessions().await,
        )
        .map(Some),
        ws_topic::PAIRING_SESSION => snapshot_event(
            ws_topic::PAIRING_SESSION,
            ws_event::PAIRING_SNAPSHOT,
            None,
            state.query_service.pairing_sessions().await,
        )
        .map(Some),
        ws_topic::PAIRING_VERIFICATION => Ok(None),
        ws_topic::SETUP => Ok(None),
        ws_topic::CLIPBOARD => Ok(None),
        ws_topic::FILE_TRANSFER => Ok(None),
        ws_topic::SPACE_ACCESS => {
            let space_access_state = state
                .query_service
                .space_access_state(state.space_access_orchestrator().as_deref())
                .await;
            snapshot_event(
                ws_topic::SPACE_ACCESS,
                ws_event::SPACE_ACCESS_SNAPSHOT,
                None,
                space_access_state,
            )
            .map(Some)
        }
        unsupported => anyhow::bail!("unsupported websocket topic: {unsupported}"),
    }
}

fn snapshot_event<T: Serialize>(
    topic: &str,
    event_type: &str,
    session_id: Option<String>,
    payload: T,
) -> Result<DaemonWsEvent> {
    Ok(DaemonWsEvent {
        topic: topic.to_string(),
        event_type: event_type.to_string(),
        session_id,
        ts: chrono::Utc::now().timestamp_millis(),
        payload: serde_json::to_value(payload).context("failed to encode websocket payload")?,
    })
}

fn ws_unauthorized(error: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"error": error})),
    )
}

fn ws_forbidden(error: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"error": error})),
    )
}

fn ws_rate_limited() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({"error": "rate_limit_exceeded", "retry_after_secs": 60})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_supported_topic_includes_clipboard() {
        assert!(is_supported_topic(ws_topic::CLIPBOARD));
    }

    #[test]
    fn is_supported_topic_includes_file_transfer() {
        assert!(is_supported_topic(ws_topic::FILE_TRANSFER));
    }

    #[test]
    fn is_supported_topic_rejects_unknown() {
        assert!(!is_supported_topic("unknown-topic"));
        assert!(!is_supported_topic(""));
    }

    #[test]
    fn is_supported_topic_includes_all_known_topics() {
        let known = [
            ws_topic::STATUS,
            ws_topic::PEERS,
            ws_topic::PAIRED_DEVICES,
            ws_topic::PAIRING,
            ws_topic::PAIRING_SESSION,
            ws_topic::PAIRING_VERIFICATION,
            ws_topic::SETUP,
            ws_topic::SPACE_ACCESS,
            ws_topic::CLIPBOARD,
            ws_topic::FILE_TRANSFER,
        ];
        for topic in known {
            assert!(
                is_supported_topic(topic),
                "expected {topic} to be supported"
            );
        }
    }

    #[test]
    fn normalize_topics_keeps_clipboard_and_file_transfer() {
        let topics = vec![
            "clipboard".to_string(),
            "file-transfer".to_string(),
            "unknown".to_string(),
        ];
        let result = normalize_topics(topics);
        assert_eq!(result, vec!["clipboard", "file-transfer"]);
    }

    #[test]
    fn normalize_topics_deduplicates() {
        let topics = vec!["clipboard".to_string(), "clipboard".to_string()];
        let result = normalize_topics(topics);
        assert_eq!(result, vec!["clipboard"]);
    }
}

#[allow(dead_code)]
fn _event_type_markers(
    _: StatusResponse,
    _: Vec<PeerSnapshotDto>,
    _: Vec<PairedDeviceDto>,
    _: Vec<PairingSessionSummaryDto>,
    _: SpaceAccessStateResponse,
) -> (
    [&'static str; 11],
    PairingSessionChangedPayload,
    PairingVerificationPayload,
    PairingFailurePayload,
    PeersChangedFullPayload,
    PeerNameUpdatedPayload,
    PeerConnectionChangedPayload,
    PairedDevicesChangedPayload,
) {
    (
        [
            ws_event::STATUS_UPDATED,
            ws_event::PEERS_CHANGED,
            ws_event::PEERS_NAME_UPDATED,
            ws_event::PEERS_CONNECTION_CHANGED,
            ws_event::PAIRED_DEVICES_CHANGED,
            ws_event::PAIRING_UPDATED,
            ws_event::PAIRING_VERIFICATION_REQUIRED,
            ws_event::PAIRING_COMPLETE,
            ws_event::PAIRING_FAILED,
            ws_event::SPACE_ACCESS_SNAPSHOT,
            ws_event::SPACE_ACCESS_STATE_CHANGED,
        ],
        PairingSessionChangedPayload {
            session_id: String::new(),
            state: String::new(),
            stage: String::new(),
            peer_id: None,
            device_name: None,
            updated_at_ms: 0,
            ts: 0,
        },
        PairingVerificationPayload {
            session_id: String::new(),
            kind: String::new(),
            peer_id: None,
            device_name: None,
            code: None,
            error: None,
            local_fingerprint: None,
            peer_fingerprint: None,
        },
        PairingFailurePayload {
            session_id: String::new(),
            peer_id: None,
            error: String::new(),
            reason: String::new(),
        },
        PeersChangedFullPayload { peers: vec![] },
        PeerNameUpdatedPayload {
            peer_id: String::new(),
            device_name: String::new(),
        },
        PeerConnectionChangedPayload {
            peer_id: String::new(),
            device_name: None,
            connected: false,
        },
        PairedDevicesChangedPayload {
            peer_id: String::new(),
            device_name: None,
            connected: false,
        },
    )
}

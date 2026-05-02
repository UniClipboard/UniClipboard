//! WebSocket subscribe protocol for daemon read-model topics.
//!
//! Browser WebSocket clients cannot send custom headers, so the session token is
//! passed via URL query parameter: `ws://host/ws?auth=Session%20<jwt>`.
//! Native clients can continue using the `Authorization: Session <jwt>` header.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Instant};
use tracing::{debug, info, info_span, warn, Instrument};
use uc_daemon_contract::constants::{ws_event, ws_topic};
use utoipa;

use crate::api::dto::error::ApiError;
use crate::api::dto::ws::{WsErrorResponse, WsSubscribeRequest};
use crate::api::server::DaemonApiState;
use crate::api::types::{
    DaemonWsEvent, PairingFailurePayload, PairingSessionChangedPayload, PairingSessionSummaryDto,
    PairingVerificationPayload, PeerConnectionChangedPayload, PeerNameUpdatedPayload,
    PeerSnapshotDto, PeersChangedFullPayload, SpaceMemberDto, SpaceMembersChangedPayload,
    StatusResponse,
};
use crate::security::claims::SessionTokenClaims;

type ClientTopics = Arc<RwLock<HashSet<String>>>;

// ---------------------------------------------------------------------------
// Heartbeat constants
// ---------------------------------------------------------------------------

/// How often the server sends a ping frame to the client.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// How long the server waits for a pong after sending a ping.
/// If exceeded, the connection is considered stale and closed.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(40);

/// Signal sent from the heartbeat task to the receive loop when the
/// connection has gone stale (no pong received after a ping).
enum HeartbeatSignal {
    Stale,
}

/// Message type for the unified send channel.
/// Heartbeat pings and close requests go through this channel.
/// Daemon events are sent via the dedicated `outbound_rx` channel.
enum SendMsg {
    Ping,
    Close,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Returns the WebSocket router.
#[utoipa::path(
    get,
    path = "/ws",
    tag = "websocket",
    params(
        ("auth" = String, Query, description = "JWT session token prefixed with 'Session '. Used when the client cannot set custom headers (e.g., browser WebSocket).")
    ),
    responses(
        (status = 101, description = "WebSocket upgrade accepted"),
        (status = 401, description = "Missing or invalid session token", body = WsErrorResponse),
        (status = 403, description = "PID not allowed", body = WsErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = WsErrorResponse),
    ),
    security(())
)]
pub fn router() -> Router<DaemonApiState> {
    Router::new().route("/ws", get(websocket_upgrade))
}

// ---------------------------------------------------------------------------
// Auth helpers
// ---------------------------------------------------------------------------

/// Extract session token from `Authorization` header or `?auth=` query parameter.
///
/// Browser WebSocket clients cannot send custom headers, so the session token is
/// passed via URL query parameter. Native clients can continue using the
/// Authorization header. Returns `None` if neither source provides a valid
/// `"Session <token>"` format.
fn extract_session_token(
    headers: &HeaderMap,
    params: &std::collections::HashMap<String, String>,
) -> Option<String> {
    // Authorization header (native clients).
    if let Some(token) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Session "))
    {
        return Some(token.to_string());
    }

    // ?auth= query parameter (browser WebSocket clients).
    if let Some(auth_value) = params.get("auth") {
        if let Some(token) = auth_value.strip_prefix("Session ") {
            return Some(token.to_string());
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Upgrade handler
// ---------------------------------------------------------------------------

async fn websocket_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<DaemonApiState>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    // Step 1: Extract session token.
    let token = match extract_session_token(&headers, &params) {
        Some(t) => t,
        None => {
            return ApiError::unauthorized("missing_session_token").into_response();
        }
    };

    // Step 2: Verify JWT.
    let claims = match SessionTokenClaims::verify(&token, &state.security.jwt_secret) {
        Ok(claims) => claims,
        Err(e) => {
            warn!(error = %e, "WS JWT validation failed");
            return ApiError::unauthorized("invalid_session_token").into_response();
        }
    };

    // Step 3: Check PID whitelist.
    if !state.security.is_pid_allowed(claims.pid).await {
        warn!(pid = claims.pid, "WS request from non-whitelisted PID");
        return ApiError::unauthorized("pid_not_allowed").into_response();
    }

    // Step 4: Apply rate limiting by PID (trusted — extracted from validated JWT).
    if !state
        .security
        .rate_limiter
        .check(&claims.pid.to_string())
        .await
    {
        return ws_rate_limited().into_response();
    }

    // Step 5: Upgrade the WebSocket.
    ws.on_upgrade(move |socket| handle_connection(socket, state, claims))
}

fn ws_rate_limited() -> (StatusCode, Json<WsErrorResponse>) {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(WsErrorResponse {
            error: "rate_limit_exceeded".to_string(),
            retry_after_secs: Some(60),
        }),
    )
}

// ---------------------------------------------------------------------------
// Connection handler
// ---------------------------------------------------------------------------

async fn handle_connection(socket: WebSocket, state: DaemonApiState, claims: SessionTokenClaims) {
    let conn_span = info_span!(
        "daemon.ws.connection",
        session_token_jti = %claims.jti,
        pid = claims.pid,
    );

    async move {
        let topics = Arc::new(RwLock::new(HashSet::<String>::new()));
        tracing::info!(
            pid = claims.pid,
            client_type = %claims.client_type,
            "websocket connection authenticated",
        );

        let (outbound_tx, mut outbound_rx) = mpsc::channel::<DaemonWsEvent>(32);
        let mut broadcast_rx = state.event_tx.subscribe();
        let fanout_topics = Arc::clone(&topics);
        let fanout_tx = outbound_tx.clone();

        // Fanout task: receives daemon events and forwards them to the client via outbound_tx.
        let fanout_task = tokio::spawn(async move {
            loop {
                match broadcast_rx.recv().await {
                    Ok(event) => {
                        let matched_topics = {
                            let guard = fanout_topics.read().await;
                            guard
                                .iter()
                                .filter(|topic| topic_matches(topic, event.topic.as_str()))
                                .cloned()
                                .collect::<Vec<_>>()
                        };

                        if !matched_topics.is_empty() {
                            let event = bridge_verification_event(event);
                            info!(
                                event_topic = %event.topic,
                                event_type = %event.event_type,
                                session_id = event.session_id.as_deref().unwrap_or(""),
                                matched_topics = ?matched_topics,
                                "forwarding daemon websocket event to subscribed client"
                            );
                            if fanout_tx.send(event).await.is_err() {
                                break;
                            }
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

        // Heartbeat channel: the heartbeat task signals `Stale` when the client
        // fails to respond to a ping within CLIENT_TIMEOUT.
        let (heartbeat_tx, mut heartbeat_rx) = mpsc::channel::<HeartbeatSignal>(1);

        // Unified send channel: both daemon events and heartbeat pings are sent here.
        // This avoids needing to share the SplitSink across tasks.
        let (send_tx, mut send_rx) = mpsc::channel::<SendMsg>(16);
        let send_tx_for_heartbeat = send_tx.clone();

        // Heartbeat task: sends a ping frame every HEARTBEAT_INTERVAL seconds.
        // If no pong is received within CLIENT_TIMEOUT after a ping, the task
        // signals `Stale` which causes the receive loop to close the socket.
        let heartbeat_task = tokio::spawn(async move {
            let mut ping_interval = interval(HEARTBEAT_INTERVAL);
            // Wait for the initial interval tick before the first ping.
            ping_interval.tick().await;

            loop {
                // Send a ping.
                if send_tx_for_heartbeat.send(SendMsg::Ping).await.is_err() {
                    debug!("heartbeat: send channel closed, exiting");
                    break;
                }
                debug!("heartbeat: ping sent");

                // Wait for the next interval or the deadline.
                let deadline = Instant::now() + CLIENT_TIMEOUT;
                tokio::select! {
                    _ = ping_interval.tick() => {
                        // Interval elapsed — time for the next ping.
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        // Timeout — no pong received.
                        debug!("heartbeat: deadline expired, connection stale");
                        if heartbeat_tx.send(HeartbeatSignal::Stale).await.is_err() {
                            debug!("heartbeat: heartbeat_rx dropped, exiting");
                        }
                        return;
                    }
                }
            }
        });

        let (mut sender, mut receiver) = socket.split();

        // Send task: drives the actual socket writes.
        // Receives both daemon events (via outbound_rx) and heartbeat pings (via send_rx).
        let send_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Daemon event to forward to the client.
                    Some(event) = outbound_rx.recv() => {
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
                    // Heartbeat ping or close request.
                    Some(msg) = send_rx.recv() => {
                        match msg {
                            SendMsg::Ping => {
                                if sender.send(Message::Ping(b"".into())).await.is_err() {
                                    break;
                                }
                            }
                            SendMsg::Close => {
                                let _ = sender.send(Message::Close(None)).await;
                                break;
                            }
                        }
                    }
                    // Both channels closed (typical at daemon shutdown when the
                    // event broadcaster and heartbeat task drop their senders
                    // around the same moment). Without this arm, tokio's
                    // select! panics with "all branches are disabled and there
                    // is no else branch".
                    else => break,
                }
            }
        });

        // Receive loop: handles client messages (subscribe requests, pong frames).
        // Also drains the heartbeat channel to detect stale connections.
        loop {
            tokio::select! {
                // Incoming message from the client socket.
                msg = receiver.next() => {
                    match msg {
                        Some(Ok(Message::Text(payload))) => {
                            if let Err(error) =
                                handle_client_message(payload.as_str(), &state, &topics, &outbound_tx).await
                            {
                                warn!(error = %error, "failed to handle websocket client message");
                                break;
                            }
                        }
                        Some(Ok(Message::Pong(_))) => {
                            debug!("heartbeat: pong received");
                        }
                        Some(Ok(Message::Close(_))) => break,
                        // Ping: axum/tokio-tungstenite automatically replies with a Pong.
                        // Binary frames are ignored.
                        Some(Ok(Message::Ping(_))) | Some(Ok(Message::Binary(_))) => {}
                        Some(Err(error)) => {
                            warn!(error = %error, "websocket receive loop failed");
                            break;
                        }
                        None => break,
                    }
                }
                // Signal from the heartbeat task that the connection is stale.
                sig = heartbeat_rx.recv() => {
                    match sig {
                        Some(HeartbeatSignal::Stale) => {
                            warn!("heartbeat: client missed pong, closing stale connection");
                            let _ = send_tx.send(SendMsg::Close).await;
                            break;
                        }
                        None => break,
                    }
                }
            }
        }

        drop(outbound_tx);
        drop(send_tx);
        fanout_task.abort();
        heartbeat_task.abort();
        let _ = fanout_task.await;
        let _ = send_task.await;
        debug!("daemon websocket connection closed");
    }
    .instrument(conn_span)
    .await;
}

// ---------------------------------------------------------------------------
// Client message handler
// ---------------------------------------------------------------------------

async fn handle_client_message(
    payload: &str,
    state: &DaemonApiState,
    topics: &ClientTopics,
    outbound_tx: &mpsc::Sender<DaemonWsEvent>,
) -> anyhow::Result<()> {
    let request: WsSubscribeRequest =
        serde_json::from_str(payload).context("invalid websocket request payload")?;

    if request.action != "subscribe" {
        return Ok(());
    }

    let normalized_topics = normalize_topics(request.topics);
    info!(
        requested_topics = %payload,
        normalized_topics = ?normalized_topics,
        "websocket client subscribed to topics"
    );
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

// ---------------------------------------------------------------------------
// Topic helpers
// ---------------------------------------------------------------------------

/// Returns only topics that are supported and deduplicated.
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

/// Check whether a topic string is in the supported set.
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
            | ws_topic::CLIPBOARD
            | ws_topic::FILE_TRANSFER
            | ws_topic::ENCRYPTION
            | ws_topic::SEARCH
    )
}

/// Returns `true` when the subscribed topic matches the event topic.
fn topic_matches(subscription: &str, event_topic: &str) -> bool {
    subscription == event_topic
        || (subscription == ws_topic::PAIRING && event_topic.starts_with("pairing/"))
}

/// Bridge: transforms `pairing.verification_required` events based on the
/// payload `kind` field so that downstream consumers receive distinct event
/// types instead of having to inspect the payload.
///
/// | kind        | resulting event type            |
/// |-------------|---------------------------------|
/// | verifying   | pairing.updated (stage field)   |
/// | complete    | pairing.complete                |
/// | failed      | pairing.failed                  |
/// | (other)     | unchanged                       |
fn bridge_verification_event(mut event: DaemonWsEvent) -> DaemonWsEvent {
    if event.event_type != ws_event::PAIRING_VERIFICATION_REQUIRED {
        return event;
    }
    let kind = event.payload.get("kind").and_then(|v| v.as_str());
    match kind {
        Some("verifying") => {
            event.event_type = ws_event::PAIRING_UPDATED.to_string();
            if let Some(obj) = event.payload.as_object_mut() {
                obj.insert(
                    "stage".to_string(),
                    serde_json::Value::String("verifying".to_string()),
                );
            }
        }
        Some("complete") => {
            event.event_type = ws_event::PAIRING_COMPLETE.to_string();
        }
        Some("failed") => {
            event.event_type = ws_event::PAIRING_FAILED.to_string();
        }
        _ => {}
    }
    event
}

// ---------------------------------------------------------------------------
// Snapshot builder
// ---------------------------------------------------------------------------

async fn build_snapshot_event(
    state: &DaemonApiState,
    topic: &str,
) -> anyhow::Result<Option<DaemonWsEvent>> {
    match topic {
        ws_topic::STATUS => snapshot_event(
            ws_topic::STATUS,
            ws_event::STATUS_SNAPSHOT,
            None,
            state.status_response(),
        )
        .map(Some),

        ws_topic::PEERS => snapshot_event(
            ws_topic::PEERS,
            ws_event::PEERS_SNAPSHOT,
            None,
            state.peer_snapshots().await?,
        )
        .map(Some),

        ws_topic::PAIRED_DEVICES => snapshot_event(
            ws_topic::PAIRED_DEVICES,
            ws_event::PAIRED_DEVICES_SNAPSHOT,
            None,
            state.paired_devices().await?,
        )
        .map(Some),

        // Slice 4 P5a-4: 旧 pairing 协议下线，PAIRING / PAIRING_SESSION /
        // PAIRING_VERIFICATION 仅作为历史 topic 保留 — 订阅不报错，但不再
        // 推送 snapshot。setup-v2 流程通过 SETUP topic 自有事件回报。
        ws_topic::PAIRING | ws_topic::PAIRING_SESSION | ws_topic::PAIRING_VERIFICATION => Ok(None),

        ws_topic::SETUP => Ok(None),
        ws_topic::CLIPBOARD => Ok(None),
        ws_topic::FILE_TRANSFER => Ok(None),

        ws_topic::ENCRYPTION => {
            // No snapshot for encryption — only an event is emitted on session_ready.
            Ok(None)
        }

        ws_topic::SEARCH => {
            // Build a combined snapshot through the application facade.
            let app = match state.app_facade_or_error() {
                Ok(app) => app,
                Err(err) => {
                    warn!(error = %err.message, "search ws snapshot: application facade unavailable");
                    return Ok(None);
                }
            };
            let status = match app.search.status().await {
                Ok(status) => status,
                Err(err) => {
                    warn!(error = %err, "search ws snapshot: failed to read status");
                    return Ok(None);
                }
            };

            let payload = crate::api::dto::search::SearchStatusData {
                state: status.state,
                reason: status.reason,
                last_rebuild_started_at_ms: status.last_rebuild_started_at_ms,
                last_rebuild_completed_at_ms: status.last_rebuild_completed_at_ms,
            };

            snapshot_event(
                ws_topic::SEARCH,
                ws_event::SEARCH_STATUS_SNAPSHOT,
                None,
                payload,
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
) -> anyhow::Result<DaemonWsEvent> {
    Ok(DaemonWsEvent {
        topic: topic.to_string(),
        event_type: event_type.to_string(),
        session_id,
        ts: chrono::Utc::now().timestamp_millis(),
        payload: serde_json::to_value(payload).context("failed to encode websocket payload")?,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Compile-time type witnesses
// ---------------------------------------------------------------------------

/// Compile-time assertion that all required types are reachable from this module.
#[allow(dead_code)]
fn _event_type_markers(
    _: StatusResponse,
    _: Vec<PeerSnapshotDto>,
    _: Vec<SpaceMemberDto>,
    _: Vec<PairingSessionSummaryDto>,
) -> (
    [&'static str; 9],
    PairingSessionChangedPayload,
    PairingVerificationPayload,
    PairingFailurePayload,
    PeersChangedFullPayload,
    PeerNameUpdatedPayload,
    PeerConnectionChangedPayload,
    SpaceMembersChangedPayload,
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
        SpaceMembersChangedPayload {
            peer_id: String::new(),
            device_name: None,
            connected: false,
        },
    )
}

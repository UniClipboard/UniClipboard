use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

use axum::body::{to_bytes, Body};
use axum::http::Request;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::ServiceExt;
use uc_daemon::api::auth::load_or_create_auth_token;
use uc_daemon::api::query::DaemonQueryService;
use uc_daemon::api::server::{build_router, DaemonApiState};
use uc_daemon::api::types::{
    DaemonWsEvent, PairedDevicesChangedPayload, PairingFailurePayload, PairingVerificationPayload,
    PeerConnectionChangedPayload, PeerNameUpdatedPayload, PeerSnapshotDto, PeersChangedFullPayload,
    SetupStateChangedPayload,
};
use uc_daemon::pairing::session_projection::upsert_pairing_snapshot;
use uc_daemon::security::SecurityState;
use uc_daemon::state::RuntimeState;

struct PairingWsHarness {
    app: axum::Router,
    url: String,
    /// JWT session token for HTTP route authentication (L2 middleware requires Session token).
    session_token: String,
    event_tx: tokio::sync::broadcast::Sender<DaemonWsEvent>,
    state: Arc<RwLock<RuntimeState>>,
    handle: tokio::task::JoinHandle<()>,
}

fn build_runtime() -> Arc<uc_app::runtime::CoreRuntime> {
    static RUNTIME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = RUNTIME_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Give each test invocation its own DB to prevent intra-binary SQLite contention.
    let profile = format!(
        "test_pairing_ws_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    );
    std::env::set_var("UC_PROFILE", &profile);
    Arc::new(uc_bootstrap::build_cli_runtime(None).unwrap())
}

async fn spawn_server() -> PairingWsHarness {
    let runtime = build_runtime();
    let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
    let query_service = Arc::new(DaemonQueryService::new(runtime, state.clone()));
    let tempdir = tempfile::tempdir().unwrap();
    let token_path = tempdir.path().join("daemon.token");
    let token = load_or_create_auth_token(&token_path).unwrap();
    // Pre-register the test process PID so session tokens pass the whitelist check.
    let pid = std::process::id();
    let security = Arc::new(SecurityState::new_with_pid(pid));
    let session_token = security.make_session_token_for_pid(pid);
    let api_state = DaemonApiState::new(query_service, token, None, security);
    let event_tx = api_state.event_tx.clone();
    let app = build_router(api_state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_app = app.clone();
    let handle = tokio::spawn(async move {
        axum::serve(listener, server_app.into_make_service())
            .await
            .unwrap();
    });

    PairingWsHarness {
        app,
        url: format!("ws://{}/ws", addr),
        session_token,
        event_tx,
        state,
        handle,
    }
}

async fn connect_with_token(
    url: &str,
    token: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let mut request = url.into_client_request().unwrap();
    // Phase 75+: WS upgrade uses "Session <token>" prefix (JWT session token), not "Bearer".
    request.headers_mut().insert(
        "Authorization",
        format!("Session {}", token.trim()).parse().unwrap(),
    );
    let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    socket
}

async fn subscribe(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    topics: &[&str],
) {
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            json!({"action": "subscribe", "topics": topics})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
}

async fn next_json(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
            .await
            .expect("next_json timed out waiting for WebSocket message")
            .expect("WebSocket stream ended")
            .expect("WebSocket message error");
        match &msg {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                return serde_json::from_str(text)
                    .expect("failed to parse WebSocket message as JSON");
            }
            tokio_tungstenite::tungstenite::Message::Close(reason) => {
                panic!(
                    "server unexpectedly closed WebSocket connection: {:?}",
                    reason
                );
            }
            tokio_tungstenite::tungstenite::Message::Ping(_)
            | tokio_tungstenite::tungstenite::Message::Pong(_) => {
                // Skip control frames and wait for the next data message.
                continue;
            }
            other => {
                panic!("unexpected WebSocket message type: {:?}", other);
            }
        }
    }
}

fn authed_get_request(uri: &str, session_token: &str) -> Request<Body> {
    // Uses JWT session token for L2 HTTP routes (auth_extractor_middleware requires Session prefix)
    Request::builder()
        .uri(uri)
        .header("Authorization", format!("Session {}", session_token.trim()))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn snapshot_contains_session_id_and_omits_verification_secrets() {
    let harness = spawn_server().await;
    upsert_pairing_snapshot(
        &harness.state,
        "session-1",
        Some("peer-1".to_string()),
        Some("Desk".to_string()),
        "request",
        1_742_371_200_000,
    )
    .await;

    let mut socket = connect_with_token(&harness.url, &harness.session_token).await;
    subscribe(&mut socket, &["pairing"]).await;

    let event = next_json(&mut socket).await;
    harness.handle.abort();

    assert_eq!(event["type"], "pairing.snapshot");
    assert!(event.get("event_type").is_none());
    let payload = event["payload"].as_array().unwrap();
    assert_eq!(payload[0]["sessionId"], "session-1");
    assert!(payload[0].get("code").is_none());
    assert!(payload[0].get("localFingerprint").is_none());
    assert!(payload[0].get("peerFingerprint").is_none());
    assert!(payload[0].get("keyslotFile").is_none());
    assert!(payload[0].get("challenge").is_none());
}

#[tokio::test]
async fn incremental_verification_event_contains_code_and_fingerprints() {
    let harness = spawn_server().await;
    let mut socket = connect_with_token(&harness.url, &harness.session_token).await;
    subscribe(&mut socket, &["pairing"]).await;
    let _snapshot = next_json(&mut socket).await;

    harness
        .event_tx
        .send(DaemonWsEvent {
            topic: "pairing/verification".to_string(),
            event_type: "pairing.verification_required".to_string(),
            session_id: Some("session-2".to_string()),
            ts: 1_742_371_200_123,
            payload: serde_json::to_value(PairingVerificationPayload {
                session_id: "session-2".to_string(),
                kind: "verification".to_string(),
                peer_id: Some("peer-2".to_string()),
                device_name: Some("Laptop".to_string()),
                code: Some("123456".to_string()),
                error: None,
                local_fingerprint: Some("local-fp".to_string()),
                peer_fingerprint: Some("peer-fp".to_string()),
            })
            .unwrap(),
        })
        .unwrap();

    let event = next_json(&mut socket).await;
    harness.handle.abort();

    assert_eq!(event["type"], "pairing.verification_required");
    assert_eq!(event["payload"]["kind"], "verification");
    assert_eq!(event["payload"]["code"], "123456");
    assert_eq!(event["payload"]["localFingerprint"], "local-fp");
    assert_eq!(event["payload"]["peerFingerprint"], "peer-fp");
}

#[tokio::test]
async fn peers_and_paired_devices_incremental_events_preserve_bridge_fields() {
    let harness = spawn_server().await;
    let mut socket = connect_with_token(&harness.url, &harness.session_token).await;
    subscribe(&mut socket, &["peers", "paired-devices"]).await;
    let _peers_snapshot = next_json(&mut socket).await;
    let _paired_devices_snapshot = next_json(&mut socket).await;

    harness
        .event_tx
        .send(DaemonWsEvent {
            topic: "peers".to_string(),
            event_type: "peers.changed".to_string(),
            session_id: None,
            ts: 1,
            payload: serde_json::to_value(PeersChangedFullPayload {
                peers: vec![PeerSnapshotDto {
                    peer_id: "peer-3".to_string(),
                    device_name: Some("Desk".to_string()),
                    addresses: vec!["/ip4/127.0.0.1/tcp/7000".to_string()],
                    is_paired: false,
                    connected: true,
                    pairing_state: "NotPaired".to_string(),
                }],
            })
            .unwrap(),
        })
        .unwrap();
    harness
        .event_tx
        .send(DaemonWsEvent {
            topic: "peers".to_string(),
            event_type: "peers.name_updated".to_string(),
            session_id: None,
            ts: 2,
            payload: serde_json::to_value(PeerNameUpdatedPayload {
                peer_id: "peer-3".to_string(),
                device_name: "Renamed".to_string(),
            })
            .unwrap(),
        })
        .unwrap();
    harness
        .event_tx
        .send(DaemonWsEvent {
            topic: "peers".to_string(),
            event_type: "peers.connection_changed".to_string(),
            session_id: None,
            ts: 3,
            payload: serde_json::to_value(PeerConnectionChangedPayload {
                peer_id: "peer-3".to_string(),
                device_name: Some("Renamed".to_string()),
                connected: false,
            })
            .unwrap(),
        })
        .unwrap();
    harness
        .event_tx
        .send(DaemonWsEvent {
            topic: "paired-devices".to_string(),
            event_type: "paired-devices.changed".to_string(),
            session_id: None,
            ts: 4,
            payload: serde_json::to_value(PairedDevicesChangedPayload {
                peer_id: "peer-3".to_string(),
                device_name: Some("Renamed".to_string()),
                connected: false,
            })
            .unwrap(),
        })
        .unwrap();

    let peers_changed = next_json(&mut socket).await;
    let peers_name_updated = next_json(&mut socket).await;
    let peers_connection_changed = next_json(&mut socket).await;
    let paired_devices_changed = next_json(&mut socket).await;
    harness.handle.abort();

    assert_eq!(peers_changed["type"], "peers.changed");
    // peers.changed now carries a full snapshot via PeersChangedFullPayload { peers: [...] }
    assert_eq!(peers_changed["payload"]["peers"][0]["peerId"], "peer-3");
    assert_eq!(peers_changed["payload"]["peers"][0]["deviceName"], "Desk");
    assert_eq!(
        peers_changed["payload"]["peers"][0]["addresses"][0],
        "/ip4/127.0.0.1/tcp/7000"
    );
    assert_eq!(peers_changed["payload"]["peers"][0]["connected"], true);

    assert_eq!(peers_name_updated["type"], "peers.name_updated");
    assert_eq!(peers_name_updated["payload"]["peerId"], "peer-3");
    assert_eq!(peers_name_updated["payload"]["deviceName"], "Renamed");

    assert_eq!(peers_connection_changed["type"], "peers.connection_changed");
    assert_eq!(peers_connection_changed["payload"]["peerId"], "peer-3");
    assert_eq!(peers_connection_changed["payload"]["connected"], false);
    assert!(peers_connection_changed["payload"]
        .get("deviceName")
        .is_some());

    assert_eq!(paired_devices_changed["type"], "paired-devices.changed");
    assert_eq!(paired_devices_changed["payload"]["peerId"], "peer-3");
    assert_eq!(paired_devices_changed["payload"]["connected"], false);
}

#[tokio::test]
async fn websocket_event_uses_type_not_event_type() {
    let harness = spawn_server().await;
    let mut socket = connect_with_token(&harness.url, &harness.session_token).await;
    subscribe(&mut socket, &["pairing"]).await;
    let _snapshot = next_json(&mut socket).await;

    harness
        .event_tx
        .send(DaemonWsEvent {
            topic: "pairing/verification".to_string(),
            event_type: "pairing.failed".to_string(),
            session_id: Some("session-3".to_string()),
            ts: 5,
            payload: serde_json::to_value(PairingFailurePayload {
                session_id: "session-3".to_string(),
                peer_id: Some("peer-3".to_string()),
                error: "transport_failed".to_string(),
                reason: "transport_failed".to_string(),
            })
            .unwrap(),
        })
        .unwrap();

    let event = next_json(&mut socket).await;
    harness.handle.abort();

    assert_eq!(event["type"], "pairing.failed");
    assert!(event.get("event_type").is_none());
}

#[tokio::test]
async fn pairing_session_http_response_omits_verification_secrets_even_with_realtime_event() {
    let harness = spawn_server().await;
    upsert_pairing_snapshot(
        &harness.state,
        "session-4",
        Some("peer-4".to_string()),
        Some("Phone".to_string()),
        "verification",
        1_742_371_200_456,
    )
    .await;

    let mut socket = connect_with_token(&harness.url, &harness.session_token).await;
    subscribe(&mut socket, &["pairing"]).await;
    let _snapshot = next_json(&mut socket).await;

    harness
        .event_tx
        .send(DaemonWsEvent {
            topic: "pairing/verification".to_string(),
            event_type: "pairing.verification_required".to_string(),
            session_id: Some("session-4".to_string()),
            ts: 6,
            payload: serde_json::to_value(PairingVerificationPayload {
                session_id: "session-4".to_string(),
                kind: "verification".to_string(),
                peer_id: Some("peer-4".to_string()),
                device_name: Some("Phone".to_string()),
                code: Some("654321".to_string()),
                error: None,
                local_fingerprint: Some("local-secret".to_string()),
                peer_fingerprint: Some("peer-secret".to_string()),
            })
            .unwrap(),
        })
        .unwrap();
    let _event = next_json(&mut socket).await;

    let response = harness
        .app
        .clone()
        .oneshot(authed_get_request(
            "/pairing/sessions/session-4",
            &harness.session_token,
        ))
        .await
        .unwrap();
    harness.handle.abort();

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["sessionId"], "session-4");
    assert!(json.get("code").is_none());
    assert!(json.get("localFingerprint").is_none());
    assert!(json.get("peerFingerprint").is_none());
    assert!(json.get("keyslotFile").is_none());
    assert!(json.get("challenge").is_none());
}

#[tokio::test]
async fn granular_pairing_topic_subscription_receives_session_events() {
    let harness = spawn_server().await;
    let mut socket = connect_with_token(&harness.url, &harness.session_token).await;
    subscribe(&mut socket, &["pairing/session"]).await;
    let _snapshot = next_json(&mut socket).await;

    harness
        .event_tx
        .send(DaemonWsEvent {
            topic: "pairing/session".to_string(),
            event_type: "pairing.updated".to_string(),
            session_id: Some("session-5".to_string()),
            ts: 7,
            payload: serde_json::json!({
                "sessionId": "session-5",
                "state": "request",
                "stage": "request",
                "peerId": "peer-5",
                "deviceName": "Desk",
                "updatedAtMs": 7,
                "ts": 7
            }),
        })
        .unwrap();

    let event = next_json(&mut socket).await;
    harness.handle.abort();

    assert_eq!(event["type"], "pairing.updated");
    assert_eq!(event["payload"]["stage"], "request");
    assert_eq!(event["payload"]["peerId"], "peer-5");
}

/// Verifies that a `pairing.verification_required` with kind=`verifying` is routed to
/// `pairing.updated` (not `pairing.verification_required`) by the bridge.
/// This makes the bridge's kind-based mapping decision explicit and test-covered.
#[tokio::test]
async fn bridge_routes_verification_required_verifying_kind_to_pairing_updated() {
    let harness = spawn_server().await;
    let mut socket = connect_with_token(&harness.url, &harness.session_token).await;
    subscribe(&mut socket, &["pairing"]).await;
    let _snapshot = next_json(&mut socket).await;

    harness
        .event_tx
        .send(DaemonWsEvent {
            topic: "pairing/verification".to_string(),
            event_type: "pairing.verification_required".to_string(),
            session_id: Some("session-v".to_string()),
            ts: 10,
            payload: serde_json::to_value(PairingVerificationPayload {
                session_id: "session-v".to_string(),
                kind: "verifying".to_string(),
                peer_id: Some("peer-v".to_string()),
                device_name: Some("Laptop".to_string()),
                code: None,
                error: None,
                local_fingerprint: None,
                peer_fingerprint: None,
            })
            .unwrap(),
        })
        .unwrap();

    let event = next_json(&mut socket).await;
    harness.handle.abort();

    // kind=verifying must route to pairing.updated, not pairing.verification_required
    assert_eq!(
        event["type"], "pairing.updated",
        "kind=verifying must be routed to pairing.updated"
    );
    assert_eq!(event["payload"]["sessionId"], "session-v");
    assert_eq!(event["payload"]["stage"], "verifying");
}

/// Verifies that a `pairing.verification_required` with kind=`complete` is routed to
/// `pairing.complete` by the bridge.
#[tokio::test]
async fn bridge_routes_verification_required_complete_kind_to_pairing_complete() {
    let harness = spawn_server().await;
    let mut socket = connect_with_token(&harness.url, &harness.session_token).await;
    subscribe(&mut socket, &["pairing"]).await;
    let _snapshot = next_json(&mut socket).await;

    harness
        .event_tx
        .send(DaemonWsEvent {
            topic: "pairing/verification".to_string(),
            event_type: "pairing.verification_required".to_string(),
            session_id: Some("session-c".to_string()),
            ts: 11,
            payload: serde_json::to_value(PairingVerificationPayload {
                session_id: "session-c".to_string(),
                kind: "complete".to_string(),
                peer_id: Some("peer-c".to_string()),
                device_name: Some("Desktop".to_string()),
                code: None,
                error: None,
                local_fingerprint: None,
                peer_fingerprint: None,
            })
            .unwrap(),
        })
        .unwrap();

    let event = next_json(&mut socket).await;
    harness.handle.abort();

    // kind=complete must route to pairing.complete
    assert_eq!(
        event["type"], "pairing.complete",
        "kind=complete must be routed to pairing.complete"
    );
    assert_eq!(event["payload"]["sessionId"], "session-c");
}

/// Verifies that a `pairing.verification_required` with kind=`failed` is routed to
/// `pairing.failed` by the bridge, and that the error field is forwarded as the reason.
#[tokio::test]
async fn bridge_routes_verification_required_failed_kind_to_pairing_failed() {
    let harness = spawn_server().await;
    let mut socket = connect_with_token(&harness.url, &harness.session_token).await;
    subscribe(&mut socket, &["pairing"]).await;
    let _snapshot = next_json(&mut socket).await;

    harness
        .event_tx
        .send(DaemonWsEvent {
            topic: "pairing/verification".to_string(),
            event_type: "pairing.verification_required".to_string(),
            session_id: Some("session-f".to_string()),
            ts: 12,
            payload: serde_json::to_value(PairingVerificationPayload {
                session_id: "session-f".to_string(),
                kind: "failed".to_string(),
                peer_id: Some("peer-f".to_string()),
                device_name: None,
                code: None,
                error: Some("verification_timeout".to_string()),
                local_fingerprint: None,
                peer_fingerprint: None,
            })
            .unwrap(),
        })
        .unwrap();

    let event = next_json(&mut socket).await;
    harness.handle.abort();

    // kind=failed must route to pairing.failed
    assert_eq!(
        event["type"], "pairing.failed",
        "kind=failed must be routed to pairing.failed"
    );
    assert_eq!(event["payload"]["sessionId"], "session-f");
}

#[tokio::test]
async fn setup_topic_subscription_receives_setup_state_changed_events() {
    let harness = spawn_server().await;
    let mut socket = connect_with_token(&harness.url, &harness.session_token).await;
    subscribe(&mut socket, &["setup"]).await;
    // setup has no snapshot event, so wait for the subscribe to be processed.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    harness
        .event_tx
        .send(DaemonWsEvent {
            topic: "setup".to_string(),
            event_type: "setup.stateChanged".to_string(),
            session_id: Some("session-setup".to_string()),
            ts: 8,
            payload: serde_json::to_value(SetupStateChangedPayload {
                session_id: Some("session-setup".to_string()),
                state: json!({
                    "JoinSpaceConfirmPeer": {
                        "short_code": "123456",
                        "peer_fingerprint": "peer-fp",
                        "error": null
                    }
                }),
            })
            .unwrap(),
        })
        .unwrap();

    let event = next_json(&mut socket).await;
    harness.handle.abort();

    assert_eq!(event["type"], "setup.stateChanged");
    assert_eq!(event["payload"]["sessionId"], "session-setup");
    assert_eq!(
        event["payload"]["state"]["JoinSpaceConfirmPeer"]["short_code"],
        "123456"
    );
}

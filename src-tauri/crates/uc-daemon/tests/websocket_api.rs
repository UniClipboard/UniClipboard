//! Integration tests for the daemon WebSocket API.
//!
//! Covers:
//! - WebSocket upgrade with Authorization header auth
//! - WebSocket upgrade with ?auth= query parameter auth (browser-style)
//! - Auth failure cases (missing, invalid, rate-limited)
//! - Subscribe protocol (snapshots, event delivery)
//! - Envelope serialization (camelCase keys)

use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

use futures_util::{SinkExt, StreamExt};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use uc_daemon::api::auth::load_or_create_auth_token;
use uc_daemon::api::query::DaemonQueryService;
use uc_daemon::api::server::{build_router, DaemonApiState};
use uc_daemon::security::SecurityState;
use uc_daemon::state::RuntimeState;

fn build_runtime() -> Arc<uc_app::runtime::CoreRuntime> {
    static RUNTIME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = RUNTIME_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let profile = format!(
        "websocket-api-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    );
    std::env::set_var("UC_PROFILE", &profile);
    Arc::new(uc_bootstrap::build_cli_runtime(None).unwrap())
}

/// Read the next JSON message from a WebSocket, skipping Ping/Pong frames.
async fn next_json(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
            .await
            .expect("next_json timed out")
            .expect("WebSocket stream ended")
            .expect("WebSocket message error");
        match &msg {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                return serde_json::from_str(text).expect("failed to parse WS message as JSON");
            }
            tokio_tungstenite::tungstenite::Message::Close(reason) => {
                panic!("server closed WebSocket: {:?}", reason);
            }
            tokio_tungstenite::tungstenite::Message::Ping(_)
            | tokio_tungstenite::tungstenite::Message::Pong(_) => continue,
            other => panic!("unexpected WebSocket message: {:?}", other),
        }
    }
}

/// Spawn a test server and return (ws_url, session_token, server_handle).
///
/// After Phase 75, WebSocket connections require a JWT session token in the
/// Authorization header using the "Session <token>" prefix instead of the old
/// "Bearer <bearer>" prefix. The session token is pre-generated using
/// `SecurityState::make_session_token_for_pid()` so tests do not need to call
/// POST /auth/connect separately.
async fn spawn_server() -> (String, String, tokio::task::JoinHandle<()>) {
    let runtime = build_runtime();
    let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
    let query_service = Arc::new(DaemonQueryService::new(runtime, state));
    let tempdir = tempfile::tempdir().unwrap();
    let token_path = tempdir.path().join("daemon.token");
    let token = load_or_create_auth_token(&token_path).unwrap();
    // Pre-register the current test process PID so the WS handler allows the connection
    let pid = std::process::id();
    let security = Arc::new(SecurityState::new_with_pid(pid));
    // Generate a valid JWT session token for the pre-registered PID
    let session_token = security.make_session_token_for_pid(pid);
    let api_state = DaemonApiState::new(query_service, token, None, security);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, build_router(api_state).into_make_service())
            .await
            .unwrap();
    });

    (format!("ws://{}/ws", addr), session_token, handle)
}

/// Connect using Authorization header auth (native/client-style).
async fn connect_with_header_auth(
    url: &str,
    session_token: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "Authorization",
        format!("Session {}", session_token.trim()).parse().unwrap(),
    );
    let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    socket
}

/// Connect using ?auth= query parameter auth (browser-style WebSocket).
/// The auth value is percent-encoded to simulate browser behavior.
async fn connect_with_query_auth(url: &str, session_token: &str) {
    let auth_value = format!("Session {}", session_token.trim());
    let encoded_auth = utf8_percent_encode(&auth_value, NON_ALPHANUMERIC);
    let auth_url = format!("{}?auth={}", url, encoded_auth);
    let request = auth_url.into_client_request().unwrap();
    let result = tokio_tungstenite::connect_async(request).await;
    assert!(
        result.is_ok(),
        "Browser-style query param auth should succeed"
    );
}

// ── Header Auth Tests (native clients) ──────────────────────────

#[tokio::test]
async fn upgrade_rejected_without_session_token() {
    let (url, _token, handle) = spawn_server().await;

    let request = url.into_client_request().unwrap();
    let result = tokio_tungstenite::connect_async(request).await;

    handle.abort();

    assert!(result.is_err());
}

#[tokio::test]
async fn header_auth_succeeds_with_valid_session_token() {
    let (url, token, handle) = spawn_server().await;
    let mut socket = connect_with_header_auth(&url, &token).await;

    // Send a subscribe message to verify the connection is valid
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"action": "subscribe", "topics": ["peers"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    // Should receive a snapshot response
    let message = socket.next().await;
    assert!(message.is_some(), "Should receive snapshot after subscribe");

    handle.abort();
}

// ── Query Param Auth Tests (browser WebSocket clients) ───────────

#[tokio::test]
async fn query_param_auth_succeeds_with_valid_session_token() {
    let (url, token, handle) = spawn_server().await;

    // Connect using browser-style query parameter auth
    connect_with_query_auth(&url, &token).await;

    // Manually connect and verify we can subscribe
    let auth_value = format!("Session {}", token.trim());
    let encoded_auth = utf8_percent_encode(&auth_value, NON_ALPHANUMERIC);
    let auth_url = format!("{}?auth={}", url, encoded_auth);

    let request = auth_url.into_client_request().unwrap();
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"action": "subscribe", "topics": ["peers"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let json: Value = next_json(&mut socket).await;

    handle.abort();

    assert_eq!(json["type"], "peers.snapshot");
    assert_eq!(json["topic"], "peers");
}

#[tokio::test]
async fn query_param_auth_with_url_encoded_session_prefix() {
    let (url, token, handle) = spawn_server().await;

    // "Session " with space is percent-encoded as "Session%20"
    let encoded_token = utf8_percent_encode(&token.trim(), NON_ALPHANUMERIC);
    let auth_url = format!("{}?auth=Session%20{}", url, encoded_token);
    let request = auth_url.into_client_request().unwrap();
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"action": "subscribe", "topics": ["status"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let json: Value = next_json(&mut socket).await;

    handle.abort();

    assert_eq!(json["type"], "status.snapshot");

    // Verify the envelope uses camelCase keys (browser receives these)
    assert!(json.get("sessionId").is_some());
}

#[tokio::test]
async fn query_param_auth_fails_with_missing_auth_param() {
    let (url, _token, handle) = spawn_server().await;

    // Connect without any auth parameter
    let request = url.into_client_request().unwrap();
    let result = tokio_tungstenite::connect_async(request).await;

    handle.abort();

    assert!(result.is_err(), "Connection without auth should fail");
}

// ── Negative Auth Tests ─────────────────────────────────────────

#[tokio::test]
async fn auth_fails_with_invalid_jwt_token() {
    let (url, _token, handle) = spawn_server().await;

    // Use a clearly invalid token
    let auth_url = format!("{}?auth=Session%20invalid.jwt.token", url);
    let request = auth_url.into_client_request().unwrap();
    let result = tokio_tungstenite::connect_async(request).await;

    handle.abort();

    // The upgrade should fail because the JWT is invalid
    assert!(result.is_err());
}

#[tokio::test]
async fn auth_fails_with_wrong_prefix_in_header() {
    let (url, token, handle) = spawn_server().await;

    // Use "Bearer" prefix instead of "Session"
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", token.trim()).parse().unwrap(),
    );
    let result = tokio_tungstenite::connect_async(request).await;

    handle.abort();

    assert!(result.is_err(), "Bearer prefix should be rejected");
}

#[tokio::test]
async fn auth_fails_with_wrong_prefix_in_query_param() {
    let (url, token, handle) = spawn_server().await;

    // Use "Bearer" prefix in query parameter
    let encoded_token = utf8_percent_encode(&token.trim(), NON_ALPHANUMERIC);
    let auth_url = format!("{}?auth=Bearer%20{}", url, encoded_token);
    let request = auth_url.into_client_request().unwrap();
    let result = tokio_tungstenite::connect_async(request).await;

    handle.abort();

    assert!(
        result.is_err(),
        "Bearer prefix in query param should be rejected"
    );
}

#[tokio::test]
async fn auth_fails_with_empty_token() {
    let (url, _token, handle) = spawn_server().await;

    // Empty token after "Session " prefix
    let auth_url = format!("{}?auth=Session%20", url);
    let request = auth_url.into_client_request().unwrap();
    let result = tokio_tungstenite::connect_async(request).await;

    handle.abort();

    assert!(result.is_err(), "Empty token should be rejected");
}

#[tokio::test]
async fn auth_fails_with_malformed_query_string() {
    let (url, token, handle) = spawn_server().await;

    // Malformed: "Session" without space
    let encoded_token = utf8_percent_encode(&token.trim(), NON_ALPHANUMERIC);
    let auth_url = format!("{}?auth=Session{}", url, encoded_token);
    let request = auth_url.into_client_request().unwrap();
    let result = tokio_tungstenite::connect_async(request).await;

    handle.abort();

    assert!(result.is_err(), "Malformed auth value should be rejected");
}

// ── Subscribe Protocol Tests ────────────────────────────────────

#[tokio::test]
async fn subscribe_peers_yields_peers_snapshot_first() {
    let (url, token, handle) = spawn_server().await;
    let mut socket = connect_with_header_auth(&url, &token).await;

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"action": "subscribe", "topics": ["peers"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let json: Value = next_json(&mut socket).await;

    handle.abort();

    assert_eq!(json["type"], "peers.snapshot");
    assert_eq!(json["topic"], "peers");
}

#[tokio::test]
async fn subscribe_multiple_topics_yields_one_snapshot_per_topic() {
    let (url, token, handle) = spawn_server().await;
    let mut socket = connect_with_header_auth(&url, &token).await;

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"action": "subscribe", "topics": ["peers", "paired-devices"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let first: Value =
        next_json(&mut socket).await;
    let second: Value =
        next_json(&mut socket).await;

    handle.abort();

    assert_eq!(first["type"], "peers.snapshot");
    assert_eq!(second["type"], "paired-devices.snapshot");
}

// ── Envelope Serialization Tests ───────────────────────────────

#[tokio::test]
async fn serialized_event_contains_session_id_key_and_not_snake_case() {
    let (url, token, handle) = spawn_server().await;
    let mut socket = connect_with_header_auth(&url, &token).await;

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"action": "subscribe", "topics": ["pairing"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let json: Value =
        next_json(&mut socket).await;

    handle.abort();

    assert!(json.get("sessionId").is_some());
    assert!(json.get("session_id").is_none());
}

#[tokio::test]
async fn serialized_event_uses_type_not_event_type_key() {
    let (url, token, handle) = spawn_server().await;
    let mut socket = connect_with_header_auth(&url, &token).await;

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"action": "subscribe", "topics": ["peers"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let json: Value =
        next_json(&mut socket).await;

    handle.abort();

    // The envelope must use "type" (camelCase), not "event_type" (snake_case)
    assert!(json.get("type").is_some(), "Event must have 'type' key");
    assert!(
        json.get("event_type").is_none(),
        "Event must NOT have 'event_type' key"
    );
}

#[tokio::test]
async fn pairing_snapshot_payload_omits_keyslot_file_and_raw_challenge() {
    let (url, token, handle) = spawn_server().await;
    let mut socket = connect_with_header_auth(&url, &token).await;

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"action": "subscribe", "topics": ["pairing"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let json: Value =
        next_json(&mut socket).await;

    handle.abort();

    let payload = serde_json::to_string(&json["payload"]).unwrap();
    assert!(!payload.contains("keyslotFile"));
    assert!(!payload.contains("challenge"));
}

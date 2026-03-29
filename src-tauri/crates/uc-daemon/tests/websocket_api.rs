use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

use futures_util::{SinkExt, StreamExt};
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
    Arc::new(uc_bootstrap::build_cli_runtime(None).unwrap())
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

async fn connect_with_token(
    url: &str,
    session_token: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let mut request = url.into_client_request().unwrap();
    // Phase 75: WS upgrade uses "Session <token>" prefix (JWT session token), not "Bearer".
    request.headers_mut().insert(
        "Authorization",
        format!("Session {}", session_token.trim()).parse().unwrap(),
    );
    let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    socket
}

#[tokio::test]
async fn upgrade_rejected_without_session_token() {
    let (url, _token, handle) = spawn_server().await;

    let request = url.into_client_request().unwrap();
    let result = tokio_tungstenite::connect_async(request).await;

    handle.abort();

    assert!(result.is_err());
}

#[tokio::test]
async fn subscribe_peers_yields_peers_snapshot_first() {
    let (url, token, handle) = spawn_server().await;
    let mut socket = connect_with_token(&url, &token).await;

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"action": "subscribe", "topics": ["peers"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let message = socket.next().await.unwrap().unwrap();
    let json: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();

    handle.abort();

    assert_eq!(json["type"], "peers.snapshot");
    assert_eq!(json["topic"], "peers");
}

#[tokio::test]
async fn subscribe_multiple_topics_yields_one_snapshot_per_topic() {
    let (url, token, handle) = spawn_server().await;
    let mut socket = connect_with_token(&url, &token).await;

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"action": "subscribe", "topics": ["peers", "paired-devices"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let first: Value =
        serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
    let second: Value =
        serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();

    handle.abort();

    assert_eq!(first["type"], "peers.snapshot");
    assert_eq!(second["type"], "paired-devices.snapshot");
}

#[tokio::test]
async fn serialized_event_contains_session_id_key_and_not_snake_case() {
    let (url, token, handle) = spawn_server().await;
    let mut socket = connect_with_token(&url, &token).await;

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"action": "subscribe", "topics": ["pairing"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let json: Value =
        serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();

    handle.abort();

    assert!(json.get("sessionId").is_some());
    assert!(json.get("session_id").is_none());
}

#[tokio::test]
async fn pairing_snapshot_payload_omits_keyslot_file_and_raw_challenge() {
    let (url, token, handle) = spawn_server().await;
    let mut socket = connect_with_token(&url, &token).await;

    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({"action": "subscribe", "topics": ["pairing"]})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();

    let json: Value =
        serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();

    handle.abort();

    let payload = serde_json::to_string(&json["payload"]).unwrap();
    assert!(!payload.contains("keyslotFile"));
    assert!(!payload.contains("challenge"));
}

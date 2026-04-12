//! WebSocket integration tests for search snapshot and rebuild progress events.
//!
//! Tests connect to the real daemon WebSocket router, subscribe using the
//! production JSON contract, and assert `DaemonWsEvent` payloads directly.
//!
//! These tests prove that:
//! - Subscribing to topic `search` yields an immediate `search.status_snapshot`.
//! - Triggering `POST /search/rebuild` emits `search.rebuild_progress` events.
//! - The event stream contains payloads with `stage == "started"` and `stage == "complete"`.
//! - After locking encryption, `/search/status` and `/search/rebuild` return 423.

use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tower::ServiceExt;
use uc_app::runtime::CoreRuntime;
use uc_core::security::model::MasterKey;
use uc_daemon::api::auth::load_or_create_auth_token;
use uc_daemon::api::query::DaemonQueryService;
use uc_daemon::api::server::{build_router, DaemonApiState};
use uc_daemon::search::coordinator::SearchCoordinator;
use uc_daemon::security::SecurityState;
use uc_daemon::state::RuntimeState;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct SearchWsHarness {
    /// HTTP app for oneshot requests (search routes).
    app: axum::Router,
    ws_url: String,
    session_token: String,
    runtime: Arc<CoreRuntime>,
    handle: tokio::task::JoinHandle<()>,
}

fn build_runtime() -> Arc<CoreRuntime> {
    static RUNTIME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = RUNTIME_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner());

    let profile = format!(
        "test_search_ws_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    );
    std::env::set_var("UC_PROFILE", &profile);
    Arc::new(uc_bootstrap::build_cli_runtime(None).expect("build_cli_runtime failed"))
}

async fn spawn_server() -> SearchWsHarness {
    let runtime = build_runtime();

    let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
    let query_service = Arc::new(DaemonQueryService::new(runtime.clone(), state));
    let tempdir = tempfile::tempdir().unwrap();
    let token_path = tempdir.path().join("daemon.token");
    let token = load_or_create_auth_token(&token_path).unwrap();

    let pid = std::process::id();
    let security = Arc::new(SecurityState::new_with_pid(pid));
    let session_token = security.make_session_token_for_pid(pid);

    // Build the DaemonApiState first so we get its event_tx (the channel the WS
    // fanout subscribes to). Then create the SearchCoordinator with the SAME event_tx
    // so that rebuild progress events reach WS clients.
    let api_state_base = DaemonApiState::new(query_service, token, Some(runtime.clone()), security);
    let coordinator = Arc::new(SearchCoordinator::new(
        runtime.clone(),
        api_state_base.event_tx.clone(),
    ));
    let api_state = api_state_base.with_search_coordinator(coordinator);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_router(api_state);

    let server_app = app.clone();
    let handle = tokio::spawn(async move {
        axum::serve(listener, server_app.into_make_service())
            .await
            .unwrap();
    });

    SearchWsHarness {
        app,
        ws_url: format!("ws://{}/ws", addr),
        session_token,
        runtime,
        handle,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn connect_ws(
    url: &str,
    token: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let mut request = url.into_client_request().unwrap();
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

/// Read the next JSON message, skipping Ping/Pong control frames.
async fn next_json(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(10), socket.next())
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
                panic!("server closed WebSocket unexpectedly: {:?}", reason);
            }
            tokio_tungstenite::tungstenite::Message::Ping(_)
            | tokio_tungstenite::tungstenite::Message::Pong(_) => continue,
            other => panic!("unexpected WebSocket message: {:?}", other),
        }
    }
}

/// Make an authenticated HTTP POST or GET request via tower oneshot.
async fn auth_http(
    app: &axum::Router,
    session_token: &str,
    method: Method,
    uri: &str,
) -> axum::response::Response {
    use axum::http::header::AUTHORIZATION;

    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Session {}", session_token))
        .body(Body::empty())
        .unwrap();

    app.clone().oneshot(request).await.unwrap()
}

async fn unlock_encryption(runtime: &Arc<CoreRuntime>) {
    let master_key = MasterKey::generate().expect("MasterKey::generate failed");
    runtime
        .wiring_deps()
        .security
        .encryption_session
        .set_master_key(master_key)
        .await
        .expect("set_master_key failed");
}

async fn lock_encryption(runtime: &Arc<CoreRuntime>) {
    runtime
        .wiring_deps()
        .security
        .encryption_session
        .clear()
        .await
        .expect("clear failed");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Proves:
/// - Subscribing to topic `search` yields an immediate `search.status_snapshot`.
/// - After unlocking, `POST /search/rebuild` returns 202 and emits at least one
///   `search.rebuild_progress` event over the WebSocket.
/// - After locking again, both `/search/status` and `/search/rebuild` return HTTP 423.
#[tokio::test]
async fn search_status_and_rebuild_routes_enforce_lock_and_emit_progress() {
    let harness = spawn_server().await;

    // Connect and subscribe to the `search` topic before unlocking.
    let mut socket = connect_ws(&harness.ws_url, &harness.session_token).await;
    subscribe(&mut socket, &["search"]).await;

    // --- Step 1: Immediately receive a search.status_snapshot ---
    let snapshot = next_json(&mut socket).await;
    assert_eq!(
        snapshot["type"], "search.status_snapshot",
        "first event on search topic subscription must be search.status_snapshot, got: {}",
        snapshot
    );
    assert_eq!(snapshot["topic"], "search");

    // --- Step 2: Unlock encryption, trigger rebuild, observe progress event ---
    unlock_encryption(&harness.runtime).await;

    let rebuild_resp = auth_http(
        &harness.app,
        &harness.session_token,
        Method::POST,
        "/search/rebuild",
    )
    .await;
    assert_eq!(
        rebuild_resp.status(),
        StatusCode::ACCEPTED,
        "POST /search/rebuild should return 202 when encryption is unlocked"
    );

    // Receive events until we find at least one search.rebuild_progress event.
    // The coordinator emits status_snapshot(rebuilding) first, then rebuild_progress events.
    let mut found_progress = false;
    let mut progress_event = serde_json::Value::Null;
    for _ in 0..10 {
        let event = next_json(&mut socket).await;
        if event["type"] == "search.rebuild_progress" {
            found_progress = true;
            progress_event = event;
            break;
        }
        // Skip status_snapshot and other non-progress events.
    }
    assert!(
        found_progress,
        "should receive at least one search.rebuild_progress after triggering rebuild"
    );
    assert_eq!(progress_event["topic"], "search");
    // The payload must have a `stage` field.
    assert!(
        progress_event["payload"]["stage"].is_string(),
        "rebuild_progress payload must have a 'stage' field, got: {}",
        progress_event["payload"]
    );

    // --- Step 3: Lock encryption → 423 on status and rebuild ---
    lock_encryption(&harness.runtime).await;

    let status_locked = auth_http(
        &harness.app,
        &harness.session_token,
        Method::GET,
        "/search/status",
    )
    .await;
    assert_eq!(
        status_locked.status(),
        StatusCode::LOCKED,
        "/search/status must return 423 when locked"
    );

    let rebuild_locked = auth_http(
        &harness.app,
        &harness.session_token,
        Method::POST,
        "/search/rebuild",
    )
    .await;
    assert_eq!(
        rebuild_locked.status(),
        StatusCode::LOCKED,
        "/search/rebuild must return 423 when locked"
    );

    harness.handle.abort();
}

/// Proves that the rebuild progress event stream contains:
/// - At least one event with `stage == "started"`.
/// - At least one event with `stage == "complete"`.
/// - Both events use `topic == "search"` and `type == "search.rebuild_progress"`.
#[tokio::test]
async fn search_rebuild_websocket_events_include_started_and_complete() {
    let harness = spawn_server().await;

    // Unlock encryption before subscribing so rebuild can run immediately.
    unlock_encryption(&harness.runtime).await;

    // Connect and subscribe to `search` topic.
    let mut socket = connect_ws(&harness.ws_url, &harness.session_token).await;
    subscribe(&mut socket, &["search"]).await;

    // Consume the initial snapshot.
    let snapshot = next_json(&mut socket).await;
    assert_eq!(
        snapshot["type"], "search.status_snapshot",
        "expected initial search.status_snapshot, got: {}",
        snapshot
    );

    // Trigger a rebuild (empty index → completes quickly, emits started + complete).
    let rebuild_resp = auth_http(
        &harness.app,
        &harness.session_token,
        Method::POST,
        "/search/rebuild",
    )
    .await;
    assert_eq!(
        rebuild_resp.status(),
        StatusCode::ACCEPTED,
        "POST /search/rebuild should be accepted"
    );

    // Collect events until we see both `started` and `complete` stage values.
    let mut saw_started = false;
    let mut saw_complete = false;

    // Poll up to 20 events with 10s timeout each. An empty-index rebuild should complete
    // in milliseconds, emitting: status_snapshot (rebuilding), started, complete,
    // status_snapshot (ready).
    for _ in 0..20 {
        let event =
            tokio::time::timeout(std::time::Duration::from_secs(10), next_json(&mut socket))
                .await
                .expect("timed out waiting for rebuild progress or completion event");

        let event_type = event["type"].as_str().unwrap_or("");

        if event_type == "search.rebuild_progress" {
            assert_eq!(
                event["topic"], "search",
                "rebuild_progress must use topic=search"
            );
            assert_eq!(
                event["type"], "search.rebuild_progress",
                "event type must be search.rebuild_progress"
            );

            let stage = event["payload"]["stage"].as_str().unwrap_or("");
            if stage == "started" {
                saw_started = true;
            }
            if stage == "complete" {
                saw_complete = true;
            }
        }

        if saw_started && saw_complete {
            break;
        }

        // A final status_snapshot (ready) signals rebuild is done.
        // If we've been collecting events and the rebuild has finished but we
        // haven't seen both stages, keep waiting (may still be in flight).
        if event_type == "search.status_snapshot"
            && event["payload"]["state"].as_str() == Some("ready")
            && (saw_started || saw_complete)
        {
            // Rebuild finished. If we're missing a stage, it may come before
            // the final snapshot. Break to report the failure.
            break;
        }
    }

    harness.handle.abort();

    assert!(
        saw_started,
        "rebuild progress event stream must contain at least one event with stage='started'"
    );
    assert!(
        saw_complete,
        "rebuild progress event stream must contain at least one event with stage='complete'"
    );
}

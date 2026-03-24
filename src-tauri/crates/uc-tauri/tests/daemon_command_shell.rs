use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::MockRuntime;
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::{InvokeRequest, WebviewWindowBuilder};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uc_core::setup::{SetupError, SetupState};
use uc_daemon::api::auth::DaemonConnectionInfo;
use uc_daemon_client::DaemonConnectionState;
use uc_tauri::commands::pairing::PairedPeer;

const PAIRING_COMMANDS_SOURCE: &str = include_str!("../src/commands/pairing.rs");

#[derive(Debug, Clone, PartialEq, Eq)]
struct CapturedRequest {
    method: String,
    path: String,
    body: String,
}

#[derive(Debug, Clone)]
struct MockHttpResponse {
    status_line: &'static str,
    body: Option<String>,
}

impl MockHttpResponse {
    fn json(value: Value) -> Self {
        Self {
            status_line: "HTTP/1.1 200 OK",
            body: Some(serde_json::to_string(&value).expect("serialize mock json body")),
        }
    }

    fn no_content() -> Self {
        Self {
            status_line: "HTTP/1.1 204 No Content",
            body: None,
        }
    }
}

fn invoke_command<T: DeserializeOwned>(
    webview: &tauri::WebviewWindow<MockRuntime>,
    cmd: &str,
    body: Value,
) -> Result<T, Value> {
    get_ipc_response(
        webview,
        InvokeRequest {
            cmd: cmd.to_string(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "http://tauri.localhost".parse().expect("valid url"),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|response| response.deserialize::<T>().expect("deserialize response"))
}

async fn spawn_http_server(
    responses: Vec<MockHttpResponse>,
) -> (SocketAddr, Arc<Mutex<Vec<CapturedRequest>>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake daemon");
    let addr = listener.local_addr().expect("local addr");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();

    tokio::spawn(async move {
        for response in responses {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut buffer = vec![0u8; 4096];
            let size = stream.read(&mut buffer).await.expect("read request");
            let request = String::from_utf8_lossy(&buffer[..size]).to_string();

            let mut lines = request.lines();
            let first_line = lines.next().expect("request line");
            let mut parts = first_line.split_whitespace();
            let method = parts.next().expect("request method").to_string();
            let path = parts.next().expect("request path").to_string();
            let body = request
                .split("\r\n\r\n")
                .nth(1)
                .unwrap_or_default()
                .to_string();

            captured
                .lock()
                .expect("lock captured requests")
                .push(CapturedRequest { method, path, body });

            let wire_response = match response.body {
                Some(body) => format!(
                    "{}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                    response.status_line,
                    body.len(),
                    body
                ),
                None => format!("{}\r\ncontent-length: 0\r\n\r\n", response.status_line),
            };

            stream
                .write_all(wire_response.as_bytes())
                .await
                .expect("write response");
        }
    });

    (addr, requests)
}

fn build_pairing_webview(
    daemon_connection: DaemonConnectionState,
) -> tauri::WebviewWindow<MockRuntime> {
    let app = mock_builder()
        .manage(daemon_connection)
        .invoke_handler(tauri::generate_handler![
            uc_tauri::commands::pairing::list_paired_devices,
            uc_tauri::commands::pairing::get_paired_peers_with_status,
            uc_tauri::commands::pairing::unpair_p2p_device
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock pairing app");
    WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("build pairing webview")
}

fn build_setup_webview(
    daemon_connection: DaemonConnectionState,
) -> tauri::WebviewWindow<MockRuntime> {
    let app = mock_builder()
        .manage(daemon_connection)
        .invoke_handler(tauri::generate_handler![
            uc_tauri::commands::setup::get_setup_state,
            uc_tauri::commands::setup::start_new_space,
            uc_tauri::commands::setup::start_join_space,
            uc_tauri::commands::setup::select_device,
            uc_tauri::commands::setup::submit_passphrase,
            uc_tauri::commands::setup::verify_passphrase,
            uc_tauri::commands::setup::confirm_peer_trust,
            uc_tauri::commands::setup::cancel_setup
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock setup app");
    WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("build setup webview")
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_commands_use_daemon_queries_and_unpair_route() {
    let (addr, captured_requests) = spawn_http_server(vec![
        MockHttpResponse::json(json!([
            {
                "peerId": "peer-a",
                "deviceName": "Peer A",
                "pairingState": "Trusted",
                "lastSeenAtMs": 1_704_067_200_000_i64,
                "connected": true
            }
        ])),
        MockHttpResponse::json(json!([
            {
                "peerId": "peer-a",
                "deviceName": "Peer A",
                "pairingState": "Trusted",
                "lastSeenAtMs": 1_704_067_200_000_i64,
                "connected": true
            }
        ])),
        MockHttpResponse::no_content(),
    ])
    .await;

    let daemon_connection = DaemonConnectionState::default();
    daemon_connection.set(DaemonConnectionInfo {
        base_url: format!("http://{addr}"),
        ws_url: format!("ws://{addr}/ws"),
        token: "test-token".to_string(),
    });
    let webview = build_pairing_webview(daemon_connection);

    let listed: Vec<PairedPeer> =
        invoke_command(&webview, "list_paired_devices", json!({})).expect("list paired devices");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].peer_id, "peer-a");
    assert_eq!(listed[0].device_name, "Peer A");
    assert!(listed[0].shared_secret.is_empty());
    assert_eq!(listed[0].paired_at, "");
    assert!(listed[0].last_known_addresses.is_empty());
    assert_eq!(
        listed[0].last_seen.as_deref(),
        Some("2024-01-01T00:00:00+00:00")
    );
    assert!(listed[0].connected);

    let with_status: Vec<PairedPeer> =
        invoke_command(&webview, "get_paired_peers_with_status", json!({}))
            .expect("get paired peers with status");
    assert_eq!(with_status.len(), listed.len());
    assert_eq!(with_status[0].peer_id, listed[0].peer_id);
    assert_eq!(with_status[0].device_name, listed[0].device_name);
    assert_eq!(with_status[0].last_seen, listed[0].last_seen);
    assert_eq!(with_status[0].connected, listed[0].connected);

    let unpair_result: Result<(), Value> =
        invoke_command(&webview, "unpair_p2p_device", json!({ "peerId": "peer-a" }));
    unpair_result.expect("unpair device");

    let requests = captured_requests
        .lock()
        .expect("lock captured requests")
        .clone();
    assert_eq!(
        requests,
        vec![
            CapturedRequest {
                method: "GET".to_string(),
                path: "/paired-devices".to_string(),
                body: String::new(),
            },
            CapturedRequest {
                method: "GET".to_string(),
                path: "/paired-devices".to_string(),
                body: String::new(),
            },
            CapturedRequest {
                method: "POST".to_string(),
                path: "/pairing/unpair".to_string(),
                body: "{\"peerId\":\"peer-a\"}".to_string(),
            },
        ]
    );

    assert!(
        !PAIRING_COMMANDS_SOURCE.contains("p2p-command-error"),
        "legacy p2p-command-error emitter should stay removed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn setup_commands_forward_to_daemon_and_deserialize_setup_state() {
    let (addr, captured_requests) = spawn_http_server(vec![
        MockHttpResponse::json(json!({
            "state": { "JoinSpaceInputPassphrase": { "error": null } },
            "sessionId": "session-1",
            "nextStepHint": "join-enter-passphrase",
            "profile": "default",
            "clipboardMode": "full",
            "deviceName": "Peer A",
            "peerId": "peer-a",
            "selectedPeerId": null,
            "selectedPeerName": null,
            "hasCompleted": false
        })),
        MockHttpResponse::json(json!({
            "state": { "CreateSpaceInputPassphrase": { "error": null } },
            "sessionId": "session-2",
            "nextStepHint": "create-space-passphrase"
        })),
        MockHttpResponse::json(json!({
            "state": { "JoinSpaceSelectDevice": { "error": null } },
            "sessionId": "session-3",
            "nextStepHint": "join-select-peer"
        })),
        MockHttpResponse::json(json!({
            "state": { "ProcessingJoinSpace": { "message": "dialing" } },
            "sessionId": "session-4",
            "nextStepHint": "join-select-peer"
        })),
        MockHttpResponse::json(json!({
            "state": {
                "JoinSpaceConfirmPeer": {
                    "short_code": "123456",
                    "peer_fingerprint": "fp-1",
                    "error": null
                }
            },
            "sessionId": "session-5",
            "nextStepHint": "host-confirm-peer"
        })),
        MockHttpResponse::json(json!({
            "state": "Completed",
            "sessionId": "session-6",
            "nextStepHint": "done"
        })),
        MockHttpResponse::json(json!({
            "state": "Welcome",
            "sessionId": "session-7",
            "nextStepHint": "idle"
        })),
    ])
    .await;

    let daemon_connection = DaemonConnectionState::default();
    daemon_connection.set(DaemonConnectionInfo {
        base_url: format!("http://{addr}"),
        ws_url: format!("ws://{addr}/ws"),
        token: "test-token".to_string(),
    });
    let webview = build_setup_webview(daemon_connection);

    let setup_state: SetupState =
        invoke_command(&webview, "get_setup_state", json!({})).expect("get setup state");
    assert_eq!(
        setup_state,
        SetupState::JoinSpaceInputPassphrase { error: None }
    );

    let new_space_state: SetupState =
        invoke_command(&webview, "start_new_space", json!({})).expect("start new space");
    assert_eq!(
        new_space_state,
        SetupState::CreateSpaceInputPassphrase { error: None }
    );

    let join_space_state: SetupState =
        invoke_command(&webview, "start_join_space", json!({})).expect("start join space");
    assert_eq!(
        join_space_state,
        SetupState::JoinSpaceSelectDevice { error: None }
    );

    let select_device_state: SetupState =
        invoke_command(&webview, "select_device", json!({ "peerId": "peer-b" }))
            .expect("select device");
    assert_eq!(
        select_device_state,
        SetupState::ProcessingJoinSpace {
            message: Some("dialing".to_string()),
        }
    );

    let verify_state: SetupState = invoke_command(
        &webview,
        "verify_passphrase",
        json!({ "passphrase": "join-passphrase" }),
    )
    .expect("verify passphrase");
    assert_eq!(
        verify_state,
        SetupState::JoinSpaceConfirmPeer {
            short_code: "123456".to_string(),
            peer_fingerprint: Some("fp-1".to_string()),
            error: None,
        }
    );

    let confirm_state: SetupState =
        invoke_command(&webview, "confirm_peer_trust", json!({})).expect("confirm peer trust");
    assert_eq!(confirm_state, SetupState::Completed);

    let cancel_state: SetupState =
        invoke_command(&webview, "cancel_setup", json!({})).expect("cancel setup");
    assert_eq!(cancel_state, SetupState::Welcome);

    let requests = captured_requests
        .lock()
        .expect("lock captured requests")
        .clone();
    assert_eq!(
        requests,
        vec![
            CapturedRequest {
                method: "GET".to_string(),
                path: "/setup/state".to_string(),
                body: String::new(),
            },
            CapturedRequest {
                method: "POST".to_string(),
                path: "/setup/host".to_string(),
                body: String::new(),
            },
            CapturedRequest {
                method: "POST".to_string(),
                path: "/setup/join".to_string(),
                body: String::new(),
            },
            CapturedRequest {
                method: "POST".to_string(),
                path: "/setup/select-peer".to_string(),
                body: "{\"peerId\":\"peer-b\"}".to_string(),
            },
            CapturedRequest {
                method: "POST".to_string(),
                path: "/setup/submit-passphrase".to_string(),
                body: "{\"passphrase\":\"join-passphrase\"}".to_string(),
            },
            CapturedRequest {
                method: "POST".to_string(),
                path: "/setup/confirm-peer".to_string(),
                body: String::new(),
            },
            CapturedRequest {
                method: "POST".to_string(),
                path: "/setup/cancel".to_string(),
                body: String::new(),
            },
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn setup_submit_passphrase_preserves_local_mismatch_contract() {
    let webview = build_setup_webview(DaemonConnectionState::default());

    let state: SetupState = invoke_command(
        &webview,
        "submit_passphrase",
        json!({
            "passphrase1": "alpha",
            "passphrase2": "beta"
        }),
    )
    .expect("submit mismatched passphrase");

    assert_eq!(
        state,
        SetupState::CreateSpaceInputPassphrase {
            error: Some(SetupError::PassphraseMismatch),
        }
    );
}

//! Process-level `uniclip get --wait` tests against a controllable daemon.
//!
//! The real two-node tests prove actual transfer and materialization. These
//! scenarios inject daemon HTTP/WS events so failure, cancellation, unknown
//! totals, unrelated activity, and reconnect cleanup are deterministic.

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use uc_e2e_tests::{TestCli, TestDaemon, TestProfile};

const EXIT_ERROR: i32 = 1;

#[derive(Clone)]
struct ScriptedState {
    scripts: Arc<Vec<Vec<Value>>>,
    next_script: Arc<AtomicUsize>,
    entries: Arc<Mutex<HashMap<String, ScriptedEntry>>>,
}

#[derive(Clone)]
struct ScriptedEntry {
    kind: &'static str,
    content: &'static str,
}

struct ScriptedDaemon {
    _daemon: TestDaemon,
    cli: TestCli,
    task: tokio::task::JoinHandle<()>,
}

impl ScriptedDaemon {
    async fn start(name: &str, scripts: Vec<Vec<Value>>, entries: &[(&str, &'static str, &'static str)]) -> Self {
        let profile = TestProfile::new(name);
        let daemon = TestDaemon::start(profile)
            .await
            .expect("start backing daemon");
        let cli = TestCli::new(&daemon.profile);
        let initialized = cli.run_capture(&[
            "init",
            "--passphrase",
            "scripted-get-wait-passphrase",
            "--device-name",
            "scripted-get-wait",
        ]);
        assert!(initialized.success(), "initialize backing daemon: {}", initialized.stderr);
        let connection_path = daemon.profile.data_dir().join("daemon.conn");
        let backing_connection: Value = serde_json::from_slice(
            &std::fs::read(&connection_path).expect("read backing daemon connection"),
        )
        .expect("decode backing daemon connection");
        let backing_pid = backing_connection["pid"].as_u64().expect("backing daemon pid");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind scripted daemon");
        let port = listener.local_addr().expect("scripted address").port();
        let state = ScriptedState {
            scripts: Arc::new(scripts),
            next_script: Arc::new(AtomicUsize::new(0)),
            entries: Arc::new(Mutex::new(
                entries
                    .iter()
                    .map(|(id, kind, content)| {
                        ((*id).to_string(), ScriptedEntry { kind, content })
                    })
                    .collect(),
            )),
        };
        let app = Router::new()
            .route("/health", get(health))
            .route("/auth/connect", post(auth_connect))
            .route("/ws", get(websocket))
            .route("/clipboard/entries", get(list_entries))
            .route("/clipboard/entries/:id", get(entry_detail))
            .with_state(state);
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve scripted daemon");
        });
        std::fs::write(
            &connection_path,
            serde_json::to_vec(&json!({
                "format": 1,
                "host": "127.0.0.1",
                "port": port,
                "token": "scripted-token",
                "pid": backing_pid,
                "startedAtMs": 1
            }))
            .expect("encode daemon connection"),
        )
        .expect("write daemon connection");
        Self {
            _daemon: daemon,
            cli,
            task,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(self.cli.binary_path());
        command
            .env("UC_PROFILE", &self.cli.profile_name)
            .env("UNICLIPBOARD_ENV", "development");
        command
    }
}

impl Drop for ScriptedDaemon {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn health() -> Json<Value> {
    Json(envelope(json!({
        "status": "ok",
        "packageVersion": "1.0.0-alpha.17",
        "apiRevision": "setup-pairing-http-routes-v2-event-wired-residency-restart-inbound-notice-summary-relay-credentials-diagnostic-capture-v1-profile-recovery-v2-custom-relays-v1",
        "residency": "standalone"
    })))
}

async fn auth_connect() -> Json<Value> {
    Json(envelope(json!({
        "sessionToken": "scripted-session",
        "expiresInSecs": 3600,
        "refreshAtSecs": 3500
    })))
}

async fn list_entries(State(state): State<ScriptedState>) -> Json<Value> {
    let entries = state.entries.lock().expect("entries lock");
    Json(envelope(Value::Array(
        entries
            .iter()
            .map(|(id, entry)| projection(id, entry.kind, entry.content))
            .collect(),
    )))
}

async fn entry_detail(
    State(state): State<ScriptedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let entries = state.entries.lock().expect("entries lock");
    let entry = entries.get(&id).expect("scripted entry must exist");
    Json(envelope(json!({
        "id": id,
        "content": entry.content,
        "sizeBytes": entry.content.len(),
        "createdAtMs": 1,
        "activeTimeMs": 1,
        "mimeType": "text/plain"
    })))
}

async fn websocket(ws: WebSocketUpgrade, State(state): State<ScriptedState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: ScriptedState) {
    let first = tokio::time::timeout(Duration::from_secs(2), socket.recv()).await;
    let Ok(Some(Ok(Message::Text(request)))) = first else {
        while socket.next().await.is_some() {}
        return;
    };
    if !request.contains("file-transfer") {
        return;
    }
    let index = state.next_script.fetch_add(1, Ordering::SeqCst);
    let Some(script) = state.scripts.get(index) else {
        return;
    };
    for event in script {
        tokio::time::sleep(Duration::from_millis(80)).await;
        if socket.send(Message::Text(event.to_string())).await.is_err() {
            return;
        }
    }
}

fn envelope(data: Value) -> Value {
    json!({ "data": data, "ts": 1 })
}

fn projection(id: &str, kind: &str, preview: &str) -> Value {
    json!({
        "id": id,
        "preview": preview,
        "hasDetail": true,
        "sizeBytes": preview.len(),
        "capturedAt": 1,
        "contentType": kind,
        "isEncrypted": true,
        "isFavorited": false,
        "updatedAt": 1,
        "activeTime": 1,
        "contentTags": [],
        "isDirectory": false
    })
}

fn event(event_type: &str, payload: Value) -> Value {
    json!({ "topic": "clipboard", "type": event_type, "ts": 1, "payload": payload })
}

fn pending(entry: &str, attempt: &str, total: Option<u64>, filenames: &[&str]) -> Value {
    event("clipboard.incoming_pending", json!({
        "entryId": entry,
        "attemptId": attempt,
        "fromDevice": "peer",
        "totalBytes": total,
        "filenames": filenames
    }))
}

fn progress(entry: &str, attempt: &str, direction: &str, bytes: u64, total: Option<u64>) -> Value {
    event("file-transfer.progress", json!({
        "transferId": format!("transfer-{entry}"),
        "entryId": entry,
        "attemptId": attempt,
        "peerId": "peer",
        "direction": direction,
        "bytesTransferred": bytes,
        "totalBytes": total
    }))
}

fn status(entry: &str, attempt: &str, status: &str) -> Value {
    event("file-transfer.status_changed", json!({
        "transferId": format!("transfer-{entry}"),
        "entryId": entry,
        "attemptId": attempt,
        "status": status,
        "reason": "scripted"
    }))
}

fn completed(entry: &str) -> Value {
    event("clipboard.new_content", json!({
        "entryId": entry,
        "preview": entry,
        "origin": "remote",
        "fromDevice": "peer"
    }))
}

fn run_pty(daemon: &ScriptedDaemon, args: &[&str]) -> String {
    let transcript = tempfile::NamedTempFile::new().expect("terminal transcript");
    let output = Command::new("script")
        .arg("-q")
        .arg(transcript.path())
        .arg(daemon.cli.binary_path())
        .args(args)
        .env("UC_PROFILE", &daemon.cli.profile_name)
        .env("UNICLIPBOARD_ENV", "development")
        .output()
        .expect("run scripted PTY command");
    assert!(output.status.success(), "PTY command failed: {}", String::from_utf8_lossy(&output.stderr));
    std::fs::read_to_string(transcript.path()).expect("read terminal transcript")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn get_wait_progress_correlates_direction_entry_attempt_and_known_total() {
    let daemon = ScriptedDaemon::start(
        "get-wait-script-known",
        vec![vec![
            pending("target", "attempt-a", Some(1000), &["target.bin"]),
            progress("target", "attempt-a", "sending", 991, Some(1000)),
            progress("other", "attempt-a", "receiving", 992, Some(1000)),
            progress("target", "attempt-b", "receiving", 993, Some(1000)),
            progress("target", "attempt-a", "receiving", 500, Some(1000)),
            completed("target"),
        ]],
        &[("target", "text", "known-total-result")],
    ).await;
    let transcript = run_pty(&daemon, &["get", "--wait"]);
    assert!(transcript.contains("500 B/1000 B"), "target progress missing: {transcript:?}");
    for unrelated in ["991 B", "992 B", "993 B"] {
        assert!(!transcript.contains(unrelated), "unrelated progress leaked: {transcript:?}");
    }
    assert!(transcript.contains("known-total-result"));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn get_wait_unknown_total_and_json_keep_stdout_clean() {
    let daemon = ScriptedDaemon::start(
        "get-wait-script-unknown",
        vec![vec![
            pending("unknown", "attempt-u", None, &["unknown.bin"]),
            progress("unknown", "attempt-u", "receiving", 4096, None),
            completed("unknown"),
        ]],
        &[("unknown", "text", "unknown-total-result")],
    ).await;
    let transcript = run_pty(&daemon, &["get", "--wait"]);
    assert!(transcript.contains("Receiving"));
    assert!(transcript.contains("4.0 KiB"), "unknown total bytes missing: {transcript:?}");

    let json_daemon = ScriptedDaemon::start(
        "get-wait-script-json",
        vec![vec![completed("json-entry")]],
        &[("json-entry", "text", "json-result")],
    ).await;
    let output = json_daemon.command().args(["--json", "get", "--wait"]).output().expect("run JSON wait");
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("stdout must be one JSON value");
    assert_eq!(value["text"], "json-result");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains('\r') && !stderr.contains("\u{1b}["));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn get_wait_filters_ignore_mismatches_and_id_does_not_lock_existing_transfer_failure() {
    let daemon = ScriptedDaemon::start(
        "get-wait-script-filter",
        vec![vec![
            status("wanted", "old-attempt", "failed"),
            completed("other"),
            completed("wanted"),
        ]],
        &[("other", "text", "wrong"), ("wanted", "text", "wanted-result")],
    ).await;
    let output = daemon.command().args(["get", "--wait", "--id", "wanted"]).output().expect("run id wait");
    assert!(output.status.success(), "id wait failed: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "wanted-result");

    let typed = ScriptedDaemon::start(
        "get-wait-script-type",
        vec![vec![completed("file-entry"), completed("text-entry")]],
        &[("file-entry", "file", "wrong"), ("text-entry", "text", "typed-result")],
    ).await;
    let output = typed.command().args(["get", "--wait", "--type", "text"]).output().expect("run type wait");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "typed-result");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn get_wait_target_failure_and_cancellation_exit_with_clean_noninteractive_output() {
    for terminal in ["failed", "cancelled"] {
        let daemon = ScriptedDaemon::start(
            &format!("get-wait-script-{terminal}"),
            vec![vec![
                pending("target", "attempt", Some(10), &["target.bin"]),
                status("target", "attempt", terminal),
            ]],
            &[],
        ).await;
        let output = daemon.command().args(["get", "--wait"]).stdout(Stdio::piped()).stderr(Stdio::piped()).output().expect("run terminal wait");
        assert_eq!(output.status.code(), Some(EXIT_ERROR));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(terminal));
        assert!(!stderr.contains('\r') && !stderr.contains("\u{1b}["));
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn get_wait_reconnect_clears_the_previous_target() {
    let daemon = ScriptedDaemon::start(
        "get-wait-script-reconnect",
        vec![
            vec![
                pending("stale", "attempt", Some(10), &["stale.bin"]),
                progress("stale", "attempt", "receiving", 5, Some(10)),
            ],
            vec![status("stale", "attempt", "failed"), completed("fresh")],
        ],
        &[("fresh", "text", "reconnected-result")],
    ).await;
    let output = daemon.command().args(["get", "--wait"]).output().expect("run reconnect wait");
    assert!(output.status.success(), "reconnect wait failed: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "reconnected-result");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("reconnecting") && stderr.contains("Reconnected"));
}

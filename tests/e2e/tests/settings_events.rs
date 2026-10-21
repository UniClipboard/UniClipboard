//! Daemon-level checks of the settings change events (ADR-013 section 7, F19–F30).
//!
//! Each test runs a real `uniclipd` on an isolated profile, subscribes to the WS topics
//! `settings` and `system` like the Go host does, and writes through `PUT /settings`. Every
//! "no event" check is followed by a write that must produce one, so silence is not taken for
//! success.
//!
//! Set `UC_E2E_ARTIFACT_DIR` to keep a JSON record per test (every write, its status and every
//! frame received after it).
//!
//! Run with:
//! `cargo build -p uc-daemon -p uc-dev-cli`
//! `cargo test --manifest-path tests/e2e/Cargo.toml --test settings_events -- --ignored --test-threads=1`

use std::path::PathBuf;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use uc_e2e_tests::{get_session_token, NodeBinarySet, TestDaemon, TestProfile};

/// How long a check waits for frames after a write. Events are sent before the HTTP response
/// returns, so this only covers delivery.
const SETTLE: Duration = Duration::from_millis(700);

struct Subscriber {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl Subscriber {
    /// Connects, subscribes and returns the `settings.snapshot` payload with the subscriber.
    async fn connect(daemon: &TestDaemon, session: &str) -> (Self, Value) {
        let url = format!("ws://127.0.0.1:{}/ws", daemon.port());
        let mut request = url.into_client_request().unwrap();
        request.headers_mut().insert(
            "Authorization",
            format!("Session {session}").parse().unwrap(),
        );
        let (stream, _) = tokio_tungstenite::connect_async(request)
            .await
            .expect("ws connect");
        let mut subscriber = Self { stream };
        subscriber
            .stream
            .send(Message::Text(
                json!({ "action": "subscribe", "topics": ["settings", "system"] }).to_string(),
            ))
            .await
            .unwrap();
        let first = subscriber
            .next(Duration::from_secs(5))
            .await
            .expect("settings.snapshot after subscribe");
        assert_eq!(first["topic"], "settings");
        assert_eq!(first["type"], "settings.snapshot");
        let payload = first["payload"].clone();
        let keys: Vec<&String> = payload.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["revisions"]);
        let mut revision_keys: Vec<&String> =
            payload["revisions"].as_object().unwrap().keys().collect();
        revision_keys.sort();
        assert_eq!(revision_keys, ["desktop", "engine"]);
        (subscriber, payload)
    }

    /// The next text frame as JSON, or `None` when the timeout passes or the connection ends.
    async fn next(&mut self, timeout: Duration) -> Option<Value> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let message = tokio::time::timeout_at(deadline, self.stream.next())
                .await
                .ok()??;
            match message {
                Ok(Message::Text(text)) => return Some(serde_json::from_str(&text).unwrap()),
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => continue,
            }
        }
    }

    /// Every frame that arrives within `window`.
    async fn drain(&mut self, window: Duration) -> Vec<Value> {
        let deadline = tokio::time::Instant::now() + window;
        let mut frames = Vec::new();
        while let Some(frame) = self
            .next(deadline.saturating_duration_since(tokio::time::Instant::now()))
            .await
        {
            frames.push(frame);
        }
        frames
    }

    /// True when the daemon closed the connection within `timeout`.
    async fn closed_within(&mut self, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match tokio::time::timeout_at(deadline, self.stream.next()).await {
                Err(_) => return false,
                Ok(None) | Ok(Some(Err(_))) | Ok(Some(Ok(Message::Close(_)))) => return true,
                Ok(Some(Ok(_))) => continue,
            }
        }
    }
}

struct Context {
    daemon: TestDaemon,
    client: reqwest::Client,
    session: String,
    record: Vec<Value>,
    name: &'static str,
}

impl Context {
    async fn start(name: &'static str) -> Self {
        let profile = TestProfile::new(name);
        std::fs::create_dir_all(profile.data_dir()).unwrap();
        let daemon = TestDaemon::start_preserving_with(profile, &NodeBinarySet::current(), None)
            .await
            .expect("daemon start");
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap();
        let session = get_session_token(&daemon, &client).await;
        Self {
            daemon,
            client,
            session,
            record: Vec::new(),
            name,
        }
    }

    async fn subscribe(&mut self) -> (Subscriber, Value) {
        let (subscriber, snapshot) = Subscriber::connect(&self.daemon, &self.session).await;
        self.record
            .push(json!({ "step": "subscribed", "snapshot": snapshot }));
        (subscriber, snapshot)
    }

    async fn get(&self) -> Value {
        let response = self
            .client
            .get(format!("{}/settings", self.daemon.base_url()))
            .header("Authorization", format!("Session {}", self.session))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        response.json::<Value>().await.unwrap()["data"].clone()
    }

    /// Writes `body` and returns the status and the settings frames that followed.
    async fn write(
        &mut self,
        subscriber: &mut Subscriber,
        step: &str,
        body: Value,
    ) -> (u16, Vec<Value>) {
        let status = self
            .client
            .put(format!("{}/settings", self.daemon.base_url()))
            .header("Authorization", format!("Session {}", self.session))
            .json(&body)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16();
        let frames = subscriber.drain(SETTLE).await;
        self.record.push(json!({
            "step": step,
            "body": body,
            "status": status,
            "frames": frames,
        }));
        let settings = frames
            .into_iter()
            .filter(|frame| frame["topic"] == "settings")
            .collect();
        (status, settings)
    }

    fn note(&mut self, step: &str, detail: Value) {
        self.record.push(json!({ "step": step, "detail": detail }));
    }

    fn keep_artifact(&self) {
        let Ok(dir) = std::env::var("UC_E2E_ARTIFACT_DIR") else {
            return;
        };
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let artifact = json!({
            "test": self.name,
            "profile": self.daemon.profile.name,
            "steps": self.record,
        });
        std::fs::write(
            dir.join(format!("{}.json", self.name)),
            serde_json::to_vec_pretty(&artifact).unwrap(),
        )
        .unwrap();
    }
}

/// Asserts one `settings.changed` of `owner` with exactly the payload keys of the contract.
fn changed(frame: &Value, owner: &str, revision: u64) -> Vec<String> {
    assert_eq!(frame["type"], "settings.changed", "{frame}");
    let payload = frame["payload"].as_object().unwrap();
    let mut keys: Vec<&String> = payload.keys().collect();
    keys.sort();
    assert_eq!(keys, ["owner", "revision", "sections"], "{frame}");
    assert_eq!(frame["payload"]["owner"], owner, "{frame}");
    assert_eq!(frame["payload"]["revision"], revision, "{frame}");
    frame["payload"]["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|section| section.as_str().unwrap().to_string())
        .collect()
}

fn revision(snapshot: &Value, owner: &str) -> u64 {
    snapshot["revisions"][owner].as_u64().unwrap()
}

#[tokio::test]
#[ignore]
async fn each_owner_reports_its_own_changes_without_values() {
    let mut context = Context::start("settings-events-owners").await;
    let (mut ws, snapshot) = context.subscribe().await;
    let desktop = revision(&snapshot, "desktop");
    let engine = revision(&snapshot, "engine");
    assert_eq!(
        desktop, 0,
        "a fresh daemon starts the product revision at 0"
    );

    // F19 + F30: a product write is one desktop event with no value in any frame.
    let (status, events) = context
        .write(
            &mut ws,
            "product write",
            json!({ "general": { "trayIcon": "cat" } }),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(changed(&events[0], "desktop", desktop + 1), ["general"]);
    assert!(!events[0].to_string().contains("cat"));

    // F20: an Engine write is one engine event naming the Engine section.
    let (status, events) = context
        .write(
            &mut ws,
            "engine write",
            json!({ "sync": { "syncOnRestore": true } }),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(changed(&events[0], "engine", engine + 1), ["sync"]);

    // F21: a mixed write is one event per owner, each with its own revision.
    let (status, events) = context
        .write(
            &mut ws,
            "mixed write",
            json!({ "general": { "trayIcon": "glyph" }, "sync": { "syncOnRestore": false } }),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(events.len(), 2, "{events:?}");
    let mut owners: Vec<&str> = events
        .iter()
        .map(|event| event["payload"]["owner"].as_str().unwrap())
        .collect();
    owners.sort();
    assert_eq!(owners, ["desktop", "engine"]);
    for event in &events {
        match event["payload"]["owner"].as_str().unwrap() {
            "desktop" => assert_eq!(changed(event, "desktop", desktop + 2), ["general"]),
            _ => assert_eq!(changed(event, "engine", engine + 2), ["sync"]),
        }
    }

    // F26: a second subscriber's snapshot already holds both revisions; the read after it is
    // current.
    let (_late, late_snapshot) = context.subscribe().await;
    assert_eq!(revision(&late_snapshot, "desktop"), desktop + 2);
    assert_eq!(revision(&late_snapshot, "engine"), engine + 2);
    let settings = context.get().await;
    assert_eq!(settings["general"]["trayIcon"], "glyph");
    assert_eq!(settings["sync"]["syncOnRestore"], false);

    context.note("completed", Value::Null);
    context.keep_artifact();
}

#[tokio::test]
#[ignore]
async fn unchanged_rejected_and_failed_writes_report_nothing() {
    let mut context = Context::start("settings-events-no-success").await;
    let (mut ws, snapshot) = context.subscribe().await;
    let mut desktop = revision(&snapshot, "desktop");
    let mut engine = revision(&snapshot, "engine");

    let (status, events) = context
        .write(
            &mut ws,
            "setup product",
            json!({ "general": { "trayIcon": "cat" } }),
        )
        .await;
    assert_eq!((status, events.len()), (200, 1), "{events:?}");
    desktop += 1;
    let (status, events) = context
        .write(
            &mut ws,
            "setup engine",
            json!({ "sync": { "syncOnRestore": true } }),
        )
        .await;
    assert_eq!((status, events.len()), (200, 1), "{events:?}");
    engine += 1;

    // F22: same values, then positive controls.
    let (status, events) = context
        .write(
            &mut ws,
            "same product value",
            json!({ "general": { "trayIcon": "cat" } }),
        )
        .await;
    assert_eq!((status, events.len()), (200, 0), "{events:?}");
    let (status, events) = context
        .write(
            &mut ws,
            "same engine value",
            json!({ "sync": { "syncOnRestore": true } }),
        )
        .await;
    assert_eq!((status, events.len()), (200, 0), "{events:?}");
    let (_, events) = context
        .write(
            &mut ws,
            "control after same value",
            json!({ "general": { "trayIcon": "glyph" } }),
        )
        .await;
    desktop += 1;
    assert_eq!(events.len(), 1, "{events:?}");
    changed(&events[0], "desktop", desktop);

    // F23: an invalid product value is refused (400 since ADR-013 section 8.3), then a positive
    // control.
    let (status, events) = context
        .write(
            &mut ws,
            "invalid product value",
            json!({ "general": { "trayIcon": "bogus" } }),
        )
        .await;
    assert_eq!((status, events.len()), (400, 0), "{events:?}");
    let (_, events) = context
        .write(
            &mut ws,
            "control after invalid value",
            json!({ "general": { "trayIcon": "cat" } }),
        )
        .await;
    desktop += 1;
    assert_eq!(events.len(), 1, "{events:?}");
    changed(&events[0], "desktop", desktop);

    // F24: the Engine rejects a value that decodes, then a positive control.
    let (status, events) = context
        .write(
            &mut ws,
            "engine rejection",
            json!({ "network": { "customRelayUrls": ["ftp://relay.example.com"] } }),
        )
        .await;
    assert_eq!((status, events.len()), (400, 0), "{events:?}");
    let (_, events) = context
        .write(
            &mut ws,
            "control after engine rejection",
            json!({ "sync": { "syncOnRestore": false } }),
        )
        .await;
    engine += 1;
    assert_eq!(events.len(), 1, "{events:?}");
    changed(&events[0], "engine", engine);

    // F25: the product file cannot be replaced while the directory is read-only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = context.daemon.profile.data_dir().clone();
        let before = std::fs::metadata(&dir).unwrap().permissions();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let (status, events) = context
            .write(
                &mut ws,
                "product write to read-only directory",
                json!({ "general": { "trayIcon": "glyph" } }),
            )
            .await;
        std::fs::set_permissions(&dir, before).unwrap();
        assert_eq!((status, events.len()), (500, 0), "{events:?}");
        assert_eq!(context.get().await["general"]["trayIcon"], "cat");
        let (_, events) = context
            .write(
                &mut ws,
                "control after failed write",
                json!({ "general": { "trayIcon": "glyph" } }),
            )
            .await;
        desktop += 1;
        assert_eq!(events.len(), 1, "{events:?}");
        changed(&events[0], "desktop", desktop);
    }

    context.note("completed", json!({ "desktop": desktop, "engine": engine }));
    context.keep_artifact();
}

#[tokio::test]
#[ignore]
async fn a_restarted_daemon_is_read_again_from_a_new_baseline() {
    let mut context = Context::start("settings-events-restart").await;
    let (mut ws, _) = context.subscribe().await;
    let (status, events) = context
        .write(
            &mut ws,
            "write before restart",
            json!({ "general": { "trayIcon": "cat" } }),
        )
        .await;
    assert_eq!((status, events.len()), (200, 1), "{events:?}");
    changed(&events[0], "desktop", 1);

    // The daemon dies; the subscriber sees the connection end.
    context.daemon.kill();
    assert!(ws.closed_within(Duration::from_secs(5)).await);
    context.note("daemon killed; connection closed", Value::Null);

    // A change made while no daemon runs must be visible after the reconnect.
    let path = context
        .daemon
        .profile
        .data_dir()
        .join("desktop-settings.json");
    let mut document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    document["general"]["trayIcon"] = json!("glyph");
    std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    context.note("file changed while stopped", json!({ "trayIcon": "glyph" }));

    context.daemon.restart_preserving().await.expect("restart");
    context.session = get_session_token(&context.daemon, &context.client).await;
    let (mut ws, snapshot) = context.subscribe().await;
    // F29: revisions start again at 0; the client reads after the snapshot and sees the file.
    assert_eq!(revision(&snapshot, "desktop"), 0);
    assert_eq!(context.get().await["general"]["trayIcon"], "glyph");
    let (_, events) = context
        .write(
            &mut ws,
            "write after restart",
            json!({ "general": { "trayIcon": "cat" } }),
        )
        .await;
    assert_eq!(events.len(), 1, "{events:?}");
    changed(&events[0], "desktop", 1);

    context.note("completed", Value::Null);
    context.keep_artifact();
}

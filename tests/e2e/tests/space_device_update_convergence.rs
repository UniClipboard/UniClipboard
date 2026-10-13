//! Space device update status must converge on a host that only re-reads
//! device trust when the daemon tells it to.
//!
//! Scenario: alice, bob and carol share one space. carol is offline while
//! alice removes bob, so alice owes carol a group update. When carol comes
//! back, alice delivers the update and its authoritative status becomes
//! `completed`. A host that follows the desktop frontend rule (re-read on
//! `device-trust.changed` with a newer revision or on
//! `system.refresh_required`, never on a timer) must reach the same state.
//!
//! The same snapshot carries a per-device `groupUpdate` projection of the
//! Engine's group key delivery queue. It must appear while carol owes the
//! update, must not advance because a host re-reads it, and must disappear
//! only after carol really accepts the update.
//!
//! `group_update_states_follow_receiver_outcomes` drives the receiver outcomes
//! through the daemon's `/e2e/space-work` Engine controls, so it needs a
//! daemon built with `--features uc-daemon/e2e-rendezvous`.
//!
//! Run with:
//! cargo test --manifest-path tests/e2e/Cargo.toml --test space_device_update_convergence -- --ignored --nocapture

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use uc_e2e_tests::{
    confirm_device_group, get_session_token, InviteSession, LocalRendezvous, NodeBinarySet,
    TestCli, TestDaemon, TestProfile,
};

const PASSPHRASE: &str = "space-device-update-convergence";
const WAIT_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a notified host may lag behind the authoritative status.
const HOST_GRACE: Duration = Duration::from_secs(10);

struct Node {
    daemon: TestDaemon,
    cli: TestCli,
    space_work_token: String,
}

impl Node {
    async fn fresh(name: &str, binaries: &NodeBinarySet, rendezvous: &LocalRendezvous) -> Self {
        let profile = TestProfile::new(name);
        let space_work_token = uuid::Uuid::new_v4().simple().to_string();
        let daemon = TestDaemon::start_clean_configured_with(
            profile,
            binaries,
            Some(&rendezvous.uri()),
            |command| configure_daemon(command, &space_work_token),
        )
        .await
        .expect("daemon start");
        let cli = TestCli::with_binaries(&daemon.profile, binaries);
        Self {
            daemon,
            cli,
            space_work_token,
        }
    }

    async fn restart(&mut self) {
        let token = self.space_work_token.clone();
        self.daemon
            .restart_preserving_configured_with(|command| configure_daemon(command, &token))
            .await
            .expect("daemon restart");
    }

    /// Engine test controls; only answered by an `e2e-rendezvous` daemon.
    async fn space_work(&self, payload: Value) -> Value {
        let client = reqwest::Client::new();
        let session = get_session_token(&self.daemon, &client).await;
        let response = client
            .post(format!("{}/e2e/space-work", self.daemon.base_url()))
            .header("Authorization", format!("Session {session}"))
            .header("x-uc-e2e-space-work-token", &self.space_work_token)
            .json(&payload)
            .send()
            .await
            .expect("space work request");
        assert!(
            response.status().is_success(),
            "space work {payload} failed: {} (is uniclipd built with e2e-rendezvous?)",
            response.status()
        );
        response.json().await.expect("space work JSON")
    }

    async fn initialized(
        name: &str,
        device_name: &str,
        binaries: &NodeBinarySet,
        rendezvous: &LocalRendezvous,
    ) -> Self {
        let node = Self::fresh(name, binaries, rendezvous).await;
        let output = node.cli.run_capture(&[
            "init",
            "--passphrase",
            PASSPHRASE,
            "--device-name",
            device_name,
        ]);
        assert!(output.success(), "init failed: {output:?}");
        node
    }
}

fn configure_daemon(command: &mut std::process::Command, space_work_token: &str) {
    command.env(
        "RUST_LOG",
        std::env::var("UC_E2E_RUST_LOG")
            .unwrap_or_else(|_| "uc_webserver::api::server=info".to_string()),
    );
    command.env("UC_E2E_SPACE_WORK_TOKEN", space_work_token);
}

fn json(output: &uc_e2e_tests::CapturedOutput) -> Value {
    serde_json::from_str(output.stdout.trim()).unwrap_or_else(|error| {
        panic!("invalid JSON ({error}): {output:?}");
    })
}

async fn join(sponsor: &Node, joiner: &Node, device_name: &str) {
    let (session, code) = InviteSession::start(&sponsor.cli).await;
    let output = assert_cmd::Command::new(joiner.cli.binary_path())
        .env("UC_PROFILE", &joiner.cli.profile_name)
        .env("UNICLIPBOARD_ENV", "development")
        .args([
            "--json",
            "join",
            "--code",
            &code,
            "--passphrase",
            PASSPHRASE,
            "--device-name",
            device_name,
        ])
        .timeout(WAIT_TIMEOUT)
        .assert();
    assert!(
        output.get_output().status.success(),
        "join failed: {:?}",
        output.get_output()
    );
    session.finish().await;
}

fn device_id(cli: &TestCli, name: &str) -> String {
    let output = cli.run_capture(&["--json", "members"]);
    assert!(output.success(), "members failed: {output:?}");
    json(&output)
        .as_array()
        .expect("members array")
        .iter()
        .find(|member| member["device_name"] == name)
        .and_then(|member| member["device_id"].as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("member {name} not found"))
}

/// Reads the same device group choices snapshot the desktop frontend reads,
/// over one session so polling does not trip the pre-auth rate limit.
#[derive(Clone)]
struct TrustReader {
    client: reqwest::Client,
    url: String,
    token: String,
}

impl TrustReader {
    async fn new(node: &Node) -> Self {
        let client = reqwest::Client::new();
        let token = get_session_token(&node.daemon, &client).await;
        Self {
            url: format!("{}/member/device-group-choices", node.daemon.base_url()),
            client,
            token,
        }
    }

    async fn raw(&self) -> Result<Value, String> {
        let response = self
            .client
            .get(&self.url)
            .header("Authorization", format!("Session {}", self.token))
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let body: Value = response.json().await.map_err(|error| error.to_string())?;
        if !status.is_success() {
            return Err(format!("{status}: {body}"));
        }
        Ok(body.get("data").cloned().unwrap_or(body))
    }

    /// The device's relationship as the frontend receives it.
    async fn device(&self, device_id: &str) -> Option<Value> {
        device_in(&self.raw().await.ok()?, device_id)
    }

    async fn snapshot(&self) -> Option<(u64, String)> {
        let value = self.raw().await.ok()?;
        Some((
            value["revision"].as_u64()?,
            value["deviceTrust"]["spaceDeviceUpdate"]["phase"]
                .as_str()?
                .to_string(),
        ))
    }
}

fn device_in(snapshot: &Value, device_id: &str) -> Option<Value> {
    snapshot["deviceTrust"]["devices"]
        .as_array()?
        .iter()
        .find(|device| device["deviceId"] == device_id)
        .cloned()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_millis() as i64
}

/// Waits until the device's `groupUpdate` satisfies `accept` (`None` = absent).
async fn wait_for_group_update(
    reader: &TrustReader,
    device_id: &str,
    accept: impl Fn(Option<&Value>) -> bool,
    what: &str,
    timeout: Duration,
) -> Option<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(snapshot) = reader.raw().await {
            // A device that left the snapshot owes nothing any more.
            let device = device_in(&snapshot, device_id);
            let update = device
                .as_ref()
                .and_then(|device| device.get("groupUpdate"))
                .filter(|value| !value.is_null());
            if accept(update) {
                eprintln!("[group-update] {what}: {update:?}");
                return update.cloned();
            }
        }
        if Instant::now() >= deadline {
            panic!(
                "{what} did not happen; last device: {:?}",
                reader.device(device_id).await
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Re-reads the projection for `window` and asserts that reading alone never
/// changes it: no new attempt before the Engine's own retry deadline and no
/// state or epoch change.
async fn assert_reads_do_not_advance(
    reader: &TrustReader,
    device_id: &str,
    expected: &Value,
    window: Duration,
) {
    let deadline = Instant::now() + window;
    let mut reads = 0;
    while Instant::now() < deadline {
        let device = reader.device(device_id).await.expect("device present");
        let update = &device["groupUpdate"];
        reads += 1;
        let retry_due = expected["nextRetryAtMs"]
            .as_i64()
            .is_some_and(|at| now_ms() >= at);
        if retry_due {
            break;
        }
        assert_eq!(update, expected, "a read changed the group update");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    eprintln!("[group-update] {reads} reads left {expected} unchanged");
}

/// Accepts any device group decision the node's user is asked to make.
async fn accept_pending_decisions(node: &Node) {
    let reader = TrustReader::new(node).await;
    let Ok(value) = reader.raw().await else {
        return;
    };
    let Some(issues) = value["issues"].as_array() else {
        return;
    };
    for issue in issues {
        let Some(choice) = issue["choices"]
            .as_array()
            .and_then(|choices| choices.iter().find(|c| c["isCurrentGroup"] == false))
        else {
            continue;
        };
        let _ = confirm_device_group(
            |args| node.cli.run_capture(args),
            issue,
            choice,
            false,
            WAIT_TIMEOUT,
        )
        .await;
    }
}

async fn wait_for_phase(reader: &TrustReader, accept: impl Fn(&str) -> bool, what: &str) -> String {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    loop {
        if let Some((_, phase)) = reader.snapshot().await {
            if accept(&phase) {
                return phase;
            }
        }
        if Instant::now() >= deadline {
            panic!(
                "{what} did not happen; last snapshot: {:?}",
                reader.raw().await
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// A host view that follows `apps/gui-go/frontend/src/contexts/DeviceTrustContext.tsx`: it only
/// re-reads after a daemon notification, skipping `device-trust.changed`
/// whose revision it has already loaded.
#[derive(Default)]
struct HostView {
    revision: Option<u64>,
    phase: Option<String>,
    snapshot: Option<Value>,
    notifications: Vec<String>,
}

async fn follow_host_notifications(node: &Node, reader: TrustReader, view: Arc<Mutex<HostView>>) {
    let client = reqwest::Client::new();
    let token = get_session_token(&node.daemon, &client).await;
    let url = format!(
        "ws://127.0.0.1:{}/ws?auth={}",
        node.daemon.port(),
        format!("Session {token}").replace(' ', "%20")
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("daemon websocket");
    socket
        .send(Message::Text(
            serde_json::json!({
                "action": "subscribe",
                "topics": ["device-trust", "system"],
            })
            .to_string(),
        ))
        .await
        .expect("subscribe");
    async fn reread(reader: &TrustReader, view: &Arc<Mutex<HostView>>) {
        if let Ok(value) = reader.raw().await {
            let mut view = view.lock().expect("host view");
            view.revision = value["revision"].as_u64();
            view.phase = value["deviceTrust"]["spaceDeviceUpdate"]["phase"]
                .as_str()
                .map(str::to_string);
            view.snapshot = Some(value);
        }
    }
    reread(&reader, &view).await;
    tokio::task::spawn(async move {
        while let Some(Ok(message)) = socket.next().await {
            let Message::Text(text) = message else {
                continue;
            };
            let Ok(event) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            let event_type = event["type"].as_str().unwrap_or_default().to_string();
            let must_reread = match event_type.as_str() {
                "system.refresh_required" => true,
                "device-trust.changed" => {
                    let revision = event["payload"]["revision"].as_u64().unwrap_or(0);
                    let loaded = view.lock().expect("host view").revision;
                    revision == 0 || loaded.is_none_or(|loaded| revision > loaded)
                }
                _ => false,
            };
            view.lock()
                .expect("host view")
                .notifications
                .push(event_type);
            if must_reread {
                reread(&reader, &view).await;
            }
        }
    });
}

async fn assert_text_arrives(sender: &Node, receiver: &Node, receiver_id: &str, text: &str) {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    loop {
        let output = sender
            .cli
            .run_capture(&["--json", "send", text, "--peer", receiver_id]);
        if output.success()
            && serde_json::from_str::<Value>(output.stdout.trim())
                .is_ok_and(|sent| sent["totalAccepted"] == 1)
        {
            break;
        }
        assert!(Instant::now() < deadline, "send failed: {output:?}");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    loop {
        let output = receiver
            .cli
            .run_capture(&["--json", "get", "--list", "--limit", "20"]);
        if output.success()
            && json(&output)
                .as_array()
                .is_some_and(|rows| rows.iter().any(|row| row["preview"] == text))
        {
            return;
        }
        assert!(Instant::now() < deadline, "text did not arrive: {text}");
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn notified_host_converges_after_pending_group_update_is_delivered() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let alice = Node::initialized("space-update-alice", "alice-node", &binaries, &rendezvous).await;
    let bob = Node::fresh("space-update-bob", &binaries, &rendezvous).await;
    let mut carol = Node::fresh("space-update-carol", &binaries, &rendezvous).await;
    join(&alice, &bob, "bob-node").await;
    join(&alice, &carol, "carol-node").await;
    let bob_id = device_id(&alice.cli, "bob-node");
    let carol_id = device_id(&alice.cli, "carol-node");
    let alice_id = device_id(&carol.cli, "alice-node");
    let truth = TrustReader::new(&alice).await;
    wait_for_phase(&truth, |phase| phase == "completed", "initial convergence").await;

    // A suspended process keeps its address, like a phone whose app is paused.
    carol.daemon.suspend().expect("suspend carol");
    let removed = alice
        .cli
        .run_capture(&["--json", "member", "remove", &bob_id]);
    assert!(removed.success(), "removal failed: {removed:?}");

    // carol really has not received the group update: the status must say so.
    let pending = wait_for_phase(
        &truth,
        |phase| phase == "updating" || phase == "retryable_failure",
        "pending group update",
    )
    .await;
    eprintln!("[space-update-convergence] alice status while carol is offline: {pending}");

    // The suspended receiver is unreachable: the update waits and the Engine
    // retries on its own backoff.
    let waiting = wait_for_group_update(
        &truth,
        &carol_id,
        |update| update.is_some_and(|u| u["state"] == "waiting"),
        "carol owes the group update",
        WAIT_TIMEOUT,
    )
    .await
    .expect("waiting projection");
    assert_eq!(waiting["recovery"], "automatic");
    assert!(waiting["epoch"].as_u64().is_some());
    assert_reads_do_not_advance(&truth, &carol_id, &waiting, Duration::from_secs(3)).await;

    let view = Arc::new(Mutex::new(HostView::default()));
    follow_host_notifications(&alice, truth.clone(), Arc::clone(&view)).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let before = view.lock().expect("host view").phase.clone();
    eprintln!("[space-update-convergence] host view while carol is offline: {before:?}");
    assert_ne!(before.as_deref(), Some("completed"));

    // Let alice's first delivery attempt fail completely so the update backs off.
    tokio::time::sleep(Duration::from_secs(12)).await;
    carol.daemon.resume().expect("resume carol");
    eprintln!("[space-update-convergence] carol resumed");

    // carol never accepted bob's removal herself, so her user decides (ADR-020).
    let carol_reader = TrustReader::new(&carol).await;
    let (issue, choice) = {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            if let Ok(value) = carol_reader.raw().await {
                let accept = value["issues"].as_array().and_then(|issues| {
                    issues.iter().find_map(|issue| {
                        let choice = issue["choices"]
                            .as_array()?
                            .iter()
                            .find(|choice| choice["isCurrentGroup"] == false)?;
                        Some((issue.clone(), choice.clone()))
                    })
                });
                if let Some(accept) = accept {
                    break accept;
                }
            }
            assert!(
                Instant::now() < deadline,
                "carol never saw the removal decision"
            );
            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
    };
    confirm_device_group(
        |args| carol.cli.run_capture(args),
        &issue,
        &choice,
        false,
        WAIT_TIMEOUT,
    )
    .await
    .expect("carol decision failed");
    eprintln!("[space-update-convergence] carol accepted the removal");

    let converged_at = {
        wait_for_phase(
            &truth,
            |phase| phase == "completed",
            "authoritative convergence",
        )
        .await;
        Instant::now()
    };
    eprintln!("[space-update-convergence] authoritative status completed");
    wait_for_group_update(
        &truth,
        &carol_id,
        |update| update.is_none(),
        "accepted update settled",
        HOST_GRACE,
    )
    .await;

    let host_deadline = converged_at + HOST_GRACE;
    loop {
        let (phase, notifications) = {
            let view = view.lock().expect("host view");
            (view.phase.clone(), view.notifications.clone())
        };
        let host_carol = view
            .lock()
            .expect("host view")
            .snapshot
            .as_ref()
            .and_then(|snapshot| device_in(snapshot, &carol_id));
        let host_settled = host_carol.is_some_and(|device| device["groupUpdate"].is_null());
        if phase.as_deref() == Some("completed") && host_settled {
            eprintln!(
                "[space-update-convergence] host view converged; notifications={notifications:?}"
            );
            break;
        }
        assert!(
            Instant::now() < host_deadline,
            "host view stayed at {phase:?} after the authoritative status completed; \
             notifications={notifications:?}"
        );
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    assert_text_arrives(&alice, &carol, &carol_id, "space update alice to carol").await;
    assert_text_arrives(&carol, &alice, &alice_id, "space update carol to alice").await;
}

/// The real Wails GUI (an `apps/gui-go/build.sh e2e` binary named by
/// `UC_E2E_GUI_BINARY`) attached to a node's daemon through the node's
/// development profile. Its in-WebView driver only observes the devices list
/// and reports each peer row change, so the assertions below read what the
/// user sees after the daemon's HTTP/WS updates. Without the variable the
/// GUI checks are skipped and the run says so.
struct GuiObserver {
    child: std::process::Child,
    evidence: std::path::PathBuf,
}

impl GuiObserver {
    fn attach(node: &Node, label: &str) -> Option<Self> {
        let Some(binary) = std::env::var_os("UC_E2E_GUI_BINARY") else {
            eprintln!("[group-update] GUI observation not-run: UC_E2E_GUI_BINARY unset");
            return None;
        };
        let evidence = std::env::temp_dir().join(format!(
            "uc-group-update-gui-{label}-{}.jsonl",
            uuid::Uuid::new_v4().as_simple()
        ));
        std::fs::write(&evidence, "").expect("evidence file");
        let child = std::process::Command::new(binary)
            .env_clear()
            .env("HOME", std::env::var_os("HOME").expect("HOME"))
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("TMPDIR", std::env::temp_dir())
            .env("UC_PROFILE", &node.daemon.profile.name)
            .env("UNICLIPBOARD_ENV", "development")
            .env("UC_DISABLE_SYSTEM_CLIPBOARD", "1")
            .env("UC_GPUI_QUICK_PANEL", "0")
            .env("UC_GUI_GO_E2E_PHASE", "device-group-update")
            .env("UC_GUI_GO_EVIDENCE", &evidence)
            .env("UC_GUI_GO_EXIT_MODE", "full")
            .stdout(std::fs::File::create(evidence.with_extension("log")).expect("GUI log"))
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .expect("start GUI");
        eprintln!(
            "[group-update] GUI attached; evidence {}",
            evidence.display()
        );
        Some(Self { child, evidence })
    }

    fn steps(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.evidence)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// Waits for the last reported row of `device_id` to satisfy `accept`.
    async fn wait_row(
        &self,
        device_id: &str,
        accept: impl Fn(&Value) -> bool,
        what: &str,
    ) -> Value {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            let steps = self.steps();
            if let Some(error) = steps.iter().find(|s| s["step"] == "driver-error") {
                panic!("GUI driver error during {what}: {error}");
            }
            let last = steps
                .iter()
                .rev()
                .filter(|s| s["step"] == "device-row")
                .find(|s| s["detail"]["deviceId"] == device_id)
                .map(|s| s["detail"].clone());
            if let Some(row) = last.filter(|row| accept(row)) {
                eprintln!("[group-update] GUI {what}: {row}");
                return row;
            }
            assert!(
                Instant::now() < deadline,
                "GUI never showed {what}; last steps: {:?}",
                steps.iter().rev().take(8).collect::<Vec<_>>()
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    async fn wait_description(&self, accept: impl Fn(&str) -> bool, what: &str) -> String {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            let last = self
                .steps()
                .iter()
                .rev()
                .find(|s| s["step"] == "device-status-description")
                .and_then(|s| s["detail"]["description"].as_str().map(str::to_string));
            if let Some(text) = last.filter(|text| accept(text)) {
                eprintln!("[group-update] GUI {what}: {text}");
                return text;
            }
            assert!(Instant::now() < deadline, "GUI never described {what}");
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

impl Drop for GuiObserver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Opens a fresh GUI on `node`, waits until its devices list shows the
/// device's row in the expected form (and, for a security update, the
/// selected device's description), then closes it. A fresh GUI reads the
/// state over the daemon's HTTP API on load; the quiet e2e window stops
/// running scripts a few seconds after start, so long-lived observation is
/// not reliable and live pushes are asserted through the notified host.
async fn gui_checkpoint(node: &Node, device_id: &str, expect_update: bool, what: &str) {
    let Some(gui) = GuiObserver::attach(node, what) else {
        return;
    };
    let accept = move |row: &Value| security_row(row) == expect_update;
    gui.wait_row(device_id, accept, what).await;
    if expect_update {
        gui.wait_description(|text| !text.is_empty(), what).await;
    }
}

fn security_row(row: &Value) -> bool {
    row["status"] == "security_update"
}

/// Removes `target` from alice's space and returns once the CLI accepted it.
fn remove_member(alice: &Node, target_id: &str) {
    let removed = alice
        .cli
        .run_capture(&["--json", "member", "remove", target_id]);
    assert!(removed.success(), "removal failed: {removed:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a locally built e2e-rendezvous daemon"]
async fn group_update_states_follow_receiver_outcomes() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let mut alice =
        Node::initialized("group-update-alice", "alice-node", &binaries, &rendezvous).await;
    let bob = Node::fresh("group-update-bob", &binaries, &rendezvous).await;
    let carol = Node::fresh("group-update-carol", &binaries, &rendezvous).await;
    let dave = Node::fresh("group-update-dave", &binaries, &rendezvous).await;
    join(&alice, &bob, "bob-node").await;
    join(&alice, &carol, "carol-node").await;
    join(&alice, &dave, "dave-node").await;
    let bob_id = device_id(&alice.cli, "bob-node");
    let carol_id = device_id(&alice.cli, "carol-node");
    let dave_id = device_id(&alice.cli, "dave-node");
    let alice_id = device_id(&carol.cli, "alice-node");
    let truth = TrustReader::new(&alice).await;
    wait_for_phase(&truth, |phase| phase == "completed", "initial convergence").await;
    let host_view = Arc::new(Mutex::new(HostView::default()));
    follow_host_notifications(&alice, truth.clone(), Arc::clone(&host_view)).await;
    gui_checkpoint(&alice, &carol_id, false, "gui-before-update").await;

    // 1. Temporary receiver failure: carol cannot apply the update yet. The
    //    Engine backs off by itself; once carol can apply it, the next
    //    automatic attempt is accepted and the projection settles. dave, who
    //    can apply it, is never blocked by carol.
    carol
        .space_work(serde_json::json!({
            "command": "set_group_update_receive_fault",
            "failure": "not_ready",
        }))
        .await;
    remove_member(&alice, &bob_id);
    let unavailable = wait_for_group_update(
        &truth,
        &carol_id,
        |update| update.is_some_and(|u| u["state"] == "receiver_unavailable"),
        "carol temporarily cannot apply the update",
        WAIT_TIMEOUT,
    )
    .await
    .expect("receiver_unavailable projection");
    assert_eq!(unavailable["recovery"], "automatic");
    assert!(
        unavailable["nextRetryAtMs"].as_i64().is_some(),
        "automatic recovery must carry the Engine retry deadline: {unavailable}"
    );
    // Verified peer contact may expedite a retry before nextRetryAtMs. Reads
    // must still report the same failure and epoch while the fault is active.
    let observation = Instant::now();
    let mut attempts = unavailable["attempts"].as_u64().expect("attempt count");
    while observation.elapsed() < Duration::from_secs(3) {
        let device = truth.device(&carol_id).await.expect("carol relationship");
        let update = &device["groupUpdate"];
        assert_eq!(update["state"], "receiver_unavailable", "{update}");
        assert_eq!(update["epoch"], unavailable["epoch"], "{update}");
        assert_eq!(update["recovery"], "automatic", "{update}");
        assert!(update["nextRetryAtMs"].as_i64().is_some(), "{update}");
        let current = update["attempts"].as_u64().expect("attempt count");
        assert!(current >= attempts, "attempt count regressed: {update}");
        attempts = current;
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    gui_checkpoint(&alice, &carol_id, true, "gui-receiver-unavailable").await;
    accept_pending_decisions(&dave).await;
    wait_for_group_update(
        &truth,
        &dave_id,
        |update| update.is_none(),
        "dave is not blocked by carol",
        WAIT_TIMEOUT,
    )
    .await;
    carol
        .space_work(serde_json::json!({ "command": "set_group_update_receive_fault" }))
        .await;
    let deadline = Instant::now() + WAIT_TIMEOUT;
    loop {
        accept_pending_decisions(&carol).await;
        let device = truth.device(&carol_id).await.expect("carol relationship");
        if device["groupUpdate"].is_null() {
            eprintln!("[group-update] carol accepted the retried update");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "carol's update did not settle after the fault cleared: {device}"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    // A host that only re-reads on daemon notifications must see the settlement.
    let settled_at = Instant::now();
    loop {
        let (host_carol, notifications) = {
            let view = host_view.lock().expect("host view");
            (
                view.snapshot.as_ref().and_then(|s| device_in(s, &carol_id)),
                view.notifications.clone(),
            )
        };
        if host_carol
            .as_ref()
            .is_some_and(|d| d["groupUpdate"].is_null())
        {
            eprintln!(
                "[group-update] notified host saw the settlement; notifications={notifications:?}"
            );
            break;
        }
        assert!(
            settled_at.elapsed() < HOST_GRACE,
            "notified host still shows {:?} after the settlement; notifications={notifications:?}",
            host_carol.map(|d| d["groupUpdate"].clone())
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    gui_checkpoint(&alice, &carol_id, false, "gui-settled").await;
    assert_text_arrives(&alice, &carol, &carol_id, "group update alice to carol").await;
    assert_text_arrives(&carol, &alice, &alice_id, "group update carol to alice").await;

    // 2. Explicit security rejection: carol rejects a tampered update. It is
    //    never resent automatically; re-reading and restarting alice leave it
    //    untouched while verified history is unavailable. Hold history at the
    //    receiver so the gate also survives the sender restart.
    carol
        .space_work(serde_json::json!({ "command": "set_membership_history_held", "held": true }))
        .await;
    alice
        .space_work(serde_json::json!({ "command": "arm_group_update_tamper", "count": 1 }))
        .await;
    remove_member(&alice, &dave_id);
    let rejected = wait_for_group_update(
        &truth,
        &carol_id,
        |update| update.is_some_and(|u| u["state"] == "rejected_by_receiver"),
        "carol explicitly rejects the tampered update",
        WAIT_TIMEOUT,
    )
    .await
    .expect("rejected_by_receiver projection");
    assert_eq!(rejected["recovery"], "review_device");
    assert!(rejected["nextRetryAtMs"].is_null(), "{rejected}");
    assert_reads_do_not_advance(&truth, &carol_id, &rejected, Duration::from_secs(5)).await;
    gui_checkpoint(&alice, &carol_id, true, "gui-rejected-by-receiver").await;
    alice.restart().await;
    let truth = TrustReader::new(&alice).await;
    let after_restart = wait_for_group_update(
        &truth,
        &carol_id,
        |update| update.is_some(),
        "rejection survives alice restart",
        WAIT_TIMEOUT,
    )
    .await
    .expect("projection after restart");
    assert_eq!(
        after_restart, rejected,
        "restart must not retry or clear an explicit rejection"
    );
    // Give any automatic resend a chance to show up; there must be none.
    assert_reads_do_not_advance(&truth, &carol_id, &rejected, Duration::from_secs(30)).await;
    gui_checkpoint(&alice, &carol_id, true, "gui-rejection-after-restart").await;
    remove_member(&alice, &carol_id);
    wait_for_group_update(
        &truth,
        &carol_id,
        |update| update.is_none(),
        "removing carol settles the rejected update",
        WAIT_TIMEOUT,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a locally built e2e-rendezvous daemon"]
async fn legacy_rejection_is_unclassified_and_waits_for_reconnect() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let mut alice =
        Node::initialized("legacy-reject-alice", "alice-node", &binaries, &rendezvous).await;
    let bob = Node::fresh("legacy-reject-bob", &binaries, &rendezvous).await;
    let carol = Node::fresh("legacy-reject-carol", &binaries, &rendezvous).await;
    join(&alice, &bob, "bob-node").await;
    join(&alice, &carol, "carol-node").await;
    let bob_id = device_id(&alice.cli, "bob-node");
    let carol_id = device_id(&alice.cli, "carol-node");
    let truth = TrustReader::new(&alice).await;
    wait_for_phase(&truth, |phase| phase == "completed", "initial convergence").await;

    // An old receiver answers with a rejection that carries no reason.
    alice
        .space_work(serde_json::json!({ "command": "arm_legacy_receiver_rejection", "count": 1 }))
        .await;
    remove_member(&alice, &bob_id);
    let unclassified = wait_for_group_update(
        &truth,
        &carol_id,
        |update| update.is_some_and(|u| u["state"] == "rejected_unclassified"),
        "legacy rejection is unclassified",
        WAIT_TIMEOUT,
    )
    .await
    .expect("rejected_unclassified projection");
    assert_eq!(unclassified["recovery"], "automatic_on_reconnect");
    assert!(unclassified["nextRetryAtMs"].is_null(), "{unclassified}");
    assert_reads_do_not_advance(&truth, &carol_id, &unclassified, Duration::from_secs(5)).await;
    gui_checkpoint(&alice, &carol_id, true, "gui-rejected-unclassified").await;
    alice.restart().await;
    let truth = TrustReader::new(&alice).await;
    let after_restart = wait_for_group_update(
        &truth,
        &carol_id,
        |update| update.is_some(),
        "unclassified rejection survives alice restart",
        WAIT_TIMEOUT,
    )
    .await
    .expect("projection after restart");
    assert_eq!(after_restart["state"], "rejected_unclassified");
    assert_eq!(after_restart["epoch"], unclassified["epoch"]);

    // Re-evaluation needs carol's authenticated return and the Engine's rate
    // limit; this window does not shorten production backoff, so a projection
    // that is still pending here is reported as incomplete, not as failure.
    let deadline = Instant::now() + Duration::from_secs(180);
    let outcome = loop {
        accept_pending_decisions(&carol).await;
        let device = truth.device(&carol_id).await.expect("carol relationship");
        if device["groupUpdate"].is_null() {
            break "settled".to_string();
        }
        if Instant::now() >= deadline {
            break format!("incomplete: {}", device["groupUpdate"]);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    eprintln!("[group-update] legacy rejection re-evaluation within 180s: {outcome}");
}

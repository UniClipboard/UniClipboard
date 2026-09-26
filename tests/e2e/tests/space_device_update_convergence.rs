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
//! Run with:
//! cargo test --manifest-path tests/e2e/Cargo.toml --test space_device_update_convergence -- --ignored --nocapture

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use uc_e2e_tests::{
    get_session_token, InviteSession, LocalRendezvous, NodeBinarySet, TestCli, TestDaemon,
    TestProfile,
};

const PASSPHRASE: &str = "space-device-update-convergence";
const WAIT_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a notified host may lag behind the authoritative status.
const HOST_GRACE: Duration = Duration::from_secs(10);

struct Node {
    daemon: TestDaemon,
    cli: TestCli,
}

impl Node {
    async fn fresh(name: &str, binaries: &NodeBinarySet, rendezvous: &LocalRendezvous) -> Self {
        let profile = TestProfile::new(name);
        let daemon = TestDaemon::start_clean_configured_with(
            profile,
            binaries,
            Some(&rendezvous.uri()),
            |command| {
                command.env(
                    "RUST_LOG",
                    std::env::var("UC_E2E_RUST_LOG")
                        .unwrap_or_else(|_| "uc_webserver::api::server=info".to_string()),
                );
            },
        )
        .await
        .expect("daemon start");
        let cli = TestCli::with_binaries(&daemon.profile, binaries);
        Self { daemon, cli }
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

/// A host view that follows `src/contexts/DeviceTrustContext.tsx`: it only
/// re-reads after a daemon notification, skipping `device-trust.changed`
/// whose revision it has already loaded.
#[derive(Default)]
struct HostView {
    revision: Option<u64>,
    phase: Option<String>,
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
        if let Some((revision, phase)) = reader.snapshot().await {
            let mut view = view.lock().expect("host view");
            view.revision = Some(revision);
            view.phase = Some(phase);
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
    let (issue_id, choice_id) = {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            if let Ok(value) = carol_reader.raw().await {
                let accept = value["issues"].as_array().and_then(|issues| {
                    issues.iter().find_map(|issue| {
                        let choice = issue["choices"]
                            .as_array()?
                            .iter()
                            .find(|choice| choice["isCurrentGroup"] == false)?;
                        Some((
                            issue["issueId"].as_str()?.to_string(),
                            choice["choiceId"].as_str()?.to_string(),
                        ))
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
    let chosen = carol.cli.run_capture(&[
        "--json", "member", "trust", "choose", "--issue", &issue_id, "--choice", &choice_id,
    ]);
    assert!(chosen.success(), "carol decision failed: {chosen:?}");
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

    let host_deadline = converged_at + HOST_GRACE;
    loop {
        let (phase, notifications) = {
            let view = view.lock().expect("host view");
            (view.phase.clone(), view.notifications.clone())
        };
        if phase.as_deref() == Some("completed") {
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

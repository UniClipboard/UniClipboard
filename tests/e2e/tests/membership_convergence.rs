use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use serde_json::Value;
use uc_e2e_tests::{
    InviteSession, LocalRendezvous, NodeBinarySet, TestCli, TestDaemon, TestProfile,
};

const PASSPHRASE: &str = "membership-convergence-e2e-passphrase";
const WAIT_TIMEOUT: Duration = Duration::from_secs(120);
const CLI_POLL_INTERVAL: Duration = Duration::from_secs(2);

struct Node {
    daemon: TestDaemon,
    cli: TestCli,
}

impl Node {
    async fn fresh(name: &str, binaries: &NodeBinarySet, rendezvous: &LocalRendezvous) -> Self {
        let profile = TestProfile::new(name);
        let daemon = TestDaemon::start_clean_with(profile, binaries, Some(&rendezvous.uri()))
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
        assert!(
            output.success(),
            "init {device_name} failed: stdout={} stderr={} log={}",
            output.stdout,
            output.stderr,
            node.daemon.diagnostic_log()
        );
        node
    }

    #[cfg(feature = "membership-diagnostics")]
    async fn restart(&mut self) {
        self.stop().await;
        self.daemon
            .restart_preserving()
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "restart {} failed: {error}\n{}",
                    self.cli.profile_name,
                    self.daemon.diagnostic_log()
                )
            });
    }

    #[cfg(feature = "membership-diagnostics")]
    async fn stop(&mut self) {
        let output = self.cli.run_capture(&["--json", "stop"]);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        while self.daemon.is_running() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "{} did not stop after the CLI request: stdout={} stderr={} log={}",
                self.cli.profile_name,
                output.stdout,
                output.stderr,
                self.daemon.diagnostic_log()
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

struct WatchSession {
    child: Child,
    events: Receiver<Value>,
}

impl WatchSession {
    async fn start(cli: &TestCli) -> Self {
        let mut child = Command::new(cli.binary_path())
            .env("UC_PROFILE", &cli.profile_name)
            .args(["--json", "watch"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("watch spawn");
        let stdout = child.stdout.take().expect("watch stdout");
        let stderr = child.stderr.take().expect("watch stderr");
        let (event_tx, event_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();

        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
            {
                if let Ok(value) = serde_json::from_str::<Value>(line.trim()) {
                    let _ = event_tx.send(value);
                }
            }
        });
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stderr)
                .lines()
                .map_while(Result::ok)
            {
                if line.contains("WATCH_READY") {
                    let _ = ready_tx.send(());
                    return;
                }
            }
        });

        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            match ready_rx.try_recv() {
                Ok(()) => break,
                Err(TryRecvError::Disconnected) => {
                    terminate_child(&mut child);
                    panic!("watch exited before WATCH_READY");
                }
                Err(TryRecvError::Empty) if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(TryRecvError::Empty) => {
                    terminate_child(&mut child);
                    panic!("watch did not emit WATCH_READY within 15s");
                }
            }
        }

        Self {
            child,
            events: event_rx,
        }
    }

    async fn wait_for_text(&self, from_device: &str, text: &str) -> Value {
        let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
        loop {
            match self.events.try_recv() {
                Ok(event)
                    if event.get("from_device").and_then(Value::as_str) == Some(from_device)
                        && event.get("text").and_then(Value::as_str) == Some(text) =>
                {
                    return event;
                }
                Ok(_) => {}
                Err(TryRecvError::Disconnected) => panic!("watch event stream closed"),
                Err(TryRecvError::Empty) if tokio::time::Instant::now() >= deadline => {
                    panic!("watch did not receive exact text from {from_device}: {text}")
                }
                Err(TryRecvError::Empty) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

impl Drop for WatchSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn join(sponsor: &Node, joiner: &Node, joiner_name: &str) -> Value {
    let (session, code) = InviteSession::start(&sponsor.cli).await;
    let output = joiner.cli.run_capture(&[
        "--json",
        "join",
        "--code",
        &code,
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        joiner_name,
    ]);
    session.finish().await;
    assert!(
        output.success(),
        "join {joiner_name} failed: stdout={} stderr={} sponsor_log={} joiner_log={}",
        output.stdout,
        output.stderr,
        sponsor.daemon.diagnostic_log(),
        joiner.daemon.diagnostic_log()
    );
    let value: Value = serde_json::from_str(output.stdout.trim())
        .unwrap_or_else(|error| panic!("join output is not JSON: {error}\n{}", output.stdout));
    assert_eq!(value.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        value.get("self_device_name").and_then(Value::as_str),
        Some(joiner_name)
    );
    for field in [
        "space_id",
        "self_device_id",
        "self_fingerprint",
        "sponsor_device_id",
        "sponsor_fingerprint",
    ] {
        assert!(
            value
                .get(field)
                .and_then(Value::as_str)
                .is_some_and(|item| !item.is_empty()),
            "join response field {field} missing: {value}"
        );
    }
    value
}

fn members(cli: &TestCli) -> Vec<Value> {
    try_members(cli).unwrap_or_else(|error| panic!("{error}"))
}

fn try_members(cli: &TestCli) -> Result<Vec<Value>, String> {
    let output = cli.run_capture(&["--json", "members"]);
    if !output.success() {
        return Err(format!(
            "members failed: stdout={} stderr={}",
            output.stdout, output.stderr
        ));
    }
    serde_json::from_str(output.stdout.trim())
        .map_err(|error| format!("members output is not JSON: {error}\n{}", output.stdout))
}

fn local_device_id(cli: &TestCli) -> String {
    members(cli)
        .into_iter()
        .find(|member| member.get("is_local").and_then(Value::as_bool) == Some(true))
        .and_then(|member| {
            member
                .get("device_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .expect("local device id")
}

fn remote_device_id(cli: &TestCli, name: &str) -> String {
    members(cli)
        .into_iter()
        .find(|member| member.get("device_name").and_then(Value::as_str) == Some(name))
        .and_then(|member| {
            member
                .get("device_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| panic!("member {name} not found"))
}

fn try_convergence(cli: &TestCli) -> Result<String, String> {
    let output = cli.run_capture(&["--json", "status"]);
    if !output.success() {
        return Err(format!(
            "status failed: stdout={} stderr={}",
            output.stdout, output.stderr
        ));
    }
    let value: Value = serde_json::from_str(output.stdout.trim())
        .map_err(|error| format!("status output is not JSON: {error}\n{}", output.stdout))?;
    value
        .get("membership_convergence")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("membership_convergence missing: {value}"))
}

async fn wait_for_convergence(node: &Node, expected: &str) {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        let last = match try_convergence(&node.cli) {
            Ok(state) if state == expected => return,
            Ok(state) => state,
            Err(error) => error,
        };
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "{} did not reach {expected}; last={last}; members={:?}; log={}",
                node.cli.profile_name,
                try_members(&node.cli),
                node.daemon.diagnostic_log()
            );
        }
        tokio::time::sleep(CLI_POLL_INTERVAL).await;
    }
}

async fn wait_for_member_count(node: &Node, expected: usize) {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        match try_members(&node.cli) {
            Ok(current) if current.len() == expected => return,
            Ok(current) if tokio::time::Instant::now() >= deadline => {
                panic!(
                    "{} member count did not reach {expected}; members={current:?}; log={}",
                    node.cli.profile_name,
                    node.daemon.diagnostic_log()
                );
            }
            Err(error) if tokio::time::Instant::now() >= deadline => {
                panic!(
                    "{} member query did not recover: {error}; log={}",
                    node.cli.profile_name,
                    node.daemon.diagnostic_log()
                );
            }
            _ => {}
        }
        tokio::time::sleep(CLI_POLL_INTERVAL).await;
    }
}

async fn assert_exact_delivery(sender: &Node, receiver: &Node, receiver_name: &str, text: &str) {
    let sender_id = local_device_id(&sender.cli);
    let receiver_id = remote_device_id(&sender.cli, receiver_name);
    let watch = WatchSession::start(&receiver.cli).await;
    let output = sender
        .cli
        .run_capture(&["--json", "send", text, "--peer", &receiver_id]);
    assert!(
        output.success(),
        "send failed: stdout={} stderr={} sender_log={} receiver_log={}",
        output.stdout,
        output.stderr,
        sender.daemon.diagnostic_log(),
        receiver.daemon.diagnostic_log()
    );
    let outcome: Value = serde_json::from_str(output.stdout.trim())
        .unwrap_or_else(|error| panic!("send output is not JSON: {error}\n{}", output.stdout));
    assert_eq!(outcome["totalAccepted"], 1, "send outcome: {outcome}");
    assert_eq!(outcome["totalOffline"], 0, "send outcome: {outcome}");
    assert_eq!(outcome["totalErrored"], 0, "send outcome: {outcome}");
    watch.wait_for_text(&sender_id, text).await;
    drop(watch);

    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        let list_output = receiver
            .cli
            .run_capture(&["--json", "get", "--list", "--limit", "20"]);
        assert!(list_output.success(), "get --list failed: {list_output:?}");
        let rows: Vec<Value> = serde_json::from_str(list_output.stdout.trim())
            .unwrap_or_else(|error| panic!("get list is not JSON: {error}"));
        if let Some(entry_id) = rows.iter().find_map(|row| {
            (row.get("preview").and_then(Value::as_str) == Some(text))
                .then(|| row.get("entry_id").and_then(Value::as_str))
                .flatten()
        }) {
            let detail = receiver
                .cli
                .run_capture(&["--json", "get", "--id", entry_id]);
            assert!(detail.success(), "get --id failed: {detail:?}");
            let detail: Value = serde_json::from_str(detail.stdout.trim())
                .unwrap_or_else(|error| panic!("get detail is not JSON: {error}"));
            assert_eq!(detail.get("text").and_then(Value::as_str), Some(text));
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "receiver history did not persist exact text {text}; log={}",
                receiver.daemon.diagnostic_log()
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[cfg(feature = "membership-diagnostics")]
async fn setup_three_nodes_with_a_offline(
    prefix: &str,
    binaries: &NodeBinarySet,
    rendezvous: &LocalRendezvous,
) -> (Node, Node, Node) {
    let mut a = Node::initialized(&format!("{prefix}-a"), "node-a", binaries, rendezvous).await;
    let b = Node::fresh(&format!("{prefix}-b"), binaries, rendezvous).await;
    join(&a, &b, "node-b").await;
    wait_for_convergence(&a, "complete").await;
    wait_for_convergence(&b, "complete").await;
    a.stop().await;

    let c = Node::fresh(&format!("{prefix}-c"), binaries, rendezvous).await;
    join(&b, &c, "node-c").await;
    wait_for_convergence(&c, "converging").await;
    (a, b, c)
}

#[tokio::test]
#[ignore]
async fn c0_single_node_reports_complete() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let node = Node::initialized("membership-c0", "node-a", &binaries, &rendezvous).await;

    wait_for_convergence(&node, "complete").await;
    assert_eq!(members(&node.cli).len(), 1);
}

#[tokio::test]
#[ignore]
async fn c1_online_sponsor_chain_converges_and_syncs_directly() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("membership-c1-a", "node-a", &binaries, &rendezvous).await;
    let b = Node::fresh("membership-c1-b", &binaries, &rendezvous).await;
    join(&a, &b, "node-b").await;
    let c = Node::fresh("membership-c1-c", &binaries, &rendezvous).await;
    join(&b, &c, "node-c").await;

    for node in [&a, &b, &c] {
        wait_for_convergence(node, "complete").await;
        wait_for_member_count(node, 3).await;
    }
    assert_exact_delivery(&a, &c, "node-c", "c1-a-to-c").await;
    assert_exact_delivery(&c, &a, "node-a", "c1-c-to-a").await;
}

#[tokio::test]
#[ignore]
#[cfg(feature = "membership-diagnostics")]
async fn c2_offline_member_recovers_without_sponsor() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let (mut a, mut b, mut c) =
        setup_three_nodes_with_a_offline("membership-c2", &binaries, &rendezvous).await;
    b.stop().await;
    c.restart().await;
    a.restart().await;

    wait_for_convergence(&a, "complete").await;
    wait_for_convergence(&c, "complete").await;
    wait_for_member_count(&a, 3).await;
    wait_for_member_count(&c, 3).await;
    assert_exact_delivery(&a, &c, "node-c", "c2-a-to-c").await;
    assert_exact_delivery(&c, &a, "node-a", "c2-c-to-a").await;
}

#[tokio::test]
#[ignore]
#[cfg(feature = "membership-diagnostics")]
async fn c2_control_recovers_when_joiner_stays_running() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let (mut a, mut b, c) =
        setup_three_nodes_with_a_offline("membership-c2-control", &binaries, &rendezvous).await;
    b.stop().await;
    a.restart().await;

    wait_for_convergence(&a, "complete").await;
    wait_for_convergence(&c, "complete").await;
    wait_for_member_count(&a, 3).await;
    wait_for_member_count(&c, 3).await;
    assert_exact_delivery(&a, &c, "node-c", "c2-control-a-to-c").await;
    assert_exact_delivery(&c, &a, "node-a", "c2-control-c-to-a").await;
}

#[tokio::test]
#[ignore]
#[cfg(feature = "membership-diagnostics")]
async fn c3_recovered_membership_survives_restart_without_duplicates() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let (mut a, mut b, mut c) =
        setup_three_nodes_with_a_offline("membership-c3", &binaries, &rendezvous).await;
    b.stop().await;
    c.restart().await;
    a.restart().await;
    wait_for_convergence(&a, "complete").await;
    wait_for_convergence(&c, "complete").await;

    a.restart().await;
    c.restart().await;
    wait_for_convergence(&a, "complete").await;
    wait_for_convergence(&c, "complete").await;
    assert_eq!(
        members(&a.cli).len(),
        3,
        "A membership duplicated after restart"
    );
    assert_eq!(
        members(&c.cli).len(),
        3,
        "C membership duplicated after restart"
    );
    assert_exact_delivery(&a, &c, "node-c", "c3-a-to-c").await;
    assert_exact_delivery(&c, &a, "node-a", "c3-c-to-a").await;
}

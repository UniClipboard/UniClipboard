#![cfg(feature = "historical-membership")]

use serde_json::Value;
use std::time::Duration;
use uc_e2e_tests::{
    InviteSession, NodeBinarySet, TestCli, TestDaemon, TestProfile, LEGACY_RELEASE_VERSION,
};

const PASSPHRASE: &str = "membership-compatibility-e2e-passphrase";

struct Node {
    daemon: TestDaemon,
    cli: TestCli,
}

impl Node {
    async fn fresh(name: &str, binaries: &NodeBinarySet) -> Self {
        let profile = TestProfile::new(name);
        let daemon = TestDaemon::start_clean_with(profile, binaries, None)
            .await
            .unwrap_or_else(|error| panic!("start {name} failed: {error}"));
        let cli = TestCli::with_binaries(&daemon.profile, binaries);
        Self { daemon, cli }
    }

    async fn initialized(name: &str, device_name: &str, binaries: &NodeBinarySet) -> Self {
        let node = Self::fresh(name, binaries).await;
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

    async fn replace_binaries(&mut self, binaries: &NodeBinarySet) {
        let binary = self.cli.binary_path().to_path_buf();
        let profile = self.cli.profile_name.clone();
        let stop_task = tokio::task::spawn_blocking(move || {
            let output = std::process::Command::new(binary)
                .env("UC_PROFILE", profile)
                .env("UNICLIPBOARD_ENV", "development")
                .args(["--json", "stop"])
                .output()
                .expect("run stop command");
            uc_e2e_tests::CapturedOutput {
                exit_code: output.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            }
        });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        while !stop_task.is_finished() {
            self.daemon.is_running();
            assert!(
                tokio::time::Instant::now() < deadline,
                "stop command did not finish before binary replacement"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let stop = stop_task.await.expect("stop task panicked");
        assert!(
            stop.success(),
            "stop before upgrade failed: stdout={} stderr={}",
            stop.stdout,
            stop.stderr
        );
        while self.daemon.is_running() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "daemon did not stop before binary replacement"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.daemon
            .restart_preserving_with(binaries, None)
            .await
            .unwrap_or_else(|error| panic!("restart with replacement binaries failed: {error}"));
        self.cli = TestCli::with_binaries(&self.daemon.profile, binaries);
    }
}

fn legacy_binaries() -> NodeBinarySet {
    let directory = std::env::var_os("UC_E2E_LEGACY_RELEASE_DIR")
        .map(std::path::PathBuf::from)
        .expect("UC_E2E_LEGACY_RELEASE_DIR must point to the verified fixed release");
    NodeBinarySet::fixed_release_dir(LEGACY_RELEASE_VERSION, directory)
        .expect("verified fixed release binaries")
}

fn members(cli: &TestCli) -> Vec<Value> {
    let output = cli.run_capture(&["--json", "members"]);
    assert!(
        output.success(),
        "members failed: stdout={} stderr={}",
        output.stdout,
        output.stderr
    );
    serde_json::from_str(output.stdout.trim())
        .unwrap_or_else(|error| panic!("members output is not JSON: {error}\n{}", output.stdout))
}

fn error_json(stderr: &str) -> Value {
    stderr
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .unwrap_or_else(|| panic!("stderr does not contain a JSON error: {stderr}"))
}

async fn join(sponsor: &Node, joiner: &Node, joiner_name: &str) {
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
    if !output.stdout.trim().is_empty() {
        let value: Value = serde_json::from_str(output.stdout.trim())
            .unwrap_or_else(|error| panic!("join output is not JSON: {error}\n{}", output.stdout));
        assert_eq!(value.get("ok").and_then(Value::as_bool), Some(true));
    }
}

async fn wait_for_member_count(node: &Node, expected: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        let current = members(&node.cli);
        if current.len() == expected {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "member count did not reach {expected}: {current:?}\n{}",
            node.daemon.diagnostic_log()
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn wait_for_convergence(node: &Node, expected: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        let output = node.cli.run_capture(&["--json", "status"]);
        if output.success() {
            let value: Value = serde_json::from_str(output.stdout.trim()).unwrap_or(Value::Null);
            if value.get("membership_convergence").and_then(Value::as_str) == Some(expected) {
                return;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "convergence did not reach {expected}: stdout={} stderr={}\n{}",
            output.stdout,
            output.stderr,
            node.daemon.diagnostic_log()
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn assert_unique_members(node: &Node, expected: usize) {
    let current = members(&node.cli);
    assert_eq!(
        current.len(),
        expected,
        "unexpected member roster: {current:?}"
    );
    let mut device_ids = current
        .iter()
        .filter_map(|member| member.get("device_id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    device_ids.sort_unstable();
    device_ids.dedup();
    assert_eq!(
        device_ids.len(),
        expected,
        "duplicate member identities: {current:?}"
    );
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

async fn assert_delivery(sender: &Node, receiver: &Node, receiver_name: &str, text: &str) {
    let receiver_id = remote_device_id(&sender.cli, receiver_name);
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

    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        let list = receiver
            .cli
            .run_capture(&["--json", "get", "--list", "--limit", "20"]);
        if list.success() {
            let rows: Vec<Value> = serde_json::from_str(list.stdout.trim()).unwrap_or_default();
            if let Some(entry_id) = rows.iter().find_map(|row| {
                (row.get("preview").and_then(Value::as_str) == Some(text))
                    .then(|| row.get("entry_id").and_then(Value::as_str))
                    .flatten()
            }) {
                let detail = receiver
                    .cli
                    .run_capture(&["--json", "get", "--id", entry_id]);
                assert!(detail.success(), "get detail failed: {detail:?}");
                let detail: Value = serde_json::from_str(detail.stdout.trim())
                    .unwrap_or_else(|error| panic!("get detail is not JSON: {error}"));
                assert_eq!(detail.get("text").and_then(Value::as_str), Some(text));
                return;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "receiver history did not persist {text}\n{}",
            receiver.daemon.diagnostic_log()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test]
#[ignore]
async fn h1_current_joiner_rejects_legacy_sponsor_without_partial_setup() {
    let legacy = legacy_binaries();
    let current = NodeBinarySet::current();
    let sponsor = Node::initialized("membership-h1-sponsor", "legacy-sponsor", &legacy).await;
    let joiner = Node::fresh("membership-h1-joiner", &current).await;
    let (session, code) = InviteSession::start(&sponsor.cli).await;

    let output = joiner.cli.run_capture(&[
        "--json",
        "join",
        "--code",
        &code,
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "current-joiner",
    ]);
    drop(session);

    assert!(
        !output.success(),
        "legacy sponsor unexpectedly accepted join"
    );
    let error = error_json(&output.stderr);
    assert_eq!(error.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        error.get("code").and_then(Value::as_str),
        Some("sponsor_upgrade_required")
    );
    assert_eq!(members(&sponsor.cli).len(), 1);

    let init = joiner.cli.run_capture(&[
        "init",
        "--passphrase",
        "independent-space-passphrase",
        "--device-name",
        "current-joiner",
    ]);
    assert!(
        init.success(),
        "failed join left partial setup: stdout={} stderr={} log={}",
        init.stdout,
        init.stderr,
        joiner.daemon.diagnostic_log()
    );
    assert_eq!(members(&joiner.cli).len(), 1);
}

#[tokio::test]
#[ignore]
async fn h2_join_succeeds_after_legacy_sponsor_is_upgraded_in_place() {
    let legacy = legacy_binaries();
    let current = NodeBinarySet::current();
    let mut sponsor = Node::initialized("membership-h2-sponsor", "upgraded-sponsor", &legacy).await;
    let joiner = Node::fresh("membership-h2-joiner", &current).await;
    let (session, code) = InviteSession::start(&sponsor.cli).await;
    let rejected = joiner.cli.run_capture(&[
        "--json",
        "join",
        "--code",
        &code,
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "current-joiner",
    ]);
    drop(session);
    assert_eq!(
        error_json(&rejected.stderr)
            .get("code")
            .and_then(Value::as_str),
        Some("sponsor_upgrade_required")
    );

    sponsor.replace_binaries(&current).await;
    join(&sponsor, &joiner, "current-joiner").await;
    wait_for_convergence(&sponsor, "complete").await;
    wait_for_convergence(&joiner, "complete").await;
    wait_for_member_count(&sponsor, 2).await;
    wait_for_member_count(&joiner, 2).await;
    assert_delivery(&sponsor, &joiner, "current-joiner", "h2-sponsor-to-joiner").await;
    assert_delivery(
        &joiner,
        &sponsor,
        "upgraded-sponsor",
        "h2-joiner-to-sponsor",
    )
    .await;
    assert!(!local_device_id(&sponsor.cli).is_empty());
}

async fn partially_upgraded_three_member_space(test_name: &str) -> (Node, Node, Node) {
    let legacy = legacy_binaries();
    let current = NodeBinarySet::current();
    let legacy_a_name = format!("{test_name}-legacy-a");
    let legacy_b_name = format!("{test_name}-legacy-b");
    let current_c_name = format!("{test_name}-current-c");

    let legacy_a = Node::initialized(&legacy_a_name, "legacy-a", &legacy).await;
    let mut upgraded_b = Node::fresh(&legacy_b_name, &legacy).await;
    join(&legacy_a, &upgraded_b, "upgraded-b").await;
    wait_for_member_count(&legacy_a, 2).await;
    wait_for_member_count(&upgraded_b, 2).await;

    upgraded_b.replace_binaries(&current).await;
    let current_c = Node::fresh(&current_c_name, &current).await;
    join(&upgraded_b, &current_c, "current-c").await;
    wait_for_member_count(&upgraded_b, 3).await;
    wait_for_convergence(&upgraded_b, "waiting_for_upgrade").await;
    wait_for_convergence(&current_c, "waiting_for_upgrade").await;

    (legacy_a, upgraded_b, current_c)
}

#[tokio::test]
#[ignore]
async fn h3_partial_upgrade_reports_waiting_for_upgrade() {
    let (_legacy_a, upgraded_b, current_c) =
        partially_upgraded_three_member_space("membership-h3").await;

    wait_for_member_count(&current_c, 3).await;
    assert_unique_members(&upgraded_b, 3);
    assert_unique_members(&current_c, 3);
}

#[tokio::test]
#[ignore]
async fn h4_final_legacy_upgrade_converges_without_repairing() {
    let current = NodeBinarySet::current();
    let (mut upgraded_a, mut offline_b, mut current_c) =
        partially_upgraded_three_member_space("membership-h4").await;

    upgraded_a.replace_binaries(&current).await;
    wait_for_convergence(&upgraded_a, "complete").await;
    wait_for_convergence(&offline_b, "complete").await;
    wait_for_convergence(&current_c, "complete").await;
    wait_for_member_count(&upgraded_a, 3).await;
    wait_for_member_count(&offline_b, 3).await;
    wait_for_member_count(&current_c, 3).await;

    offline_b.daemon.kill();
    wait_for_convergence(&upgraded_a, "complete").await;
    wait_for_convergence(&current_c, "complete").await;
    assert_unique_members(&upgraded_a, 3);
    assert_unique_members(&current_c, 3);
    assert_delivery(&upgraded_a, &current_c, "current-c", "h4-a-to-c").await;
    assert_delivery(&current_c, &upgraded_a, "legacy-a", "h4-c-to-a").await;

    upgraded_a
        .daemon
        .restart_preserving()
        .await
        .expect("restart upgraded A");
    current_c
        .daemon
        .restart_preserving()
        .await
        .expect("restart current C");
    wait_for_convergence(&upgraded_a, "complete").await;
    wait_for_convergence(&current_c, "complete").await;
    assert_unique_members(&upgraded_a, 3);
    assert_unique_members(&current_c, 3);
}

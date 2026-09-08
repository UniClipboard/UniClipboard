use std::time::Duration;

#[cfg(unix)]
use std::process::{Child, Command, Stdio};

use serde_json::Value;
use uc_e2e_tests::{
    get_session_token, InviteSession, LocalRendezvous, NodeBinarySet, TestCli, TestDaemon,
    TestProfile,
};

const PASSPHRASE: &str = "cli-engine-workflows-passphrase";
const WAIT_TIMEOUT: Duration = Duration::from_secs(120);

struct Node {
    daemon: TestDaemon,
    cli: TestCli,
}

#[cfg(unix)]
struct ChildGuard(Option<Child>);

#[cfg(unix)]
impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn child_mut(&mut self) -> &mut Child {
        self.0.as_mut().expect("child already consumed")
    }

    fn take(&mut self) -> Child {
        self.0.take().expect("child already consumed")
    }
}

#[cfg(unix)]
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
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
        assert!(
            output.success(),
            "init failed: stdout={} stderr={} log={}",
            output.stdout,
            output.stderr,
            node.daemon.diagnostic_log()
        );
        node
    }

    async fn restart(&mut self) {
        self.daemon
            .restart_preserving_configured_with(|command| {
                command.env(
                    "RUST_LOG",
                    std::env::var("UC_E2E_RUST_LOG")
                        .unwrap_or_else(|_| "uc_webserver::api::server=info".to_string()),
                );
            })
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "restart {} failed: {error}\n{}",
                    self.cli.profile_name,
                    self.daemon.diagnostic_log()
                )
            });
    }
}

fn json(output: &uc_e2e_tests::CapturedOutput) -> Value {
    serde_json::from_str(output.stdout.trim()).unwrap_or_else(|error| {
        panic!(
            "CLI output is not JSON: {error}; stdout={} stderr={}",
            output.stdout, output.stderr
        )
    })
}

fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'[') {
            index += 2;
            while index < bytes.len() {
                let byte = bytes[index];
                index += 1;
                if (0x40..=0x7e).contains(&byte) {
                    break;
                }
            }
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn daemon_request_count(node: &Node, method: &str, path: &str) -> usize {
    let log = strip_ansi(&node.daemon.diagnostic_log());
    log.lines()
        .filter(|line| {
            line.contains("daemon http request received")
                && line.contains(&format!("method={method}"))
                && line.contains(&format!("path={path}"))
        })
        .count()
}

fn assert_request_delta(node: &Node, method: &str, path: &str, before: usize, expected: usize) {
    assert_eq!(
        daemon_request_count(node, method, path) - before,
        expected,
        "unexpected {method} {path} request count; log={}",
        node.daemon.diagnostic_log()
    );
}

async fn join(sponsor: &Node, joiner: &Node, device_name: &str, no_wait: bool) -> Value {
    let (session, code) = InviteSession::start(&sponsor.cli).await;
    let result = run_join(joiner, &code, device_name, no_wait);
    session.finish().await;
    result
}

fn run_join(joiner: &Node, code: &str, device_name: &str, no_wait: bool) -> Value {
    let requests_before = daemon_request_count(joiner, "POST", "/v2/setup/redeem");
    let mut args = vec![
        "--json",
        "join",
        "--code",
        code,
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        device_name,
    ];
    if no_wait {
        args.push("--no-wait");
    }
    let asserted = assert_cmd::Command::new(joiner.cli.binary_path())
        .env("UC_PROFILE", &joiner.cli.profile_name)
        .env("UNICLIPBOARD_ENV", "development")
        .args(&args)
        .timeout(if no_wait {
            Duration::from_secs(10)
        } else {
            WAIT_TIMEOUT
        })
        .assert();
    let output = asserted.get_output();
    assert!(
        output.status.success(),
        "join failed: stdout={} stderr={} joiner_log={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        joiner.daemon.diagnostic_log()
    );
    assert_request_delta(joiner, "POST", "/v2/setup/redeem", requests_before, 1);
    serde_json::from_slice(&output.stdout).expect("join result must be JSON")
}

fn members(cli: &TestCli) -> Vec<Value> {
    let output = cli.run_capture(&["--json", "members"]);
    assert!(output.success(), "members failed: {output:?}");
    json(&output)
        .as_array()
        .expect("members must be an array")
        .clone()
}

async fn setup_state(node: &Node) -> Value {
    let client = reqwest::Client::new();
    let token = get_session_token(&node.daemon, &client).await;
    let response = client
        .get(format!("{}/v2/setup/state", node.daemon.base_url()))
        .header("Authorization", format!("Session {token}"))
        .send()
        .await
        .expect("setup state request");
    assert!(response.status().is_success(), "setup state request failed");
    let body: Value = response.json().await.expect("setup state json");
    body["data"].clone()
}

fn device_id(cli: &TestCli, name: &str) -> String {
    members(cli)
        .into_iter()
        .find(|member| member["device_name"] == name)
        .and_then(|member| member["device_id"].as_str().map(str::to_string))
        .unwrap_or_else(|| panic!("member {name} not found"))
}

async fn wait_for_trust_change(node: &Node) -> Value {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        let output = node
            .cli
            .run_capture(&["--json", "member", "trust", "status"]);
        if output.success() {
            let value = json(&output);
            if value["issues"]
                .as_array()
                .is_some_and(|issues| !issues.is_empty())
            {
                return value;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "device trust change did not arrive; last={output:?}; log={}",
            node.daemon.diagnostic_log()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn confirm_device_group(
    node: &Node,
    expected_issue: &Value,
    expected_choice: &Value,
    confirm_local_removal: bool,
) -> Value {
    let issue_id = expected_issue["issueId"].as_str().expect("issue id");
    let choice_id = expected_choice["choiceId"].as_str().expect("choice id");
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        // Each submission represents a new user confirmation of freshly read facts.
        let current = node
            .cli
            .run_capture(&["--json", "member", "trust", "status"]);
        assert!(
            current.success(),
            "read choices before confirmation failed: {current:?}"
        );
        let current = json(&current);
        let issue = current["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .find(|issue| issue["issueId"] == issue_id)
            .expect("same issue must remain current");
        let choice = issue["choices"]
            .as_array()
            .expect("choices")
            .iter()
            .find(|choice| choice["choiceId"] == choice_id)
            .expect("same choice must remain available");
        assert_eq!(
            issue["reason"]["changes"],
            expected_issue["reason"]["changes"]
        );
        assert_eq!(
            choice["memberDeviceIds"],
            expected_choice["memberDeviceIds"]
        );
        assert_eq!(choice["membersComplete"], true);
        assert_eq!(
            choice["requiresRePairing"],
            expected_choice["requiresRePairing"]
        );
        assert_eq!(
            choice["impact"]["localDeviceOutcome"],
            expected_choice["impact"]["localDeviceOutcome"]
        );

        let mut args = vec![
            "--json", "member", "trust", "choose", "--issue", issue_id, "--choice", choice_id,
        ];
        if confirm_local_removal {
            args.push("--confirm-local-removal");
        }
        let before = daemon_request_count(node, "POST", "/member/device-group-choices");
        let output = node.cli.run_capture(&args);
        assert_request_delta(node, "POST", "/member/device-group-choices", before, 1);
        let result = json(&output);
        if result["result"]["outcome"] != "state_changed" {
            assert!(
                output.success(),
                "device group confirmation failed: {output:?}"
            );
            return result;
        }
        assert_eq!(output.exit_code, 1);
        assert_eq!(result["ok"], false);
        assert!(
            result["state"]["revision"].as_u64().expect("new revision")
                > current["revision"].as_u64().expect("reviewed revision")
        );
        assert!(
            tokio::time::Instant::now() < deadline,
            "device group never settled: {result}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test]
#[ignore]
async fn join_commands_report_none_then_real_active_join() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let sponsor = Node::initialized(
        "cli-workflow-join-sponsor",
        "sponsor-node",
        &binaries,
        &rendezvous,
    )
    .await;
    let mut joiner = Node::fresh("cli-workflow-joiner", &binaries, &rendezvous).await;

    let status = joiner.cli.run_capture(&["--json", "join", "status"]);
    assert!(status.success(), "join status failed: {status:?}");
    assert_eq!(json(&status)["status"], "none");

    let human_status = joiner.cli.run_capture(&["join", "status"]);
    assert!(
        human_status.success(),
        "human join status failed: {human_status:?}"
    );
    assert!(human_status.stderr.contains("none"), "{human_status:?}");

    let cancel = joiner.cli.run_capture(&["--json", "join", "cancel"]);
    assert!(cancel.success(), "empty join cancel failed: {cancel:?}");
    assert_eq!(json(&cancel)["status"], "none");

    let joined = join(&sponsor, &joiner, "joiner-node", false).await;
    assert_eq!(joined["ok"], true);
    assert_eq!(joined["status"], "active");
    let join_id = joined["join_id"].as_str().expect("active join id");

    let status = joiner.cli.run_capture(&["--json", "join", "status"]);
    assert!(status.success(), "active join status failed: {status:?}");
    let status = json(&status);
    assert_eq!(status["status"], "active");
    assert_eq!(status["join_id"], join_id);

    let cancel_requests_before = daemon_request_count(&joiner, "POST", "/v2/setup/cancel-join");
    let cancel = joiner.cli.run_capture(&["--json", "join", "cancel"]);
    assert!(cancel.success(), "active join cancel failed: {cancel:?}");
    let cancel = json(&cancel);
    assert_eq!(cancel["ok"], true);
    assert_eq!(cancel["status"], "active");
    assert_eq!(cancel["join_id"], join_id);
    assert_request_delta(
        &joiner,
        "POST",
        "/v2/setup/cancel-join",
        cancel_requests_before,
        0,
    );

    let human_status = joiner.cli.run_capture(&["join", "status"]);
    assert!(
        human_status.success(),
        "human active join status failed: {human_status:?}"
    );
    assert!(
        human_status.stderr.contains("active") || human_status.stderr.contains("Join completed")
    );

    joiner.restart().await;
    let restarted_status = joiner.cli.run_capture(&["--json", "join", "status"]);
    assert!(
        restarted_status.success(),
        "join status after daemon restart failed: {restarted_status:?}"
    );
    let restarted_status = json(&restarted_status);
    assert_eq!(restarted_status["status"], "active");
    assert_eq!(restarted_status["join_id"], join_id);
}

#[cfg(unix)]
#[tokio::test]
#[ignore]
async fn no_wait_returns_a_saved_pending_join_while_sponsor_is_offline() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let mut sponsor = Node::initialized(
        "cli-no-wait-sponsor",
        "sponsor-node",
        &binaries,
        &rendezvous,
    )
    .await;
    let joiner = Node::fresh("cli-no-wait-joiner", &binaries, &rendezvous).await;
    let (session, code) = InviteSession::start(&sponsor.cli).await;
    sponsor
        .daemon
        .suspend()
        .expect("hold sponsor offline before joining");

    let joined = run_join(&joiner, &code, "joiner-node", true);
    assert_eq!(joined["ok"], true);
    assert_eq!(joined["status"], "pending");
    let join_id = joined["join_id"].as_str().expect("pending join id");

    let status = joiner.cli.run_capture(&["--json", "join", "status"]);
    assert!(status.success(), "saved join status failed: {status:?}");
    let status = json(&status);
    assert_eq!(status["status"], "pending");
    assert_eq!(status["join_id"], join_id);

    let cancelled = joiner.cli.run_capture(&["--json", "join", "cancel"]);
    assert!(
        cancelled.success(),
        "cancel saved join failed: {cancelled:?}"
    );
    assert_eq!(json(&cancelled)["join_id"], join_id);
    let status = joiner.cli.run_capture(&["--json", "join", "status"]);
    assert!(status.success(), "cancelled join status failed: {status:?}");
    let status = json(&status);
    assert_eq!(status["status"], "rejected");
    assert_eq!(status["reason"], "cancelled");
    assert_eq!(status["join_id"], join_id);
    drop(session);
}

#[tokio::test]
#[ignore]
async fn space_reset_rebuilds_membership_and_preserves_local_history() {
    let binaries = NodeBinarySet::current_dev_cli();
    let rendezvous = LocalRendezvous::start().await;
    let mut alice = Node::initialized(
        "cli-workflow-reset-alice",
        "alice-node",
        &binaries,
        &rendezvous,
    )
    .await;
    assert_eq!(setup_state(&alice).await["rePairingRequired"], false);

    alice.daemon.kill();
    let seeded = alice.cli.run_capture(&[
        "dev",
        "seed-clipboard",
        "--text",
        "history survives space reset",
    ]);
    assert!(seeded.success(), "history seed failed: {seeded:?}");
    let entry_id = seeded
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("SEED_ENTRY_ID="))
        .expect("seeded entry id")
        .to_string();
    alice.restart().await;

    let requests_before = daemon_request_count(&alice, "POST", "/v2/setup/reset");
    let reset = alice
        .cli
        .run_capture(&["--json", "space", "reset", "--yes"]);
    assert!(reset.success(), "space reset failed: {reset:?}");
    assert_eq!(
        json(&reset),
        serde_json::json!({ "ok": true, "status": "rebuilt" })
    );
    assert_request_delta(&alice, "POST", "/v2/setup/reset", requests_before, 1);

    let after_reset = members(&alice.cli);
    assert_eq!(after_reset.len(), 1);
    assert_eq!(after_reset[0]["device_name"], "alice-node");
    assert_eq!(setup_state(&alice).await["rePairingRequired"], true);

    let history = alice.cli.run_capture(&["--json", "get", "--list"]);
    assert!(
        history.success(),
        "history list after reset failed: {history:?}"
    );
    assert!(json(&history)
        .as_array()
        .is_some_and(|entries| { entries.iter().any(|entry| entry["entry_id"] == entry_id) }));

    alice.restart().await;
    let restarted_history = alice.cli.run_capture(&["--json", "get", "--list"]);
    assert!(
        restarted_history.success(),
        "history list after restart failed: {restarted_history:?}"
    );
    assert!(json(&restarted_history)
        .as_array()
        .is_some_and(|entries| { entries.iter().any(|entry| entry["entry_id"] == entry_id) }));
    assert_eq!(members(&alice.cli).len(), 1);
    assert_eq!(setup_state(&alice).await["rePairingRequired"], true);
}

#[cfg(unix)]
#[tokio::test]
#[ignore]
async fn pending_join_survives_ctrl_c_and_daemon_restart_then_can_be_cancelled() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let sponsor_profile = TestProfile::new("cli-workflow-pending-sponsor");
    let sponsor_daemon = TestDaemon::start_clean_configured_with(
        sponsor_profile,
        &binaries,
        Some(&rendezvous.uri()),
        |command| {
            command.env(
                "RUST_LOG",
                "uc_application=info,uc_webserver::api::server=info",
            );
        },
    )
    .await
    .expect("sponsor daemon start");
    let mut sponsor = Node {
        cli: TestCli::with_binaries(&sponsor_daemon.profile, &binaries),
        daemon: sponsor_daemon,
    };
    let sponsor_init = sponsor.cli.run_capture(&[
        "init",
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "sponsor-node",
    ]);
    assert!(
        sponsor_init.success(),
        "sponsor init failed: {sponsor_init:?}"
    );
    let joiner_profile = TestProfile::new("cli-workflow-pending-joiner");
    let joiner_daemon = TestDaemon::start_clean_configured_with(
        joiner_profile,
        &binaries,
        Some(&rendezvous.uri()),
        |command| {
            command.env(
                "RUST_LOG",
                "uc_application=info,uc_webserver::api::server=info",
            );
        },
    )
    .await
    .expect("joiner daemon start");
    let mut joiner = Node {
        cli: TestCli::with_binaries(&joiner_daemon.profile, &binaries),
        daemon: joiner_daemon,
    };
    let (session, code) = InviteSession::start(&sponsor.cli).await;
    let join_requests_before = daemon_request_count(&joiner, "POST", "/v2/setup/redeem");
    // Joining is persisted before network exchange; an offline sponsor keeps it pending.
    sponsor
        .daemon
        .suspend()
        .expect("suspend sponsor before joining");

    let child = Command::new(joiner.cli.binary_path())
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
            "joiner-node",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn waiting join");
    let mut waiting_join = ChildGuard::new(child);

    let pending_deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let pending = loop {
        let status = joiner.cli.run_capture(&["--json", "join", "status"]);
        if status.success() {
            let value = json(&status);
            if value["status"] == "pending" {
                break value;
            }
        }
        assert!(
            tokio::time::Instant::now() < pending_deadline,
            "pending join did not become observable; joiner_log={}",
            joiner.daemon.diagnostic_log()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    let join_id = pending["join_id"]
        .as_str()
        .expect("pending join id")
        .to_string();
    assert_request_delta(&joiner, "POST", "/v2/setup/redeem", join_requests_before, 1);

    let join_pid = waiting_join.child_mut().id();
    assert_eq!(unsafe { libc::kill(join_pid as i32, libc::SIGINT) }, 0);
    let interrupt_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while waiting_join
        .child_mut()
        .try_wait()
        .expect("join child state")
        .is_none()
    {
        assert!(
            tokio::time::Instant::now() < interrupt_deadline,
            "waiting join did not stop after Ctrl-C; joiner_log={}",
            joiner.daemon.diagnostic_log()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let interrupted = waiting_join
        .take()
        .wait_with_output()
        .expect("join wait output");
    assert_eq!(interrupted.status.code(), Some(130));
    let interrupted_json: Value = serde_json::from_slice(&interrupted.stdout)
        .unwrap_or_else(|error| panic!("interrupted output is not JSON: {error}"));
    assert_eq!(interrupted_json["code"], "interrupted");

    let after_ctrl_c = joiner.cli.run_capture(&["--json", "join", "status"]);
    assert!(
        after_ctrl_c.success(),
        "status after Ctrl-C failed: {after_ctrl_c:?}"
    );
    let after_ctrl_c = json(&after_ctrl_c);
    assert_eq!(after_ctrl_c["status"], "pending");
    assert_eq!(after_ctrl_c["join_id"], join_id);
    let human_pending = joiner.cli.run_capture(&["join", "status"]);
    assert!(
        human_pending.success(),
        "human pending status failed: {human_pending:?}"
    );
    assert!(
        human_pending.stderr.contains("pending"),
        "{human_pending:?}"
    );

    joiner.restart().await;
    let after_restart = joiner.cli.run_capture(&["--json", "join", "status"]);
    assert!(
        after_restart.success(),
        "status after restart failed: {after_restart:?}"
    );
    let after_restart = json(&after_restart);
    assert_eq!(after_restart["status"], "pending");
    assert_eq!(after_restart["join_id"], join_id);

    let cancel_requests_before = daemon_request_count(&joiner, "POST", "/v2/setup/cancel-join");
    let cancelled = joiner.cli.run_capture(&["--json", "join", "cancel"]);
    assert!(cancelled.success(), "pending cancel failed: {cancelled:?}");
    let cancelled = json(&cancelled);
    assert_eq!(cancelled["ok"], true);
    assert_eq!(cancelled["join_id"], join_id);
    assert_eq!(cancelled["cancel_requested"], true);
    assert_request_delta(
        &joiner,
        "POST",
        "/v2/setup/cancel-join",
        cancel_requests_before,
        1,
    );

    let cancelled_again = joiner.cli.run_capture(&["--json", "join", "cancel"]);
    assert!(
        cancelled_again.success(),
        "repeated pending cancel failed: {cancelled_again:?}"
    );
    let cancelled_again = json(&cancelled_again);
    assert_eq!(cancelled_again["join_id"], join_id);
    assert_eq!(cancelled_again["ok"], true);
    assert_eq!(cancelled_again["status"], "rejected");
    assert_eq!(cancelled_again["reason"], "cancelled");
    assert_request_delta(
        &joiner,
        "POST",
        "/v2/setup/cancel-join",
        cancel_requests_before,
        1,
    );

    drop(session);
}

#[tokio::test]
#[ignore]
async fn invalid_join_passphrase_is_clear_in_json_and_human_output() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let sponsor = Node::initialized(
        "cli-workflow-rejected-sponsor",
        "sponsor-node",
        &binaries,
        &rendezvous,
    )
    .await;
    let joiner = Node::fresh("cli-workflow-rejected-joiner", &binaries, &rendezvous).await;

    let (session, code) = InviteSession::start(&sponsor.cli).await;
    let rejected_requests_before = daemon_request_count(&joiner, "POST", "/v2/setup/redeem");
    let rejected = joiner.cli.run_capture(&[
        "--json",
        "join",
        "--code",
        &code,
        "--passphrase",
        "wrong-passphrase",
        "--device-name",
        "joiner-node",
    ]);
    drop(session);
    assert_eq!(
        rejected.exit_code, 1,
        "invalid passphrase must fail: {rejected:?}"
    );
    let rejected_json = json(&rejected);
    assert_eq!(rejected_json["ok"], false);
    assert_eq!(rejected_json["status"], "rejected");
    assert_eq!(rejected_json["reason"], "authentication_rejected");
    let rejected_join_id = rejected_json["join_id"].as_str().expect("rejected join id");
    assert_request_delta(
        &joiner,
        "POST",
        "/v2/setup/redeem",
        rejected_requests_before,
        1,
    );

    let status = joiner.cli.run_capture(&["--json", "join", "status"]);
    assert!(status.success(), "join status failed: {status:?}");
    let status = json(&status);
    assert_eq!(status["ok"], true);
    assert_eq!(status["status"], "rejected");
    assert_eq!(status["reason"], "authentication_rejected");
    assert_eq!(status["join_id"], rejected_join_id);

    let (human_session, human_code) = InviteSession::start(&sponsor.cli).await;
    let human_requests_before = daemon_request_count(&joiner, "POST", "/v2/setup/redeem");
    let human = joiner.cli.run_capture(&[
        "join",
        "--code",
        &human_code,
        "--passphrase",
        "wrong-passphrase",
        "--device-name",
        "joiner-node",
    ]);
    drop(human_session);
    assert_eq!(
        human.exit_code, 1,
        "human invalid passphrase must fail: {human:?}"
    );
    assert!(
        human.stderr.contains("Join request was rejected"),
        "{human:?}"
    );
    assert!(
        human.stderr.contains("authentication_rejected"),
        "{human:?}"
    );
    assert_request_delta(
        &joiner,
        "POST",
        "/v2/setup/redeem",
        human_requests_before,
        1,
    );
}

#[tokio::test]
#[ignore]
async fn member_sync_cli_reads_partially_updates_and_rereads_engine_state() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let alice = Node::initialized(
        "cli-workflow-sync-alice",
        "alice-node",
        &binaries,
        &rendezvous,
    )
    .await;
    let bob = Node::fresh("cli-workflow-sync-bob", &binaries, &rendezvous).await;
    join(&alice, &bob, "bob-node", false).await;
    let bob_id = device_id(&alice.cli, "bob-node");

    let human_before = alice.cli.run_capture(&["member", "sync", "show", &bob_id]);
    assert!(
        human_before.success(),
        "human sync show failed: {human_before:?}"
    );
    assert!(
        human_before.stderr.contains("Member sync"),
        "{human_before:?}"
    );
    assert!(human_before.stderr.contains(&bob_id), "{human_before:?}");

    let before = alice
        .cli
        .run_capture(&["--json", "member", "sync", "show", &bob_id]);
    assert!(before.success(), "sync show failed: {before:?}");
    let before = json(&before);
    assert_eq!(before["status"], "current");
    let original_send_types = before["send_content_types"].clone();

    let sync_path = format!("/member/{bob_id}/sync-preferences");
    let update_requests_before = daemon_request_count(&alice, "PATCH", &sync_path);
    let updated = alice.cli.run_capture(&[
        "--json",
        "member",
        "sync",
        "set",
        &bob_id,
        "--send",
        "off",
        "--receive-types",
        "text,image",
    ]);
    assert!(updated.success(), "sync set failed: {updated:?}");
    let updated = json(&updated);
    assert_eq!(updated["status"], "updated");
    assert_eq!(updated["device_id"], bob_id);
    assert_eq!(updated["send_enabled"], false);
    assert_eq!(updated["send_content_types"], original_send_types);
    assert_eq!(
        updated["receive_content_types"],
        serde_json::json!(["text", "image"])
    );
    assert_request_delta(&alice, "PATCH", &sync_path, update_requests_before, 1);

    let reread = alice
        .cli
        .run_capture(&["--json", "member", "sync", "show", &bob_id]);
    assert!(reread.success(), "sync reread failed: {reread:?}");
    let reread = json(&reread);
    assert_eq!(reread["send_enabled"], false);
    assert_eq!(
        reread["receive_content_types"],
        serde_json::json!(["text", "image"])
    );

    let human_update_requests_before = daemon_request_count(&alice, "PATCH", &sync_path);
    let human_updated =
        alice
            .cli
            .run_capture(&["member", "sync", "set", &bob_id, "--receive", "off"]);
    assert!(
        human_updated.success(),
        "human sync update failed: {human_updated:?}"
    );
    assert!(
        human_updated
            .stderr
            .contains("Member sync settings updated"),
        "{human_updated:?}"
    );
    assert!(
        human_updated.stderr.contains("receive"),
        "{human_updated:?}"
    );
    assert!(human_updated.stderr.contains("off"), "{human_updated:?}");
    assert_request_delta(&alice, "PATCH", &sync_path, human_update_requests_before, 1);
}

#[tokio::test]
#[ignore]
async fn member_trust_cli_keeps_applies_and_rejects_stale_decisions() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let alice = Node::initialized(
        "cli-workflow-trust-alice",
        "alice-node",
        &binaries,
        &rendezvous,
    )
    .await;
    let bob = Node::fresh("cli-workflow-trust-bob", &binaries, &rendezvous).await;
    join(&alice, &bob, "bob-node", false).await;
    let carol = Node::fresh("cli-workflow-trust-carol", &binaries, &rendezvous).await;
    join(&alice, &carol, "carol-node", false).await;

    let bob_id = device_id(&alice.cli, "bob-node");
    let removal = alice
        .cli
        .run_capture(&["--json", "member", "remove", &bob_id]);
    assert!(removal.success(), "member removal failed: {removal:?}");

    let bob_change = wait_for_trust_change(&bob).await;
    assert_eq!(bob_change["issues"].as_array().unwrap().len(), 1);
    let bob_issue = &bob_change["issues"][0];
    let bob_change_id = bob_issue["issueId"]
        .as_str()
        .expect("bob change id")
        .to_string();
    let bob_choice = bob_issue["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|choice| choice["isCurrentGroup"] == false)
        .expect("removal choice");
    let bob_choice_id = bob_choice["choiceId"].as_str().unwrap();
    assert_eq!(bob_choice["impact"]["localDeviceOutcome"], "removed");

    let carol_change = wait_for_trust_change(&carol).await;
    assert_eq!(carol_change["issues"].as_array().unwrap().len(), 1);
    let carol_issue = &carol_change["issues"][0];
    let carol_change_id = carol_issue["issueId"]
        .as_str()
        .expect("carol change id")
        .to_string();
    assert_eq!(carol_change_id, bob_change_id);
    let carol_choice = carol_issue["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|choice| choice["isCurrentGroup"] == true)
        .expect("current group choice");
    let carol_choice_id = carol_choice["choiceId"].as_str().unwrap();

    let human_status = carol.cli.run_capture(&["member", "trust", "status"]);
    assert!(
        human_status.success(),
        "human trust status failed: {human_status:?}"
    );
    assert!(
        human_status.stderr.contains("Device groups"),
        "{human_status:?}"
    );
    assert!(
        human_status.stderr.contains(&carol_change_id),
        "{human_status:?}"
    );

    let decision_path = "/member/device-group-choices";
    let decision_requests_before = daemon_request_count(&carol, "POST", decision_path);
    let human_stale = carol.cli.run_capture(&[
        "member",
        "trust",
        "choose",
        "--issue",
        "stale-change-id",
        "--choice",
        carol_choice_id,
    ]);
    assert_eq!(
        human_stale.exit_code, 1,
        "human stale decision must fail: {human_stale:?}"
    );
    assert!(
        human_stale.stderr.contains("no longer current"),
        "{human_stale:?}"
    );
    assert_request_delta(&carol, "POST", decision_path, decision_requests_before, 0);

    let stale = carol.cli.run_capture(&[
        "--json",
        "member",
        "trust",
        "choose",
        "--issue",
        "stale-change-id",
        "--choice",
        carol_choice_id,
    ]);
    assert_eq!(stale.exit_code, 1, "stale decision must fail: {stale:?}");
    let stale = json(&stale);
    assert_eq!(stale["code"], "device_group_state_changed");
    assert_request_delta(&carol, "POST", decision_path, decision_requests_before, 0);

    let kept = confirm_device_group(&carol, carol_issue, carol_choice, false).await;
    assert_eq!(kept["ok"], true);
    assert_eq!(kept["result"]["outcome"], "completed");
    assert_eq!(kept["state"]["deviceTrust"]["localMembership"], "active");
    assert!(kept["state"]["deviceTrust"]["currentChange"].is_null());

    let apply_requests_before = daemon_request_count(&bob, "POST", decision_path);
    let missing_confirmation = bob.cli.run_capture(&[
        "--json",
        "member",
        "trust",
        "choose",
        "--issue",
        &bob_change_id,
        "--choice",
        bob_choice_id,
    ]);
    assert_eq!(
        missing_confirmation.exit_code, 1,
        "local removal without confirmation must fail: {missing_confirmation:?}"
    );
    assert_eq!(
        json(&missing_confirmation)["code"],
        "local_removal_confirmation_required"
    );
    assert_request_delta(&bob, "POST", decision_path, apply_requests_before, 0);

    let applied = confirm_device_group(&bob, bob_issue, bob_choice, true).await;
    assert_eq!(applied["ok"], true);
    assert_eq!(applied["result"]["outcome"], "completed");
    assert_eq!(
        applied["state"]["deviceTrust"]["localMembership"],
        "removed"
    );
    assert!(applied["state"]["deviceTrust"]["currentChange"].is_null());
}

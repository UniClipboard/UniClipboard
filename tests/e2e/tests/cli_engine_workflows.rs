use std::time::Duration;

#[cfg(unix)]
use std::process::{Child, Command, Stdio};

use serde_json::Value;
use uc_e2e_tests::{
    InviteSession, LocalRendezvous, NodeBinarySet, TestCli, TestDaemon, TestProfile,
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
                command.env("RUST_LOG", "uc_webserver::api::server=info");
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
            "space",
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
                command.env("RUST_LOG", "uc_webserver::api::server=info");
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
    let requests_before = daemon_request_count(joiner, "POST", "/v2/setup/redeem");
    let mut args = vec![
        "--json",
        "space",
        "join",
        "--code",
        &code,
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        device_name,
    ];
    if no_wait {
        args.push("--no-wait");
    }
    let output = joiner.cli.run_capture(&args);
    session.finish().await;
    assert!(
        output.success(),
        "join failed: stdout={} stderr={} sponsor_log={} joiner_log={}",
        output.stdout,
        output.stderr,
        sponsor.daemon.diagnostic_log(),
        joiner.daemon.diagnostic_log()
    );
    assert_request_delta(joiner, "POST", "/v2/setup/redeem", requests_before, 1);
    json(&output)
}

fn members(cli: &TestCli) -> Vec<Value> {
    let output = cli.run_capture(&["--json", "members"]);
    assert!(output.success(), "members failed: {output:?}");
    json(&output)
        .as_array()
        .expect("members must be an array")
        .clone()
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
            if value["status"] == "pending_change" && value["current_change"].is_object() {
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

    let status = joiner
        .cli
        .run_capture(&["--json", "space", "join", "status"]);
    assert!(status.success(), "join status failed: {status:?}");
    assert_eq!(json(&status)["status"], "none");

    let human_status = joiner.cli.run_capture(&["space", "join", "status"]);
    assert!(
        human_status.success(),
        "human join status failed: {human_status:?}"
    );
    assert!(human_status.stderr.contains("none"), "{human_status:?}");

    let cancel = joiner
        .cli
        .run_capture(&["--json", "space", "join", "cancel"]);
    assert!(cancel.success(), "empty join cancel failed: {cancel:?}");
    assert_eq!(json(&cancel)["status"], "none");

    let joined = join(&sponsor, &joiner, "joiner-node", true).await;
    assert_eq!(joined["ok"], true);
    assert_eq!(joined["status"], "active");
    let join_id = joined["join_id"].as_str().expect("active join id");

    let status = joiner
        .cli
        .run_capture(&["--json", "space", "join", "status"]);
    assert!(status.success(), "active join status failed: {status:?}");
    let status = json(&status);
    assert_eq!(status["status"], "active");
    assert_eq!(status["join_id"], join_id);

    let cancel_requests_before = daemon_request_count(&joiner, "POST", "/v2/setup/cancel-join");
    let cancel = joiner
        .cli
        .run_capture(&["--json", "space", "join", "cancel"]);
    assert!(cancel.success(), "active join cancel failed: {cancel:?}");
    let cancel = json(&cancel);
    assert_eq!(cancel["status"], "active");
    assert_eq!(cancel["join_id"], join_id);
    assert_request_delta(
        &joiner,
        "POST",
        "/v2/setup/cancel-join",
        cancel_requests_before,
        0,
    );

    let human_status = joiner.cli.run_capture(&["space", "join", "status"]);
    assert!(
        human_status.success(),
        "human active join status failed: {human_status:?}"
    );
    assert!(
        human_status.stderr.contains("active") || human_status.stderr.contains("Join completed")
    );

    joiner.restart().await;
    let restarted_status = joiner
        .cli
        .run_capture(&["--json", "space", "join", "status"]);
    assert!(
        restarted_status.success(),
        "join status after daemon restart failed: {restarted_status:?}"
    );
    let restarted_status = json(&restarted_status);
    assert_eq!(restarted_status["status"], "active");
    assert_eq!(restarted_status["join_id"], join_id);
}

#[tokio::test]
#[ignore]
async fn reset_space_keeps_local_history_and_permanently_discards_old_device_relationships() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let mut alice = Node::initialized(
        "cli-workflow-reset-alice",
        "alice-node",
        &binaries,
        &rendezvous,
    )
    .await;
    let bob = Node::fresh("cli-workflow-reset-bob", &binaries, &rendezvous).await;
    let joined = join(&alice, &bob, "bob-node", false).await;
    assert_eq!(joined["status"], "active");

    let sent = alice.cli.run_capture(&["send", "reset-preserved-text"]);
    assert!(sent.success(), "send before reset failed: {sent:?}");
    assert_eq!(members(&alice.cli).len(), 2);

    let requests_before = daemon_request_count(&alice, "POST", "/v2/setup/reset");
    let reset = alice
        .cli
        .run_capture(&["--json", "space", "reset", "--yes"]);
    assert!(reset.success(), "reset failed: {reset:?}");
    assert_eq!(
        json(&reset),
        serde_json::json!({ "ok": true, "status": "reset" })
    );
    assert_request_delta(&alice, "POST", "/v2/setup/reset", requests_before, 1);

    let after_reset = members(&alice.cli);
    assert_eq!(
        after_reset.len(),
        1,
        "old members survived reset: {after_reset:?}"
    );
    assert_eq!(after_reset[0]["device_name"], "alice-node");

    let history = alice
        .cli
        .run_capture(&["--json", "search", "reset-preserved-text"]);
    assert!(
        history.success(),
        "history search after reset failed: {history:?}"
    );
    assert!(
        json(&history)["data"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty()),
        "local history was not retained: {history:?}"
    );

    alice.restart().await;
    assert_eq!(
        members(&alice.cli).len(),
        1,
        "old members returned after restart"
    );
}

#[tokio::test]
#[ignore]
async fn legacy_space_commands_warn_and_keep_json_output_parseable() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let node = Node::fresh("cli-workflow-legacy-space", &binaries, &rendezvous).await;

    let invite = node.cli.run_capture(&["invite"]);
    assert!(
        !invite.success(),
        "invite unexpectedly succeeded without setup"
    );
    assert!(invite.stderr.contains("`uniclip invite` is deprecated"));

    let init = node.cli.run_capture(&[
        "init",
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "legacy-node",
    ]);
    assert!(init.success(), "legacy init failed: {init:?}");
    assert!(init.stderr.contains("`uniclip init` is deprecated"));

    let status = node.cli.run_capture(&["--json", "status"]);
    assert!(status.success(), "legacy status failed: {status:?}");
    let _ = json(&status);
    assert!(status.stderr.contains("`uniclip status` is deprecated"));

    let join = node.cli.run_capture(&["--json", "join", "status"]);
    assert!(join.success(), "legacy join status failed: {join:?}");
    let _ = json(&join);
    assert!(join.stderr.contains("`uniclip join` is deprecated"));

    let reset = node.cli.run_capture(&["--json", "reset-space", "--yes"]);
    assert!(reset.success(), "legacy reset failed: {reset:?}");
    assert_eq!(
        json(&reset),
        serde_json::json!({ "ok": true, "status": "reset" })
    );
    assert!(reset
        .stderr
        .contains("`uniclip reset-space --yes` is deprecated"));
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
        "space",
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

    let child = Command::new(joiner.cli.binary_path())
        .env("UC_PROFILE", &joiner.cli.profile_name)
        .env("UNICLIPBOARD_ENV", "development")
        .args([
            "--json",
            "space",
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

    let candidate_deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        if joiner
            .daemon
            .diagnostic_log()
            .contains("\"DurableAdmission\"")
        {
            sponsor.daemon.suspend().expect("suspend sponsor daemon");
            break;
        }
        assert!(
            waiting_join
                .child_mut()
                .try_wait()
                .expect("join child state")
                .is_none(),
            "join completed before sponsor suspension"
        );
        assert!(
            tokio::time::Instant::now() < candidate_deadline,
            "durable admission message did not arrive; sponsor_log={} joiner_log={}",
            sponsor.daemon.diagnostic_log(),
            joiner.daemon.diagnostic_log(),
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    let pending_deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let pending = loop {
        let status = joiner
            .cli
            .run_capture(&["--json", "space", "join", "status"]);
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
    let interrupted = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::task::spawn_blocking(move || waiting_join.take().wait_with_output()),
    )
    .await
    .expect("waiting join did not stop after Ctrl-C")
    .expect("join wait task")
    .expect("join wait output");
    assert_eq!(interrupted.status.code(), Some(130));
    let interrupted_json: Value = serde_json::from_slice(&interrupted.stdout)
        .unwrap_or_else(|error| panic!("interrupted output is not JSON: {error}"));
    assert_eq!(interrupted_json["code"], "interrupted");

    let after_ctrl_c = joiner
        .cli
        .run_capture(&["--json", "space", "join", "status"]);
    assert!(
        after_ctrl_c.success(),
        "status after Ctrl-C failed: {after_ctrl_c:?}"
    );
    let after_ctrl_c = json(&after_ctrl_c);
    assert_eq!(after_ctrl_c["status"], "pending");
    assert_eq!(after_ctrl_c["join_id"], join_id);
    let human_pending = joiner.cli.run_capture(&["space", "join", "status"]);
    assert!(
        human_pending.success(),
        "human pending status failed: {human_pending:?}"
    );
    assert!(
        human_pending.stderr.contains("pending"),
        "{human_pending:?}"
    );

    joiner.restart().await;
    let after_restart = joiner
        .cli
        .run_capture(&["--json", "space", "join", "status"]);
    assert!(
        after_restart.success(),
        "status after restart failed: {after_restart:?}"
    );
    let after_restart = json(&after_restart);
    assert_eq!(after_restart["status"], "pending");
    assert_eq!(after_restart["join_id"], join_id);

    let cancel_requests_before = daemon_request_count(&joiner, "POST", "/v2/setup/cancel-join");
    let cancelled = joiner
        .cli
        .run_capture(&["--json", "space", "join", "cancel"]);
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

    let cancelled_again = joiner
        .cli
        .run_capture(&["--json", "space", "join", "cancel"]);
    assert!(
        cancelled_again.success(),
        "repeated pending cancel failed: {cancelled_again:?}"
    );
    let cancelled_again = json(&cancelled_again);
    assert_eq!(cancelled_again["join_id"], join_id);
    assert_eq!(cancelled_again["cancel_requested"], true);
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
        "space",
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
    assert!(rejected_json["code"].is_string(), "{rejected_json}");
    assert!(rejected_json["message"].is_string(), "{rejected_json}");
    assert_request_delta(
        &joiner,
        "POST",
        "/v2/setup/redeem",
        rejected_requests_before,
        1,
    );

    let status = joiner
        .cli
        .run_capture(&["--json", "space", "join", "status"]);
    assert!(status.success(), "join status failed: {status:?}");
    let status = json(&status);
    assert_eq!(status["ok"], true);
    assert_eq!(status["status"], "none");

    let (human_session, human_code) = InviteSession::start(&sponsor.cli).await;
    let human_requests_before = daemon_request_count(&joiner, "POST", "/v2/setup/redeem");
    let human = joiner.cli.run_capture(&[
        "space",
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
    assert!(human.stderr.contains("Join failed"), "{human:?}");
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
    let bob_change_id = bob_change["current_change"]["change_id"]
        .as_str()
        .expect("bob change id")
        .to_string();
    assert_eq!(
        bob_change["current_change"]["apply_impact"]["local_device_outcome"],
        "removed"
    );

    let carol_change = wait_for_trust_change(&carol).await;
    let carol_change_id = carol_change["current_change"]["change_id"]
        .as_str()
        .expect("carol change id")
        .to_string();
    assert_eq!(carol_change_id, bob_change_id);

    let human_status = carol.cli.run_capture(&["member", "trust", "status"]);
    assert!(
        human_status.success(),
        "human trust status failed: {human_status:?}"
    );
    assert!(
        human_status.stderr.contains("Device trust"),
        "{human_status:?}"
    );
    assert!(
        human_status.stderr.contains(&carol_change_id),
        "{human_status:?}"
    );

    let decision_path = "/member/device-trust/decision";
    let decision_requests_before = daemon_request_count(&carol, "POST", decision_path);
    let human_stale =
        carol
            .cli
            .run_capture(&["member", "trust", "keep", "--change", "stale-change-id"]);
    assert_eq!(
        human_stale.exit_code, 1,
        "human stale decision must fail: {human_stale:?}"
    );
    assert!(human_stale.stderr.contains("changed"), "{human_stale:?}");
    assert_request_delta(&carol, "POST", decision_path, decision_requests_before, 0);

    let stale = carol.cli.run_capture(&[
        "--json",
        "member",
        "trust",
        "keep",
        "--change",
        "stale-change-id",
    ]);
    assert_eq!(stale.exit_code, 1, "stale decision must fail: {stale:?}");
    let stale = json(&stale);
    assert_eq!(stale["code"], "device_trust_state_changed");
    assert_eq!(stale["current_change_id"], carol_change_id);
    assert_request_delta(&carol, "POST", decision_path, decision_requests_before, 0);

    let keep_requests_before = daemon_request_count(&carol, "POST", decision_path);
    let kept = carol.cli.run_capture(&[
        "--json",
        "member",
        "trust",
        "keep",
        "--change",
        &carol_change_id,
    ]);
    assert!(kept.success(), "keep decision failed: {kept:?}");
    assert_eq!(json(&kept)["status"], "kept_current_device_group");
    assert_request_delta(&carol, "POST", decision_path, keep_requests_before, 1);

    let apply_requests_before = daemon_request_count(&bob, "POST", decision_path);
    let missing_confirmation = bob.cli.run_capture(&[
        "--json",
        "member",
        "trust",
        "apply",
        "--change",
        &bob_change_id,
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

    let applied = bob.cli.run_capture(&[
        "--json",
        "member",
        "trust",
        "apply",
        "--change",
        &bob_change_id,
        "--confirm-local-removal",
    ]);
    assert!(applied.success(), "apply decision failed: {applied:?}");
    assert_eq!(json(&applied)["status"], "applied");
    assert_request_delta(&bob, "POST", decision_path, apply_requests_before, 1);
}

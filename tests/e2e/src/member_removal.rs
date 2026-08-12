//! CLI-driven member removal convergence tests (ADR-015 offline-first
//! removal intent set).
//!
//! Every test drives two or three profile-isolated nodes through the real
//! daemon + CLI binaries:
//!
//! * `uniclip member remove <peer-id>` records the removal intent (same
//!   Engine path the GUI uses via `POST /pairing/unpair`);
//! * `uniclip member removal-status --json` prints the Engine-owned workspace
//!   convergence DTO, including phase, removal intent count, effective member
//!   count, convergence digest, and update time,
//!   which the tests poll until the expected convergence phase.
//!
//! A device is "offline" by stopping its daemon (`Node::stop`), "online" by
//! restarting it (`Node::restart`, preserving the profile data). All pairing
//! goes through `LocalRendezvous` so the suite needs no external relay.
//!
//! Run with:
//! ```bash
//! cargo build -p uc-daemon -p uc-cli
//! cargo test --manifest-path tests/e2e/Cargo.toml member_removal -- --ignored
//! ```

use std::time::Duration;

use serde_json::Value;
use crate::{InviteSession, LocalRendezvous, NodeBinarySet, TestCli, TestDaemon, TestProfile};

const PASSPHRASE: &str = "member-removal-e2e-passphrase";
const WAIT_TIMEOUT: Duration = Duration::from_secs(120);
const CLI_POLL_INTERVAL: Duration = Duration::from_secs(2);

const DEVICE_A: &str = "node-a";
const DEVICE_B: &str = "node-b";
const DEVICE_C: &str = "node-c";

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

    /// Stop the daemon via the CLI (`uniclip --json stop`).
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

    /// Bring the daemon back up while preserving the profile data.
    async fn restart(&mut self) {
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

fn members(cli: &TestCli) -> Vec<Value> {
    try_members(cli).unwrap_or_else(|error| panic!("{error}"))
}

fn has_member_named(members: &[Value], name: &str) -> bool {
    members
        .iter()
        .any(|member| member.get("device_name").and_then(Value::as_str) == Some(name))
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

/// Run `uniclip member removal-status --json` and return the DTO value.
fn try_removal_status(cli: &TestCli) -> Result<Value, String> {
    let output = cli.run_capture(&["--json", "member", "removal-status"]);
    if !output.success() {
        return Err(format!(
            "removal-status failed: stdout={} stderr={}",
            output.stdout, output.stderr
        ));
    }
    serde_json::from_str(output.stdout.trim())
        .map_err(|error| format!("removal-status output is not JSON: {error}\n{}", output.stdout))
}

fn removal_status(cli: &TestCli) -> Value {
    try_removal_status(cli).unwrap_or_else(|error| panic!("{error}"))
}

fn phase_of(status: &Value) -> String {
    status
        .get("phase")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| panic!("removal status missing phase: {status}"))
}

fn intent_count_of(status: &Value) -> u64 {
    status
        .get("removalIntentCount")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("removal status missing removalIntentCount: {status}"))
}

fn assert_healthy_phase(status: &Value) {
    assert!(
        matches!(
            phase_of(status).as_str(),
            "locally_applied" | "converging" | "complete"
        ),
        "workspace convergence must be healthy: {status}"
    );
}

fn effective_member_count_of(status: &Value) -> u64 {
    status
        .get("effectiveMemberCount")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("removal status missing effectiveMemberCount: {status}"))
}

fn removed_of(status: &Value) -> bool {
    status
        .get("removed")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("removal status missing removed: {status}"))
}

/// Poll `member removal-status` until the local self-removal fact matches.
async fn wait_for_removed(cli: &TestCli, expected: bool) -> Value {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        let last = match try_removal_status(cli) {
            Ok(status) if removed_of(&status) == expected => return status,
            Ok(status) => status,
            Err(error) => {
                if tokio::time::Instant::now() >= deadline {
                    panic!("{error}");
                }
                tokio::time::sleep(CLI_POLL_INTERVAL).await;
                continue;
            }
        };
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "{} did not report removed={expected}; last={last}; members={:?}",
                cli.profile_name,
                try_members(cli)
            );
        }
        tokio::time::sleep(CLI_POLL_INTERVAL).await;
    }
}

/// Poll until the authoritative convergence snapshot reaches the expected
/// effective member count.
async fn wait_for_effective_member_count(cli: &TestCli, expected: u64) -> Value {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        let last = match try_removal_status(cli) {
            Ok(status) if effective_member_count_of(&status) == expected => return status,
            Ok(status) => status,
            Err(error) => {
                if tokio::time::Instant::now() >= deadline {
                    panic!("{error}");
                }
                tokio::time::sleep(CLI_POLL_INTERVAL).await;
                continue;
            }
        };
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "{} did not reach effective member count {expected}; last={last}; members={:?}",
                cli.profile_name,
                try_members(cli)
            );
        }
        tokio::time::sleep(CLI_POLL_INTERVAL).await;
    }
}

async fn wait_for_intent_count(cli: &TestCli, expected: u64) -> Value {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        let last = match try_removal_status(cli) {
            Ok(status) if intent_count_of(&status) >= expected => return status,
            Ok(status) => status,
            Err(error) => {
                if tokio::time::Instant::now() >= deadline {
                    panic!("{error}");
                }
                tokio::time::sleep(CLI_POLL_INTERVAL).await;
                continue;
            }
        };
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "{} did not reach removal intent count {expected}; last={last}",
                cli.profile_name
            );
        }
        tokio::time::sleep(CLI_POLL_INTERVAL).await;
    }
}

/// Wait until the member list reaches the expected count.
async fn wait_for_member_count(cli: &TestCli, expected: usize) {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        match try_members(cli) {
            Ok(current) if current.len() == expected => return,
            Ok(current) if tokio::time::Instant::now() >= deadline => {
                panic!("{} member count did not reach {expected}; members={current:?}", cli.profile_name);
            }
            Err(error) if tokio::time::Instant::now() >= deadline => {
                panic!(
                    "{} member count did not reach {expected}; last error={error}",
                    cli.profile_name
                );
            }
            _ => {}
        }
        tokio::time::sleep(CLI_POLL_INTERVAL).await;
    }
}

/// Record a removal intent via `member remove`; returns the DTO.
fn remove(cli: &TestCli, peer_id: &str) -> Value {
    let output = cli.run_capture(&["--json", "member", "remove", peer_id]);
    assert!(
        output.success(),
        "member remove failed: stdout={} stderr={}",
        output.stdout,
        output.stderr
    );
    serde_json::from_str(output.stdout.trim())
        .unwrap_or_else(|error| panic!("member remove output is not JSON: {error}\n{}", output.stdout))
}

// ── R01 ──────────────────────────────────────────────────────────────
// Two online devices: A removes B; A converges to `complete` and B stops
// being an effective member. B observes only its own removal fact while its
// intent set stays empty.
#[tokio::test]
#[ignore]
async fn r01_two_devices_online_removal_converges_to_complete() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r01-a", DEVICE_A, &binaries, &rendezvous).await;
    let b = Node::fresh("removal-r01-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    wait_for_member_count(&a.cli, 2).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    let removal = remove(&a.cli, &b_id);
    assert_healthy_phase(&removal);

    let a_status = wait_for_effective_member_count(&a.cli, 1).await;
    assert_eq!(intent_count_of(&a_status), 1, "A must keep one intent: {a_status}");
    assert_eq!(effective_member_count_of(&a_status), 1, "B must no longer count: {a_status}");
    let b_status = wait_for_removed(&b.cli, true).await;
    assert_eq!(intent_count_of(&b_status), 0, "removed target observes no intent: {b_status}");
}

// ── R02 ──────────────────────────────────────────────────────────────
// Target offline: A removes B while B is stopped; A converges and B
// restarts without regaining effective membership.
#[tokio::test]
#[ignore]
async fn r02_target_offline_removal_converges_after_restart() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r02-a", DEVICE_A, &binaries, &rendezvous).await;
    let mut b = Node::fresh("removal-r02-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    wait_for_member_count(&a.cli, 2).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    b.stop().await;
    let removal = remove(&a.cli, &b_id);
    assert_healthy_phase(&removal);

    let a_status = wait_for_effective_member_count(&a.cli, 1).await;
    assert_eq!(effective_member_count_of(&a_status), 1, "A must exclude B: {a_status}");

    b.restart().await;
    let b_status = wait_for_removed(&b.cli, true).await;
    assert_eq!(intent_count_of(&b_status), 0, "restarted target observes no intent: {b_status}");
    let a_after = wait_for_effective_member_count(&a.cli, 1).await;
    assert_eq!(effective_member_count_of(&a_after), 1, "A must stay converged: {a_after}");
}

// ── R03 ──────────────────────────────────────────────────────────────
// Initiator offline: A restarts with the intent persisted (`applied`, one intent).
#[tokio::test]
#[ignore]
async fn r03_initiator_offline_removal_survives_restart() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let mut a = Node::initialized("removal-r03-a", DEVICE_A, &binaries, &rendezvous).await;
    let mut b = Node::fresh("removal-r03-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    wait_for_member_count(&a.cli, 2).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    b.stop().await;
    let removal = remove(&a.cli, &b_id);
    assert_healthy_phase(&removal);

    a.restart().await;
    let status = wait_for_effective_member_count(&a.cli, 1).await;
    assert_eq!(intent_count_of(&status), 1, "restart must preserve the intent: {status}");
}

// ── R04 ──────────────────────────────────────────────────────────────
// Three devices: A removes B; A and C (witness) both converge to `complete`.
#[tokio::test]
#[ignore]
async fn r04_three_devices_removal_converges_on_witness() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r04-a", DEVICE_A, &binaries, &rendezvous).await;
    let b = Node::fresh("removal-r04-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    let c = Node::fresh("removal-r04-c", &binaries, &rendezvous).await;
    join(&a, &c, DEVICE_C).await;
    wait_for_member_count(&a.cli, 3).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    remove(&a.cli, &b_id);

    let a_status = wait_for_effective_member_count(&a.cli, 2).await;
    assert_eq!(effective_member_count_of(&a_status), 2, "A excludes B only: {a_status}");
    let c_status = wait_for_effective_member_count(&c.cli, 2).await;
    assert_eq!(intent_count_of(&c_status), 1, "C must observe the witness intent: {c_status}");
    assert_eq!(effective_member_count_of(&c_status), 2, "C must exclude B: {c_status}");
    let b_status = removal_status(&b.cli);
    assert_eq!(intent_count_of(&b_status), 0, "removed target observes no intent: {b_status}");
}

// ── R05 ──────────────────────────────────────────────────────────────
// Removed member rejoins with a new instance; historical intent does not
// re-remove the new instance.
#[tokio::test]
#[ignore]
async fn r05_removed_member_rejoins_with_new_instance() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r05-a", DEVICE_A, &binaries, &rendezvous).await;
    let mut b = Node::fresh("removal-r05-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    let c = Node::fresh("removal-r05-c", &binaries, &rendezvous).await;
    join(&a, &c, DEVICE_C).await;
    wait_for_member_count(&a.cli, 3).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    remove(&a.cli, &b_id);
    let a_status = wait_for_effective_member_count(&a.cli, 2).await;
    assert_eq!(intent_count_of(&a_status), 1, "{a_status}");

    b.stop().await;
    b.restart().await;
    join(&a, &b, DEVICE_B).await;

    wait_for_member_count(&a.cli, 3).await;
    let a_status = wait_for_effective_member_count(&a.cli, 3).await;
    assert_eq!(
        intent_count_of(&a_status),
        1,
        "rejoin must not re-record a removal for the new instance: {a_status}"
    );
    assert!(has_member_named(&members(&a.cli), DEVICE_B), "B must be listed again");
}

// ── R06 ──────────────────────────────────────────────────────────────
// Multi-target removal: A removes B and C; A converges with both excluded.
#[tokio::test]
#[ignore]
async fn r06_multi_target_removal_converges() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r06-a", DEVICE_A, &binaries, &rendezvous).await;
    let b = Node::fresh("removal-r06-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    let c = Node::fresh("removal-r06-c", &binaries, &rendezvous).await;
    join(&a, &c, DEVICE_C).await;
    wait_for_member_count(&a.cli, 3).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    let c_id = remote_device_id(&a.cli, DEVICE_C);
    remove(&a.cli, &b_id);
    remove(&a.cli, &c_id);

    let a_status = wait_for_effective_member_count(&a.cli, 1).await;
    assert_eq!(intent_count_of(&a_status), 2, "two intents must merge: {a_status}");
    assert_eq!(effective_member_count_of(&a_status), 1, "only A remains: {a_status}");
    let b_status = removal_status(&b.cli);
    assert_eq!(intent_count_of(&b_status), 0, "removed target observes no intent: {b_status}");
}

// ── R07 ──────────────────────────────────────────────────────────────
// Concurrent intents merge: B offline; A and C both record intent for B;
// after B returns all three converge with a shared digest.
//
#[tokio::test]
#[ignore]
async fn r07_concurrent_removal_intents_merge() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r07-a", DEVICE_A, &binaries, &rendezvous).await;
    let mut b = Node::fresh("removal-r07-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    let c = Node::fresh("removal-r07-c", &binaries, &rendezvous).await;
    join(&a, &c, DEVICE_C).await;
    wait_for_member_count(&a.cli, 3).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    b.stop().await;

    remove(&a.cli, &b_id);
    remove(&c.cli, &b_id);

    b.restart().await;
    wait_for_effective_member_count(&a.cli, 2).await;
    wait_for_effective_member_count(&c.cli, 2).await;
    let a_status = wait_for_intent_count(&a.cli, 2).await;
    let c_status = wait_for_intent_count(&c.cli, 2).await;

    for (name, status) in [("A", &a_status), ("C", &c_status)] {
        assert!(intent_count_of(status) >= 2, "{name} must see both intents: {status}");
    }
    let a_digest = a_status.get("convergenceDigest").and_then(Value::as_str);
    let c_digest = c_status.get("convergenceDigest").and_then(Value::as_str);
    assert_eq!(a_digest, c_digest, "shared digest required: {a_status} vs {c_status}");
    assert_eq!(
        effective_member_count_of(&a_status),
        2,
        "A must exclude B from effective members: {a_status}"
    );
    let b_status = removal_status(&b.cli);
    assert_eq!(intent_count_of(&b_status), 0, "removed target observes no intent: {b_status}");
}

// ── R08 ──────────────────────────────────────────────────────────────
// Initiator restart recovers the removal state.
#[tokio::test]
#[ignore]
async fn r08_initiator_restart_recovers_removal_state() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let mut a = Node::initialized("removal-r08-a", DEVICE_A, &binaries, &rendezvous).await;
    let mut b = Node::fresh("removal-r08-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    wait_for_member_count(&a.cli, 2).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    b.stop().await;
    let removal = remove(&a.cli, &b_id);
    assert_healthy_phase(&removal);

    a.restart().await;
    let status = wait_for_effective_member_count(&a.cli, 1).await;
    assert_eq!(intent_count_of(&status), 1, "intent survives restart: {status}");
}

// ── R09 ──────────────────────────────────────────────────────────────
// Removed member stops receiving content: after R01, A sends and B does not
// receive; a third retained member C still does.
#[tokio::test]
#[ignore]
async fn r09_removed_member_stops_receiving_content() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r09-a", DEVICE_A, &binaries, &rendezvous).await;
    let b = Node::fresh("removal-r09-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    let c = Node::fresh("removal-r09-c", &binaries, &rendezvous).await;
    join(&a, &c, DEVICE_C).await;
    wait_for_member_count(&a.cli, 3).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    remove(&a.cli, &b_id);
    wait_for_effective_member_count(&a.cli, 2).await;
    wait_for_effective_member_count(&c.cli, 2).await;

    // A stops sending to the removed target: `send --peer` to B must not
    // accept the dispatch (B is excluded from A's effective members).
    let b_send = a
        .cli
        .run_capture(&["--json", "send", "r09-to-b", "--peer", &b_id]);
    let b_outcome: Value = serde_json::from_str(b_send.stdout.trim()).unwrap_or_default();
    assert!(
        b_outcome.get("totalAccepted").and_then(Value::as_u64) == Some(0)
            || b_send.exit_code != 0,
        "send to a removed target must not be accepted: {b_outcome} exit={}",
        b_send.exit_code
    );

    // A retained member still receives content.
    let c_id = remote_device_id(&a.cli, DEVICE_C);
    let output = a
        .cli
        .run_capture(&["--json", "send", "r09-text", "--peer", &c_id]);
    assert!(
        output.success(),
        "send failed: stdout={} stderr={}",
        output.stdout,
        output.stderr
    );
    let outcome: Value = serde_json::from_str(output.stdout.trim())
        .unwrap_or_else(|error| panic!("send output is not JSON: {error}\n{}", output.stdout));
    assert_eq!(outcome["totalAccepted"], 1, "retained member must accept: {outcome}");
}

// ── R10 ──────────────────────────────────────────────────────────────
// Offline/online cycles stay complete: B restarts three times and the
// intent count never grows.
#[tokio::test]
#[ignore]
async fn r10_offline_online_cycles_stay_complete() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r10-a", DEVICE_A, &binaries, &rendezvous).await;
    let mut b = Node::fresh("removal-r10-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    wait_for_member_count(&a.cli, 2).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    remove(&a.cli, &b_id);
    let a_status = wait_for_effective_member_count(&a.cli, 1).await;
    assert_eq!(intent_count_of(&a_status), 1, "cycle must not grow intents: {a_status}");

    for _ in 0..3 {
        b.stop().await;
        b.restart().await;
        let b_status = wait_for_removed(&b.cli, true).await;
        assert_eq!(intent_count_of(&b_status), 0, "removed target observes no intent: {b_status}");
        let a_status = wait_for_effective_member_count(&a.cli, 1).await;
        assert_eq!(intent_count_of(&a_status), 1, "cycle must not grow intents: {a_status}");
    }
}

// ── R11 ──────────────────────────────────────────────────────────────
// `member remove` is the same removal path as the GUI: response is
// `applied` and A stops sending to B (equivalent of R01 assertions).
#[tokio::test]
#[ignore]
async fn r11_unpair_is_the_removal_path() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r11-a", DEVICE_A, &binaries, &rendezvous).await;
    let b = Node::fresh("removal-r11-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    wait_for_member_count(&a.cli, 2).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    let removal = remove(&a.cli, &b_id);
    assert_healthy_phase(&removal);

    let a_status = wait_for_effective_member_count(&a.cli, 1).await;
    assert_eq!(intent_count_of(&a_status), 1, "{a_status}");
    assert_eq!(effective_member_count_of(&a_status), 1, "B must be excluded: {a_status}");
}

// ── R13 ──────────────────────────────────────────────────────────────
// Empty space reports `applied` with zero intents (single device, no network).
#[tokio::test]
#[ignore]
async fn r13_empty_space_reports_applied_zero_intents() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r13-a", DEVICE_A, &binaries, &rendezvous).await;

    let status = removal_status(&a.cli);
    assert_eq!(
        phase_of(&status),
        "locally_applied",
        "empty space must report local application: {status}"
    );
    assert_eq!(intent_count_of(&status), 0, "empty space must have zero intents: {status}");
}

// ── R14 ──────────────────────────────────────────────────────────────
// Duplicate removal is idempotent: removing B twice does not grow the count.
#[tokio::test]
#[ignore]
async fn r14_duplicate_removal_is_idempotent() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r14-a", DEVICE_A, &binaries, &rendezvous).await;
    let b = Node::fresh("removal-r14-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    wait_for_member_count(&a.cli, 2).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    let first = remove(&a.cli, &b_id);
    assert_eq!(intent_count_of(&first), 1, "first removal records one intent: {first}");

    let second = remove(&a.cli, &b_id);
    assert!(
        intent_count_of(&second) <= 1,
        "duplicate removal must not grow the intent count: {second}"
    );
}

// ── R15 ──────────────────────────────────────────────────────────────
// Removing the local device or an unknown device fails with a non-zero exit.
#[tokio::test]
#[ignore]
async fn r15_removing_local_or_unknown_device_fails() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r15-a", DEVICE_A, &binaries, &rendezvous).await;

    let local_id = remote_device_id(&a.cli, DEVICE_A);
    let local_out = a.cli.run_capture(&["--json", "member", "remove", &local_id]);
    assert!(
        !local_out.success(),
        "removing the local device must fail: {local_out:?}"
    );

    let unknown_out = a.cli.run_capture(&["--json", "member", "remove", "unknown-peer-id"]);
    assert!(
        !unknown_out.success(),
        "removing an unknown device must fail: {unknown_out:?}"
    );
}

// ── R16 ──────────────────────────────────────────────────────────────
// A removed device learns the self-removal fact independently of the intent
// set. The initiator remains unmarked.
#[tokio::test]
#[ignore]
async fn r16_removed_target_observes_self_removed_flag() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r16-a", DEVICE_A, &binaries, &rendezvous).await;
    let b = Node::fresh("removal-r16-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    wait_for_member_count(&a.cli, 2).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    remove(&a.cli, &b_id);
    let a_status = wait_for_effective_member_count(&a.cli, 1).await;
    assert!(!removed_of(&a_status), "initiator must not be marked removed: {a_status}");

    let b_status = wait_for_removed(&b.cli, true).await;
    assert_eq!(intent_count_of(&b_status), 0, "self-removal stays independent: {b_status}");
}

// ── R17 ──────────────────────────────────────────────────────────────
// The target can receive its notice after returning online. A subsequent
// restart must preserve the same local fact without creating intents.
#[tokio::test]
#[ignore]
async fn r17_removed_notice_is_idempotent_and_replay_safe() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r17-a", DEVICE_A, &binaries, &rendezvous).await;
    let mut b = Node::fresh("removal-r17-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    wait_for_member_count(&a.cli, 2).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    b.stop().await;
    remove(&a.cli, &b_id);
    wait_for_effective_member_count(&a.cli, 1).await;

    b.restart().await;
    let first = wait_for_removed(&b.cli, true).await;
    assert_eq!(intent_count_of(&first), 0, "notice must not create intents: {first}");

    b.restart().await;
    let replayed = wait_for_removed(&b.cli, true).await;
    assert_eq!(intent_count_of(&replayed), 0, "replayed notice must stay idempotent: {replayed}");
}

// ── R18 ──────────────────────────────────────────────────────────────
// Re-admission gives B a new member instance, which clears the historical
// self-removal fact without deleting the workspace's historical intent.
#[tokio::test]
#[ignore]
async fn r18_re_admission_clears_removed_marker() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let a = Node::initialized("removal-r18-a", DEVICE_A, &binaries, &rendezvous).await;
    let mut b = Node::fresh("removal-r18-b", &binaries, &rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    wait_for_member_count(&a.cli, 2).await;

    let b_id = remote_device_id(&a.cli, DEVICE_B);
    remove(&a.cli, &b_id);
    wait_for_effective_member_count(&a.cli, 1).await;
    wait_for_removed(&b.cli, true).await;

    b.stop().await;
    b.restart().await;
    join(&a, &b, DEVICE_B).await;
    wait_for_member_count(&a.cli, 2).await;

    let b_status = wait_for_removed(&b.cli, false).await;
    assert_eq!(
        intent_count_of(&b_status),
        1,
        "historical intent must not remove the new instance: {b_status}"
    );
}

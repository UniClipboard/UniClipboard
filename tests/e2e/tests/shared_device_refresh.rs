//! E2E tests for the Engine-owned shared-device refresh round, driven through
//! the hidden `dev shared-device-refresh` CLI commands.
//!
//! # Controlled topology
//!
//! The Engine's membership gossip converges automatically: when C pairs with A
//! while B is online, B learns C on its own within seconds, which would leave
//! the refresh with nothing to do (`already_present`). To build the stable
//! "B only knows A, does not know C" precondition the tests exploit the
//! gossip delivery semantics instead of timing races:
//!
//! 1. B pairs with A and reaches `complete` convergence while A knows no one
//!    else — B's member list is exactly {A, B} with no timing involved.
//! 2. B's daemon is stopped, so A's outbox delivery of C's announcement to B
//!    permanently fails and A's convergence stays `converging`.
//! 3. C pairs with A. B cannot have learned C while its daemon was down.
//! 4. B's daemon restarts; B's own first gossip pass runs before its P2P
//!    connection to A is ready, fails, and backs off (~30s). Starting the
//!    refresh immediately after the restart therefore runs well before any
//!    automatic completion can reach B. The tests assert this precondition
//!    explicitly (B's member list must not contain C before the refresh), so
//!    a future Engine behavior change fails loudly instead of flaking.
//!
//! Every async assertion polls with a deadline; success always requires BOTH
//! the refresh snapshot reporting `connected` AND the authoritative paired
//! device list containing the target device.
//!
//! Requires binaries built with `dev-tools` on the CLI:
//! `cargo build -p uc-daemon -p uc-cli --features uc-cli/dev-tools`
//! Run with: cargo test --manifest-path tests/e2e/Cargo.toml -- --ignored

use std::time::Duration;

use serde_json::Value;
use uc_e2e_tests::{InviteSession, LocalRendezvous, NodeBinarySet, TestCli, TestDaemon, TestProfile};

const PASSPHRASE: &str = "shared-device-refresh-e2e-passphrase";
const WAIT_TIMEOUT: Duration = Duration::from_secs(120);
// Polls are deliberately slow: every CLI invocation exchanges a fresh daemon
// session token (`/auth/connect`), which is rate-limited per daemon
// (PREAUTH_MAX_REQUESTS = 100 per 60s). `members` costs three auths per call.
const CLI_POLL_INTERVAL: Duration = Duration::from_secs(2);
const MEMBERS_POLL_INTERVAL: Duration = Duration::from_secs(3);

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
        tokio::time::sleep(MEMBERS_POLL_INTERVAL).await;
    }
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

/// Start a refresh round through the hidden CLI (the CLI process exits
/// immediately; the round keeps running in the daemon). Returns the request id.
async fn start_refresh(cli: &TestCli) -> String {
    let output = cli.run_capture(&["--json", "dev", "shared-device-refresh", "start"]);
    assert!(
        output.success(),
        "shared-device-refresh start failed: stdout={} stderr={}",
        output.stdout,
        output.stderr
    );
    let value: Value = serde_json::from_str(output.stdout.trim())
        .unwrap_or_else(|error| panic!("start output is not JSON: {error}\n{}", output.stdout));
    value
        .get("requestId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| panic!("start response missing requestId: {value}"))
}

/// Query the current snapshot for one refresh round.
async fn refresh_status(cli: &TestCli, request_id: &str) -> Value {
    let output = cli.run_capture(&[
        "--json",
        "dev",
        "shared-device-refresh",
        "status",
        "--request-id",
        request_id,
    ]);
    assert!(
        output.success(),
        "shared-device-refresh status failed: stdout={} stderr={}",
        output.stdout,
        output.stderr
    );
    serde_json::from_str(output.stdout.trim())
        .unwrap_or_else(|error| panic!("status output is not JSON: {error}\n{}", output.stdout))
}

fn snapshot_phase(snapshot: &Value) -> String {
    snapshot
        .get("phase")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| panic!("snapshot missing phase: {snapshot}"))
}

fn snapshot_device_state(snapshot: &Value, display_name: &str) -> Option<String> {
    snapshot
        .get("devices")
        .and_then(Value::as_array)
        .and_then(|devices| {
            devices.iter().find_map(|device| {
                (device.get("displayName").and_then(Value::as_str) == Some(display_name))
                    .then(|| device.get("state").and_then(Value::as_str).map(str::to_string))
                    .flatten()
            })
        })
}

async fn wait_for_snapshot_state(
    node: &Node,
    request_id: &str,
    expected: &str,
    context: &str,
) -> Value {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        let snapshot = refresh_status(&node.cli, request_id).await;
        if snapshot_device_state(&snapshot, DEVICE_C) == Some(expected.to_string()) {
            return snapshot;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "{context}: node-c never reached {expected}; last snapshot={snapshot}; log={}",
                node.daemon.diagnostic_log()
            );
        }
        tokio::time::sleep(CLI_POLL_INTERVAL).await;
    }
}

async fn wait_for_member_named(node: &Node, name: &str) {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        match try_members(&node.cli) {
            Ok(current) if has_member_named(&current, name) => return,
            Ok(current) if tokio::time::Instant::now() >= deadline => {
                panic!(
                    "{} never saw member {name}; members={current:?}; log={}",
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
        tokio::time::sleep(MEMBERS_POLL_INTERVAL).await;
    }
}

async fn wait_for_snapshot_phase(node: &Node, request_id: &str, expected: &str, context: &str) {
    let deadline = tokio::time::Instant::now() + WAIT_TIMEOUT;
    loop {
        let snapshot = refresh_status(&node.cli, request_id).await;
        if snapshot_phase(&snapshot) == expected {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "{context}: phase never reached {expected}; last snapshot={snapshot}; log={}",
                node.daemon.diagnostic_log()
            );
        }
        tokio::time::sleep(CLI_POLL_INTERVAL).await;
    }
}

/// Build the star topology with B offline while C joins, restart B, and
/// assert the precondition: B's member list must not contain C yet.
///
/// B's own first gossip pass after the restart fails before its P2P link to A
/// is ready and backs off (~30s), which is what makes "refresh right after the
/// restart" deterministic rather than a race against automatic completion.
///
/// When `stop_c` is true, C is stopped BEFORE B restarts: the caller wants to
/// refresh against an offline C, and starting the refresh immediately after
/// the restart must not be delayed by stopping C — A redelivers C's
/// announcement to B within a couple of seconds of B coming back online,
/// which would turn C into `already_present` and empty the round.
async fn restart_b_ignorant_of_c(
    prefix: &str,
    binaries: &NodeBinarySet,
    rendezvous: &LocalRendezvous,
    stop_c: bool,
) -> (Node, Node, Node) {
    let a = Node::initialized(&format!("{prefix}-a"), DEVICE_A, binaries, rendezvous).await;
    let mut b = Node::fresh(&format!("{prefix}-b"), binaries, rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    wait_for_convergence(&a, "complete").await;
    wait_for_convergence(&b, "complete").await;
    wait_for_member_count(&b, 2).await;

    // B offline so A's announcement delivery for C can never reach B.
    b.stop().await;
    let mut c = Node::fresh(&format!("{prefix}-c"), binaries, rendezvous).await;
    join(&a, &c, DEVICE_C).await;
    // A stays `converging` because its outbox to B cannot be delivered; do not
    // wait for it.

    if stop_c {
        c.stop().await;
    }
    b.restart().await;
    let current = members(&b.cli);
    assert!(
        current.len() == 2 && !has_member_named(&current, DEVICE_C),
        "precondition failed: B must not know C before the refresh \
         (automatic gossip completed too early); members={current:?}; log={}",
        b.daemon.diagnostic_log()
    );
    (a, b, c)
}

/// A is connected to C, B only knows A. B refreshes, the CLI exits at once,
/// and the round still connects C in the daemon — proven by polling the same
/// request afterwards and by the authoritative device list on B.
#[tokio::test]
#[ignore]
async fn refresh_connects_third_device_after_cli_exits() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let (_a, b, _c) = restart_b_ignorant_of_c("sdr1", &binaries, &rendezvous, false).await;

    let request_id = start_refresh(&b.cli).await;
    wait_for_snapshot_state(&b, &request_id, "connected", "sdr1 first round").await;
    wait_for_member_named(&b, DEVICE_C).await;
    let current = members(&b.cli);
    assert_eq!(current.len(), 3, "B must now know all three devices");
}

/// C is offline when B refreshes: C shows `waiting_for_peer`. Starting C
/// again — without any new refresh request — must finish the original round
/// as `connected` and surface C in B's device list.
#[tokio::test]
#[ignore]
async fn offline_device_recovers_without_new_refresh() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let (_a, b, mut c) = restart_b_ignorant_of_c("sdr2", &binaries, &rendezvous, true).await;

    let request_id = start_refresh(&b.cli).await;
    wait_for_snapshot_state(&b, &request_id, "waiting_for_peer", "sdr2 offline round").await;

    c.restart().await;
    wait_for_snapshot_state(&b, &request_id, "connected", "sdr2 recovery").await;
    wait_for_member_named(&b, DEVICE_C).await;
}

/// A refresh round is not persisted in the daemon: after a daemon restart the
/// old request id must be reported gone with a stable error, and a new round
/// must be startable right away.
#[tokio::test]
#[ignore]
async fn daemon_restart_invalidates_old_request() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let (_a, mut b, _c) = restart_b_ignorant_of_c("sdr3", &binaries, &rendezvous, false).await;

    let request_id = start_refresh(&b.cli).await;
    b.restart().await;

    let output = b.cli.run_capture(&[
        "--json",
        "dev",
        "shared-device-refresh",
        "status",
        "--request-id",
        &request_id,
    ]);
    assert!(
        !output.success(),
        "old request must not survive a daemon restart (exit 0)"
    );
    assert!(
        output.stdout.contains("shared_device_refresh_not_found")
            || output.stderr.contains("shared_device_refresh_not_found"),
        "old request must fail with the stable error code: stdout={} stderr={}",
        output.stdout,
        output.stderr
    );

    let new_request_id = start_refresh(&b.cli).await;
    assert_ne!(new_request_id, request_id, "a fresh round must get a new id");
    let snapshot = refresh_status(&b.cli, &new_request_id).await;
    assert!(
        snapshot.get("requestId").and_then(Value::as_str) == Some(new_request_id.as_str()),
        "new round must be queryable: {snapshot}"
    );
}

/// When every known device is already paired, a refresh must complete as
/// `round_completed` without connecting anything new, and all counts must be
/// consistent with the device list.
#[tokio::test]
#[ignore]
async fn refresh_with_no_new_candidates_completes_empty() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let (_a, b, _c) = restart_b_ignorant_of_c("sdr4", &binaries, &rendezvous, false).await;

    // First round connects C so B knows the whole space.
    let first = start_refresh(&b.cli).await;
    wait_for_snapshot_state(&b, &first, "connected", "sdr4 first round").await;
    wait_for_member_named(&b, DEVICE_C).await;

    // Second round: no new candidates remain. C is reported as already
    // present, so the empty result means: nothing discovered, nothing
    // connecting, nothing connected.
    let second = start_refresh(&b.cli).await;
    assert_ne!(second, first, "a completed round must not be reused");
    wait_for_snapshot_phase(&b, &second, "round_completed", "sdr4 second round").await;

    let snapshot = refresh_status(&b.cli, &second).await;
    let devices = snapshot
        .get("devices")
        .and_then(Value::as_array)
        .expect("snapshot must carry devices");
    assert_eq!(
        snapshot.get("totalCount").and_then(Value::as_u64),
        Some(devices.len() as u64),
        "totalCount must match the device list: {snapshot}"
    );
    for device in devices {
        let state = device.get("state").and_then(Value::as_str).unwrap_or("");
        assert!(
            !matches!(state, "discovered" | "connecting" | "connected"),
            "a round with no new candidates must not discover or connect anything: {snapshot}"
        );
    }
}

/// Starting a second refresh while the first round is still in flight must
/// reuse the same request id; after the round completes a new start gets a
/// fresh id. C is held offline so the first round stays in `connecting` long
/// enough for two back-to-back CLI starts to overlap it.
#[tokio::test]
#[ignore]
async fn concurrent_start_reuses_active_request() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    let (_a, b, mut c) = restart_b_ignorant_of_c("sdr5", &binaries, &rendezvous, true).await;

    let first = start_refresh(&b.cli).await;
    let second = start_refresh(&b.cli).await;
    assert_eq!(
        first, second,
        "an in-flight round must be reused, not restarted"
    );

    // Let C come back; the original round completes.
    c.restart().await;
    wait_for_snapshot_state(&b, &first, "connected", "sdr5 completion").await;

    // A completed round must not be reused.
    let third = start_refresh(&b.cli).await;
    assert_ne!(third, first, "a completed round must not be reused");
}

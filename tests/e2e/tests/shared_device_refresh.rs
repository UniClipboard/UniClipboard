//! E2E coverage for automatic shared-device discovery after an offline member
//! returns.
//!
//! # Controlled topology
//!
//! The Engine's membership gossip converges automatically: when C pairs with A
//! while B is online, B learns C on its own within seconds. To build the stable
//! "B only knows A, does not know C" precondition the tests exploit the
//! gossip delivery semantics instead of timing races:
//!
//! 1. B pairs with A while A knows no one else — B's member list is exactly
//!    {A, B} with no timing involved.
//! 2. B's daemon is stopped, so A's outbox delivery of C's announcement to B
//!    permanently fails and A's convergence stays `converging`.
//! 3. C pairs with A. B cannot have learned C while its daemon was down.
//! 4. B's daemon restarts before C, so B initially still has only A. Once C
//!    restarts, automatic discovery must make B and C visible to each other.
//! Run with: cargo test --manifest-path tests/e2e/Cargo.toml -- --ignored

use std::time::Duration;

use serde_json::Value;
use uc_e2e_tests::{
    InviteSession, LocalRendezvous, NodeBinarySet, TestCli, TestDaemon, TestProfile,
};

const PASSPHRASE: &str = "shared-device-refresh-e2e-passphrase";
const WAIT_TIMEOUT: Duration = Duration::from_secs(120);
// Polls are deliberately slow: every CLI invocation exchanges a fresh daemon
// session token (`/auth/connect`), which is rate-limited per daemon
// (PREAUTH_MAX_REQUESTS = 100 per 60s). `members` costs three auths per call.
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

/// Build the star topology with B offline while C joins, restart B, and
/// assert the precondition: B's member list must not contain C yet.
///
async fn restart_b_ignorant_of_c(
    prefix: &str,
    binaries: &NodeBinarySet,
    rendezvous: &LocalRendezvous,
) -> (Node, Node, Node) {
    let a = Node::initialized(&format!("{prefix}-a"), DEVICE_A, binaries, rendezvous).await;
    let mut b = Node::fresh(&format!("{prefix}-b"), binaries, rendezvous).await;
    join(&a, &b, DEVICE_B).await;
    wait_for_member_count(&b, 2).await;

    // B offline so A's announcement delivery for C can never reach B.
    b.stop().await;
    let mut c = Node::fresh(&format!("{prefix}-c"), binaries, rendezvous).await;
    join(&a, &c, DEVICE_C).await;
    // A stays `converging` because its outbox to B cannot be delivered; do not
    // wait for it.

    c.stop().await;
    b.restart().await;
    let current = members(&b.cli);
    assert!(
        current.len() == 2 && !has_member_named(&current, DEVICE_C),
        "precondition failed: B must not know C before automatic discovery \
         (automatic gossip completed too early); members={current:?}; log={}",
        b.daemon.diagnostic_log()
    );
    (a, b, c)
}

/// B and C are each paired with A but not with one another. C returns from an
/// offline restart and unlocks its existing space. Without a manual refresh,
/// the Engine must automatically discover the other shared member and finish
/// the mutual membership convergence.
///
/// This is the product red case for the reported unlock flow: both B and C
/// must expose each other in their authoritative member lists. A refresh
/// snapshot alone is not sufficient evidence of success.
#[tokio::test]
#[ignore]
async fn unlocking_offline_member_automatically_discovers_other_shared_member() {
    let binaries = NodeBinarySet::current();
    let rendezvous = LocalRendezvous::start().await;
    // `_a` retains A's Node so its daemon stays alive for the whole test.
    let (_a, b, mut c) = restart_b_ignorant_of_c("sdr6", &binaries, &rendezvous).await;

    c.restart().await;

    wait_for_member_named(&b, DEVICE_C).await;
    wait_for_member_named(&c, DEVICE_B).await;
    wait_for_member_count(&b, 3).await;
    wait_for_member_count(&c, 3).await;
}

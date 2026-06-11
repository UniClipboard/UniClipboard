//! E2E tests for the dual-node pairing lifecycle: one-sided unpair and
//! re-pairing (issue #1023 regression).
//!
//! Unpair is locally autonomous — `POST /pairing/unpair` only deletes the
//! local member/trust records and never notifies the peer. These tests
//! verify that the records left behind on the *other* side no longer block
//! a fresh invite/join round (pre-#1023 the stale rows made admit/trust
//! fail with `AlreadyAdmitted` / `AlreadyTrusted`, rejecting the pairing).
//!
//! The first-pair happy path is covered by
//! `clipboard_sync::pair_invite_join_full_handshake`.
//!
//! Run with: cargo test -p uc-e2e-tests -- --ignored

use std::time::Duration;

use serde_json::Value;
use uc_e2e_tests::{get_session_token, invite_join_round, pair_two_nodes, TestCli, TestDaemon};

const PASSPHRASE: &str = "dual-node-e2e-passphrase";

/// Fetch `--json members` and return the parsed array.
fn members_json(cli: &TestCli) -> Vec<Value> {
    let out = cli.run_capture(&["--json", "members"]);
    assert!(out.success(), "members failed: {}", out.stderr);
    serde_json::from_str::<Value>(out.stdout.trim())
        .unwrap_or_else(|e| panic!("members output not valid JSON: {e}\nstdout: {}", out.stdout))
        .as_array()
        .unwrap_or_else(|| panic!("members output not a JSON array: {}", out.stdout))
        .clone()
}

/// Device id of the unique remote (non-local) member.
fn remote_member_id(cli: &TestCli) -> String {
    let members = members_json(cli);
    let remote: Vec<&Value> = members
        .iter()
        .filter(|m| !m.get("is_local").and_then(|v| v.as_bool()).unwrap_or(false))
        .collect();
    assert_eq!(
        remote.len(),
        1,
        "expected exactly one remote member, got {remote:?}"
    );
    remote[0]
        .get("device_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("remote member missing device_id: {:?}", remote[0]))
        .to_string()
}

/// One-sided unpair via the daemon HTTP API (the CLI has no unpair command
/// yet). Asserts the endpoint answers `204 No Content`.
async fn unpair_via_api(daemon: &TestDaemon, peer_id: &str) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let token = get_session_token(daemon, &client).await;

    let resp = client
        .post(format!("{}/pairing/unpair", daemon.base_url()))
        .header("Authorization", format!("Session {token}"))
        .json(&serde_json::json!({ "peerId": peer_id }))
        .send()
        .await
        .expect("unpair request");

    assert_eq!(
        resp.status().as_u16(),
        204,
        "unpair should answer 204, got {}",
        resp.status()
    );
}

/// Assert both sides converge on exactly two members (local + peer) and
/// each sees the other by device name. The strict `== 2` also catches a
/// "duplicate add" — a second row for the same peer would surface here.
fn assert_two_members_each_side(alice_cli: &TestCli, bob_cli: &TestCli) {
    let alice_members = members_json(alice_cli);
    assert_eq!(
        alice_members.len(),
        2,
        "alice should see exactly 2 members (no duplicate bob): {alice_members:?}"
    );
    assert!(
        alice_members.iter().any(|m| {
            m.get("device_name")
                .and_then(|v| v.as_str())
                .map(|n| n.contains("bob"))
                .unwrap_or(false)
        }),
        "alice members should contain bob: {alice_members:?}"
    );

    let bob_members = members_json(bob_cli);
    assert_eq!(
        bob_members.len(),
        2,
        "bob should see exactly 2 members (no duplicate alice): {bob_members:?}"
    );
    assert!(
        bob_members.iter().any(|m| {
            m.get("device_name")
                .and_then(|v| v.as_str())
                .map(|n| n.contains("alice"))
                .unwrap_or(false)
        }),
        "bob members should contain alice: {bob_members:?}"
    );
}

/// Issue #1023 original scenario: the joiner (Bob) unpairs one-sidedly, so
/// the sponsor (Alice) keeps stale member/trust rows for Bob. A fresh
/// invite/join round must succeed — pre-#1023 Alice's `finalise_verified`
/// hit `AlreadyAdmitted` and rejected every re-pair with `Internal`.
#[tokio::test]
#[ignore]
async fn re_pair_after_joiner_unpairs_succeeds() {
    let (alice_daemon, alice_cli, bob_daemon, bob_cli) =
        pair_two_nodes("repair-joiner", PASSPHRASE).await;

    // Bob one-sidedly unpairs Alice.
    let alice_id = remote_member_id(&bob_cli);
    unpair_via_api(&bob_daemon, &alice_id).await;

    // Bob's roster is clean; Alice's stale view of Bob must survive — that
    // asymmetry is the precondition of the bug.
    assert_eq!(
        members_json(&bob_cli).len(),
        1,
        "bob should only see himself after unpair"
    );
    assert_eq!(
        members_json(&alice_cli).len(),
        2,
        "alice must keep her stale member record (one-sided unpair)"
    );

    // Re-invite + re-join.
    let join_out = invite_join_round(&alice_cli, &bob_cli, PASSPHRASE, "bob-node").await;
    assert!(
        join_out.success(),
        "re-join after one-sided unpair must succeed (issue #1023), exit={}: stdout={}, stderr={}",
        join_out.exit_code,
        join_out.stdout,
        join_out.stderr,
    );

    // Settle, then verify both rosters converged without duplicates.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_two_members_each_side(&alice_cli, &bob_cli);

    drop(alice_daemon);
    drop(bob_daemon);
}

/// Mirror scenario: the sponsor (Alice) unpairs one-sidedly, so the joiner
/// (Bob) keeps stale member/trust rows for Alice. The re-join exercises the
/// joiner-side replace path in the redeem use case.
#[tokio::test]
#[ignore]
async fn re_pair_after_sponsor_unpairs_succeeds() {
    let (alice_daemon, alice_cli, bob_daemon, bob_cli) =
        pair_two_nodes("repair-sponsor", PASSPHRASE).await;

    // Alice one-sidedly unpairs Bob.
    let bob_id = remote_member_id(&alice_cli);
    unpair_via_api(&alice_daemon, &bob_id).await;

    assert_eq!(
        members_json(&alice_cli).len(),
        1,
        "alice should only see herself after unpair"
    );
    assert_eq!(
        members_json(&bob_cli).len(),
        2,
        "bob must keep his stale member record (one-sided unpair)"
    );

    // Re-invite + re-join: Bob redeems while still holding Alice's old rows.
    let join_out = invite_join_round(&alice_cli, &bob_cli, PASSPHRASE, "bob-node").await;
    assert!(
        join_out.success(),
        "re-join with stale joiner-side records must succeed (issue #1023), exit={}: stdout={}, stderr={}",
        join_out.exit_code,
        join_out.stdout,
        join_out.stderr,
    );

    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_two_members_each_side(&alice_cli, &bob_cli);

    drop(alice_daemon);
    drop(bob_daemon);
}

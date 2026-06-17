//! E2E test for the merged `join` switch path.
//!
//! `uniclip join --switch` opts into the switch-space migration (re-encrypting
//! local history under the new sponsor's master key). This test verifies the
//! *routing + happy path*: an already-initialized Bob runs `join --switch
//! --yes` against Alice's invitation and ends up a member of Alice's space.
//!
//! NOTE: data round-trip integrity — seeded clipboard history surviving the
//! re-encryption — is intentionally NOT covered here. The headless E2E binary
//! has no real OS clipboard, and it is built without `dev-tools` (CI runs
//! `cargo build -p uc-daemon -p uc-cli`), so there is no `dev seed-clipboard`
//! / `dev dump-clipboard` and no way to populate or read local history. That
//! assertion lives in `scripts/test_switch_space_e2e.sh`, which runs with
//! `dev-tools` against a real macOS clipboard.
//!
//! Run with: cargo test -p uc-e2e-tests -- --ignored

use std::time::Duration;

use serde_json::Value;
use uc_e2e_tests::{invite_switch_round, setup_initialized_node, TestCli};

const PASSPHRASE_ALICE: &str = "switch-alice-passphrase";
const PASSPHRASE_BOB: &str = "switch-bob-passphrase";

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

/// True if any member's `device_name` contains `needle`.
fn has_member_named(members: &[Value], needle: &str) -> bool {
    members.iter().any(|m| {
        m.get("device_name")
            .and_then(|v| v.as_str())
            .map(|n| n.contains(needle))
            .unwrap_or(false)
    })
}

/// Bob already has his own space; running `join --switch` must take the
/// destructive switch path (not first-time redeem) and migrate him into
/// Alice's space.
#[tokio::test]
#[ignore]
async fn already_set_up_join_routes_to_switch_and_migrates_membership() {
    // Alice: her own space A.
    let (alice_daemon, alice_cli) =
        setup_initialized_node("switch-alice", "alice-node", PASSPHRASE_ALICE).await;

    // Bob: his own space B. He must explicitly pass `--switch` to migrate.
    let (bob_daemon, bob_cli) =
        setup_initialized_node("switch-bob", "bob-node", PASSPHRASE_BOB).await;

    // Bob switches into Alice's space via `join --switch --yes`.
    let switch_out = invite_switch_round(&alice_cli, &bob_cli, PASSPHRASE_ALICE).await;
    assert!(
        switch_out.success(),
        "bob switch (join --switch --yes) failed (exit={}): stdout={}, stderr={}",
        switch_out.exit_code,
        switch_out.stdout,
        switch_out.stderr,
    );

    // The switch path prints "Switched space" + a `migrated_records` line on
    // stderr (all ui output goes to Term::stderr). Redeem prints "Joined
    // space" with no `migrated_records` — so these confirm `--switch` took the
    // destructive switch, not a first-time join.
    assert!(
        switch_out.stderr.contains("Switched space"),
        "expected switch-path output, got stderr: {}",
        switch_out.stderr,
    );
    assert!(
        switch_out.stderr.contains("migrated_records"),
        "switch output should report migrated_records, got stderr: {}",
        switch_out.stderr,
    );

    // Settle, then both sides should converge on exactly two members
    // (local + peer). Bob's old space B had only himself, so a clean switch
    // leaves him with [bob, alice]; the strict `== 2` also catches a stale
    // row left behind from space B.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let bob_members = members_json(&bob_cli);
    assert_eq!(
        bob_members.len(),
        2,
        "after switch, bob should see exactly 2 members (himself + alice): {bob_members:?}"
    );
    assert!(
        has_member_named(&bob_members, "alice"),
        "after switch, bob must see alice as a member: {bob_members:?}"
    );

    let alice_members = members_json(&alice_cli);
    assert_eq!(
        alice_members.len(),
        2,
        "after switch, alice should see exactly 2 members (herself + bob): {alice_members:?}"
    );
    assert!(
        has_member_named(&alice_members, "bob"),
        "after switch, alice must see bob as a member: {alice_members:?}"
    );

    drop(alice_daemon);
    drop(bob_daemon);
}

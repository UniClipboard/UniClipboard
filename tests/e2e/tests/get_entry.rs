//! E2E tests for `uniclip get` — the one-shot reader for already-synced
//! clipboard entries (issue #1025).
//!
//! `get` is the non-blocking counterpart to `recv`: instead of subscribing and
//! waiting for the *next* inbound file, it reads what is *already* in the
//! daemon's history and returns immediately. These tests verify the contract
//! that scripts / agents depend on:
//!
//! - argument-level guards (invalid `--type`, mutually-exclusive selectors);
//! - empty-history behaviour and the dedicated exit codes;
//! - `--list` output (human + JSON);
//! - the signature property: `get` does NOT block (unlike `recv`).
//!
//! **Headless limitation** (see `clipboard_history.rs`): in this environment
//! there is no OS clipboard capture and pairing is not wired up, so the history
//! starts empty. We therefore cannot exercise the happy-path materialization of
//! a real image/file/text entry here — that path is covered by unit tests in
//! `uc-cli` and must be validated on a real paired node. What we CAN pin down
//! end-to-end is every selection / contract / exit-code branch around it.
//!
//! Exit codes (mirrors `uc-cli/src/exit_codes.rs`):
//! - `6` = EXIT_NO_MATCH — no entry matched the selector.
//! - `7` = EXIT_CONTENT_UNAVAILABLE — matched but payload Lost / not downloaded.
//!
//! Run with: cargo test -p uc-e2e-tests -- --ignored

use std::time::Duration;

use uc_e2e_tests::{TestCli, TestDaemon, TestProfile};

const EXIT_NO_MATCH: i32 = 6;

/// Start a daemon and init a space, returning (daemon, cli). The daemon stays
/// alive (held by the returned handle) so `get` reuses it as a running peer.
async fn setup_initialized_node(name: &str) -> (TestDaemon, TestCli) {
    let profile = TestProfile::new(name);
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    let out = cli.run_capture(&[
        "init",
        "--passphrase",
        "get-test-passphrase-e2e",
        "--device-name",
        "get-e2e-device",
    ]);
    assert!(
        out.success(),
        "init failed (exit={}): {}",
        out.exit_code,
        out.stderr
    );

    (daemon, cli)
}

// ── Empty-history selection / exit codes ─────────────────────────────

/// `get` (no selector) on an empty history returns EXIT_NO_MATCH rather than
/// blocking or erroring — there simply is no entry to return.
#[tokio::test]
#[ignore]
async fn get_empty_history_returns_no_match() {
    let (_daemon, cli) = setup_initialized_node("get-empty").await;

    let out = cli.run_capture(&["get"]);
    assert_eq!(
        out.exit_code, EXIT_NO_MATCH,
        "expected EXIT_NO_MATCH on empty history; stdout={}, stderr={}",
        out.stdout, out.stderr
    );
}

/// `get --type image` on an empty history returns EXIT_NO_MATCH and the error
/// names the kind that was not found.
#[tokio::test]
#[ignore]
async fn get_type_image_empty_history_returns_no_match() {
    let (_daemon, cli) = setup_initialized_node("get-type-empty").await;

    let out = cli.run_capture(&["get", "--type", "image"]);
    assert_eq!(
        out.exit_code, EXIT_NO_MATCH,
        "expected EXIT_NO_MATCH for --type image on empty history; stderr={}",
        out.stderr
    );
    assert!(
        out.stderr.to_lowercase().contains("image"),
        "error should mention the requested kind 'image', got stderr={}",
        out.stderr
    );
}

/// `get --id <nonexistent>` returns EXIT_NO_MATCH — the id is not in the
/// scanned window.
#[tokio::test]
#[ignore]
async fn get_nonexistent_id_returns_no_match() {
    let (_daemon, cli) = setup_initialized_node("get-bad-id").await;

    let out = cli.run_capture(&["get", "--id", "ent-does-not-exist"]);
    assert_eq!(
        out.exit_code, EXIT_NO_MATCH,
        "expected EXIT_NO_MATCH for a nonexistent --id; stderr={}",
        out.stderr
    );
}

/// In `--json` mode, a no-match must NOT print anything to stdout (errors go to
/// stderr). Scripts can rely on "empty stdout ⇒ nothing materialized".
#[tokio::test]
#[ignore]
async fn get_json_no_match_emits_no_stdout() {
    let (_daemon, cli) = setup_initialized_node("get-json-nomatch").await;

    let out = cli.run_capture(&["--json", "get"]);
    assert_eq!(
        out.exit_code, EXIT_NO_MATCH,
        "expected EXIT_NO_MATCH; stderr={}",
        out.stderr
    );
    assert!(
        out.stdout.trim().is_empty(),
        "no-match must leave stdout empty in --json mode, got stdout={}",
        out.stdout
    );
}

// ── --list output ────────────────────────────────────────────────────

/// `get --list` on an empty history exits 0 (listing nothing is not a failure).
#[tokio::test]
#[ignore]
async fn get_list_empty_history_succeeds() {
    let (_daemon, cli) = setup_initialized_node("get-list-empty").await;

    let out = cli.run_capture(&["get", "--list"]);
    assert!(
        out.success(),
        "get --list should exit 0 on empty history; exit={}, stderr={}",
        out.exit_code,
        out.stderr
    );
}

/// `get --list --json` emits a well-formed JSON array on stdout (empty when the
/// history is empty), so callers can parse it unconditionally.
#[tokio::test]
#[ignore]
async fn get_list_json_outputs_valid_array() {
    let (_daemon, cli) = setup_initialized_node("get-list-json").await;

    let out = cli.run_capture(&["--json", "get", "--list"]);
    assert!(
        out.success(),
        "get --list --json should exit 0; exit={}, stderr={}",
        out.exit_code,
        out.stderr
    );

    let parsed: serde_json::Value = serde_json::from_str(out.stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "stdout should be valid JSON, got err={e}, stdout={}",
            out.stdout
        )
    });
    assert!(
        parsed.is_array(),
        "get --list --json stdout should be a JSON array, got: {parsed}"
    );
}

// ── Signature behaviour: get does NOT block ──────────────────────────

/// The defining contrast with `recv`: `get` must return promptly instead of
/// waiting for an inbound entry. We spawn it against an empty history and assert
/// it terminates well within a timeout (and with EXIT_NO_MATCH).
#[tokio::test]
#[ignore]
async fn get_is_non_blocking() {
    let (_daemon, cli) = setup_initialized_node("get-nonblock").await;

    let mut child = std::process::Command::new(cli.binary_path())
        .env("UC_PROFILE", &cli.profile_name)
        .args(["get"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn get");

    // Poll for exit; `get` should finish quickly (one-shot, no waiting).
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    let status = loop {
        match child.try_wait().expect("try_wait failed") {
            Some(status) => break Some(status),
            None => {
                if std::time::Instant::now() >= deadline {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };

    let status = match status {
        Some(s) => s,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("get blocked: it did not exit within 15s on an empty history");
        }
    };
    assert_eq!(
        status.code(),
        Some(EXIT_NO_MATCH),
        "non-blocking get on empty history should exit EXIT_NO_MATCH"
    );
}

// ── Argument-level contracts (clap; no daemon needed) ────────────────

/// `--type` only accepts the four known kinds; an unknown value is rejected by
/// clap before any runtime logic.
#[tokio::test]
#[ignore]
async fn get_invalid_type_rejected() {
    let profile = TestProfile::new("get-bad-type");
    let cli = TestCli::new(&profile);

    let out = cli.run_capture(&["get", "--type", "video"]);
    assert!(
        !out.success(),
        "get --type video should be rejected; exit={}",
        out.exit_code
    );
    let combined = format!("{}{}", out.stdout, out.stderr);
    assert!(
        combined.contains("invalid value") || combined.contains("possible values"),
        "expected a clap value-enum rejection, got: {combined}"
    );
}

/// `--type` and `--id` are mutually exclusive (select-newest-of-kind vs
/// select-specific-id). clap rejects the combination.
#[tokio::test]
#[ignore]
async fn get_type_and_id_mutually_exclusive() {
    let profile = TestProfile::new("get-type-id-mutex");
    let cli = TestCli::new(&profile);

    let out = cli.run_capture(&["get", "--type", "image", "--id", "ent-1"]);
    assert!(
        !out.success(),
        "get --type … --id … should be rejected; exit={}",
        out.exit_code
    );
    let combined = format!("{}{}", out.stdout, out.stderr);
    assert!(
        combined.contains("cannot be used with") || combined.contains("conflict"),
        "expected a clap conflict error, got: {combined}"
    );
}

/// `--list` cannot be combined with a selector.
#[tokio::test]
#[ignore]
async fn get_list_conflicts_with_selectors() {
    let profile = TestProfile::new("get-list-mutex");
    let cli = TestCli::new(&profile);

    let out = cli.run_capture(&["get", "--list", "--type", "image"]);
    assert!(
        !out.success(),
        "get --list --type … should be rejected; exit={}",
        out.exit_code
    );
    let combined = format!("{}{}", out.stdout, out.stderr);
    assert!(
        combined.contains("cannot be used with") || combined.contains("conflict"),
        "expected a clap conflict error, got: {combined}"
    );
}

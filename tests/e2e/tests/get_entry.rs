//! E2E tests for `uniclip get` — the one-shot reader for already-synced
//! clipboard entries (issue #1025).
//!
//! Bare `get` reads what is already in history and returns immediately, while
//! `get --wait` subscribes for the next remote entry. These tests verify the contract
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

#[cfg(target_os = "linux")]
fn shell_quote(value: &std::ffi::OsStr) -> String {
    let value = value.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn script_command(
    transcript: &std::path::Path,
    binary: &std::path::Path,
    args: &[&str],
) -> std::process::Command {
    let mut command = std::process::Command::new("script");
    command.arg("-q");
    #[cfg(target_os = "linux")]
    {
        let invocation = std::iter::once(binary.as_os_str())
            .chain(args.iter().map(std::ffi::OsStr::new))
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" ");
        command.args(["-c", &invocation]).arg(transcript);
    }
    #[cfg(not(target_os = "linux"))]
    {
        command.arg(transcript).arg(binary).args(args);
    }
    command
}

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

/// Waiting is an explicit mode: unlike bare `get`, it must not consume the
/// empty/current history and exit before a new remote entry arrives.
#[tokio::test]
#[ignore]
async fn get_wait_blocks_until_ctrl_c() {
    let (_daemon, cli) = setup_initialized_node("get-wait-blocks").await;
    let mut child = std::process::Command::new(cli.binary_path())
        .env("UC_PROFILE", &cli.profile_name)
        .args(["get", "--wait"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn get --wait");

    std::thread::sleep(Duration::from_secs(2));
    assert!(
        child.try_wait().expect("read wait state").is_none(),
        "get --wait must not use an entry that existed before its subscription"
    );
    let signal_result = unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
    assert_eq!(signal_result, 0, "failed to send SIGINT to get --wait");
    let output = wait_for_output(child, Duration::from_secs(10));
    assert!(output.status.success(), "Ctrl-C should stop cleanly");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains('\r') && !stderr.contains("\u{1b}["),
        "non-interactive Ctrl-C must not leave terminal control sequences: {stderr:?}"
    );
}

/// Two concurrent waiters attach to the same daemon and independently receive
/// the next text entry. A later waiter materializes a synced file through the
/// same command path.
#[tokio::test]
#[ignore]
async fn get_wait_receives_text_and_file_without_replacing_daemon() {
    let (mut alice_daemon, alice_cli, mut bob_daemon, bob_cli) =
        uc_e2e_tests::pair_two_nodes("get-wait-content", "get-wait-content-pass").await;

    let spawn_waiter = || {
        std::process::Command::new(bob_cli.binary_path())
            .env("UC_PROFILE", &bob_cli.profile_name)
            .env("UNICLIPBOARD_ENV", "development")
            .args(["get", "--wait"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn get --wait")
    };

    let first = spawn_waiter();
    let second = spawn_waiter();
    tokio::time::sleep(Duration::from_secs(2)).await;

    let payload = format!("get-wait-text-{}", std::process::id());
    let sent = alice_cli.run_capture(&["send", &payload]);
    assert!(sent.success(), "text send failed: {}", sent.stderr);

    for child in [first, second] {
        let output = wait_for_output(child, Duration::from_secs(20));
        assert!(output.status.success(), "waiter failed: {}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), payload);
    }
    assert!(alice_daemon.is_running());
    assert!(bob_daemon.is_running());

    let output_dir = tempfile::tempdir().expect("get output directory");
    let file_waiter = std::process::Command::new(bob_cli.binary_path())
        .env("UC_PROFILE", &bob_cli.profile_name)
        .env("UNICLIPBOARD_ENV", "development")
        .args(["get", "--wait", "--out"])
        .arg(output_dir.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn file waiter");
    tokio::time::sleep(Duration::from_secs(2)).await;

    let source_dir = tempfile::tempdir().expect("source directory");
    let source = source_dir.path().join("waited.bin");
    let source_bytes: Vec<u8> = (0..32 * 1024 * 1024)
        .map(|index| ((index * 31 + 17) % 251) as u8)
        .collect();
    std::fs::write(&source, &source_bytes).expect("write source file");
    let sent = alice_cli.run_capture(&["send", "--file", source.to_str().expect("source path")]);
    assert!(sent.success(), "file send failed: {}", sent.stderr);

    let output = wait_for_output(file_waiter, Duration::from_secs(30));
    assert!(output.status.success(), "file waiter failed: {}", String::from_utf8_lossy(&output.stderr));
    let received = std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    assert_eq!(
        std::fs::read(received).expect("read received file"),
        source_bytes,
        "the materialized large file must be byte-identical"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains('\r') && !stderr.contains("\u{1b}["),
        "non-interactive stderr must not contain terminal control sequences: {stderr:?}"
    );
}

/// JSON mode keeps stdout reserved for the final result while transfer status
/// remains on stderr. This covers the same real daemon/file path as the human
/// output test rather than a renderer-only fixture.
#[tokio::test]
#[ignore]
async fn get_wait_json_keeps_stdout_parseable_for_a_real_file() {
    let (mut alice_daemon, alice_cli, mut bob_daemon, bob_cli) =
        uc_e2e_tests::pair_two_nodes("get-wait-json", "get-wait-json-pass").await;
    let output_dir = tempfile::tempdir().expect("get output directory");
    let waiter = std::process::Command::new(bob_cli.binary_path())
        .env("UC_PROFILE", &bob_cli.profile_name)
        .env("UNICLIPBOARD_ENV", "development")
        .args(["--json", "get", "--wait", "--type", "file", "--out"])
        .arg(output_dir.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn JSON file waiter");
    tokio::time::sleep(Duration::from_secs(2)).await;

    let source_dir = tempfile::tempdir().expect("source directory");
    let source = source_dir.path().join("json-wait.bin");
    let source_bytes = vec![0x5a; 8 * 1024 * 1024];
    std::fs::write(&source, &source_bytes).expect("write source file");
    let sent = alice_cli.run_capture(&["send", "--file", source.to_str().expect("source path")]);
    assert!(sent.success(), "file send failed: {}", sent.stderr);

    let output = wait_for_output(waiter, Duration::from_secs(60));
    assert!(output.status.success(), "JSON waiter failed: {}", String::from_utf8_lossy(&output.stderr));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("stdout must contain exactly one final JSON value");
    let received = std::path::PathBuf::from(value["path"].as_str().expect("JSON path"));
    assert_eq!(std::fs::read(received).expect("read received file"), source_bytes);
    assert!(alice_daemon.is_running());
    assert!(bob_daemon.is_running());
}

/// Runs the receiver behind a real pseudo-terminal and preserves the raw
/// transcript when `UC_PROGRESS_EVIDENCE_PATH` is set.
#[tokio::test]
#[ignore]
async fn get_wait_shows_real_progress_in_an_interactive_terminal() {
    let (mut alice_daemon, alice_cli, mut bob_daemon, bob_cli) =
        uc_e2e_tests::pair_two_nodes("get-wait-pty", "get-wait-pty-pass").await;
    let output_dir = tempfile::tempdir().expect("get output directory");
    let transcript = std::env::var_os("UC_PROGRESS_EVIDENCE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| output_dir.path().join("terminal-progress.log"));
    let output_dir_arg = output_dir.path().to_string_lossy().into_owned();
    let mut waiter_command = script_command(
        &transcript,
        bob_cli.binary_path(),
        &["get", "--wait", "--type", "file", "--out", &output_dir_arg],
    );
    let waiter = waiter_command
        .env("UC_PROFILE", &bob_cli.profile_name)
        .env("UNICLIPBOARD_ENV", "development")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn pseudo-terminal waiter");
    tokio::time::sleep(Duration::from_secs(2)).await;

    let source_dir = tempfile::tempdir().expect("source directory");
    let source = source_dir.path().join("interactive-progress.bin");
    let source_bytes: Vec<u8> = (0..64 * 1024 * 1024)
        .map(|index| ((index * 13 + 7) % 251) as u8)
        .collect();
    std::fs::write(&source, &source_bytes).expect("write source file");
    let sent = alice_cli.run_capture(&["send", "--file", source.to_str().expect("source path")]);
    assert!(sent.success(), "file send failed: {}", sent.stderr);

    let output = wait_for_output(waiter, Duration::from_secs(90));
    assert!(output.status.success(), "PTY waiter failed: {}", String::from_utf8_lossy(&output.stderr));
    let transcript_bytes = std::fs::read(&transcript).expect("read terminal transcript");
    let transcript_text = String::from_utf8_lossy(&transcript_bytes);
    assert!(transcript_text.contains("Receiving"), "terminal transcript did not show receiving state");
    assert!(transcript_text.contains('%'), "terminal transcript did not show percentage progress");
    let received = output_dir.path().join("interactive-progress.bin");
    assert_eq!(std::fs::read(received).expect("read received file"), source_bytes);
    assert!(alice_daemon.is_running());
    assert!(bob_daemon.is_running());
}

fn wait_for_output(mut child: std::process::Child, timeout: Duration) -> std::process::Output {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if child.try_wait().expect("read child state").is_some() {
            return child.wait_with_output().expect("collect child output");
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().expect("collect timed-out child output");
            panic!(
                "command timed out; stderr={}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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

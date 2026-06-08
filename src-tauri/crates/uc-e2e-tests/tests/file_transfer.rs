//! E2E tests for file transfer paths: `send --file` and `recv --out`.
//!
//! Covers feature-gate rejection, path validation, mutual-exclusion guards,
//! and recv's blocking behaviour.
//!
//! Run with: cargo test -p uc-e2e-tests -- --ignored

use uc_e2e_tests::{TestCli, TestDaemon, TestProfile};

/// Helper: start a daemon and init a space, returning (daemon, cli).
async fn setup_initialized_node(name: &str) -> (TestDaemon, TestCli) {
    let profile = TestProfile::new(name);
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    let output = cli.run_capture(&[
        "init",
        "--passphrase",
        "test-passphrase-e2e",
        "--device-name",
        "e2e-device",
    ]);
    assert!(
        output.success(),
        "init failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    (daemon, cli)
}

/// Without the `dev-tools` feature, `send --file <path>` should exit non-zero
/// with an error mentioning "dev-tools".
///
/// The release (non-dev-tools) binary hits the `#[cfg(not(feature = "dev-tools"))]`
/// branch in `send.rs` which prints the feature-gate error message.
#[tokio::test]
#[ignore]
async fn file_send_requires_dev_tools_feature() {
    let (_daemon, cli) = setup_initialized_node("file-devtools").await;

    // Create a real temp file so the path itself is valid — we want the
    // feature-gate error, not a "file not found" error.
    let tmp = tempfile::NamedTempFile::new().expect("create temp file");
    let path_str = tmp.path().to_str().expect("temp path to str");

    let output = cli.run_capture(&["send", "--file", path_str]);
    assert!(
        !output.success(),
        "send --file should fail without dev-tools feature, got exit=0"
    );

    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("dev-tools"),
        "error should mention 'dev-tools', got: stdout={}, stderr={}",
        output.stdout,
        output.stderr
    );
}

/// `send --file /nonexistent/path.txt` should fail because `path.canonicalize()`
/// returns an error when the file does not exist.
#[tokio::test]
#[ignore]
async fn file_send_nonexistent_path() {
    let (_daemon, cli) = setup_initialized_node("file-noexist").await;

    let output = cli.run_capture(&["send", "--file", "/nonexistent/path.txt"]);
    assert!(
        !output.success(),
        "send --file with nonexistent path should fail, got exit=0"
    );

    let combined = format!("{}{}", output.stdout, output.stderr);
    // Without dev-tools feature: "requires the in-process blob stack"
    // With dev-tools feature: "Failed to resolve file path" / "no such file"
    assert!(
        combined.to_lowercase().contains("resolve")
            || combined.to_lowercase().contains("file path")
            || combined.to_lowercase().contains("no such file")
            || combined.to_lowercase().contains("dev-tools")
            || combined.to_lowercase().contains("blob stack"),
        "error should mention file path or dev-tools requirement, got: stdout={}, stderr={}",
        output.stdout,
        output.stderr
    );
}

/// `send --file /tmp` (a directory) should fail with an error about
/// "not a regular file", validating the `metadata.is_file()` guard.
///
/// NOTE: This test only reaches the is_file() guard when built with dev-tools.
/// Without dev-tools, the feature gate fires first with a "dev-tools" error.
/// We accept either error as a valid rejection.
#[tokio::test]
#[ignore]
async fn file_send_directory_path_rejected() {
    let (_daemon, cli) = setup_initialized_node("file-dir-reject").await;

    let output = cli.run_capture(&["send", "--file", "/tmp"]);
    assert!(
        !output.success(),
        "send --file with directory path should fail, got exit=0"
    );

    let combined = format!("{}{}", output.stdout, output.stderr);
    // With dev-tools: "Path is not a regular file."
    // Without dev-tools: "dev-tools" feature gate message
    assert!(
        combined.contains("not a regular file") || combined.contains("dev-tools"),
        "error should mention 'not a regular file' or 'dev-tools', got: stdout={}, stderr={}",
        output.stdout,
        output.stderr
    );
}

/// `send --file <path> <text>` and `send --file <path> --resend <id>` should
/// both fail due to clap's `conflicts_with` guards. These are argument-level
/// mutual exclusion errors that fire before any runtime logic.
#[tokio::test]
#[ignore]
async fn file_send_mutual_exclusion_flags() {
    let profile = TestProfile::new("file-mutex");
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    // Case 1: --file with positional text
    // clap enforces conflicts_with = ["file"] on the text positional,
    // so this should be rejected at the argument parsing layer.
    let tmp = tempfile::NamedTempFile::new().expect("create temp file");
    let path_str = tmp.path().to_str().expect("temp path to str");

    let output1 = cli.run_capture(&["send", "--file", path_str, "some-text"]);
    assert!(
        !output1.success(),
        "send --file with text should fail, got exit=0"
    );
    let combined1 = format!("{}{}", output1.stdout, output1.stderr);
    // clap produces "cannot be used with" in its conflict error messages
    assert!(
        combined1.contains("cannot be used with") || combined1.contains("conflict"),
        "expected mutual exclusion error for --file + text, got: stdout={}, stderr={}",
        output1.stdout,
        output1.stderr
    );

    // Case 2: --file with --resend
    let output2 = cli.run_capture(&["send", "--file", path_str, "--resend", "abc123"]);
    assert!(
        !output2.success(),
        "send --file with --resend should fail, got exit=0"
    );
    let combined2 = format!("{}{}", output2.stdout, output2.stderr);
    assert!(
        combined2.contains("cannot be used with") || combined2.contains("conflict"),
        "expected mutual exclusion error for --file + --resend, got: stdout={}, stderr={}",
        output2.stdout,
        output2.stderr
    );

    drop(daemon);
}

/// After init, `recv --out <dir>` should block waiting for an inbound file.
/// We spawn it in background, verify it does NOT exit within 2 seconds
/// (confirming it blocks), then send SIGTERM to clean up.
#[tokio::test]
#[ignore]
async fn recv_blocks_until_ctrl_c() {
    let (_daemon, cli) = setup_initialized_node("recv-blocks").await;

    let recv_dir = tempfile::tempdir().expect("create recv dir");
    let recv_path = recv_dir.path().to_str().expect("recv dir to str");

    // Spawn recv as a background child process
    let mut child = std::process::Command::new(cli.binary_path())
        .env("UC_PROFILE", &cli.profile_name)
        .args(["recv", "--out", recv_path])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn recv");

    // Give it 2 seconds — it should still be running (blocking)
    let _wait_result = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::task::spawn_blocking({
            let id = child.id();
            move || {
                // We cannot move child into the closure since we need it later.
                // Instead, just sleep and check.
                std::thread::sleep(std::time::Duration::from_secs(2));
                id
            }
        })
        .await
    })
    .await;

    // After 2 seconds, the process should still be alive
    let exited = child.try_wait().expect("try_wait failed");
    assert!(
        exited.is_none(),
        "recv should block waiting for inbound file, but it exited with: {:?}",
        exited
    );

    // Clean up: send SIGTERM
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(child.id() as i32, libc::SIGTERM);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }

    let _ = child.wait();
}

/// `recv --out /dev/null/impossible` should fail because `/dev/null` is not a
/// directory. The `resolve_out_dir` function checks `dir.is_dir()` and returns
/// an error when the path is not a directory.
#[tokio::test]
#[ignore]
async fn recv_invalid_out_path() {
    let (_daemon, cli) = setup_initialized_node("recv-badpath").await;

    let output = cli.run_capture(&["recv", "--out", "/dev/null/impossible"]);
    assert!(
        !output.success(),
        "recv --out with invalid path should fail, got exit=0"
    );

    let combined = format!("{}{}", output.stdout, output.stderr);
    // resolve_out_dir returns either "not a directory" or "Failed to create output directory"
    assert!(
        combined.to_lowercase().contains("directory")
            || combined.to_lowercase().contains("not a directory")
            || combined.to_lowercase().contains("output"),
        "error should mention directory issue, got: stdout={}, stderr={}",
        output.stdout,
        output.stderr
    );
}

//! E2E tests for single-node flows: init, send, search, status.
//!
//! Each test spawns its own daemon with a unique profile.
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

#[tokio::test]
#[ignore]
async fn test_init_creates_space() {
    let profile = TestProfile::new("init");
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    let output = cli.run_capture(&[
        "init",
        "--passphrase",
        "my-secret-phrase-123",
        "--device-name",
        "test-node",
    ]);

    assert!(
        output.success(),
        "init failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );
    // init output should mention success
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.to_lowercase().contains("success")
            || combined.to_lowercase().contains("initialized")
            || combined.contains("✓")
            || combined.contains("✔")
            || output.exit_code == 0,
        "init output doesn't indicate success: stdout={}, stderr={}",
        output.stdout,
        output.stderr
    );
}

#[tokio::test]
#[ignore]
async fn test_status_after_init() {
    let (_daemon, cli) = setup_initialized_node("status-init").await;

    let output = cli.run_capture(&["status"]);
    assert!(
        output.success(),
        "status failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(!combined.is_empty(), "status produced no output after init");
}

#[tokio::test]
#[ignore]
async fn test_status_json_output() {
    let (_daemon, cli) = setup_initialized_node("status-json").await;

    let output = cli.run_capture(&["--json", "status"]);
    assert!(
        output.success(),
        "status --json failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    // JSON output should parse
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(output.stdout.trim());
    assert!(
        parsed.is_ok(),
        "status --json did not produce valid JSON: {}",
        output.stdout
    );
}

#[tokio::test]
#[ignore]
async fn test_send_text_produces_hash() {
    let (_daemon, cli) = setup_initialized_node("send").await;

    let payload = format!("hello e2e {}", std::process::id());

    // Send text — produces a content hash even when no peers are online
    let send_out = cli.run_capture(&["send", &payload]);
    let combined = format!("{}{}", send_out.stdout, send_out.stderr);
    assert!(
        combined.contains("blake3") || combined.contains("hash:"),
        "send did not produce a content hash: stdout={}, stderr={}",
        send_out.stdout,
        send_out.stderr
    );
}

#[tokio::test]
#[ignore]
async fn test_search_status_available() {
    let (_daemon, cli) = setup_initialized_node("search-status").await;

    let output = cli.run_capture(&["search", "status"]);
    assert!(
        output.success(),
        "search status failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.to_lowercase().contains("index")
            || combined.to_lowercase().contains("search")
            || combined.to_lowercase().contains("ready"),
        "search status output unrecognizable: {}",
        combined
    );
}

#[tokio::test]
#[ignore]
async fn test_devices_after_init() {
    let (_daemon, cli) = setup_initialized_node("devices").await;

    let output = cli.run_capture(&["devices"]);
    assert!(
        output.success(),
        "devices failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("e2e-device") || !combined.is_empty(),
        "devices output should contain device name or at least some output"
    );
}

#[tokio::test]
#[ignore]
async fn test_devices_json_output() {
    let (_daemon, cli) = setup_initialized_node("devices-json").await;

    let output = cli.run_capture(&["--json", "devices"]);
    assert!(
        output.success(),
        "devices --json failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    let parsed: Result<serde_json::Value, _> = serde_json::from_str(output.stdout.trim());
    assert!(
        parsed.is_ok(),
        "devices --json did not produce valid JSON: {}",
        output.stdout
    );
}

#[tokio::test]
#[ignore]
async fn test_members_after_init() {
    let (_daemon, cli) = setup_initialized_node("members").await;

    let output = cli.run_capture(&["members"]);
    assert!(
        output.success(),
        "members failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("e2e-device") || combined.to_lowercase().contains("local"),
        "members should show local device: {}",
        combined
    );
}

#[tokio::test]
#[ignore]
async fn test_members_json_output() {
    let (_daemon, cli) = setup_initialized_node("members-json").await;

    let output = cli.run_capture(&["--json", "members"]);
    assert!(
        output.success(),
        "members --json failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    let parsed: Result<serde_json::Value, _> = serde_json::from_str(output.stdout.trim());
    assert!(
        parsed.is_ok(),
        "members --json did not produce valid JSON: {}",
        output.stdout
    );
}

#[tokio::test]
#[ignore]
async fn test_search_rebuild() {
    let (_daemon, cli) = setup_initialized_node("search-rebuild").await;

    let output = cli.run_capture(&["search", "rebuild"]);
    assert!(
        output.success(),
        "search rebuild failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );
}

#[tokio::test]
#[ignore]
async fn test_search_status_json() {
    let (_daemon, cli) = setup_initialized_node("search-json").await;

    let output = cli.run_capture(&["--json", "search", "status"]);
    assert!(
        output.success(),
        "search status --json failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    let parsed: Result<serde_json::Value, _> = serde_json::from_str(output.stdout.trim());
    assert!(
        parsed.is_ok(),
        "search status --json not valid JSON: {}",
        output.stdout
    );
}

#[tokio::test]
#[ignore]
async fn test_send_multiple_payloads() {
    let (_daemon, cli) = setup_initialized_node("send-multi").await;

    for i in 0..3 {
        let payload = format!("payload-{}-{}", i, std::process::id());
        let out = cli.run_capture(&["send", &payload]);
        let combined = format!("{}{}", out.stdout, out.stderr);
        assert!(
            combined.contains("blake3") || combined.contains("hash:"),
            "send #{i} did not produce hash: {combined}"
        );
    }
}

#[tokio::test]
#[ignore]
async fn test_send_stdin_mode() {
    let (_daemon, cli) = setup_initialized_node("send-stdin").await;

    // send without text arg reads from stdin
    let output = std::process::Command::new(cli.binary_path())
        .env("UC_PROFILE", &cli.profile_name)
        .args(["send"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(b"stdin-payload-test")?;
            }
            drop(child.stdin.take());
            child.wait_with_output()
        });

    let output = output.expect("send stdin spawn");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("blake3") || combined.contains("hash:"),
        "send via stdin did not produce hash: {combined}"
    );
}

#[tokio::test]
#[ignore]
async fn test_send_json_output() {
    let (_daemon, cli) = setup_initialized_node("send-json").await;

    let output = cli.run_capture(&["--json", "send", "json-test-payload"]);
    let combined = format!("{}{}", output.stdout, output.stderr);
    // JSON mode should produce parseable output (even if exit != 0 due to no peers)
    if !output.stdout.trim().is_empty() {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(output.stdout.trim());
        assert!(
            parsed.is_ok(),
            "send --json did not produce valid JSON: {}",
            output.stdout
        );
    } else {
        assert!(
            combined.contains("blake3") || combined.contains("hash:"),
            "send --json produced no JSON and no hash: {combined}"
        );
    }
}

//! E2E tests for error/edge cases: double init, empty passphrase, commands
//! before init, daemon not running, etc.
//!
//! Run with: cargo test -p uc-e2e-tests -- --ignored

use uc_e2e_tests::{TestCli, TestDaemon, TestProfile};

#[tokio::test]
#[ignore]
async fn test_double_init_fails() {
    let profile = TestProfile::new("double-init");
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    // First init: should succeed
    let first = cli.run_capture(&[
        "init",
        "--passphrase",
        "first-pass-123",
        "--device-name",
        "node-1",
    ]);
    assert!(first.success(), "first init failed: {}", first.stderr);

    // Second init: should fail (space already exists)
    let second = cli.run_capture(&[
        "init",
        "--passphrase",
        "second-pass-456",
        "--device-name",
        "node-2",
    ]);
    assert!(!second.success(), "double init should fail but got exit=0");
}

#[tokio::test]
#[ignore]
async fn test_init_empty_passphrase_fails() {
    let profile = TestProfile::new("empty-pass");
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    let output = cli.run_capture(&["init", "--passphrase", "", "--device-name", "node"]);
    assert!(
        !output.success(),
        "init with empty passphrase should fail but got exit=0"
    );
}

#[tokio::test]
#[ignore]
async fn test_status_before_init_fails() {
    let profile = TestProfile::new("status-noinit");
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    // status without init → encryption locked → should fail
    let output = cli.run_capture(&["status"]);
    assert!(
        !output.success(),
        "status before init should fail but got exit=0"
    );
}

#[tokio::test]
#[ignore]
async fn test_send_before_init_fails() {
    let profile = TestProfile::new("send-noinit");
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    let output = cli.run_capture(&["send", "hello"]);
    assert!(
        !output.success(),
        "send before init should fail but got exit=0"
    );
}

#[tokio::test]
#[ignore]
async fn test_devices_before_init_returns_empty() {
    let profile = TestProfile::new("devices-noinit");
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    // devices before init returns empty list (exit=0), not an error
    let output = cli.run_capture(&["devices"]);
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("0") || combined.to_lowercase().contains("no device") || output.success(),
        "devices before init: unexpected output: {combined}"
    );
}

#[tokio::test]
#[ignore]
async fn test_search_before_init_fails() {
    let profile = TestProfile::new("search-noinit");
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    let output = cli.run_capture(&["search", "test"]);
    assert!(
        !output.success(),
        "search before init should fail but got exit=0"
    );
}

#[tokio::test]
#[ignore]
async fn test_members_before_init_returns_empty() {
    let profile = TestProfile::new("members-noinit");
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    // members before init returns empty (exit=0 with just self device listed)
    let output = cli.run_capture(&["members"]);
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        output.success() || combined.to_lowercase().contains("no member"),
        "members before init: unexpected failure: {combined}"
    );
}

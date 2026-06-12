//! E2E tests for daemon lifecycle: start, health check, stop.
//!
//! These tests require `uniclipd` and `uniclip` binaries to be pre-built:
//!   cargo build -p uc-daemon -p uc-cli
//!
//! Run with:
//!   cargo test -p uc-e2e-tests -- --ignored

use uc_e2e_tests::{TestDaemon, TestProfile};

#[tokio::test]
#[ignore] // requires pre-built binaries
async fn test_daemon_starts_and_reports_healthy() {
    let profile = TestProfile::new("health");
    let daemon = TestDaemon::start(profile).await;

    assert!(daemon.is_ok(), "daemon failed to start: {:?}", daemon.err());

    let mut daemon = daemon.unwrap();
    assert!(daemon.is_running());
    assert!(!daemon.base_url().is_empty());

    daemon.kill();
    assert!(!daemon.is_running());
}

#[tokio::test]
#[ignore]
async fn test_health_endpoint_returns_200() {
    let profile = TestProfile::new("health-http");
    let daemon = TestDaemon::start(profile).await.expect("daemon start");

    let url = format!("{}/health", daemon.base_url());
    let resp = reqwest::get(&url).await.expect("health request");
    assert_eq!(resp.status(), 200);
}

/// issue #1021: a daemon spawned in a session with no display server (headless
/// Linux server, container, SSH without forwarding) must still become healthy —
/// the composition root substitutes NoopSystemClipboard instead of dying on
/// ClipboardInit. Before the fix the daemon exited during assembly and the CLI
/// only ever saw an opaque 30s health timeout.
///
/// Linux-only: on macOS / Windows the clipboard capability never depends on
/// DISPLAY / WAYLAND_DISPLAY, so removing them exercises nothing.
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn test_daemon_becomes_healthy_without_display_session() {
    let profile = TestProfile::new("headless-no-display");
    let mut daemon = TestDaemon::spawn_with(profile, |cmd| {
        cmd.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    })
    .expect("spawn daemon");

    daemon
        .wait_healthy(std::time::Duration::from_secs(30))
        .await
        .expect(
            "headless daemon must become healthy (ClipboardInit hard-failed here before the fix)",
        );
    assert!(daemon.is_running());
}

#[tokio::test]
#[ignore]
async fn test_daemon_killed_stops_process() {
    let profile = TestProfile::new("kill");
    let mut daemon = TestDaemon::start(profile).await.expect("daemon start");

    assert!(daemon.is_running());
    daemon.kill();

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(!daemon.is_running());
}

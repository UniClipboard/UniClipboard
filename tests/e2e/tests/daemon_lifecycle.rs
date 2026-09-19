//! E2E tests for daemon lifecycle: start, health check, stop.
//!
//! These tests require `uniclipd` and `uniclip` binaries to be pre-built:
//!   cargo build -p uc-daemon -p uc-cli
//!
//! Run with:
//!   cargo test -p uc-e2e-tests -- --ignored

use std::process::{Command, Stdio};
use std::time::Duration;

use uc_e2e_tests::{TestCli, TestDaemon, TestProfile};

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

/// A temporarily unresponsive daemon is still the profile's incumbent. `recv`
/// must wait for and reuse it instead of spawning a same-version contender,
/// whose single-instance arbitration would terminate the original daemon.
#[cfg(unix)]
#[tokio::test]
#[ignore]
async fn recv_reuses_live_daemon_after_transient_health_timeout() {
    let profile = TestProfile::new("recv-reuses-live-daemon");
    let mut daemon = TestDaemon::start(profile).await.expect("daemon start");
    let cli = TestCli::new(&daemon.profile);
    let init = cli.run_capture(&[
        "init",
        "--passphrase",
        "recv-reuse-test-pass",
        "--device-name",
        "recv-reuse-node",
    ]);
    assert!(init.success(), "init failed: {}", init.stderr);

    daemon.suspend().expect("suspend incumbent daemon");
    let out_dir = tempfile::tempdir().expect("receive output directory");
    let mut recv = Command::new(cli.binary_path())
        .env("UC_PROFILE", &cli.profile_name)
        .env("UNICLIPBOARD_ENV", "development")
        .arg("recv")
        .arg("--out")
        .arg(out_dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn recv");

    // Cross the CLI's two-second health-probe timeout. Before the fix this
    // classified the live incumbent as absent and spawned a contender.
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    daemon.resume().expect("resume incumbent daemon");
    daemon
        .wait_healthy(Duration::from_secs(10))
        .await
        .expect("incumbent should become healthy again");
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        daemon.is_running(),
        "recv must not terminate the incumbent daemon"
    );
    assert!(
        recv.try_wait().expect("read recv state").is_none(),
        "recv should connect to the resumed daemon and wait for an inbound file"
    );

    let _ = recv.kill();
    let _ = recv.wait();
}

/// Foreground `start` is also a reuse-or-spawn entry. A live daemon that is
/// temporarily unresponsive must be reused after recovery, not replaced by a
/// new foreground contender.
#[cfg(unix)]
#[tokio::test]
#[ignore]
async fn foreground_start_reuses_live_daemon_after_transient_health_timeout() {
    let profile = TestProfile::new("foreground-start-reuses-live-daemon");
    let mut daemon = TestDaemon::start(profile).await.expect("daemon start");
    let cli = TestCli::new(&daemon.profile);
    let init = cli.run_capture(&[
        "init",
        "--passphrase",
        "foreground-start-reuse-test-pass",
        "--device-name",
        "foreground-start-reuse-node",
    ]);
    assert!(init.success(), "init failed: {}", init.stderr);

    daemon.suspend().expect("suspend incumbent daemon");
    let child = Command::new(cli.binary_path())
        .env("UC_PROFILE", &cli.profile_name)
        .env("UNICLIPBOARD_ENV", "development")
        .args(["start", "--foreground"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn foreground start");

    tokio::time::sleep(Duration::from_millis(2_500)).await;
    daemon.resume().expect("resume incumbent daemon");
    daemon
        .wait_healthy(Duration::from_secs(10))
        .await
        .expect("incumbent should become healthy again");
    let output = child.wait_with_output().expect("wait for foreground start");

    assert!(
        output.status.success(),
        "foreground start failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        daemon.is_running(),
        "foreground start must not terminate the incumbent daemon"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("already running"),
        "foreground start should report that it reused the incumbent: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// Background `start` must use the same live-incumbent protection at its final
/// promote-or-spawn decision, not only during the preceding setup check.
#[cfg(unix)]
#[tokio::test]
#[ignore]
async fn background_start_reuses_live_daemon_after_transient_health_timeout() {
    let profile = TestProfile::new("background-start-reuses-live-daemon");
    let mut daemon = TestDaemon::start(profile).await.expect("daemon start");
    let cli = TestCli::new(&daemon.profile);
    let init = cli.run_capture(&[
        "init",
        "--passphrase",
        "background-start-reuse-test-pass",
        "--device-name",
        "background-start-reuse-node",
    ]);
    assert!(init.success(), "init failed: {}", init.stderr);

    daemon.suspend().expect("suspend incumbent daemon");
    let child = Command::new(cli.binary_path())
        .env("UC_PROFILE", &cli.profile_name)
        .env("UNICLIPBOARD_ENV", "development")
        .arg("start")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn background start");

    tokio::time::sleep(Duration::from_millis(2_500)).await;
    daemon.resume().expect("resume incumbent daemon");
    daemon
        .wait_healthy(Duration::from_secs(10))
        .await
        .expect("incumbent should become healthy again");
    let output = child.wait_with_output().expect("wait for background start");

    assert!(
        output.status.success(),
        "background start failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        daemon.is_running(),
        "background start must not terminate the incumbent daemon"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("already running"),
        "background start should report that it reused the incumbent: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// With no daemon and no initialized Space, `recv` keeps its established
/// behavior: start a transient daemon, report the setup error, then let the
/// transient daemon reclaim itself.
#[tokio::test]
#[ignore]
async fn recv_without_daemon_preserves_setup_error_and_transient_cleanup() {
    let profile = TestProfile::new("recv-no-daemon");
    profile.cleanup();
    let cli = TestCli::new(&profile);

    let output = cli.run_capture(&["recv"]);
    assert_ne!(output.exit_code, 0, "recv must fail before Space setup");
    assert!(
        output.stderr.contains("No space on this profile"),
        "unexpected recv error: {}",
        output.stderr
    );

    let conn_path = profile.data_dir().join("daemon.conn");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while conn_path.exists() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        !conn_path.exists(),
        "the transient daemon should exit after recv reports the setup error"
    );
}

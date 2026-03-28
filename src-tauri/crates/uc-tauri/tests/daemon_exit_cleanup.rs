//! Canonical regression target for Phase 46.6 daemon exit cleanup work.
//!
//! Run with:
//! `cd src-tauri && cargo test -p uc-tauri --test daemon_exit_cleanup -- --test-threads=1`
//! `cd src-tauri && cargo test -p uc-tauri --test daemon_bootstrap_contract -- --test-threads=1`
//!
//! Note (Phase 68): Tests that required constructing a real CommandChild (which is only
//! possible inside a Tauri runtime) have been removed. The daemon spawn/shutdown path
//! is verified at integration-test time via `bun tauri dev` end-to-end verification.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use uc_daemon::api::types::HealthResponse;
use uc_daemon::DAEMON_API_REVISION;
use uc_tauri::bootstrap::run::ProbeOutcome;
use uc_daemon_client::daemon_lifecycle::GuiOwnedDaemonState;

const EXIT_CLEANUP_COMMAND: &str =
    "cargo test -p uc-tauri --test daemon_exit_cleanup -- --test-threads=1";
const BOOTSTRAP_CONTRACT_COMMAND: &str =
    "cargo test -p uc-tauri --test daemon_bootstrap_contract -- --test-threads=1";
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const CLEANUP_POLL_INTERVAL: Duration = Duration::from_millis(20);

fn compatible_health() -> HealthResponse {
    HealthResponse {
        status: "ok".to_string(),
        package_version: env!("CARGO_PKG_VERSION").to_string(),
        api_revision: DAEMON_API_REVISION.to_string(),
    }
}

fn assert_canonical_commands() {
    assert_eq!(EXIT_CLEANUP_COMMAND, "cargo test -p uc-tauri --test daemon_exit_cleanup -- --test-threads=1");
    assert_eq!(BOOTSTRAP_CONTRACT_COMMAND, "cargo test -p uc-tauri --test daemon_bootstrap_contract -- --test-threads=1");
}

fn main_rs_source() -> String {
    let main_rs = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../src/main.rs")
        .canonicalize()
        .expect("resolve src/main.rs path");
    fs::read_to_string(main_rs).expect("read src/main.rs")
}

fn close_requested_block(source: &str) -> &str {
    let start = source
        .find("if let tauri::WindowEvent::CloseRequested { api, .. } = event {")
        .expect("close request block should exist");
    let end = source[start..]
        .find("info!(\"Main window hidden to tray\");")
        .map(|offset| start + offset)
        .expect("close request block should log tray hide");
    &source[start..end]
}

#[tokio::test]
async fn daemon_exit_cleanup_skips_when_gui_did_not_spawn_daemon() {
    let probe_outcome = ProbeOutcome::Compatible(compatible_health());
    let state = GuiOwnedDaemonState::default();

    assert_canonical_commands();
    assert!(matches!(probe_outcome, ProbeOutcome::Compatible(_)));

    let cleaned = state
        .shutdown_owned_daemon(CLEANUP_TIMEOUT, CLEANUP_POLL_INTERVAL)
        .await
        .expect("compatible existing daemon should be a cleanup no-op");

    assert!(!cleaned);
    assert_eq!(state.snapshot_pid(), None);
}

#[tokio::test]
async fn daemon_exit_cleanup_is_idempotent_across_repeated_exit_requests() {
    let state = GuiOwnedDaemonState::default();

    // With no owned child, begin_exit_cleanup can toggle
    assert!(state.begin_exit_cleanup());
    assert!(!state.begin_exit_cleanup());

    // shutdown_owned_daemon is a no-op when no child is registered
    let cleaned = state
        .shutdown_owned_daemon(CLEANUP_TIMEOUT, CLEANUP_POLL_INTERVAL)
        .await
        .expect("no-owned-child cleanup should succeed");
    assert!(!cleaned);

    state.finish_exit_cleanup();
    assert!(state.begin_exit_cleanup());

    let second_cleaned = state
        .shutdown_owned_daemon(CLEANUP_TIMEOUT, CLEANUP_POLL_INTERVAL)
        .await
        .expect("second exit cleanup should be a no-op");
    assert!(!second_cleaned);

    state.finish_exit_cleanup();
}

#[test]
fn main_window_close_request_hides_to_tray_without_daemon_cleanup() {
    let source = main_rs_source();
    let close_block = close_requested_block(&source);

    assert!(source.contains("WindowEvent::CloseRequested"));
    assert!(close_block.contains("api.prevent_close();"));
    assert!(close_block.contains("let _ = window.hide();"));
    assert!(!close_block.contains("shutdown_owned_daemon"));
    assert!(!close_block.contains("terminate_local_daemon_pid"));
}

fn process_is_not_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run kill -0");
        !status.success()
    }

    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}")])
            .output()
            .expect("run tasklist");
        let stdout = String::from_utf8_lossy(&output.stdout);
        !stdout.contains(&pid.to_string())
    }
}

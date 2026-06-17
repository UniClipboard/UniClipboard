//! E2E tests for the `uniclip debug` command group.
//!
//! Covers the persistent debug-mode flag (`status` / `on` / `off`) and the
//! `export-logs` archive, exercising the full CLI -> daemon HTTP -> facade path.
//! These flows are space-independent: the diagnostics endpoints only read/write
//! settings and the logs directory, so no `init` is required.
//!
//! Each test spawns its own daemon with a unique profile.
//! Run with: cargo test -p uc-e2e-tests -- --ignored

use uc_e2e_tests::{TestCli, TestDaemon, TestProfile};

/// Helper: start a daemon (no space init) and return (daemon, cli).
async fn start_daemon(name: &str) -> (TestDaemon, TestCli) {
    let profile = TestProfile::new(name);
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);
    (daemon, cli)
}

#[tokio::test]
#[ignore]
async fn test_debug_status_default() {
    let (_daemon, cli) = start_daemon("debug-status").await;

    let output = cli.run_capture(&["debug", "status"]);
    assert!(
        output.success(),
        "debug status failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    // Human-readable output should report the debug flag; default is off.
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.to_lowercase().contains("debug"),
        "debug status output doesn't mention debug: {combined}"
    );
}

#[tokio::test]
#[ignore]
async fn test_debug_status_json_default() {
    let (_daemon, cli) = start_daemon("debug-status-json").await;

    let output = cli.run_capture(&["--json", "debug", "status"]);
    assert!(
        output.success(),
        "debug status --json failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    let parsed: serde_json::Value =
        serde_json::from_str(output.stdout.trim()).unwrap_or_else(|e| {
            panic!(
                "debug status --json not valid JSON ({e}): {}",
                output.stdout
            )
        });

    // Fresh daemon: debug mode off, no restart pending, profile name populated.
    assert_eq!(
        parsed.get("debugMode").and_then(|v| v.as_bool()),
        Some(false),
        "expected debugMode=false on a fresh daemon: {parsed}"
    );
    assert_eq!(
        parsed.get("restartRequired").and_then(|v| v.as_bool()),
        Some(false),
        "status should never request a restart: {parsed}"
    );
    let profile = parsed
        .get("effectiveLogProfile")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        !profile.is_empty(),
        "effectiveLogProfile should be a non-empty string: {parsed}"
    );
}

#[tokio::test]
#[ignore]
async fn test_debug_on_off_round_trip() {
    let (_daemon, cli) = start_daemon("debug-toggle").await;

    // Enable: the update result must flag debugMode=true and a pending restart.
    let on = cli.run_capture(&["--json", "debug", "on"]);
    assert!(
        on.success(),
        "debug on failed (exit={}): {}",
        on.exit_code,
        on.stderr
    );
    let on_json: serde_json::Value = serde_json::from_str(on.stdout.trim())
        .unwrap_or_else(|e| panic!("debug on --json not valid JSON ({e}): {}", on.stdout));
    assert_eq!(
        on_json.get("debugMode").and_then(|v| v.as_bool()),
        Some(true),
        "debug on should report debugMode=true: {on_json}"
    );
    assert_eq!(
        on_json.get("restartRequired").and_then(|v| v.as_bool()),
        Some(true),
        "toggling debug mode should require a restart: {on_json}"
    );

    // The persisted flag should be visible on the next status read.
    let after_on = cli.run_capture(&["--json", "debug", "status"]);
    assert!(
        after_on.success(),
        "status after debug on failed (exit={}): {}",
        after_on.exit_code,
        after_on.stderr
    );
    let after_on_json: serde_json::Value = serde_json::from_str(after_on.stdout.trim())
        .unwrap_or_else(|e| panic!("status --json not valid JSON ({e}): {}", after_on.stdout));
    assert_eq!(
        after_on_json.get("debugMode").and_then(|v| v.as_bool()),
        Some(true),
        "status should reflect debug mode enabled: {after_on_json}"
    );

    // Disable: status must return to debugMode=false.
    let off = cli.run_capture(&["--json", "debug", "off"]);
    assert!(
        off.success(),
        "debug off failed (exit={}): {}",
        off.exit_code,
        off.stderr
    );
    let off_json: serde_json::Value = serde_json::from_str(off.stdout.trim())
        .unwrap_or_else(|e| panic!("debug off --json not valid JSON ({e}): {}", off.stdout));
    assert_eq!(
        off_json.get("debugMode").and_then(|v| v.as_bool()),
        Some(false),
        "debug off should report debugMode=false: {off_json}"
    );

    let after_off = cli.run_capture(&["--json", "debug", "status"]);
    assert!(
        after_off.success(),
        "status after debug off failed (exit={}): {}",
        after_off.exit_code,
        after_off.stderr
    );
    let after_off_json: serde_json::Value = serde_json::from_str(after_off.stdout.trim())
        .unwrap_or_else(|e| panic!("status --json not valid JSON ({e}): {}", after_off.stdout));
    assert_eq!(
        after_off_json.get("debugMode").and_then(|v| v.as_bool()),
        Some(false),
        "status should reflect debug mode disabled: {after_off_json}"
    );
}

#[tokio::test]
#[ignore]
async fn test_debug_export_logs_creates_zip() {
    let (_daemon, cli) = start_daemon("debug-export").await;

    let output = cli.run_capture(&["--json", "debug", "export-logs", "--since-hours", "1"]);
    assert!(
        output.success(),
        "debug export-logs failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    let parsed: serde_json::Value = serde_json::from_str(output.stdout.trim())
        .unwrap_or_else(|e| panic!("export-logs --json not valid JSON ({e}): {}", output.stdout));
    let path = parsed
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("export-logs --json missing string path: {parsed}"))
        .to_owned();

    // The export writes to the real Downloads directory. Register cleanup via an
    // RAII guard so the archive is removed even if an assertion below panics.
    struct ZipCleanup(std::path::PathBuf);
    impl Drop for ZipCleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _cleanup = ZipCleanup(std::path::PathBuf::from(&path));

    assert!(
        path.ends_with(".zip"),
        "exported archive should be a .zip: {path}"
    );
    assert!(
        std::path::Path::new(&path).exists(),
        "export-logs reported {path} but the file does not exist"
    );
}

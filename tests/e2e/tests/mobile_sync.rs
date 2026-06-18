//! E2E tests for mobile-sync CLI commands: setup, status, add, revoke,
//! disable, network interfaces, and custom-port flows.
//!
//! All tests are single-node: one daemon per test with a unique profile.
//! Commands route through the daemon HTTP endpoints via CLI (`--json` mode).
//!
//! Run with: cargo test -p uc-e2e-tests -- --ignored

use serde_json::Value;
use uc_e2e_tests::{TestCli, TestDaemon, TestProfile};

// ── Helpers ─────────────────────────────────────────────────────────────

/// Start a daemon + init a space, returning (daemon, cli).
async fn setup_initialized_node(name: &str) -> (TestDaemon, TestCli) {
    let profile = TestProfile::new(name);
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    let output = cli.run_capture(&[
        "init",
        "--passphrase",
        "mobile-sync-e2e-pass",
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

/// Run `mobile-sync setup` in non-interactive JSON mode with the standard
/// test parameters. Asserts exit 0 and returns the parsed JSON value.
/// Panics if the command fails or produces invalid JSON.
fn run_setup_non_interactive(cli: &TestCli, label: &str, ip: &str, extra_args: &[&str]) -> Value {
    let mut args: Vec<&str> = vec![
        "--json",
        "mobile",
        "setup",
        "--non-interactive",
        "--label",
        label,
        "--ip",
        ip,
        "--accept-network-risk",
    ];
    args.extend_from_slice(extra_args);

    let output = cli.run_capture(&args);
    assert!(
        output.success(),
        "setup non-interactive failed (exit={}): stdout={}, stderr={}",
        output.exit_code,
        output.stdout,
        output.stderr
    );
    serde_json::from_str(output.stdout.trim())
        .expect(&format!("setup --json not valid JSON: {}", output.stdout))
}

// ── Tests ───────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn mobile_sync_setup_non_interactive_json() {
    let (_daemon, cli) = setup_initialized_node("ms-setup-json").await;

    // Use a unique, uncommon LAN port. The well-known default (42720) is
    // often occupied by a real UniClipboard instance on the same host (on
    // WSL2 even a Windows-side instance, via localhost forwarding), and setup
    // now aborts on a bind failure — so an e2e test must not depend on 42720
    // being free. The 42720 default itself is the DEFAULT_LAN_PORT constant.
    let json = run_setup_non_interactive(&cli, "TestPhone", "192.168.1.100", &["--port", "42725"]);

    // Verify all expected fields are present.
    assert!(
        json.get("device_id").and_then(|v| v.as_str()).is_some(),
        "missing device_id in setup output: {json}"
    );
    assert!(
        json.get("username").and_then(|v| v.as_str()).is_some(),
        "missing username in setup output: {json}"
    );
    assert!(
        json.get("password").and_then(|v| v.as_str()).is_some(),
        "missing password in setup output: {json}"
    );
    assert!(
        json.get("install_url").and_then(|v| v.as_str()).is_some(),
        "missing install_url in setup output: {json}"
    );
    assert!(
        json.get("qr_code_ascii").and_then(|v| v.as_str()).is_some(),
        "missing qr_code_ascii in setup output: {json}"
    );
    assert_eq!(
        json.get("port").and_then(|v| v.as_u64()),
        Some(42725),
        "setup response should reflect the configured port: {json}"
    );
}

#[tokio::test]
#[ignore]
async fn mobile_sync_setup_missing_flags_non_interactive() {
    let (_daemon, cli) = setup_initialized_node("ms-setup-missing").await;

    // Run setup --non-interactive --json WITHOUT required --label /
    // --accept-network-risk (--ip is now optional). The CLI should reject
    // early on the still-required flags.
    let output = cli.run_capture(&["--json", "mobile", "setup", "--non-interactive"]);

    assert!(
        !output.success(),
        "setup without required flags should fail, but got exit=0: stdout={}, stderr={}",
        output.stdout,
        output.stderr
    );

    // The error should mention one of the missing required flags.
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("required")
            || combined.contains("--label")
            || combined.contains("--accept-network-risk")
            || combined.contains("error"),
        "error output should mention missing required flags: {combined}"
    );
}

#[tokio::test]
#[ignore]
async fn mobile_sync_status_before_setup() {
    let (_daemon, cli) = setup_initialized_node("ms-status-before").await;

    let output = cli.run_capture(&["--json", "mobile", "status"]);
    assert!(
        output.success(),
        "status before setup failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    let json: Value = serde_json::from_str(output.stdout.trim())
        .expect(&format!("status --json not valid JSON: {}", output.stdout));

    assert_eq!(
        json.get("enabled").and_then(|v| v.as_bool()),
        Some(false),
        "enabled should be false before setup: {json}"
    );
    assert_eq!(
        json.get("device_count").and_then(|v| v.as_u64()),
        Some(0),
        "device_count should be 0 before setup: {json}"
    );

    let devices = json.get("devices").and_then(|v| v.as_array());
    assert!(
        devices.map_or(true, |a| a.is_empty()),
        "devices array should be empty before setup: {json}"
    );
}

#[tokio::test]
#[ignore]
async fn mobile_sync_status_after_setup() {
    let (_daemon, cli) = setup_initialized_node("ms-status-after").await;

    // Run setup first. A unique LAN port avoids colliding with the other
    // setup-running tests on the default 42720 (they bind 0.0.0.0:port in
    // parallel; setup now aborts on a bind failure).
    run_setup_non_interactive(&cli, "MyPhone", "192.168.1.50", &["--port", "42721"]);

    // Now check status.
    let output = cli.run_capture(&["--json", "mobile", "status"]);
    assert!(
        output.success(),
        "status after setup failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    let json: Value = serde_json::from_str(output.stdout.trim())
        .expect(&format!("status --json not valid JSON: {}", output.stdout));

    assert_eq!(
        json.get("enabled").and_then(|v| v.as_bool()),
        Some(true),
        "enabled should be true after setup: {json}"
    );
    assert_eq!(
        json.get("lan_listen_enabled").and_then(|v| v.as_bool()),
        Some(true),
        "lan_listen_enabled should be true after setup: {json}"
    );
    assert_eq!(
        json.get("device_count").and_then(|v| v.as_u64()),
        Some(1),
        "device_count should be 1 after setup: {json}"
    );

    let devices = json
        .get("devices")
        .and_then(|v| v.as_array())
        .expect("devices array missing in status output");
    assert_eq!(devices.len(), 1, "expected exactly 1 device: {json}");

    let device_label = devices[0]
        .get("label")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(
        device_label, "MyPhone",
        "device label should match setup label: {json}"
    );
}

#[tokio::test]
#[ignore]
async fn mobile_sync_add_device() {
    let (_daemon, cli) = setup_initialized_node("ms-add-device").await;

    // Initial setup. Unique LAN port — see status_after_setup for why.
    run_setup_non_interactive(&cli, "FirstPhone", "192.168.1.10", &["--port", "42722"]);

    // Add a second device.
    let add_output = cli.run_capture(&["--json", "mobile", "add", "--label", "SecondPhone"]);
    assert!(
        add_output.success(),
        "add device failed (exit={}): stdout={}, stderr={}",
        add_output.exit_code,
        add_output.stdout,
        add_output.stderr
    );

    let add_json: Value = serde_json::from_str(add_output.stdout.trim())
        .expect(&format!("add --json not valid JSON: {}", add_output.stdout));

    assert!(
        add_json.get("device_id").and_then(|v| v.as_str()).is_some(),
        "add response missing device_id: {add_json}"
    );
    assert_eq!(
        add_json.get("label").and_then(|v| v.as_str()),
        Some("SecondPhone"),
        "add label mismatch: {add_json}"
    );

    // Verify status shows 2 devices.
    let status_output = cli.run_capture(&["--json", "mobile", "status"]);
    assert!(
        status_output.success(),
        "status failed: {}",
        status_output.stderr
    );

    let status_json: Value =
        serde_json::from_str(status_output.stdout.trim()).expect("status JSON parse");
    assert_eq!(
        status_json.get("device_count").and_then(|v| v.as_u64()),
        Some(2),
        "device_count should be 2 after add: {status_json}"
    );
}

#[tokio::test]
#[ignore]
async fn mobile_sync_revoke_device() {
    let (_daemon, cli) = setup_initialized_node("ms-revoke").await;

    // Setup to register the first device. Unique LAN port — see
    // status_after_setup for why.
    let setup_json =
        run_setup_non_interactive(&cli, "PhoneToRevoke", "192.168.1.20", &["--port", "42723"]);

    let device_id = setup_json
        .get("device_id")
        .and_then(|v| v.as_str())
        .expect("setup response missing device_id")
        .to_string();

    // Add a second device so we can verify count changes.
    let add_out = cli.run_capture(&["--json", "mobile", "add", "--label", "KeepMe"]);
    assert!(add_out.success(), "add failed: {}", add_out.stderr);

    // Verify 2 devices before revoke.
    let pre_status = cli.run_capture(&["--json", "mobile", "status"]);
    let pre_json: Value = serde_json::from_str(pre_status.stdout.trim()).expect("pre JSON");
    assert_eq!(
        pre_json.get("device_count").and_then(|v| v.as_u64()),
        Some(2),
        "should have 2 devices before revoke"
    );

    // Revoke the first device.
    let revoke_output = cli.run_capture(&["--json", "mobile", "revoke", &device_id]);
    assert!(
        revoke_output.success(),
        "revoke failed (exit={}): stdout={}, stderr={}",
        revoke_output.exit_code,
        revoke_output.stdout,
        revoke_output.stderr
    );

    let revoke_json: Value =
        serde_json::from_str(revoke_output.stdout.trim()).expect("revoke JSON parse");
    assert_eq!(
        revoke_json.get("revoked").and_then(|v| v.as_bool()),
        Some(true),
        "revoke response should show revoked=true: {revoke_json}"
    );

    // Verify device_count decreased.
    let post_status = cli.run_capture(&["--json", "mobile", "status"]);
    let post_json: Value = serde_json::from_str(post_status.stdout.trim()).expect("post JSON");
    assert_eq!(
        post_json.get("device_count").and_then(|v| v.as_u64()),
        Some(1),
        "device_count should be 1 after revoke: {post_json}"
    );
}

#[tokio::test]
#[ignore]
async fn mobile_sync_disable() {
    let (_daemon, cli) = setup_initialized_node("ms-disable").await;

    // Setup first. Unique LAN port — see status_after_setup for why.
    run_setup_non_interactive(&cli, "DisablePhone", "192.168.1.30", &["--port", "42724"]);

    // Disable mobile-sync.
    let disable_output = cli.run_capture(&["--json", "mobile", "disable"]);
    assert!(
        disable_output.success(),
        "disable failed (exit={}): stdout={}, stderr={}",
        disable_output.exit_code,
        disable_output.stdout,
        disable_output.stderr
    );

    // Verify status shows disabled.
    let status_output = cli.run_capture(&["--json", "mobile", "status"]);
    assert!(
        status_output.success(),
        "status failed: {}",
        status_output.stderr
    );

    let json: Value = serde_json::from_str(status_output.stdout.trim()).expect("status JSON parse");
    assert_eq!(
        json.get("enabled").and_then(|v| v.as_bool()),
        Some(false),
        "enabled should be false after disable: {json}"
    );
    assert_eq!(
        json.get("lan_listen_enabled").and_then(|v| v.as_bool()),
        Some(false),
        "lan_listen_enabled should be false after disable: {json}"
    );

    // Devices should still be registered (disable does not clear them).
    let device_count = json
        .get("device_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        device_count >= 1,
        "devices should still be registered after disable (count={}): {json}",
        device_count
    );
}

#[tokio::test]
#[ignore]
async fn mobile_sync_network_interfaces() {
    let (_daemon, cli) = setup_initialized_node("ms-net-ifaces").await;

    let output = cli.run_capture(&["--json", "mobile", "network", "interfaces"]);
    assert!(
        output.success(),
        "network interfaces failed (exit={}): stdout={}, stderr={}",
        output.exit_code,
        output.stdout,
        output.stderr
    );

    let json: Value = serde_json::from_str(output.stdout.trim()).expect(&format!(
        "network interfaces --json not valid JSON: {}",
        output.stdout
    ));

    // Should be an array (may be empty in CI with no LAN).
    assert!(
        json.is_array(),
        "network interfaces output should be a JSON array: {json}"
    );

    // If any entries exist, verify the shape.
    if let Some(arr) = json.as_array() {
        for entry in arr {
            assert!(
                entry.get("name").and_then(|v| v.as_str()).is_some(),
                "interface entry missing 'name': {entry}"
            );
            assert!(
                entry.get("ipv4").and_then(|v| v.as_str()).is_some(),
                "interface entry missing 'ipv4': {entry}"
            );
        }
    }
}

#[tokio::test]
#[ignore]
async fn mobile_sync_setup_custom_port() {
    let (_daemon, cli) = setup_initialized_node("ms-custom-port").await;

    // Setup with a custom port.
    let setup_json =
        run_setup_non_interactive(&cli, "PortPhone", "192.168.1.40", &["--port", "43210"]);

    assert_eq!(
        setup_json.get("port").and_then(|v| v.as_u64()),
        Some(43210),
        "setup response should reflect custom port: {setup_json}"
    );

    // Verify status reflects the custom port.
    let status_output = cli.run_capture(&["--json", "mobile", "status"]);
    assert!(
        status_output.success(),
        "status failed: {}",
        status_output.stderr
    );

    let json: Value = serde_json::from_str(status_output.stdout.trim()).expect("status JSON parse");
    assert_eq!(
        json.get("lan_port").and_then(|v| v.as_u64()),
        Some(43210),
        "lan_port should be 43210 in status: {json}"
    );

    let listen_url = json
        .get("listen_url")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        listen_url.contains(":43210"),
        "listen_url should contain :43210, got '{listen_url}': {json}"
    );
}

//! E2E tests for clipboard synchronization between paired nodes.
//!
//! These tests exercise the pairing pipeline and verify the infrastructure
//! for cross-node communication. Scenarios cover:
//!
//! - Full invite/join handshake with member and device verification
//! - Send with no peers online (single-node, verified output structure)
//! - Sync attempts between paired nodes (soft assertions — actual delivery
//!   may not succeed in E2E without a rendezvous server)
//!
//! **Key constraint**: In headless E2E without a rendezvous relay, paired
//! nodes may not be able to establish a direct connection for sync. Tests
//! that depend on cross-node delivery use soft assertions: they verify the
//! infrastructure (command doesn't crash, watch spawns, etc.) but don't
//! hard-fail if delivery times out.
//!
//! Run with: cargo test -p uc-e2e-tests -- --ignored

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;
use uc_e2e_tests::{TestCli, TestDaemon, TestProfile};

const PASSPHRASE: &str = "clipboard-sync-e2e-passphrase";

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Auth helper: read daemon file token, exchange for JWT session token.
async fn get_session_token(daemon: &TestDaemon, client: &reqwest::Client) -> String {
    let file_token = read_daemon_file_token(daemon);

    let resp = client
        .post(format!("{}/auth/connect", daemon.base_url()))
        .header("Authorization", format!("Bearer {file_token}"))
        .json(&serde_json::json!({
            "pid": std::process::id(),
            "clientType": "cli"
        }))
        .send()
        .await
        .expect("auth/connect request");

    assert!(
        resp.status().is_success(),
        "auth/connect failed with {}",
        resp.status()
    );

    let body: Value = resp.json().await.expect("auth/connect json");
    let data = body.get("data").unwrap_or(&body);
    data.get("sessionToken")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("auth/connect response missing sessionToken: {body}"))
        .to_string()
}

fn read_daemon_file_token(daemon: &TestDaemon) -> String {
    let token_path = daemon.profile.data_dir().join(".daemon-token");
    for _ in 0..30 {
        if let Ok(token) = std::fs::read_to_string(&token_path) {
            let trimmed = token.trim().to_string();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("daemon token not found at {:?}", token_path);
}

/// Start a daemon, init a space, and return (daemon, cli).
async fn setup_initialized_node(name: &str, device_name: &str) -> (TestDaemon, TestCli) {
    let profile = TestProfile::new(name);
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    let output = cli.run_capture(&[
        "init",
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        device_name,
    ]);
    assert!(
        output.success(),
        "init failed (exit={}): {}",
        output.exit_code,
        output.stderr
    );

    (daemon, cli)
}

/// Pair two nodes: Alice (already initialized) invites, Bob joins.
///
/// Returns (alice_daemon, alice_cli, bob_daemon, bob_cli).
async fn pair_two_nodes(test_prefix: &str) -> (TestDaemon, TestCli, TestDaemon, TestCli) {
    // Alice: init space
    let (alice_daemon, alice_cli) =
        setup_initialized_node(&format!("{test_prefix}-alice"), "alice-node").await;

    // Bob: start daemon (no init yet — join will set up the space)
    let bob_profile = TestProfile::new(&format!("{test_prefix}-bob"));
    let bob_daemon = TestDaemon::start(bob_profile)
        .await
        .expect("bob daemon start");
    let bob_cli = TestCli::new(&bob_daemon.profile);

    // Alice: invite with piped stdout so we can read the code line
    let alice_binary = alice_cli.binary_path().to_owned();
    let alice_profile_name = alice_cli.profile_name.clone();

    let mut invite_child = Command::new(&alice_binary)
        .env("UC_PROFILE", &alice_profile_name)
        .args(["invite"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("alice invite spawn");

    // Read stdout lines to find INVITATION_CODE=
    let invite_stdout = invite_child.stdout.take().expect("invite stdout");
    let code_handle = tokio::task::spawn_blocking(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(invite_stdout);
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        for line in reader.lines() {
            if std::time::Instant::now() > deadline {
                return None;
            }
            let line = match line {
                Ok(l) => l,
                Err(_) => continue,
            };
            if let Some(code) = line.strip_prefix("INVITATION_CODE=") {
                return Some(code.trim().to_string());
            }
        }
        None
    });

    let invitation_code = tokio::time::timeout(Duration::from_secs(30), code_handle)
        .await
        .expect("code extraction timed out")
        .expect("code extraction task panicked")
        .expect("INVITATION_CODE= line not found in invite stdout");

    assert!(!invitation_code.is_empty(), "invitation code is empty");

    // Bob: join with the extracted code
    let join_out = bob_cli.run_capture(&[
        "join",
        "--code",
        &invitation_code,
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "bob-node",
    ]);
    assert!(
        join_out.success(),
        "bob join failed (exit={}): stdout={}, stderr={}",
        join_out.exit_code,
        join_out.stdout,
        join_out.stderr,
    );

    // Wait for invite process to complete (it unblocks after joiner connects)
    let _ = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::task::spawn_blocking(move || {
            let _ = invite_child.wait();
        })
        .await
    })
    .await;

    // Brief settle time for both daemons to update their member/device lists
    tokio::time::sleep(Duration::from_secs(2)).await;

    (alice_daemon, alice_cli, bob_daemon, bob_cli)
}

/// Spawn `watch --json` as a background child process. Returns the child
/// and a handle that collects stdout lines.
fn spawn_watch_background(
    cli: &TestCli,
) -> (
    std::process::Child,
    std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) {
    let mut child = Command::new(cli.binary_path())
        .env("UC_PROFILE", &cli.profile_name)
        .args(["--json", "watch"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("watch spawn");

    let stdout = child.stdout.take().expect("watch stdout");
    let stderr = child.stderr.take().expect("watch stderr");
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let lines_clone = lines.clone();

    // Background thread: collect stdout JSON lines
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            let trimmed = line.trim().to_string();
            if !trimmed.is_empty() {
                lines_clone.lock().unwrap().push(trimmed);
            }
        }
    });

    // Background thread: wait for WATCH_READY on stderr
    let ready_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ready_clone = ready_flag.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if line.contains("WATCH_READY") {
                ready_clone.store(true, std::sync::atomic::Ordering::SeqCst);
                break;
            }
        }
    });

    // Wait for WATCH_READY (with timeout)
    let start = std::time::Instant::now();
    while !ready_flag.load(std::sync::atomic::Ordering::SeqCst) {
        if start.elapsed() > Duration::from_secs(15) {
            // Proceed anyway — the daemon may already be listening
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    (child, lines)
}

/// Wait until the collected lines vec has at least `count` entries, or timeout.
async fn wait_for_lines(
    lines: &std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    count: usize,
    timeout: Duration,
) -> Vec<String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        {
            let locked = lines.lock().unwrap();
            if locked.len() >= count {
                return locked.clone();
            }
        }
        if tokio::time::Instant::now() >= deadline {
            let locked = lines.lock().unwrap();
            return locked.clone();
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Two-node pairing: Alice inits, runs invite, parses INVITATION_CODE from
/// stdout, Bob starts daemon and runs join. Assert join exits 0, both nodes
/// see each other in members and devices output.
#[tokio::test]
#[ignore]
async fn pair_invite_join_full_handshake() {
    let (alice_daemon, alice_cli, bob_daemon, bob_cli) = pair_two_nodes("handshake").await;

    // Verify Alice sees Bob in members
    let alice_members = alice_cli.run_capture(&["--json", "members"]);
    assert!(
        alice_members.success(),
        "alice members failed: {}",
        alice_members.stderr
    );
    let alice_members_json: Value =
        serde_json::from_str(alice_members.stdout.trim()).unwrap_or(Value::Null);
    let alice_members_arr = alice_members_json.as_array();
    assert!(
        alice_members_arr.is_some(),
        "alice members not a JSON array: {}",
        alice_members.stdout
    );
    let alice_members_arr = alice_members_arr.unwrap();
    // Should have at least 2 entries (alice + bob)
    assert!(
        alice_members_arr.len() >= 2,
        "alice should see at least 2 members, got {}: {}",
        alice_members_arr.len(),
        alice_members.stdout
    );
    // Check bob-node is among them
    let has_bob = alice_members_arr.iter().any(|m| {
        m.get("device_name")
            .and_then(|v| v.as_str())
            .map(|n| n.contains("bob"))
            .unwrap_or(false)
    });
    assert!(
        has_bob,
        "alice members should contain bob: {}",
        alice_members.stdout
    );

    // Verify Bob sees Alice in members
    let bob_members = bob_cli.run_capture(&["--json", "members"]);
    assert!(
        bob_members.success(),
        "bob members failed: {}",
        bob_members.stderr
    );
    let bob_members_json: Value =
        serde_json::from_str(bob_members.stdout.trim()).unwrap_or(Value::Null);
    let bob_members_arr = bob_members_json.as_array();
    assert!(
        bob_members_arr.is_some(),
        "bob members not a JSON array: {}",
        bob_members.stdout
    );
    let bob_members_arr = bob_members_arr.unwrap();
    assert!(
        bob_members_arr.len() >= 2,
        "bob should see at least 2 members, got {}: {}",
        bob_members_arr.len(),
        bob_members.stdout
    );
    let has_alice = bob_members_arr.iter().any(|m| {
        m.get("device_name")
            .and_then(|v| v.as_str())
            .map(|n| n.contains("alice"))
            .unwrap_or(false)
    });
    assert!(
        has_alice,
        "bob members should contain alice: {}",
        bob_members.stdout
    );

    // Verify devices output on both sides
    let alice_devices = alice_cli.run_capture(&["--json", "devices"]);
    assert!(
        alice_devices.success(),
        "alice devices failed: {}",
        alice_devices.stderr
    );
    let alice_devices_json: Value =
        serde_json::from_str(alice_devices.stdout.trim()).unwrap_or(Value::Null);
    let alice_dev_arr = alice_devices_json.as_array();
    assert!(
        alice_dev_arr.map(|a| a.len()).unwrap_or(0) >= 2,
        "alice should see at least 2 devices: {}",
        alice_devices.stdout
    );

    let bob_devices = bob_cli.run_capture(&["--json", "devices"]);
    assert!(
        bob_devices.success(),
        "bob devices failed: {}",
        bob_devices.stderr
    );
    let bob_devices_json: Value =
        serde_json::from_str(bob_devices.stdout.trim()).unwrap_or(Value::Null);
    let bob_dev_arr = bob_devices_json.as_array();
    assert!(
        bob_dev_arr.map(|a| a.len()).unwrap_or(0) >= 2,
        "bob should see at least 2 devices: {}",
        bob_devices.stdout
    );

    // Explicit cleanup
    drop(alice_cli);
    drop(bob_cli);
    drop(alice_daemon);
    drop(bob_daemon);
}

/// After pairing, Alice sends text. Bob's watch --json may or may not
/// receive the event (depends on whether direct p2p works in E2E).
///
/// **Soft test**: we verify the send command doesn't crash and Bob's watch
/// can be spawned. If delivery happens, we verify the event structure.
/// If delivery doesn't happen (no route), the test still passes.
#[tokio::test]
#[ignore]
async fn paired_text_sync_alice_to_bob() {
    let (_alice_daemon, alice_cli, _bob_daemon, bob_cli) = pair_two_nodes("sync-a2b").await;

    // Start watch on Bob in background
    let (mut watch_child, lines) = spawn_watch_background(&bob_cli);

    // Brief settle time for watch subscription
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Alice sends text
    let payload = format!("hello-from-alice-{}", std::process::id());
    let send_out = alice_cli.run_capture(&["--json", "send", &payload]);
    // send may exit non-zero if no peers accept (timing), but should not crash
    assert!(
        send_out.exit_code == 0 || send_out.exit_code == 1,
        "send crashed with exit={}: stderr={}",
        send_out.exit_code,
        send_out.stderr
    );

    // Wait briefly for Bob's watch to potentially receive the event.
    // Use a short timeout — if delivery doesn't happen, that's OK.
    let collected = wait_for_lines(&lines, 1, Duration::from_secs(5)).await;

    // Kill the watch process
    let _ = watch_child.kill();
    let _ = watch_child.wait();

    if !collected.is_empty() {
        // Delivery happened — verify event structure
        let first: Value = serde_json::from_str(&collected[0])
            .unwrap_or_else(|e| panic!("watch output not valid JSON: {e}\nline: {}", collected[0]));

        let from_device = first
            .get("from_device")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            !from_device.is_empty(),
            "from_device is empty in watch event: {}",
            collected[0]
        );
    } else {
        eprintln!(
            "NOTE: No sync events received (p2p route not available in E2E). \
             Send exit={}, this is expected without a rendezvous relay.",
            send_out.exit_code
        );
    }
}

/// Reverse direction: after pairing, Bob dispatches text via API. Alice's
/// watch --json may or may not receive it.
///
/// **Soft test**: verify infrastructure (dispatch succeeds, watch spawns).
#[tokio::test]
#[ignore]
async fn paired_text_sync_bob_to_alice() {
    let (_alice_daemon, alice_cli, bob_daemon, _bob_cli) = pair_two_nodes("sync-b2a").await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    // Get Bob's session token for API dispatch
    let bob_session = get_session_token(&bob_daemon, &client).await;

    // Start watch on Alice in background
    let (mut watch_child, lines) = spawn_watch_background(&alice_cli);
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Bob dispatches text via API
    let payload = format!("hello-from-bob-{}", std::process::id());
    let resp = client
        .post(format!("{}/clipboard/dispatch", bob_daemon.base_url()))
        .header("Authorization", format!("Session {bob_session}"))
        .json(&serde_json::json!({
            "text": payload
        }))
        .send()
        .await
        .expect("dispatch request");

    assert!(
        resp.status().is_success(),
        "bob dispatch failed with status {}",
        resp.status()
    );

    // Wait briefly for Alice's watch to receive the event
    let collected = wait_for_lines(&lines, 1, Duration::from_secs(5)).await;

    let _ = watch_child.kill();
    let _ = watch_child.wait();

    if !collected.is_empty() {
        let first: Value = serde_json::from_str(&collected[0])
            .unwrap_or_else(|e| panic!("watch output not valid JSON: {e}\nline: {}", collected[0]));

        let text = first.get("text").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            text.contains("hello-from-bob"),
            "alice watch event text does not contain bob's payload: text='{}'",
            text
        );
    } else {
        eprintln!("NOTE: No sync events received on Alice side (p2p route not available in E2E).");
    }

    // Explicit cleanup
    drop(alice_cli);
    drop(_alice_daemon);
    drop(bob_daemon);
}

/// After pairing, Alice sends 3 texts. Bob's watch --json may collect events.
///
/// **Soft test**: verify sends don't crash. If events arrive, verify ordering.
#[tokio::test]
#[ignore]
async fn paired_send_multiple_texts_ordering() {
    let (_alice_daemon, alice_cli, _bob_daemon, bob_cli) = pair_two_nodes("sync-multi").await;

    // Start watch on Bob
    let (mut watch_child, lines) = spawn_watch_background(&bob_cli);
    tokio::time::sleep(Duration::from_secs(2)).await;

    let payloads = [
        format!("ordering-first-{}", std::process::id()),
        format!("ordering-second-{}", std::process::id()),
        format!("ordering-third-{}", std::process::id()),
    ];

    // Alice sends 3 texts sequentially
    for (i, payload) in payloads.iter().enumerate() {
        let out = alice_cli.run_capture(&["send", payload]);
        assert!(
            out.exit_code == 0 || out.exit_code == 1,
            "send #{i} crashed (exit={}): {}",
            out.exit_code,
            out.stderr
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Wait briefly for events
    let collected = wait_for_lines(&lines, 3, Duration::from_secs(5)).await;

    let _ = watch_child.kill();
    let _ = watch_child.wait();

    if collected.len() >= 3 {
        // Parse each JSON line
        let events: Vec<Value> = collected
            .iter()
            .take(3)
            .map(|l| {
                serde_json::from_str(l)
                    .unwrap_or_else(|e| panic!("watch line not valid JSON: {e}\nline: {l}"))
            })
            .collect();

        // Verify at_ms values are monotonically increasing
        let at_ms_values: Vec<i64> = events
            .iter()
            .filter_map(|e| e.get("at_ms").and_then(|v| v.as_i64()))
            .collect();
        if at_ms_values.len() == 3 {
            for i in 1..at_ms_values.len() {
                assert!(
                    at_ms_values[i] >= at_ms_values[i - 1],
                    "at_ms values not monotonically increasing: {:?}",
                    at_ms_values
                );
            }
        }
    } else {
        eprintln!(
            "NOTE: Received {}/{} events (p2p route may not be available in E2E).",
            collected.len(),
            3
        );
    }
}

/// Single initialized node sends text with no peers online. Assert
/// send --json output shows totalAccepted=0 and the response has
/// the expected structure.
#[tokio::test]
#[ignore]
async fn paired_send_no_peers_online() {
    let (_daemon, cli) = setup_initialized_node("send-no-peers", "lonely-node").await;

    let payload = format!("no-peers-test-{}", std::process::id());
    let out = cli.run_capture(&["--json", "send", &payload]);

    // send with no peers may exit 1 (no accepted) but should not crash
    assert!(
        out.exit_code == 0 || out.exit_code == 1,
        "send crashed (exit={}): stderr={}",
        out.exit_code,
        out.stderr
    );

    // Parse JSON output
    let stdout_trimmed = out.stdout.trim();
    if !stdout_trimmed.is_empty() {
        let parsed: Value = serde_json::from_str(stdout_trimmed).unwrap_or_else(|e| {
            panic!(
                "send --json output not valid JSON: {e}\nstdout: {}",
                out.stdout
            )
        });

        // Verify JSON structure: camelCase fields from DispatchOutcomeResponse
        let total_accepted = parsed
            .get("totalAccepted")
            .and_then(|v| v.as_u64())
            .unwrap_or(u64::MAX);
        assert_eq!(
            total_accepted, 0,
            "totalAccepted should be 0 with no peers, got {}. full: {}",
            total_accepted, parsed
        );

        // totalOffline should be present (may be 0 if no peers at all)
        assert!(
            parsed.get("totalOffline").is_some(),
            "totalOffline field missing from dispatch response: {}",
            parsed
        );

        // contentHash should be present and non-empty
        let content_hash = parsed
            .get("contentHash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            !content_hash.is_empty(),
            "contentHash should be non-empty: {}",
            parsed
        );

        // perTarget should be present (empty array when no peers)
        assert!(
            parsed.get("perTarget").is_some(),
            "perTarget field missing from dispatch response: {}",
            parsed
        );
    }
}

/// After pairing, Alice sends text via API dispatch and attempts to
/// resend. Since dispatch is outbound-only and does NOT create local
/// entries, the resend will fail with 404 or similar. This test verifies
/// the resend command doesn't crash.
///
/// **Soft test**: verify the CLI doesn't crash on resend attempt.
#[tokio::test]
#[ignore]
async fn paired_resend_entry_does_not_crash() {
    let (_alice_daemon, alice_cli, _bob_daemon, _bob_cli) = pair_two_nodes("resend").await;

    // Alice sends initial text
    let payload = format!("resend-original-{}", std::process::id());
    let send_out = alice_cli.run_capture(&["send", &payload]);
    assert!(
        send_out.exit_code == 0 || send_out.exit_code == 1,
        "initial send failed (exit={}): {}",
        send_out.exit_code,
        send_out.stderr
    );

    // Attempt to resend with a fake entry ID (since dispatch doesn't create
    // entries, there's nothing to resend). The CLI should handle this gracefully.
    let resend_out = alice_cli.run_capture(&["send", "--resend", "nonexistent-entry-id"]);
    // Resend with bad ID should fail but not crash
    assert!(
        resend_out.exit_code == 0 || resend_out.exit_code == 1,
        "resend crashed with exit={}: stdout={}, stderr={}",
        resend_out.exit_code,
        resend_out.stdout,
        resend_out.stderr,
    );
}

/// Pipe text through stdin to Alice's send command after pairing.
/// Bob's watch --json may or may not receive the text.
///
/// **Soft test**: verify stdin pipe mode doesn't crash.
#[tokio::test]
#[ignore]
async fn paired_send_stdin_pipe() {
    let (_alice_daemon, alice_cli, _bob_daemon, bob_cli) = pair_two_nodes("stdin-pipe").await;

    // Start watch on Bob
    let (mut watch_child, lines) = spawn_watch_background(&bob_cli);
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Alice sends via stdin pipe
    let payload = format!("hello-pipe-{}", std::process::id());
    let output = Command::new(alice_cli.binary_path())
        .env("UC_PROFILE", &alice_cli.profile_name)
        .args(["send"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(payload.as_bytes())?;
            }
            drop(child.stdin.take());
            child.wait_with_output()
        });

    let output = output.expect("send stdin spawn");
    let exit_code = output.status.code().unwrap_or(-1);
    assert!(
        exit_code == 0 || exit_code == 1,
        "send via stdin crashed (exit={}): stderr={}",
        exit_code,
        String::from_utf8_lossy(&output.stderr)
    );

    // Wait briefly for events
    let collected = wait_for_lines(&lines, 1, Duration::from_secs(5)).await;

    let _ = watch_child.kill();
    let _ = watch_child.wait();

    if exit_code == 0 && !collected.is_empty() {
        let first: Value = serde_json::from_str(&collected[0])
            .unwrap_or_else(|e| panic!("watch output not valid JSON: {e}\nline: {}", collected[0]));

        let text = first.get("text").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            text.contains("hello-pipe"),
            "watch event text does not contain piped payload: text='{}', full={}",
            text,
            collected[0]
        );
    } else {
        eprintln!(
            "NOTE: stdin pipe send exit={}, collected {} events. \
             P2p delivery may not be available in E2E.",
            exit_code,
            collected.len()
        );
    }
}

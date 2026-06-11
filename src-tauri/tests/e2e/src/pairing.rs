//! Pairing helpers shared by multi-node E2E tests.
//!
//! `uniclip invite` blocks until a joiner completes (or the invitation
//! expires), printing `INVITATION_CODE=<code>` early on. [`InviteSession`]
//! wraps that lifecycle: spawn with piped stdout, capture the code line,
//! and make sure the process is reaped or killed when the round ends.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::{CapturedOutput, TestCli, TestDaemon, TestProfile};

/// Start a daemon, init a space, and return (daemon, cli).
pub async fn setup_initialized_node(
    name: &str,
    device_name: &str,
    passphrase: &str,
) -> (TestDaemon, TestCli) {
    let profile = TestProfile::new(name);
    let daemon = TestDaemon::start(profile)
        .await
        .expect("daemon start failed");
    let cli = TestCli::new(&daemon.profile);

    let output = cli.run_capture(&[
        "init",
        "--passphrase",
        passphrase,
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

/// A live `uniclip invite` process whose pairing code has been captured.
///
/// Dropping the session kills the process if it is still running, so a
/// panicking test never leaks an orphaned `invite`.
pub struct InviteSession {
    child: Child,
}

impl InviteSession {
    /// Spawn `uniclip invite` for the given CLI profile and block until the
    /// `INVITATION_CODE=` line appears on its stdout (30s deadline).
    pub async fn start(cli: &TestCli) -> (Self, String) {
        let mut child = Command::new(cli.binary_path())
            .env("UC_PROFILE", &cli.profile_name)
            .args(["invite"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("invite spawn");

        let invite_stdout = child.stdout.take().expect("invite stdout");
        // Own the child via the session guard *before* awaiting the code
        // extraction. If the timeout fires (or a later assert panics) the
        // unwind drops `session`, whose `Drop` kills and reaps the process —
        // a bare `Child` would otherwise leak the `invite` as an orphan,
        // since `std::process::Child`'s own `Drop` neither kills nor waits.
        let session = Self { child };

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

        let code = tokio::time::timeout(Duration::from_secs(30), code_handle)
            .await
            .expect("code extraction timed out")
            .expect("code extraction task panicked")
            .expect("INVITATION_CODE= line not found in invite stdout");
        assert!(!code.is_empty(), "invitation code is empty");

        (session, code)
    }

    /// Wait for the invite process to exit (it unblocks once the joiner
    /// completes the handshake). If it overstays the deadline, returning
    /// lets `Drop` kill it.
    pub async fn finish(mut self) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                return; // Drop kills the straggler.
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

impl Drop for InviteSession {
    fn drop(&mut self) {
        // Harmless if the process already exited and was reaped.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Run one invite (sponsor) + join (joiner) round between two running
/// nodes. Returns the join output for the caller to assert on; the
/// sponsor's invite process is reaped (or killed) before returning.
pub async fn invite_join_round(
    sponsor_cli: &TestCli,
    joiner_cli: &TestCli,
    passphrase: &str,
    joiner_device_name: &str,
) -> CapturedOutput {
    let (session, code) = InviteSession::start(sponsor_cli).await;

    let join_out = joiner_cli.run_capture(&[
        "join",
        "--code",
        &code,
        "--passphrase",
        passphrase,
        "--device-name",
        joiner_device_name,
    ]);

    session.finish().await;
    join_out
}

/// Pair two fresh nodes: Alice inits a space and invites, Bob joins.
///
/// Returns (alice_daemon, alice_cli, bob_daemon, bob_cli) with the join
/// asserted successful and a brief settle delay applied.
pub async fn pair_two_nodes(
    test_prefix: &str,
    passphrase: &str,
) -> (TestDaemon, TestCli, TestDaemon, TestCli) {
    // Alice: init space
    let (alice_daemon, alice_cli) =
        setup_initialized_node(&format!("{test_prefix}-alice"), "alice-node", passphrase).await;

    // Bob: start daemon (no init yet — join will set up the space)
    let bob_profile = TestProfile::new(&format!("{test_prefix}-bob"));
    let bob_daemon = TestDaemon::start(bob_profile)
        .await
        .expect("bob daemon start");
    let bob_cli = TestCli::new(&bob_daemon.profile);

    let join_out = invite_join_round(&alice_cli, &bob_cli, passphrase, "bob-node").await;
    assert!(
        join_out.success(),
        "bob join failed (exit={}): stdout={}, stderr={}",
        join_out.exit_code,
        join_out.stdout,
        join_out.stderr,
    );

    // Brief settle time for both daemons to update their member/device lists
    tokio::time::sleep(Duration::from_secs(2)).await;

    (alice_daemon, alice_cli, bob_daemon, bob_cli)
}

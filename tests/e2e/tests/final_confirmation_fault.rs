use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use uc_e2e_tests::{
    InviteSession, LocalRendezvous, NodeBinarySet, TestCli, TestDaemon, TestProfile,
};

const PASSPHRASE: &str = "connection-recovery-synthetic-passphrase";
const DEADLINE: Duration = Duration::from_secs(120);

struct JoinerHost {
    child: Child,
    input: ChildStdin,
    output: Receiver<Value>,
    _root: tempfile::TempDir,
}

impl JoinerHost {
    fn start(binary: &str, rendezvous: &LocalRendezvous) -> Self {
        let root = tempfile::tempdir().expect("isolated joiner root");
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start test-only joiner host");
        let input = child.stdin.take().expect("joiner input");
        let output = child.stdout.take().expect("joiner output");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if let Ok(value) = serde_json::from_str(&line) {
                    if tx.send(value).is_err() {
                        return;
                    }
                }
            }
        });
        let mut host = Self {
            child,
            input,
            output: rx,
            _root: root,
        };
        host.write(json!({ "root": host._root.path(), "rendezvous": rendezvous.uri() }));
        let ready = host.output.recv_timeout(DEADLINE).expect("test host ready");
        assert_eq!(ready["ready"], true);
        host
    }

    fn write(&mut self, request: Value) {
        serde_json::to_writer(&mut self.input, &request).expect("write host request");
        self.input.write_all(b"\n").expect("finish host request");
        self.input.flush().expect("flush host request");
    }

    fn request(&mut self, request: Value) -> Value {
        self.write(request);
        let response = self
            .output
            .recv_timeout(DEADLINE)
            .expect("test host response");
        assert!(
            response["error"].is_null(),
            "test host returned an error: {response}"
        );
        response["ok"].clone()
    }

    fn command(&mut self, name: &str) -> Value {
        self.request(json!({ "command": name }))
    }

    fn wait(&mut self, kind: &str, after_sequence: u64) -> Value {
        self.request(json!({
            "command": "wait_space_work_event",
            "kind": kind,
            "after_sequence": after_sequence,
        }))
    }
}

impl Drop for JoinerHost {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn cli_json(cli: &TestCli, args: &[&str]) -> Value {
    let output = cli.run_capture(args);
    assert!(
        output.success(),
        "CLI command failed: {:?}: {}",
        args,
        output.stderr
    );
    serde_json::from_str(output.stdout.trim()).expect("CLI JSON response")
}

async fn wait_for_member(cli: &TestCli, name: &str, count: usize) -> String {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let members = cli_json(cli, &["--json", "members"]);
        if let Some(id) = members
            .as_array()
            .filter(|rows| {
                rows.len() == count
                    && rows.iter().filter(|row| row["device_name"] == name).count() == 1
            })
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row["device_name"] == name)
                    .and_then(|row| row["device_id"].as_str())
            })
        {
            return id.to_owned();
        }
        assert!(
            Instant::now() < deadline,
            "{name} did not appear exactly once in {count} members"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_online_member(cli: &TestCli, name: &str) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let members = cli_json(cli, &["--json", "members"]);
        if members.as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["device_name"] == name && row["state"] == "online")
        }) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{name} remained offline after admission"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires UC_E2E_SPACE_WORK_HOST and isolated real processes"]
async fn final_confirmation_retry_excludes_ordinary_work_and_completes() {
    let host_bin = std::env::var("UC_E2E_SPACE_WORK_HOST").expect("test host binary path");
    let rendezvous = LocalRendezvous::start().await;
    let profile = TestProfile::new("space-work-confirmation");
    let mut daemon =
        TestDaemon::start_clean_with(profile, &NodeBinarySet::current(), Some(&rendezvous.uri()))
            .await
            .expect("isolated sponsor daemon");
    let cli = TestCli::new(&daemon.profile);
    let init = cli.run_capture(&[
        "init",
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "Sponsor",
    ]);
    assert!(init.success(), "sponsor initialization: {}", init.stderr);
    let third_profile = TestProfile::new("space-work-third");
    let third_daemon = TestDaemon::start_clean_with(
        third_profile,
        &NodeBinarySet::current(),
        Some(&rendezvous.uri()),
    )
    .await
    .expect("isolated third daemon");
    let third_cli = TestCli::new(&third_daemon.profile);
    let (third_invitation, third_code) = InviteSession::start(&cli).await;
    let third_join = cli_json(
        &third_cli,
        &[
            "--json",
            "join",
            "--code",
            &third_code,
            "--passphrase",
            PASSPHRASE,
            "--device-name",
            "Third",
        ],
    );
    assert_eq!(
        third_join["status"], "active",
        "third device did not join: {third_join}"
    );
    third_invitation.finish().await;
    wait_for_member(&cli, "Third", 2).await;
    wait_for_member(&third_cli, "Sponsor", 2).await;
    let (invitation, code) = InviteSession::start(&cli).await;
    let mut joiner = JoinerHost::start(&host_bin, &rendezvous);
    let after_sequence = joiner.command("arm_complete_ack_failure")["after_sequence"]
        .as_u64()
        .expect("fault arm sequence");

    let joined = joiner.request(json!({ "command": "join", "invitation": code, "name": "Joiner" }));
    assert!(matches!(
        joined["status"].as_str(),
        Some("pending" | "processing")
    ));
    let failed = joiner.wait("final_confirmation_connection_failed", after_sequence);
    let failed_seq = failed["sequence"].as_u64().expect("failure sequence");
    let before = cli_json(&cli, &["--json", "members"]);
    assert!(
        before
            .as_array()
            .is_some_and(|rows| rows.iter().all(|row| row["device_name"] != "Joiner")),
        "candidate was visible before final confirmation"
    );
    let third_before = cli_json(&third_cli, &["--json", "members"]);
    assert!(
        third_before
            .as_array()
            .is_some_and(|rows| rows.iter().all(|row| row["device_name"] != "Joiner")),
        "third device saw candidate before final confirmation"
    );
    let retry = joiner.wait("final_confirmation_retry_started", failed_seq);
    let retry_seq = retry["sequence"].as_u64().expect("retry sequence");
    let events = joiner.command("space_work_events");
    let between: Vec<_> = events
        .as_array()
        .expect("ordered work events")
        .iter()
        .filter(|event| {
            let seq = event["sequence"].as_u64().unwrap_or_default();
            seq > failed_seq && seq < retry_seq
        })
        .map(|event| {
            (
                event["sequence"].as_u64().unwrap_or_default(),
                event["kind"].as_str().unwrap_or("unknown"),
            )
        })
        .collect();
    let member_updates = between
        .iter()
        .filter(|(_, kind)| *kind == "ordinary_member_update_started")
        .count();
    let history_syncs = between
        .iter()
        .filter(|(_, kind)| *kind == "membership_history_sync_started")
        .count();
    eprintln!("failure_seq={failed_seq} retry_seq={retry_seq} between={between:?} member_updates={member_updates} history_syncs={history_syncs}");
    assert_eq!(
        (member_updates, history_syncs),
        (0, 0),
        "ordinary member work preempted confirmation retry"
    );
    let reply = joiner.wait("final_confirmation_reply_received", retry_seq);
    assert!(reply["sequence"].as_u64().unwrap_or_default() > retry_seq);

    let deadline = Instant::now() + DEADLINE;
    loop {
        let setup = joiner.command("setup");
        if setup["has_completed"] == true && setup["space_id"].is_string() {
            break;
        }
        assert!(Instant::now() < deadline, "joiner never completed publicly");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let peer_id = wait_for_member(&cli, "Joiner", 3).await;
    wait_for_member(&third_cli, "Joiner", 3).await;
    wait_for_online_member(&cli, "Joiner").await;
    let sent = cli_json(
        &cli,
        &[
            "--json",
            "send",
            "fault-retry-sponsor-to-joiner",
            "--peer",
            &peer_id,
        ],
    );
    assert_eq!(sent["totalAccepted"], 1, "actual transfer rejected");
    let deadline = Instant::now() + DEADLINE;
    loop {
        let entries = joiner.command("history");
        let received = entries.as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["preview"] == "fault-retry-sponsor-to-joiner")
        });
        if received {
            break;
        }
        assert!(Instant::now() < deadline, "content did not reach joiner");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let local_id = cli_json(&cli, &["--json", "members"])
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row["is_local"] == true))
        .and_then(|row| row["device_id"].as_str())
        .expect("sponsor device id")
        .to_owned();
    let sent_back = joiner.request(
        json!({ "command": "send", "peer": local_id, "text": "fault-retry-joiner-to-sponsor" }),
    );
    assert_eq!(
        sent_back["total_accepted"], 1,
        "reverse transfer rejected: {sent_back}"
    );
    let deadline = Instant::now() + DEADLINE;
    loop {
        let entries = cli_json(&cli, &["--json", "get", "--list", "--limit", "20"]);
        let received = entries.as_array().is_some_and(|rows| {
            rows.iter()
                .any(|row| row["preview"] == "fault-retry-joiner-to-sponsor")
        });
        if received {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "reverse content did not reach sponsor"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    daemon
        .restart_preserving()
        .await
        .expect("sponsor daemon restart");
    let deadline = Instant::now() + DEADLINE;
    loop {
        let members = cli_json(&cli, &["--json", "members"]);
        if members.as_array().is_some_and(|rows| {
            rows.iter()
                .filter(|row| row["device_name"] == "Joiner")
                .count()
                == 1
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "joiner not present exactly once after restart"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    wait_for_member(&third_cli, "Joiner", 3).await;
    invitation.finish().await;
}

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use reqwest::Client;
use serde_json::{json, Value};
use uc_e2e_tests::{
    get_session_token, InviteSession, LocalRendezvous, NodeBinarySet, TestCli, TestDaemon,
    TestProfile,
};

const DEADLINE: Duration = Duration::from_secs(390);

fn passphrase() -> String {
    std::env::var("ABANDONED_PAIRING_PASSPHRASE").expect("ABANDONED_PAIRING_PASSPHRASE")
}

fn binaries() -> NodeBinarySet {
    let directory =
        std::env::var("ABANDONED_PAIRING_BINARY_DIR").expect("ABANDONED_PAIRING_BINARY_DIR");
    NodeBinarySet::fixed_release_dir_with_discovery(
        "abandoned-pairing-local",
        directory,
        uc_e2e_tests::DaemonEndpointDiscovery::ConnectionFile,
    )
    .expect("abandoned pairing binaries")
}

fn profile(run: &str, role: &str) -> TestProfile {
    TestProfile::for_upgrade_fixture(&format!("e2e-abandoned-pairing-{run}-{role}"))
        .expect("valid retained profile")
}

fn cli_json(cli: &TestCli, args: &[&str]) -> Result<Value, String> {
    let output = cli.run_capture(args);
    if !output.success() {
        return Err(format!(
            "exit={} stdout={} stderr={}",
            output.exit_code,
            output.stdout.trim(),
            output.stderr.trim()
        ));
    }
    serde_json::from_str(output.stdout.trim())
        .map_err(|error| format!("invalid JSON: {error}; stdout={}", output.stdout.trim()))
}

struct SpaceWorkControl {
    base_url: String,
    client: Client,
    session: String,
    token: String,
}

impl SpaceWorkControl {
    async fn new(daemon: &TestDaemon, token: String) -> Self {
        let client = Client::new();
        Self {
            base_url: daemon.base_url(),
            session: get_session_token(daemon, &client).await,
            client,
            token,
        }
    }

    async fn work(&self, command: &str) -> Value {
        let response = self
            .client
            .post(format!("{}/e2e/space-work", self.base_url))
            .header("Authorization", format!("Session {}", self.session))
            .header("x-uc-e2e-space-work-token", &self.token)
            .json(&json!({ "command": command }))
            .send()
            .await
            .expect("space work request");
        assert!(
            response.status().is_success(),
            "space work status: {}",
            response.status()
        );
        response.json().await.expect("space work JSON")
    }
}

async fn start_clean(
    profile: TestProfile,
    binaries: &NodeBinarySet,
    rendezvous: &LocalRendezvous,
    token: Option<&str>,
) -> TestDaemon {
    TestDaemon::start_clean_configured_with(profile, binaries, Some(&rendezvous.uri()), |command| {
        if let Some(token) = token {
            command.env("UC_E2E_SPACE_WORK_TOKEN", token);
        }
    })
    .await
    .expect("start clean daemon")
}

async fn join(sponsor: &TestCli, joiner: &TestCli, name: &str) {
    let (invitation, code) = InviteSession::start(sponsor).await;
    let passphrase = passphrase();
    let output = joiner.run_capture(&[
        "--json",
        "join",
        "--code",
        &code,
        "--passphrase",
        &passphrase,
        "--device-name",
        name,
    ]);
    invitation.finish().await;
    assert!(output.success(), "join {name}: {output:?}");
    let value: Value = serde_json::from_str(output.stdout.trim()).expect("join JSON");
    assert_eq!(value["status"], "active", "join {name}: {value}");
}

async fn abandon_at_final_confirmation(
    sponsor: &TestCli,
    joiner: &mut TestDaemon,
    joiner_cli: &TestCli,
    control: &SpaceWorkControl,
    name: &str,
) {
    assert_eq!(
        control.work("arm_joiner_final_confirmation_pause").await["status"],
        "armed"
    );
    let (invitation, code) = InviteSession::start(sponsor).await;
    let passphrase = passphrase();
    let output = joiner_cli.run_capture(&[
        "--json",
        "join",
        "--code",
        &code,
        "--passphrase",
        &passphrase,
        "--device-name",
        name,
        "--no-wait",
    ]);
    assert!(
        output.success(),
        "start interrupted join {name}: {output:?}"
    );
    assert_eq!(
        control.work("wait_joiner_final_confirmation_pause").await["status"],
        "entered"
    );
    joiner.kill();
    invitation.finish().await;
}

async fn wait_for_member_count(cli: &TestCli, expected: usize) -> Vec<Value> {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Ok(value) = cli_json(cli, &["--json", "members"]) {
            if let Some(rows) = value.as_array() {
                if rows.len() == expected {
                    return rows.clone();
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "member count did not reach {expected}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

struct WatchSession {
    child: Child,
}

impl WatchSession {
    async fn start(cli: &TestCli) -> Self {
        let mut child = Command::new(cli.binary_path())
            .env("UC_PROFILE", &cli.profile_name)
            .env("UNICLIPBOARD_ENV", "development")
            .args(["--json", "watch"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start public watch command");
        let stderr = child.stderr.take().expect("watch stderr");
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if line.contains("WATCH_READY") {
                    let _ = ready_tx.send(());
                    return;
                }
            }
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match ready_rx.try_recv() {
                Ok(()) => return Self { child },
                Err(TryRecvError::Disconnected) => panic!("watch exited before ready"),
                Err(TryRecvError::Empty) if Instant::now() >= deadline => {
                    panic!("watch did not become ready")
                }
                Err(TryRecvError::Empty) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    }
}

impl Drop for WatchSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn wait_for_online_member(cli: &TestCli, name: &str) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if cli_json(cli, &["--json", "members"])
            .ok()
            .and_then(|value| value.as_array().cloned())
            .is_some_and(|rows| {
                rows.iter()
                    .any(|row| row["device_name"] == name && row["state"] == "online")
            })
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{name} did not become publicly online"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn wait_for_converged_status(cli: &TestCli) -> Value {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Ok(value) = cli_json(cli, &["--json", "member", "trust", "status"]) {
            let pairings = &value["deviceTrust"]["inboundPairings"];
            if pairings.as_array().is_some_and(|rows| {
                rows.len() == 2
                    && rows.iter().all(|row| {
                        matches!(
                            row["status"].as_str(),
                            Some("failed" | "confirmation_missed")
                        )
                    })
            }) {
                return value;
            }
        }
        assert!(
            Instant::now() < deadline,
            "old candidates did not converge publicly"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn assert_transfer(sender: &TestCli, receiver: &TestCli, receiver_name: &str, text: &str) {
    let members = wait_for_member_count(sender, 3).await;
    let receiver_id = members
        .iter()
        .find(|row| row["device_name"] == receiver_name)
        .and_then(|row| row["device_id"].as_str())
        .expect("receiver member id");
    let deadline = Instant::now() + DEADLINE;
    loop {
        let output = sender.run_capture(&["--json", "send", text, "--peer", receiver_id]);
        if let Ok(sent) = serde_json::from_str::<Value>(output.stdout.trim()) {
            if output.success() && sent["totalAccepted"] == 1 {
                break;
            }
            assert_eq!(sent["totalOffline"], 1, "unexpected send outcome: {sent}");
        }
        assert!(
            Instant::now() < deadline,
            "receiver did not become available: exit={} stdout={} stderr={}",
            output.exit_code,
            output.stdout.trim(),
            output.stderr.trim()
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    loop {
        if cli_json(receiver, &["--json", "get", "--list", "--limit", "20"])
            .ok()
            .and_then(|value| value.as_array().cloned())
            .is_some_and(|rows| rows.iter().any(|row| row["preview"] == text))
        {
            return;
        }
        assert!(Instant::now() < deadline, "text did not arrive: {text}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn write_evidence(run: &str, phase: &str, value: &Value) {
    let root = PathBuf::from(
        std::env::var("ABANDONED_PAIRING_EVIDENCE_DIR").expect("ABANDONED_PAIRING_EVIDENCE_DIR"),
    );
    std::fs::create_dir_all(&root).expect("create evidence directory");
    std::fs::write(
        root.join(format!("{run}-{phase}.json")),
        serde_json::to_vec_pretty(value).expect("evidence JSON"),
    )
    .expect("write evidence");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicit red/green binary directory and retained profiles"]
async fn abandoned_pairings_red_green() {
    assert!(
        std::env::var_os("UC_E2E_KEEP_PROFILES").is_some(),
        "set UC_E2E_KEEP_PROFILES=1 so the red profile survives for the green run"
    );
    let run = std::env::var("ABANDONED_PAIRING_RUN_ID").expect("ABANDONED_PAIRING_RUN_ID");
    let phase = std::env::var("ABANDONED_PAIRING_PHASE").expect("ABANDONED_PAIRING_PHASE");
    let binaries = binaries();
    let rendezvous = LocalRendezvous::start().await;

    if phase == "red" {
        let mut sponsor = start_clean(profile(&run, "sponsor"), &binaries, &rendezvous, None).await;
        let sponsor_cli = TestCli::with_binaries(&sponsor.profile, &binaries);
        let passphrase = passphrase();
        let init = sponsor_cli.run_capture(&[
            "init",
            "--passphrase",
            &passphrase,
            "--device-name",
            "Sponsor",
        ]);
        assert!(init.success(), "sponsor init: {init:?}");
        let mut existing =
            start_clean(profile(&run, "existing"), &binaries, &rendezvous, None).await;
        let existing_cli = TestCli::with_binaries(&existing.profile, &binaries);
        join(&sponsor_cli, &existing_cli, "Existing").await;
        wait_for_member_count(&sponsor_cli, 2).await;

        for index in 1..=2 {
            let token = uuid::Uuid::new_v4().simple().to_string();
            let mut joiner = start_clean(
                profile(&run, &format!("abandoned-{index}")),
                &binaries,
                &rendezvous,
                Some(&token),
            )
            .await;
            let joiner_cli = TestCli::with_binaries(&joiner.profile, &binaries);
            let control = SpaceWorkControl::new(&joiner, token).await;
            abandon_at_final_confirmation(
                &sponsor_cli,
                &mut joiner,
                &joiner_cli,
                &control,
                &format!("Abandoned {index}"),
            )
            .await;
        }

        let before_restart = cli_json(&sponsor_cli, &["--json", "member", "trust", "status"])
            .expect_err("baseline must expose the public device-query failure");
        sponsor.restart_preserving().await.expect("restart sponsor");
        existing
            .restart_preserving()
            .await
            .expect("restart existing member");
        let after_restart = cli_json(&sponsor_cli, &["--json", "member", "trust", "status"])
            .expect_err("baseline failure must survive restart");
        write_evidence(
            &run,
            "red",
            &json!({
                "phase": "red",
                "public_query_failed": true,
                "before_restart": before_restart,
                "after_restart": after_restart,
                "formal_members": wait_for_member_count(&sponsor_cli, 2).await.len(),
                "profiles": {
                    "sponsor": sponsor.profile.name,
                    "existing": existing.profile.name,
                }
            }),
        );
        sponsor.stop_gracefully().await.expect("stop sponsor");
        existing
            .stop_gracefully()
            .await
            .expect("stop existing member");
        return;
    }

    assert_eq!(phase, "green");
    let mut sponsor = TestDaemon::start_preserving_with(
        profile(&run, "sponsor"),
        &binaries,
        Some(&rendezvous.uri()),
    )
    .await
    .expect("start retained sponsor with fixed binaries");
    let mut existing = TestDaemon::start_preserving_with(
        profile(&run, "existing"),
        &binaries,
        Some(&rendezvous.uri()),
    )
    .await
    .expect("start retained existing member with fixed binaries");
    let sponsor_cli = TestCli::with_binaries(&sponsor.profile, &binaries);
    let existing_cli = TestCli::with_binaries(&existing.profile, &binaries);
    let converged = wait_for_converged_status(&sponsor_cli).await;
    wait_for_member_count(&sponsor_cli, 2).await;
    sponsor
        .restart_preserving()
        .await
        .expect("restart fixed sponsor");
    existing
        .restart_preserving()
        .await
        .expect("restart fixed existing member");
    wait_for_converged_status(&sponsor_cli).await;

    let joiner = start_clean(profile(&run, "rejoin"), &binaries, &rendezvous, None).await;
    let joiner_cli = TestCli::with_binaries(&joiner.profile, &binaries);
    join(&sponsor_cli, &joiner_cli, "Rejoined").await;
    let sponsor_members = wait_for_member_count(&sponsor_cli, 3).await;
    wait_for_member_count(&existing_cli, 3).await;
    assert_eq!(
        sponsor_members
            .iter()
            .filter(|row| row["device_name"] == "Rejoined")
            .count(),
        1,
        "rejoined member must appear once"
    );
    write_evidence(
        &run,
        "green-convergence",
        &json!({
            "phase": "green-convergence",
            "public_query_recovered": true,
            "inbound_pairings": converged["deviceTrust"]["inboundPairings"],
            "formal_members": sponsor_members.len(),
            "rejoined_count": 1,
            "restart_consistent": true,
        }),
    );
    let _sponsor_watch = WatchSession::start(&sponsor_cli).await;
    let _joiner_watch = WatchSession::start(&joiner_cli).await;
    wait_for_online_member(&sponsor_cli, "Rejoined").await;
    wait_for_online_member(&joiner_cli, "Sponsor").await;
    assert_transfer(
        &sponsor_cli,
        &joiner_cli,
        "Rejoined",
        "abandoned-pairing-sponsor-to-rejoined",
    )
    .await;
    assert_transfer(
        &joiner_cli,
        &sponsor_cli,
        "Sponsor",
        "abandoned-pairing-rejoined-to-sponsor",
    )
    .await;
    write_evidence(
        &run,
        "green",
        &json!({
            "phase": "green",
            "public_query_recovered": true,
            "inbound_pairings": converged["deviceTrust"]["inboundPairings"],
            "formal_members": sponsor_members.len(),
            "rejoined_count": 1,
            "restart_consistent": true,
            "bidirectional_text_transfer": true,
            "profiles": {
                "sponsor": sponsor.profile.name,
                "existing": existing.profile.name,
                "rejoined": joiner.profile.name,
            }
        }),
    );
}

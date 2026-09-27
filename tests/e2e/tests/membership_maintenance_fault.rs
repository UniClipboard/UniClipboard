use std::time::{Duration, Instant};

use reqwest::Client;
use serde_json::{json, Value};
use uc_e2e_tests::{
    get_session_token, InviteSession, LocalRendezvous, NodeBinarySet, TestCli, TestDaemon,
    TestProfile,
};

const DEADLINE: Duration = Duration::from_secs(120);

fn cli_json(cli: &TestCli, args: &[&str]) -> Value {
    let output = cli.run_capture(args);
    assert!(output.success(), "CLI {:?}: {}", args, output.stderr);
    serde_json::from_str(output.stdout.trim()).expect("CLI JSON")
}

struct SponsorControl {
    base_url: String,
    client: Client,
    session: String,
    token: String,
}

impl SponsorControl {
    async fn new(daemon: &TestDaemon, token: String) -> Self {
        let client = Client::new();
        Self {
            base_url: daemon.base_url(),
            session: get_session_token(daemon, &client).await,
            client,
            token,
        }
    }

    async fn work(&self, payload: Value) -> Value {
        let response = tokio::time::timeout(
            DEADLINE,
            self.client
                .post(format!("{}/e2e/space-work", self.base_url))
                .header("Authorization", format!("Session {}", self.session))
                .header("x-uc-e2e-space-work-token", &self.token)
                .json(&payload)
                .send(),
        )
        .await
        .expect("Space work response deadline")
        .expect("Space work request");
        assert!(
            response.status().is_success(),
            "Space work status: {}",
            response.status()
        );
        response.json().await.expect("Space work JSON")
    }

    async fn wait(&self, kind: &str, after_sequence: u64) -> Value {
        self.work(json!({
            "command": "wait_space_work_event",
            "kind": kind,
            "after_sequence": after_sequence,
        }))
        .await
    }

    async fn opportunity(&self) {
        let response = self
            .client
            .post(format!("{}/presence/opportunity", self.base_url))
            .header("Authorization", format!("Session {}", self.session))
            .json(&json!({ "reason": "network_changed" }))
            .send()
            .await
            .expect("notify connectivity opportunity");
        assert_eq!(response.status(), reqwest::StatusCode::NO_CONTENT);
    }
}

async fn wait_update_status(cli: &TestCli, phase: &str) -> Value {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let status = cli_json(cli, &["--json", "member", "trust", "status"]);
        let update = &status["deviceTrust"]["spaceDeviceUpdate"];
        if update["phase"] == phase {
            return update.clone();
        }
        assert!(
            Instant::now() < deadline,
            "space device update phase {phase} not observed: {update}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn scenario(failure: &str, count: usize) {
    let rendezvous = LocalRendezvous::start().await;
    let passphrase = uuid::Uuid::new_v4().to_string();
    let profile = TestProfile::new("maintenance-sponsor");
    let token = uuid::Uuid::new_v4().simple().to_string();
    let daemon = TestDaemon::start_clean_configured_with(
        profile,
        &NodeBinarySet::current(),
        Some(&rendezvous.uri()),
        |command| {
            command.env("UC_E2E_SPACE_WORK_TOKEN", &token);
        },
    )
    .await
    .expect("isolated sponsor daemon");
    let cli = TestCli::new(&daemon.profile);
    let control = SponsorControl::new(&daemon, token).await;
    let init = cli.run_capture(&[
        "init",
        "--passphrase",
        &passphrase,
        "--device-name",
        "Sponsor",
    ]);
    assert!(init.success(), "sponsor init: {}", init.stderr);
    let (invitation, code) = InviteSession::start(&cli).await;
    let armed = control
        .work(json!({
            "command": "arm_membership_history_failures",
            "failure": failure,
            "count": count,
        }))
        .await;
    let baseline = armed["after_sequence"].as_u64().expect("arm baseline");

    let joiner_profile = TestProfile::new("maintenance-joiner");
    let joiner_daemon = TestDaemon::start_clean_with(
        joiner_profile,
        &NodeBinarySet::current(),
        Some(&rendezvous.uri()),
    )
    .await
    .expect("isolated joiner daemon");
    let joiner = TestCli::new(&joiner_daemon.profile);
    let joined = cli_json(
        &joiner,
        &[
            "--json",
            "join",
            "--code",
            &code,
            "--passphrase",
            &passphrase,
            "--device-name",
            "Joiner",
            "--no-wait",
        ],
    );
    assert!(
        joined["status"] == "pending"
            || joined["status"] == "processing"
            || joined["status"] == "active",
        "unexpected join: {joined}"
    );
    let failure_kind = if failure == "retryable" {
        "membership_history_sync_retryable_failure"
    } else {
        "membership_history_sync_needs_attention"
    };
    let failed = control.wait(failure_kind, baseline).await;
    let failed_seq = failed["sequence"].as_u64().expect("failure sequence");
    let expected_phase = if failure == "retryable" {
        "retryable_failure"
    } else {
        "needs_attention"
    };
    let update = wait_update_status(&cli, expected_phase).await;
    let mut unused_failures = 0;
    if failure == "retryable" {
        assert!(
            update["nextRetryAtMs"].as_i64().is_some(),
            "Engine deadline absent: {update}"
        );
        assert!(update["reason"].is_null() && update["recovery"].is_null());
    } else {
        assert_eq!(update["reason"], "device_state_rejected");
        assert_eq!(update["recovery"], "review_devices");
        assert!(update["nextRetryAtMs"].is_null());
        let cleared = control
            .work(json!({ "command": "clear_membership_history_failures" }))
            .await;
        let remaining = cleared["remaining"].as_u64().expect("unused failures");
        assert!(
            remaining > 0 && remaining < count as u64,
            "unexpected remaining: {remaining}"
        );
        unused_failures = remaining as usize;
        eprintln!("case={failure} failure_seq={failed_seq} remaining={remaining}");
        control.opportunity().await;
    }
    let reply = control
        .wait("membership_history_sync_reply_received", failed_seq)
        .await;
    let reply_seq = reply["sequence"].as_u64().expect("reply sequence");
    let completed = wait_update_status(&cli, "completed").await;
    assert!(completed["nextRetryAtMs"].is_null());
    let events = control
        .work(json!({ "command": "space_work_events" }))
        .await;
    let observed: Vec<_> = events
        .as_array()
        .expect("ordered events")
        .iter()
        .filter(|event| {
            event["sequence"]
                .as_u64()
                .is_some_and(|seq| seq > baseline && seq <= reply_seq)
        })
        .filter_map(|event| event["kind"].as_str())
        .filter(|kind| kind.starts_with("membership_history_sync_"))
        .collect();
    let failures = observed
        .iter()
        .filter(|kind| **kind == failure_kind)
        .count();
    assert_eq!(failures, count - unused_failures);
    assert_eq!(observed.first(), Some(&"membership_history_sync_started"));
    assert_eq!(
        observed.last(),
        Some(&"membership_history_sync_reply_received")
    );
    assert!(
        observed.iter().position(|kind| *kind == failure_kind)
            < observed
                .iter()
                .position(|kind| *kind == "membership_history_sync_reply_received"),
        "failure must precede the real reply"
    );
    if failure == "retryable" {
        assert_eq!(
            observed,
            [
                "membership_history_sync_started",
                "membership_history_sync_retryable_failure",
                "membership_history_sync_started",
                "membership_history_sync_reply_received",
            ]
        );
    }
    eprintln!("case={failure} baseline={baseline} reply_seq={reply_seq} observed={observed:?} public_phase={}", completed["phase"]);
    invitation.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires locally built e2e-rendezvous daemon"]
async fn retryable_history_failure_recovers_on_engine_deadline() {
    scenario("retryable", 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires locally built e2e-rendezvous daemon"]
async fn rejected_history_failure_recovers_after_opportunity() {
    scenario("needs_attention", 1024).await;
}

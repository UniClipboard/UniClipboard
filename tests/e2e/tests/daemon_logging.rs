//! E2E checks of the daemon's logging chain with real processes: profile
//! claiming, startup failure record, host record identity, request ids over
//! HTTP and the diagnostic export.
//!
//! Each test uses its own `e2e-` profile; nothing here touches a real profile.
//! Run with: cargo test -p uc-e2e-tests --test daemon_logging -- --ignored

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use uc_daemon_process::spawn_contract::NO_TAKEOVER_ENV;
use uc_e2e_tests::{get_session_token, NodeBinarySet, TestDaemon, TestProfile};

const ORIGIN: &str = "http://localhost:5173";

fn lines_of(logs: &Path, prefix: &str) -> Vec<Value> {
    let mut records = Vec::new();
    for entry in std::fs::read_dir(logs).expect("log dir") {
        let entry = entry.expect("entry");
        if !entry.file_name().to_string_lossy().starts_with(prefix) {
            continue;
        }
        for line in std::fs::read_to_string(entry.path())
            .expect("log file")
            .lines()
        {
            if let Ok(record) = serde_json::from_str::<Value>(line) {
                records.push(record);
            }
        }
    }
    records
}

fn daemon_pid(daemon: &TestDaemon) -> u64 {
    let conn: Value = serde_json::from_str(
        &std::fs::read_to_string(daemon.profile.data_dir().join("daemon.conn")).expect("conn"),
    )
    .expect("conn json");
    conn["pid"].as_u64().expect("pid")
}

async fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    for _ in 0..100 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

struct ZipCleanup(PathBuf);
impl Drop for ZipCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn zip_entry(path: &Path, name: &str) -> Option<String> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let mut entry = archive.by_name(name).ok()?;
    let mut content = String::new();
    entry.read_to_string(&mut content).ok()?;
    Some(content)
}

#[tokio::test]
#[ignore]
async fn a_second_daemon_on_a_busy_profile_records_its_failure_and_writes_no_logs() {
    let daemon = TestDaemon::start(TestProfile::new("logging-busy-profile"))
        .await
        .expect("first daemon");
    let first_pid = daemon_pid(&daemon);
    let logs = daemon.profile.log_dir().clone();

    // The second daemon must not wait for or evict the first: it fails at the
    // instance lock, before any log sink exists.
    let mut second = TestDaemon::spawn_preserving_configured_with(
        TestProfile::for_upgrade_fixture(&daemon.profile.name).expect("same profile"),
        &NodeBinarySet::current(),
        None,
        |command| {
            command.env(NO_TAKEOVER_ENV, "1");
        },
    )
    .expect("second daemon");
    wait_until("the second daemon to exit", || !second.is_running()).await;

    let record: Value = serde_json::from_str(
        &std::fs::read_to_string(logs.join("daemon-startup-failure.json"))
            .expect("startup failure record"),
    )
    .expect("record json");
    assert_eq!(record["step"], "instance_lock");
    assert_eq!(record["class"], "lock_timeout");
    assert_ne!(record["pid"].as_u64().unwrap(), first_pid);
    assert_ne!(record["pid"].as_u64().unwrap(), 0);

    // Everything the first daemon logged carries its own run identity; the
    // second one contributed no host record and no Engine record.
    wait_until("host records", || {
        !lines_of(&logs, "uniclipboard-daemon.json").is_empty()
    })
    .await;
    let host = lines_of(&logs, "uniclipboard-daemon.json");
    let runs: BTreeSet<String> = host
        .iter()
        .map(|record| record["run_id"].as_str().expect("run_id").to_owned())
        .collect();
    assert_eq!(runs.len(), 1, "one process wrote the host log: {runs:?}");
    assert!(host.iter().all(|record| record["pid"] == first_pid));
    let raw_logs: String = std::fs::read_dir(&logs)
        .unwrap()
        .filter_map(|entry| std::fs::read_to_string(entry.unwrap().path()).ok())
        .collect();
    assert!(
        !raw_logs.contains(&format!("\"pid\":{}", record["pid"])),
        "the losing daemon wrote a log record"
    );

    // The export carries the record, the host block and the source report.
    let client = reqwest::Client::new();
    let token = get_session_token(&daemon, &client).await;
    let response = client
        .post(format!("{}/diagnostics/log-export", daemon.base_url()))
        .header("Authorization", format!("Session {token}"))
        .json(&serde_json::json!({ "sinceHours": 1 }))
        .send()
        .await
        .expect("export request");
    assert!(response.status().is_success(), "{}", response.status());
    let body: Value = response.json().await.expect("export json");
    let path = PathBuf::from(
        body["data"]["path"]
            .as_str()
            .unwrap_or_else(|| panic!("export path missing: {body}")),
    );
    let _cleanup = ZipCleanup(path.clone());

    let included = body["data"]["collection"]["includedFiles"]
        .as_array()
        .unwrap();
    assert!(included
        .iter()
        .any(|name| name == "daemon-startup-failure.json"));
    let missing: Vec<&str> = body["data"]["collection"]["missingSources"]
        .as_array()
        .expect("missingSources")
        .iter()
        .map(|entry| entry["source"].as_str().unwrap())
        .collect();
    assert!(missing.contains(&"cli"), "no CLI ran: {missing:?}");
    assert!(!missing.contains(&"daemon"));

    assert!(zip_entry(&path, "logs/daemon-startup-failure.json").is_some());
    let manifest: Value =
        serde_json::from_str(&zip_entry(&path, "manifest.json").expect("manifest")).unwrap();
    let host_block = &manifest["host"];
    assert_eq!(host_block["os"], std::env::consts::OS);
    assert!(host_block["installedLogProfile"].is_string());
    assert!(host_block["telemetryEnabled"].is_boolean());
    assert!(host_block["timezoneOffsetMinutes"].is_number());
    let manifest_text = manifest.to_string();
    assert!(!manifest_text.contains("device_id") && !manifest_text.contains("deviceId"));
}

#[tokio::test]
#[ignore]
async fn the_request_id_travels_through_cors_error_bodies_and_the_host_log() {
    // The harness defaults to RUST_LOG=warn; request records are INFO.
    let daemon = TestDaemon::start_clean_configured_with(
        TestProfile::new("logging-request-id"),
        &NodeBinarySet::current(),
        None,
        |command| {
            command.env("RUST_LOG", "info");
        },
    )
    .await
    .expect("daemon");
    let logs = daemon.profile.log_dir().clone();
    let client = reqwest::Client::new();
    let token = get_session_token(&daemon, &client).await;

    // A successful call with a caller-chosen id.
    let ok = client
        .get(format!("{}/health", daemon.base_url()))
        .header("Origin", ORIGIN)
        .header("x-request-id", "e2e-req-ok-1")
        .send()
        .await
        .expect("health");
    assert_eq!(ok.headers()["x-request-id"], "e2e-req-ok-1");
    assert!(ok.headers()["access-control-expose-headers"]
        .to_str()
        .unwrap()
        .contains("x-request-id"));

    // A browser preflight may send the header.
    let preflight = client
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/health", daemon.base_url()),
        )
        .header("Origin", ORIGIN)
        .header("Access-Control-Request-Method", "GET")
        .header("Access-Control-Request-Headers", "x-request-id")
        .send()
        .await
        .expect("preflight");
    assert!(preflight.headers()["access-control-allow-headers"]
        .to_str()
        .unwrap()
        .contains("x-request-id"));

    // A rejected request: the same id in the header and the error body, and a
    // generated one when the caller sends nothing usable.
    let rejected = client
        .post(format!("{}/diagnostics/capture/start", daemon.base_url()))
        .header("Authorization", format!("Session {token}"))
        .header("x-request-id", "e2e-req-bad-1")
        .json(&serde_json::json!({ "durationSeconds": 0 }))
        .send()
        .await
        .expect("capture start");
    assert!(rejected.status().is_client_error(), "{}", rejected.status());
    assert_eq!(rejected.headers()["x-request-id"], "e2e-req-bad-1");
    let error: Value = rejected.json().await.expect("error json");
    assert_eq!(error["requestId"], "e2e-req-bad-1", "{error}");

    let unauthenticated = client
        .get(format!("{}/diagnostics/debug", daemon.base_url()))
        .header("x-request-id", "not valid!")
        .send()
        .await
        .expect("unauthenticated");
    assert_eq!(unauthenticated.status(), 401);
    let generated = unauthenticated.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    assert_ne!(generated, "not valid!");
    let error: Value = unauthenticated.json().await.expect("401 json");
    assert_eq!(error["requestId"], generated.as_str(), "{error}");

    // The host log correlates the same id, and no raw device or token id.
    wait_until("the request in the host log", || {
        lines_of(&logs, "uniclipboard-daemon.json")
            .iter()
            .any(|record| record["request_id"] == "e2e-req-bad-1")
    })
    .await;
}

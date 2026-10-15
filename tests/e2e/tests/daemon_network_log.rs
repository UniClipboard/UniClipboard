//! E2E checks of the daemon's independent network-library log with real
//! processes: configuration, time-limited capture control, and what the
//! diagnostic export carries and says about it.
//!
//! Each test uses its own `e2e-` profile; nothing here touches a real profile.
//! The budget below is an explicit test parameter, not a product default.
//! Run with: cargo test -p uc-e2e-tests --test daemon_network_log -- --ignored

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};
use uc_e2e_tests::{
    get_session_token, invite_join_round, NodeBinarySet, TestCli, TestDaemon, TestProfile,
};

const TEST_BUDGET: &str = "record_bytes=8192,queue_records=256,file_bytes=1048576,run_bytes=4194304,root_bytes=16777216,retention_secs=86400";

/// Removes the exported package when the test ends. With `UC_E2E_KEEP_ARTIFACTS=<dir>` a
/// copy is kept there first, for offline cross-checks and review.
struct ZipCleanup(PathBuf);
impl ZipCleanup {
    fn keep_as(self, label: &str) -> Self {
        if let Some(dir) = std::env::var_os("UC_E2E_KEEP_ARTIFACTS") {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).expect("artifact dir");
            std::fs::copy(&self.0, dir.join(format!("{label}.zip"))).expect("keep package");
        }
        self
    }
}
impl Drop for ZipCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn zip_entries(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).expect("zip")).expect("zip");
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("entry");
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read entry");
        entries.insert(entry.name().to_owned(), bytes);
    }
    entries
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct Api {
    base: String,
    token: String,
    client: reqwest::Client,
}

impl Api {
    async fn new(daemon: &TestDaemon) -> Self {
        let client = reqwest::Client::new();
        let token = get_session_token(daemon, &client).await;
        Self {
            base: daemon.base_url(),
            token,
            client,
        }
    }

    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .header("Authorization", format!("Session {}", self.token));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.expect("request");
        let status = response.status().as_u16();
        (status, response.json().await.unwrap_or(Value::Null))
    }

    async fn export(&self) -> (PathBuf, Value) {
        let (status, body) = self
            .call(
                reqwest::Method::POST,
                "/diagnostics/log-export",
                Some(serde_json::json!({ "sinceHours": 1 })),
            )
            .await;
        assert_eq!(status, 200, "{body}");
        let path = PathBuf::from(body["data"]["path"].as_str().expect("export path"));
        (path, body["data"].clone())
    }
}

/// The Engine revision the workspace pins (`uc-engine` in the root Cargo.toml).
fn pinned_engine_rev() -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let text = std::fs::read_to_string(manifest).expect("root Cargo.toml");
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with("uc-engine"))
        .expect("uc-engine pin");
    let start = line.find("rev = \"").expect("rev") + "rev = \"".len();
    line[start..start + 40].to_owned()
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

#[tokio::test]
#[ignore]
async fn without_a_budget_the_network_log_is_not_configured_and_the_business_export_is_unaffected()
{
    let daemon = TestDaemon::start(TestProfile::new("network-log-unconfigured"))
        .await
        .expect("daemon");
    let api = Api::new(&daemon).await;

    let (status, body) = api
        .call(reqwest::Method::GET, "/diagnostics/network", None)
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["availability"], "notConfigured");
    assert!(body["data"]["counters"].is_null() && body["data"]["budget"].is_null());

    let (status, body) = api
        .call(
            reqwest::Method::POST,
            "/diagnostics/network/start",
            Some(serde_json::json!({ "durationSeconds": 60 })),
        )
        .await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["code"], "network_log_not_configured");

    let (path, data) = api.export().await;
    let _cleanup = ZipCleanup(path.clone()).keep_as("unconfigured-daemon");
    assert_eq!(data["enginePreparation"]["flush"], "completed");
    assert_eq!(
        data["enginePreparation"]["network"]["availability"],
        "notConfigured"
    );
    assert_eq!(data["collection"]["network"]["outcome"], "notConfigured");
    let entries = zip_entries(&path);
    assert!(
        entries.keys().all(|name| !name.starts_with("network/")),
        "{:?}",
        entries.keys().collect::<Vec<_>>()
    );
    let manifest: Value = serde_json::from_slice(&entries["manifest.json"]).expect("manifest");
    let commit = manifest["host"]["buildCommit"]
        .as_str()
        .expect("build commit");
    assert!(
        commit == "unknown"
            || (commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit())),
        "{commit}"
    );
    assert!(!daemon.profile.log_dir().join("network").exists());
}

#[tokio::test]
#[ignore]
async fn an_explicit_budget_turns_the_log_on_and_the_export_accounts_for_every_file() {
    let daemon = TestDaemon::start_clean_configured_with(
        TestProfile::new("network-log-configured"),
        &NodeBinarySet::current(),
        None,
        |command| {
            command.env("UC_NETWORK_LOG_BUDGET", TEST_BUDGET);
        },
    )
    .await
    .expect("daemon");
    let api = Api::new(&daemon).await;
    let logs = daemon.profile.log_dir().clone();

    // Ready, with the budget the daemon passed to the Engine.
    let mut status = Value::Null;
    for _ in 0..100 {
        let (code, body) = api
            .call(reqwest::Method::GET, "/diagnostics/network", None)
            .await;
        assert_eq!(code, 200, "{body}");
        status = body["data"].clone();
        if status["state"] == "ready" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(status["availability"], "configured", "{status}");
    assert_eq!(status["state"], "ready", "{status}");
    assert_eq!(status["role"], "daemon");
    assert_eq!(status["mode"], "standard");
    assert_eq!(status["budget"]["runBytes"], "4194304");
    assert_eq!(status["budget"]["retentionSeconds"], "86400");
    let run_id = status["runId"].as_str().expect("run id").to_owned();
    let run_dir = logs.join("network").join("daemon").join(&run_id);
    assert!(run_dir.join("run.json").is_file());

    // A bounded window: one window at a time, never extended.
    for bad in [0_u32, 901] {
        let (code, _) = api
            .call(
                reqwest::Method::POST,
                "/diagnostics/network/start",
                Some(serde_json::json!({ "durationSeconds": bad })),
            )
            .await;
        assert!(code == 400 || code == 422, "duration {bad} gave {code}");
    }
    let (code, first) = api
        .call(
            reqwest::Method::POST,
            "/diagnostics/network/start",
            Some(serde_json::json!({ "durationSeconds": 60 })),
        )
        .await;
    assert_eq!(code, 200, "{first}");
    let capture_id = first["data"]["captureId"]
        .as_str()
        .expect("capture id")
        .to_owned();
    assert_eq!(first["data"]["alreadyActive"], false);
    assert_eq!(first["data"]["status"]["mode"], "detailed");
    assert_eq!(first["data"]["status"]["captureId"], capture_id.as_str());
    let (_, second) = api
        .call(
            reqwest::Method::POST,
            "/diagnostics/network/start",
            Some(serde_json::json!({ "durationSeconds": 600 })),
        )
        .await;
    assert_eq!(second["data"]["alreadyActive"], true, "{second}");
    assert_eq!(second["data"]["captureId"], capture_id.as_str());
    assert_eq!(second["data"]["durationMs"], first["data"]["durationMs"]);

    let (code, mismatch) = api
        .call(
            reqwest::Method::POST,
            "/diagnostics/network/stop",
            Some(serde_json::json!({ "captureId": "not-the-open-window" })),
        )
        .await;
    assert_eq!(code, 409, "{mismatch}");
    assert_eq!(mismatch["code"], "network_capture_mismatch");
    let (code, stopped) = api
        .call(
            reqwest::Method::POST,
            "/diagnostics/network/stop",
            Some(serde_json::json!({ "captureId": capture_id })),
        )
        .await;
    assert_eq!(code, 200, "{stopped}");
    assert_eq!(stopped["data"]["mode"], "standard");
    assert!(stopped["data"]["captureId"].is_null());

    // Things the export must never pick up, planted beside the live run.
    std::fs::write(run_dir.join("notes.txt"), "not a log").expect("plant file");
    std::fs::write(logs.join("network").join("stray.jsonl"), "{}\n").expect("plant root file");
    std::fs::create_dir_all(
        logs.join("network")
            .join("unknown_role")
            .join("a".repeat(32)),
    )
    .expect("plant role");
    #[cfg(unix)]
    {
        let secret = logs.join("..").join("e2e-secret-target.txt");
        std::fs::write(&secret, "secret").expect("secret");
        std::os::unix::fs::symlink(&secret, run_dir.join("network.20260101.9.jsonl"))
            .expect("plant symlink");
    }

    let (path, data) = api.export().await;
    let _cleanup = ZipCleanup(path.clone()).keep_as("configured-daemon");

    // Business and network are prepared separately; the business flush is intact.
    assert_eq!(data["enginePreparation"]["flush"], "completed");
    let prepared = &data["enginePreparation"]["network"];
    assert_eq!(prepared["availability"], "configured");
    assert_eq!(prepared["flush"], "completed", "{prepared}");
    assert_eq!(prepared["otherProcessesFlushed"], false);
    assert_eq!(prepared["status"]["runId"], run_id.as_str());

    let collection = &data["collection"]["network"];
    let runs = collection["runs"].as_array().expect("runs");
    let current = runs
        .iter()
        .find(|run| run["runId"] == run_id.as_str())
        .expect("current run in the package");
    assert_eq!(current["currentRun"], true);
    assert!(
        collection["ignoredEntries"].as_u64().unwrap() >= 3,
        "{collection}"
    );
    let reasons: Vec<&str> = collection["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reason| reason.as_str().unwrap())
        .collect();
    assert!(reasons.contains(&"entriesIgnored"), "{reasons:?}");
    assert_ne!(
        collection["outcome"], "complete",
        "ignored entries keep it from being reported complete"
    );

    // Only allowed files, each with the digest of exactly the copied bytes.
    let entries = zip_entries(&path);
    let prefix = format!("network/daemon/{run_id}/");
    let in_package: Vec<&String> = entries
        .keys()
        .filter(|name| name.starts_with("network/"))
        .collect();
    assert!(
        in_package.iter().all(|name| {
            let file = name.strip_prefix(&prefix).unwrap_or("");
            file == "run.json"
                || (file.starts_with("network.")
                    && file.ends_with(".jsonl")
                    && file != "network.20260101.9.jsonl")
        }),
        "{in_package:?}"
    );
    assert!(entries.contains_key(&format!("{prefix}run.json")));
    for file in current["files"].as_array().expect("files") {
        let name = file["name"].as_str().unwrap();
        let bytes = &entries[&format!("{prefix}{name}")];
        assert_eq!(file["sha256"], sha256_hex(bytes).as_str(), "{name}");
        assert_eq!(file["bytes"], bytes.len().to_string().as_str(), "{name}");
        if let Some(prepared_match) = file["matchesPrepared"].as_bool() {
            assert!(
                prepared_match,
                "{name} differs from what the Engine flushed"
            );
        }
    }
    let manifest: Value = serde_json::from_slice(&entries["manifest.json"]).expect("manifest");
    assert_eq!(manifest["collection"]["network"], *collection);

    // Source identity: the run manifest names the Engine build that wrote it, and the
    // status names the same source; both must be the revision pinned in Cargo.toml.
    let run_manifest: Value =
        serde_json::from_slice(&entries[&format!("{prefix}run.json")]).expect("run.json");
    let pinned = pinned_engine_rev();
    eprintln!(
        "[provenance] pinned {pinned}; run.json commit {} state {}; status source {}",
        run_manifest["engine_source_commit"],
        run_manifest["engine_source_state"],
        data["enginePreparation"]["status"]["sourceCommit"]
    );
    assert_eq!(run_manifest["engine_source_commit"], pinned.as_str());
    assert_eq!(
        data["enginePreparation"]["status"]["sourceCommit"],
        pinned.as_str()
    );

    // The business log and the network log stay in their own sources.
    let business = std::fs::read_dir(&logs)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("engine.") || name.starts_with("uniclipboard-daemon.json")
        })
        .map(|entry| std::fs::read_to_string(entry.path()).unwrap_or_default())
        .collect::<String>();
    assert!(
        !business.contains("\"library\":\"iroh\"") && !business.contains("iroh_relay"),
        "third-party network records must not reach the business or host logs"
    );

    // A graceful stop must not be delayed by the network log.
    let mut daemon = daemon;
    let started = std::time::Instant::now();
    daemon.stop_gracefully().await.expect("graceful stop");
    wait_until("the daemon to exit", || !daemon.is_running()).await;
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "shutdown took {:?}",
        started.elapsed()
    );
    assert!(run_dir.join("lease.lock").exists() || !run_dir.exists());
}

const PASSPHRASE: &str = "network-log-e2e-passphrase";

/// Two real daemons pair over the real transport with the network log on and a
/// detailed window open on both. Whatever they record must reach the package
/// intact and must carry no address, key, URL, passphrase or invitation.
#[tokio::test]
#[ignore]
async fn two_paired_daemons_record_connection_events_without_identifiers() {
    let binaries = NodeBinarySet::current();
    let configure = |command: &mut std::process::Command| {
        command.env("UC_NETWORK_LOG_BUDGET", TEST_BUDGET);
    };
    let alice_daemon = TestDaemon::start_clean_configured_with(
        TestProfile::new("network-log-alice"),
        &binaries,
        None,
        configure,
    )
    .await
    .expect("alice daemon");
    let alice_cli = TestCli::with_binaries(&alice_daemon.profile, &binaries);
    let init = alice_cli.run_capture(&[
        "init",
        "--passphrase",
        PASSPHRASE,
        "--device-name",
        "alice-node",
    ]);
    assert!(
        init.success(),
        "init failed: {} {}",
        init.stdout,
        init.stderr
    );
    let bob_daemon = TestDaemon::start_clean_configured_with(
        TestProfile::new("network-log-bob"),
        &binaries,
        None,
        configure,
    )
    .await
    .expect("bob daemon");
    let bob_cli = TestCli::with_binaries(&bob_daemon.profile, &binaries);

    let alice = Api::new(&alice_daemon).await;
    let bob = Api::new(&bob_daemon).await;
    for api in [&alice, &bob] {
        let (code, started) = api
            .call(
                reqwest::Method::POST,
                "/diagnostics/network/start",
                Some(serde_json::json!({ "durationSeconds": 300 })),
            )
            .await;
        assert_eq!(code, 200, "{started}");
    }

    let join = invite_join_round(&alice_cli, &bob_cli, PASSPHRASE, "bob-node").await;
    assert!(
        join.success(),
        "join failed: {} {}",
        join.stdout,
        join.stderr
    );
    tokio::time::sleep(Duration::from_secs(3)).await;

    let mut total_records = 0_usize;
    for (name, api) in [("alice", &alice), ("bob", &bob)] {
        let (code, status) = api
            .call(reqwest::Method::GET, "/diagnostics/network", None)
            .await;
        assert_eq!(code, 200, "{status}");
        let counters = &status["data"]["counters"];
        eprintln!("[{name}] counters {counters}");
        assert_eq!(status["data"]["state"], "ready", "{name}: {status}");

        let (path, data) = api.export().await;
        let _cleanup = ZipCleanup(path.clone()).keep_as(&format!("paired-{name}"));
        let prepared = &data["enginePreparation"]["network"];
        assert_eq!(prepared["flush"], "completed", "{name}: {prepared}");
        let collection = &data["collection"]["network"];
        eprintln!(
            "[{name}] outcome {} reasons {}",
            collection["outcome"], collection["reasons"]
        );

        let entries = zip_entries(&path);
        let run = collection["runs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|run| run["currentRun"] == true)
            .unwrap_or_else(|| panic!("{name}: no current run: {collection}"));
        let run_id = run["runId"].as_str().unwrap();
        let mut content = String::new();
        for file in run["files"].as_array().unwrap() {
            let file_name = file["name"].as_str().unwrap();
            let bytes = &entries[&format!("network/daemon/{run_id}/{file_name}")];
            assert_eq!(
                file["sha256"],
                sha256_hex(bytes).as_str(),
                "{name}/{file_name}"
            );
            // The Engine prepares digests for the segments, not for run.json.
            if file_name.ends_with(".jsonl") {
                assert_eq!(file["matchesPrepared"], true, "{name}/{file_name}: {file}");
            }
            if file_name.ends_with(".jsonl") {
                let text = String::from_utf8(bytes.clone()).expect("utf8 records");
                for line in text.lines() {
                    let record: Value = serde_json::from_str(line)
                        .unwrap_or_else(|error| panic!("{name}: bad record {line}: {error}"));
                    assert!(record.is_object(), "{line}");
                    total_records += 1;
                }
                content.push_str(&text);
            }
        }
        eprintln!("[{name}] network records:\n{content}");

        // Identifiers must not appear in any form.
        let forbidden: Vec<(&str, bool)> = vec![
            ("passphrase", content.contains(PASSPHRASE)),
            (
                "device name",
                content.contains("alice-node") || content.contains("bob-node"),
            ),
            (
                "url",
                content.contains("http://") || content.contains("https://"),
            ),
            ("ipv4", has_ipv4(&content)),
            ("ipv6", content.contains("::1") || content.contains("fe80:")),
            ("64-hex key", has_long_hex(&content, 52)),
        ];
        for (what, found) in forbidden {
            assert!(!found, "{name}: network records contain a {what}");
        }
    }
    assert!(
        total_records > 0,
        "no network record was written while two daemons paired in a detailed window"
    );
}

fn has_ipv4(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit() {
            let start = index;
            let mut groups = 0;
            let mut cursor = index;
            loop {
                let digits_start = cursor;
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                if cursor == digits_start || cursor - digits_start > 3 {
                    break;
                }
                groups += 1;
                if groups == 4 {
                    return true;
                }
                if cursor < bytes.len() && bytes[cursor] == b'.' {
                    cursor += 1;
                } else {
                    break;
                }
            }
            index = start + 1;
        } else {
            index += 1;
        }
    }
    false
}

fn has_long_hex(text: &str, min: usize) -> bool {
    let mut run = 0;
    for byte in text.bytes() {
        if byte.is_ascii_hexdigit() {
            run += 1;
            if run >= min {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

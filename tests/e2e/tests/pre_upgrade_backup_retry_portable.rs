//! Platform-neutral pre-upgrade backup retry matrix (macOS, Linux, Windows).
//!
//! No archived fixture is needed: the profile is created by a clean first run
//! of the daemon under test, then marked as last used by an older product
//! version (`upgrade-cursor.json`), so the next launch is an upgrade that must
//! take a pre-upgrade backup. Profiles are random `dev-upgrade-*` ones with
//! development storage; no real user data or system credential is touched.
//!
//! Failure modes, written before the assertions:
//! 1. Stale prepared backup: the first upgrade launch is killed right after the
//!    file backup record (`current`) is published, then a host-owned file in
//!    the data root changes. The shipped 1.1.1 daemon then fails every later
//!    launch (`backup_failed`, `retryable=true`); the fixed daemon recaptures,
//!    reaches `service_ready` and keeps the earlier copy byte-identical.
//! 2. Vanished spool: the captured pending-content spool disappears before the
//!    retry while the data root is unchanged. Same expectation as 1.
//! 3. Published security record: once it exists, a changed source must NOT
//!    trigger a recapture; the original copy stays the only copy.
//! 4. Control: killed after the capture with no change recovers.
//!
//! `UC_E2E_EXPECT_SHIPPED_DEFECT=1` runs the same cells against a daemon that
//! still has the defect (the shipped 1.1.1 build) and asserts the stuck state
//! instead, to prove that the cells discriminate.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use uc_e2e_tests::{NodeBinarySet, TestDaemon, TestProfile};

fn expects_defect() -> bool {
    std::env::var_os("UC_E2E_EXPECT_SHIPPED_DEFECT").is_some()
}

fn visit(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, found);
        } else {
            found.push(path);
        }
    }
}

fn files_under(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    visit(directory, &mut found);
    found.sort();
    found
}

fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|value| value.to_str()).unwrap_or("")
}

fn name_present(directory: &Path, name: &str) -> bool {
    files_under(directory).iter().any(|path| file_name(path) == name)
}

fn count_extension(directory: &Path, extension: &str) -> usize {
    files_under(directory)
        .iter()
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some(extension))
        .count()
}

/// Content digest of every backup file except the lease and the two pointers,
/// which legitimately move to a newer copy.
fn immutable_backup_files(directory: &Path) -> BTreeMap<String, String> {
    use sha2::{Digest, Sha256};
    files_under(directory)
        .into_iter()
        .filter(|path| !matches!(file_name(path), ".lease" | "current" | "security-current"))
        .map(|path| {
            let relative = path
                .strip_prefix(directory)
                .expect("backup file below backup directory")
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(&path).expect("read backup file");
            let digest: String = Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            (relative, digest)
        })
        .collect()
}

async fn startup_status(profile: &TestProfile) -> Option<Value> {
    let bytes = std::fs::read(profile.data_dir().join("daemon-startup.conn")).ok()?;
    let conn: Value = serde_json::from_slice(&bytes).ok()?;
    let port = conn["port"].as_u64()?;
    let token = conn["token"].as_str()?;
    reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/startup"))
        .bearer_auth(token)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()
}

/// Waits until this launch either succeeds or fails. `after` is the number of
/// launches already seen so a stale status file from a killed launch is ignored.
async fn settled_startup(profile: &TestProfile, previous_attempt: Option<String>) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(status) = startup_status(profile).await {
            let attempt = status["progress"]["attempt_id"].as_str().map(str::to_string);
            let state = status["progress"]["state"].as_str().unwrap_or_default();
            if attempt != previous_attempt && (status["service_ready"] == true || state == "failed")
            {
                return status;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "startup neither completed nor failed within 120s"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn quiet(command: &mut std::process::Command) {
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
}

fn describe(label: &str, status: &Value) {
    let progress = &status["progress"];
    eprintln!(
        "CELL {label}: service_ready={} state={} failure={} current_step={}",
        status["service_ready"],
        progress["state"],
        progress["failure"],
        progress["upgrade"]["current_step"],
    );
}

/// A profile created by a clean run of the daemon under test, then marked as
/// last used by an older product version. Returns the live (stopped) handle so
/// the profile is not cleaned up early, plus the profile name.
async fn upgrade_ready_profile(label: &str) -> (TestDaemon, String) {
    // Keep the profile name short: Engine nests long hashed directories below it
    // and Windows paths beyond MAX_PATH fail with ERROR_PATH_NOT_FOUND.
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let name = format!("dev-up-{label}-{}", &unique[..8]);
    let profile = TestProfile::for_upgrade_fixture(&name).expect("create isolated profile");
    let mut daemon = TestDaemon::start_clean_with(profile, &NodeBinarySet::current(), None)
        .await
        .expect("clean first run");
    daemon.stop_gracefully().await.expect("stop first run");
    std::fs::write(
        daemon.profile.data_dir().join("upgrade-cursor.json"),
        br#"{"schema_version":1,"last_seen_version":"1.1.0"}"#,
    )
    .expect("mark the profile as last used by an older version");
    (daemon, name)
}

/// Starts the daemon on the profile and kills it as soon as `marker` appears in
/// the backup directory. `forbidden` must still be absent afterwards.
async fn interrupt_when(name: &str, marker: &str, forbidden: Option<&str>) -> TestDaemon {
    let profile = TestProfile::for_upgrade_fixture(name).expect("reopen profile");
    let backups = profile.upgrade_backup_dir().clone();
    let mut daemon =
        TestDaemon::spawn_preserving_configured_with(profile, &NodeBinarySet::current(), None, quiet)
            .expect("spawn upgrade launch");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    while !name_present(&backups, marker) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{marker} was never published"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    daemon.kill();
    if let Some(forbidden) = forbidden {
        assert!(
            !name_present(&backups, forbidden),
            "the launch published {forbidden} before it was killed; rerun the cell"
        );
    }
    daemon
}

fn change_host_file(daemon: &TestDaemon) {
    std::fs::write(daemon.profile.data_dir().join("visual-effects.json"), b"{}")
        .expect("write host preference file");
}

async fn retry_and_judge(daemon: &mut TestDaemon, label: &str) -> Value {
    daemon.respawn_preserving_configured_with(quiet).expect("respawn");
    let status = settled_startup(&daemon.profile, None).await;
    describe(label, &status);
    if expects_defect() {
        assert_eq!(status["service_ready"], false, "defect build must be stuck: {status}");
        assert_eq!(status["progress"]["failure"]["reason"], "backup_failed");
        assert_eq!(status["progress"]["failure"]["retryable"], true);
        // The stuck state must persist across further launches.
        daemon.respawn_preserving_configured_with(quiet).expect("respawn again");
        let again = settled_startup(&daemon.profile, None).await;
        assert_eq!(again["service_ready"], false, "defect build must stay stuck");
    } else {
        assert_eq!(status["service_ready"], true, "retry must recover: {status}");
    }
    status
}

#[tokio::test]
#[ignore = "requires a built uniclipd"]
async fn control_interrupted_capture_without_data_change_recovers() {
    let (_first, name) = upgrade_ready_profile("unchanged").await;
    let mut daemon = interrupt_when(&name, "current", Some("security-current")).await;
    daemon.respawn_preserving_configured_with(quiet).expect("respawn");
    let status = settled_startup(&daemon.profile, None).await;
    describe("interrupted-unchanged", &status);
    assert_eq!(status["service_ready"], true);
}

#[tokio::test]
#[ignore = "requires a built uniclipd"]
async fn stale_prepared_backup_is_recaptured_and_the_old_copy_is_kept() {
    let (_first, name) = upgrade_ready_profile("stale").await;
    let mut daemon = interrupt_when(&name, "current", Some("security-current")).await;
    let backups = daemon.profile.upgrade_backup_dir().clone();
    let before = immutable_backup_files(&backups);
    assert_eq!(count_extension(&backups, "files"), 1);
    change_host_file(&daemon);

    retry_and_judge(&mut daemon, "stale-retry").await;
    if !expects_defect() {
        assert_eq!(count_extension(&backups, "files"), 2, "old and new copy");
        assert!(name_present(&backups, "security-current"));
    }
    let after = immutable_backup_files(&backups);
    for (file, digest) in &before {
        assert_eq!(after.get(file), Some(digest), "old backup file {file} changed");
    }
}

#[tokio::test]
#[ignore = "requires a built uniclipd"]
async fn vanished_captured_spool_is_a_source_change_and_recovers() {
    let (first, name) = upgrade_ready_profile("spool").await;
    let spool = first.profile.cache_dir().join("spool");
    std::fs::create_dir_all(&spool).expect("create synthetic spool");
    std::fs::write(spool.join("pending-synthetic"), b"synthetic pending content")
        .expect("write synthetic spool entry");

    let mut daemon = interrupt_when(&name, "current", Some("security-current")).await;
    let backups = daemon.profile.upgrade_backup_dir().clone();
    let before = immutable_backup_files(&backups);
    assert!(spool.exists(), "the launch must leave the spool in place");
    std::fs::remove_dir_all(&spool).expect("remove the captured spool");

    retry_and_judge(&mut daemon, "vanished-spool").await;
    let after = immutable_backup_files(&backups);
    for (file, digest) in &before {
        assert_eq!(after.get(file), Some(digest), "old backup file {file} changed");
    }
    if !expects_defect() {
        assert_eq!(count_extension(&backups, "files"), 2, "old and new copy");
    }
}

#[tokio::test]
#[ignore = "requires a built uniclipd"]
async fn published_security_record_forbids_recapture_after_the_source_changes() {
    let (_first, name) = upgrade_ready_profile("published").await;
    let mut daemon = interrupt_when(&name, "security-current", None).await;
    let backups = daemon.profile.upgrade_backup_dir().clone();
    let before = immutable_backup_files(&backups);
    change_host_file(&daemon);

    daemon.respawn_preserving_configured_with(quiet).expect("respawn");
    let status = settled_startup(&daemon.profile, None).await;
    describe("published-then-changed", &status);
    assert_eq!(status["service_ready"], true);
    assert_eq!(
        count_extension(&backups, "files"),
        1,
        "the original copy must remain the only copy"
    );
    assert_eq!(immutable_backup_files(&backups), before);
}

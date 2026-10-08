//! Retrying a pre-upgrade backup whose first launch was interrupted.
//!
//! Failure modes, written before the assertions were changed:
//!
//! 1. Stale prepared backup: the first launch publishes the file backup record
//!    (`current`) and is killed before the security record. A host-owned file
//!    in the data root then changes. Before the Engine fix every retry failed
//!    with `backup_failed` / `retryable=true` because the retained capture
//!    digest no longer matched. Expected now: the retry recaptures and reaches
//!    `service_ready`. Fails if the retry still fails, if the old copy is lost
//!    or altered, or if history is lost.
//! 2. Older target with a security record (the reporter's 1.1.0 -> 1.1.1
//!    state): a completed upgrade left a security record for an older target;
//!    a new target's prepared backup goes stale. The old record must not block
//!    the recapture and must stay byte-identical.
//! 3. Published security record: once the security record for the prepared
//!    copy exists, upgrade writes may have started, so a changed source must
//!    NOT trigger a recapture; the original copy stays the only copy.
//! 3b. Vanished spool: the capture included the pending-content spool
//!    (`<cache>/spool`), which then disappears before the retry. This is a
//!    source change, not a storage failure: the retry must recapture (without
//!    the spool) and reach `service_ready`. Before the Engine follow-up the
//!    vanished spool surfaced as a plain missing-file error.
//! 4. Controls: baseline upgrade, and interruption without any data change.
//!
//! Scenario 2 needs a second daemon binary with a newer workspace version
//! (`UC_E2E_NEXT_DAEMON`); it is skipped with a message when unset.
//!
//! The profile is a random `dev-upgrade-*` profile with development file keys;
//! no real user data or system keychain entry is touched.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use uc_e2e_tests::{
    get_session_token, NodeBinarySet, TestDaemon, TestProfile, UpgradeUserdataFixture,
};

fn fixture_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/upgrades/v0.19.3/macos-aarch64/single-node-mixed-content")
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

fn name_present(directory: &Path, name: &str) -> bool {
    files_under(directory)
        .iter()
        .any(|path| path.file_name().and_then(|value| value.to_str()) == Some(name))
}

fn count_extension(directory: &Path, extension: &str) -> usize {
    files_under(directory)
        .iter()
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some(extension))
        .count()
}

/// Content digest of every file in the backup directory, keyed by file name
/// relative to the directory, excluding the lease lock file.
fn backup_snapshot(directory: &Path) -> BTreeMap<String, String> {
    files_under(directory)
        .into_iter()
        .filter(|path| path.file_name().and_then(|value| value.to_str()) != Some(".lease"))
        .map(|path| {
            let relative = path
                .strip_prefix(directory)
                .expect("backup file below backup directory")
                .to_string_lossy()
                .into_owned();
            let bytes = std::fs::read(&path).expect("read backup file");
            let digest = digest_hex(&bytes);
            (relative, digest)
        })
        .collect()
}

fn digest_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
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

/// Waits until startup either succeeds or fails, returning the final status.
async fn settled_startup(profile: &TestProfile) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(status) = startup_status(profile).await {
            let state = status["progress"]["state"].as_str().unwrap_or_default();
            if status["service_ready"] == true || state == "failed" {
                return status;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "startup neither completed nor failed within 90s"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn restored_profile(label: &str) -> TestProfile {
    let fixture = UpgradeUserdataFixture::load(fixture_directory()).expect("load fixture");
    let profile = TestProfile::for_upgrade_fixture(&format!(
        "dev-upgrade-retry-{label}-{}",
        uuid::Uuid::new_v4().as_simple()
    ))
    .expect("create isolated profile");
    fixture
        .restore_into(profile.data_dir(), profile.cache_dir(), &profile.name)
        .expect("restore fixture");
    profile
}

/// The harness redirects daemon output into a file inside the data root, which
/// the product never does. Silence it so only product-owned files can change.
fn quiet(command: &mut std::process::Command) {
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
}

fn binaries_from(daemon: Option<PathBuf>) -> NodeBinarySet {
    let mut binaries = NodeBinarySet::current();
    if let Some(daemon) = daemon {
        binaries.daemon = daemon;
    }
    binaries
}

/// Starts the daemon and kills it as soon as `marker` is published in the
/// backup directory. `forbidden` must still be absent afterwards.
async fn interrupt_when(
    profile: TestProfile,
    binaries: &NodeBinarySet,
    marker: &str,
    forbidden: Option<&str>,
) -> TestDaemon {
    let backups = profile.upgrade_backup_dir().clone();
    let mut daemon =
        TestDaemon::spawn_preserving_configured_with(profile, binaries, None, quiet)
            .expect("spawn first launch");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
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
            "the first launch published {forbidden} before it was killed; rerun the cell"
        );
    }
    daemon
}

fn write_host_preference(daemon: &TestDaemon) {
    // A host-owned preference file outside the excluded list changes after the
    // capture, as the GUI or quick panel helper can do while the daemon starts.
    std::fs::write(daemon.profile.data_dir().join("visual-effects.json"), b"{}")
        .expect("write host preference file");
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

async fn assert_history_preserved(daemon: &TestDaemon) {
    let expected: Value = serde_json::from_slice(
        &std::fs::read(fixture_directory().join("expected.json")).expect("read expectations"),
    )
    .expect("decode expectations");
    let passphrase = expected["passphrase"].as_str().expect("fixture passphrase");
    let client = reqwest::Client::new();
    let session = get_session_token(daemon, &client).await;
    let unlock = client
        .post(format!(
            "{}/encryption/unlock-with-passphrase",
            daemon.base_url()
        ))
        .header("Authorization", format!("Session {session}"))
        .json(&serde_json::json!({ "passphrase": passphrase }))
        .send()
        .await
        .expect("unlock upgraded profile");
    assert!(unlock.status().is_success(), "upgraded profile did not unlock");
    let history: Value = client
        .get(format!("{}/clipboard/entries?limit=100", daemon.base_url()))
        .header("Authorization", format!("Session {session}"))
        .send()
        .await
        .expect("read history")
        .json()
        .await
        .expect("decode history");
    let entries = history
        .get("data")
        .unwrap_or(&history)
        .as_array()
        .expect("history array");
    for record in expected["records"].as_array().expect("expected records") {
        let id = record["id"].as_str().expect("record id");
        assert!(
            entries.iter().any(|entry| entry["id"].as_str() == Some(id)),
            "history lost record {id}"
        );
    }
}

#[tokio::test]
#[ignore = "requires macOS arm64 and a built uniclipd"]
async fn control_baseline_upgrade_completes() {
    let profile = restored_profile("baseline");
    let daemon = TestDaemon::spawn_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        quiet,
    )
    .expect("spawn");
    let status = settled_startup(&daemon.profile).await;
    describe("baseline", &status);
    assert_eq!(status["service_ready"], true);
}

#[tokio::test]
#[ignore = "requires macOS arm64 and a built uniclipd"]
async fn control_interrupted_capture_without_data_change_recovers() {
    let mut daemon = interrupt_when(
        restored_profile("unchanged"),
        &NodeBinarySet::current(),
        "current",
        Some("security-current"),
    )
    .await;
    daemon.respawn_preserving_configured_with(quiet).expect("respawn");
    let status = settled_startup(&daemon.profile).await;
    describe("interrupted-unchanged", &status);
    assert_eq!(status["service_ready"], true);
}

#[tokio::test]
#[ignore = "requires macOS arm64 and a built uniclipd"]
async fn stale_prepared_backup_is_recaptured_and_the_old_copy_is_kept() {
    let mut daemon = interrupt_when(
        restored_profile("changed"),
        &NodeBinarySet::current(),
        "current",
        Some("security-current"),
    )
    .await;
    let backups = daemon.profile.upgrade_backup_dir().clone();
    let before = backup_snapshot(&backups);
    assert_eq!(count_extension(&backups, "files"), 1);
    write_host_preference(&daemon);

    // Restart through the harness so it also discovers the daemon endpoint.
    daemon
        .restart_preserving_with(&NodeBinarySet::current(), None)
        .await
        .expect("retry must recover and become healthy");
    let status = settled_startup(&daemon.profile).await;
    describe("changed-retry", &status);
    assert_eq!(status["service_ready"], true, "retry must recover: {status}");
    assert_history_preserved(&daemon).await;

    let after = backup_snapshot(&backups);
    for (name, digest) in &before {
        if name.rsplit('/').next() == Some("current") {
            continue; // the pointer moves to the new copy
        }
        assert_eq!(
            after.get(name),
            Some(digest),
            "old backup file {name} was altered or removed"
        );
    }
    assert_eq!(count_extension(&backups, "files"), 2, "old and new copy");
    assert!(name_present(&backups, "security-current"));

    // An ordinary restart must neither add nor change backup files.
    let settled = backup_snapshot(&backups);
    daemon.restart_preserving().await.expect("restart");
    assert_eq!(backup_snapshot(&backups), settled);
}

#[tokio::test]
#[ignore = "requires macOS arm64, a built uniclipd and UC_E2E_NEXT_DAEMON"]
async fn older_target_security_record_does_not_block_recapture() {
    let Some(next) = std::env::var_os("UC_E2E_NEXT_DAEMON").map(PathBuf::from) else {
        eprintln!("SKIPPED: UC_E2E_NEXT_DAEMON is not set");
        return;
    };
    let profile = restored_profile("older-target");
    let profile_name = profile.name.clone();
    let backups = profile.upgrade_backup_dir().clone();

    // Complete an upgrade with the current daemon; it publishes a security
    // record for its own (older) target version.
    let mut daemon = TestDaemon::spawn_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        quiet,
    )
    .expect("spawn current");
    let status = settled_startup(&daemon.profile).await;
    assert_eq!(status["service_ready"], true);
    daemon.stop_gracefully().await.expect("stop current daemon");
    let older = backup_snapshot(&backups);
    assert!(name_present(&backups, "security-current"));

    // The newer daemon captures for its new target and is interrupted.
    let next_binaries = binaries_from(Some(next));
    daemon.kill();
    let profile = TestProfile::for_upgrade_fixture(&profile_name).expect("reopen profile");
    let mut daemon = interrupt_when(profile, &next_binaries, "current", None).await;
    write_host_preference(&daemon);
    daemon.respawn_preserving_configured_with(quiet).expect("respawn");
    let status = settled_startup(&daemon.profile).await;
    describe("older-target-retry", &status);
    assert_eq!(status["service_ready"], true, "retry must recover: {status}");
    let after = backup_snapshot(&backups);
    for (name, digest) in &older {
        if matches!(
            name.rsplit('/').next(),
            Some("current" | "security-current")
        ) {
            continue; // pointers move to the new copy
        }
        assert_eq!(after.get(name), Some(digest), "older backup {name} changed");
    }
}

#[tokio::test]
#[ignore = "requires macOS arm64 and a built uniclipd"]
async fn published_security_record_forbids_recapture_after_the_source_changes() {
    let mut daemon = interrupt_when(
        restored_profile("published"),
        &NodeBinarySet::current(),
        "security-current",
        None,
    )
    .await;
    let backups = daemon.profile.upgrade_backup_dir().clone();
    let before = backup_snapshot(&backups);
    write_host_preference(&daemon);

    daemon.respawn_preserving_configured_with(quiet).expect("respawn");
    let status = settled_startup(&daemon.profile).await;
    describe("published-then-changed", &status);
    assert_eq!(status["service_ready"], true);
    assert_eq!(
        count_extension(&backups, "files"),
        1,
        "the original copy must remain the only copy"
    );
    let after = backup_snapshot(&backups);
    for (name, digest) in &before {
        assert_eq!(after.get(name), Some(digest), "{name} changed");
    }
}

/// UTC calendar date from the BSD `date` tool (this test is macOS-only).
fn utc_date(adjust: &[&str]) -> String {
    let output = std::process::Command::new("date")
        .arg("-u")
        .args(adjust)
        .arg("+%Y-%m-%d")
        .output()
        .expect("run date");
    String::from_utf8(output.stdout).expect("date output").trim().to_string()
}

/// Engine diagnostics must survive a full local log quota: filler from an old
/// day is evicted so today's file receives records (the reporter's Engine logs
/// were saturated at 99,999,982 of 100,000,000 bytes and silently dropped).
#[tokio::test]
#[ignore = "requires macOS arm64 and a built uniclipd"]
async fn full_engine_log_quota_does_not_starve_the_current_day() {
    let profile = restored_profile("log-quota");
    let log_dir = profile.log_dir().clone();
    std::fs::create_dir_all(&log_dir).expect("create log directory");
    let day = utc_date(&["-v-3d"]);
    let filler = log_dir.join(format!("engine.{day}.jsonl"));
    std::fs::write(&filler, vec![b'\n'; 99_999_982]).expect("write synthetic filler");

    let daemon = TestDaemon::spawn_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        quiet,
    )
    .expect("spawn");
    let status = settled_startup(&daemon.profile).await;
    describe("log-quota", &status);
    assert_eq!(status["service_ready"], true);
    let today = log_dir.join(format!("engine.{}.jsonl", utc_date(&[])));
    let written = std::fs::metadata(&today).map(|meta| meta.len()).unwrap_or(0);
    eprintln!(
        "CELL log-quota: today_bytes={written} filler_present={}",
        filler.exists()
    );
    assert!(written > 0, "today's Engine log stayed empty under a full quota");
    assert!(!filler.exists(), "the old filler was not evicted");
}

#[tokio::test]
#[ignore = "requires macOS arm64 and a built uniclipd"]
async fn vanished_captured_spool_is_a_source_change_and_recovers() {
    let profile = restored_profile("spool");
    let spool = profile.cache_dir().join("spool");
    std::fs::create_dir_all(&spool).expect("create synthetic spool");
    std::fs::write(spool.join("pending-synthetic"), b"synthetic pending content")
        .expect("write synthetic spool entry");

    let mut daemon = interrupt_when(
        profile,
        &NodeBinarySet::current(),
        "current",
        Some("security-current"),
    )
    .await;
    let backups = daemon.profile.upgrade_backup_dir().clone();
    let before = backup_snapshot(&backups);
    assert!(spool.exists(), "the first launch must leave the spool in place");
    std::fs::remove_dir_all(&spool).expect("remove the captured spool");

    // Quiet respawn: the harness log in the data root must not change, so the
    // data-root digest stays equal and only the vanished spool differs.
    daemon.respawn_preserving_configured_with(quiet).expect("respawn");
    let status = settled_startup(&daemon.profile).await;
    describe("vanished-spool", &status);
    assert_eq!(status["service_ready"], true, "retry must recover: {status}");
    daemon
        .restart_preserving_with(&NodeBinarySet::current(), None)
        .await
        .expect("restart after recovery");
    assert_history_preserved(&daemon).await;

    let after = backup_snapshot(&backups);
    for (name, digest) in &before {
        if matches!(
            name.rsplit('/').next(),
            Some("current" | "security-current")
        ) {
            continue;
        }
        assert_eq!(after.get(name), Some(digest), "old backup file {name} changed");
    }
    assert_eq!(count_extension(&backups, "files"), 2, "old and new copy");
}

/// Startup cleanup on a directory that is already over quota must evict an old
/// day and keep the current day's file, which may hold the only diagnostics of
/// this run.
#[tokio::test]
#[ignore = "requires macOS arm64 and a built uniclipd"]
async fn startup_cleanup_over_quota_keeps_the_current_day_file() {
    let profile = restored_profile("log-over-quota");
    let log_dir = profile.log_dir().clone();
    std::fs::create_dir_all(&log_dir).expect("create log directory");
    let old = log_dir.join(format!("engine.{}.jsonl", utc_date(&["-v-3d"])));
    let today = log_dir.join(format!("engine.{}.jsonl", utc_date(&[])));
    std::fs::write(&old, vec![b'\n'; 60_000_000]).expect("write old filler");
    std::fs::write(&today, vec![b'\n'; 60_000_000]).expect("write current-day filler");

    let daemon = TestDaemon::spawn_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        quiet,
    )
    .expect("spawn");
    let status = settled_startup(&daemon.profile).await;
    describe("log-over-quota", &status);
    assert_eq!(status["service_ready"], true);
    let size = std::fs::metadata(&today).map(|meta| meta.len()).unwrap_or(0);
    eprintln!(
        "CELL log-over-quota: today_bytes={size} old_present={}",
        old.exists()
    );
    assert!(size >= 60_000_000, "the current-day file was deleted or truncated");
    assert!(!old.exists(), "the old-day file was not evicted");
}

fn shipped_binaries() -> Option<NodeBinarySet> {
    let daemon = std::env::var_os("UC_E2E_SHIPPED_DAEMON").map(PathBuf::from)?;
    Some(binaries_from(Some(daemon)))
}

/// Shipped release -> candidate on one isolated profile: history, the earlier
/// backup and its security record must survive the product upgrade.
/// `UC_E2E_SHIPPED_DAEMON` is the previous release's `uniclipd`.
#[tokio::test]
#[ignore = "requires macOS arm64, a built uniclipd and UC_E2E_SHIPPED_DAEMON"]
async fn shipped_release_to_candidate_keeps_history_and_the_earlier_backup() {
    let Some(shipped) = shipped_binaries() else {
        eprintln!("SKIPPED: UC_E2E_SHIPPED_DAEMON is not set");
        return;
    };
    let profile = restored_profile("shipped-upgrade");
    let backups = profile.upgrade_backup_dir().clone();
    let mut daemon = TestDaemon::start_preserving_with(profile, &shipped, None)
        .await
        .expect("shipped release starts on the restored profile");
    assert_history_preserved(&daemon).await;
    daemon.stop_gracefully().await.expect("stop shipped release");
    let first = backup_snapshot(&backups);
    assert!(name_present(&backups, "security-current"));

    daemon
        .restart_preserving_with(&NodeBinarySet::current(), None)
        .await
        .expect("candidate must start on the shipped release's profile");
    assert_history_preserved(&daemon).await;
    let after = backup_snapshot(&backups);
    for (name, digest) in &first {
        if matches!(
            name.rsplit('/').next(),
            Some("current" | "security-current")
        ) {
            continue;
        }
        assert_eq!(after.get(name), Some(digest), "earlier backup file {name} changed");
    }
    assert_eq!(count_extension(&backups, "files"), 2, "earlier and new backup");
    eprintln!("CELL shipped-to-candidate: ready, history intact, files=2");
}

/// The reporter's state: the shipped release is stuck on a stale prepared
/// backup. Installing the candidate must unstick the same profile without
/// touching the earlier copy.
#[tokio::test]
#[ignore = "requires macOS arm64, a built uniclipd and UC_E2E_SHIPPED_DAEMON"]
async fn profile_stuck_on_the_shipped_release_recovers_with_the_candidate() {
    let Some(shipped) = shipped_binaries() else {
        eprintln!("SKIPPED: UC_E2E_SHIPPED_DAEMON is not set");
        return;
    };
    let mut daemon = interrupt_when(
        restored_profile("stuck-shipped"),
        &shipped,
        "current",
        Some("security-current"),
    )
    .await;
    let backups = daemon.profile.upgrade_backup_dir().clone();
    write_host_preference(&daemon);
    let stuck_before = backup_snapshot(&backups);

    daemon.respawn_preserving_configured_with(quiet).expect("respawn shipped");
    let status = settled_startup(&daemon.profile).await;
    describe("stuck-on-shipped", &status);
    assert_eq!(status["service_ready"], false, "shipped release should be stuck");
    assert_eq!(status["progress"]["failure"]["reason"], "backup_failed");

    daemon
        .restart_preserving_with(&NodeBinarySet::current(), None)
        .await
        .expect("candidate must recover the stuck profile");
    assert_history_preserved(&daemon).await;
    let after = backup_snapshot(&backups);
    for (name, digest) in &stuck_before {
        if matches!(
            name.rsplit('/').next(),
            Some("current" | "security-current")
        ) {
            continue;
        }
        assert_eq!(after.get(name), Some(digest), "earlier backup file {name} changed");
    }
    eprintln!("CELL stuck-shipped-to-candidate: recovered, history intact");
}

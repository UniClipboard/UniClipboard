//! Linux profiles whose key encryption key lives in the pre-1.0 file key store.
//!
//! Versions before 1.0 degraded to `<app_data_root>/keyring` whenever the
//! Secret Service probe failed. These tests restore a real v0.19.3 profile
//! that holds its KEK only in that store and start the current daemon against
//! private session buses, never the host's session bus or keyring:
//!
//! - `UC_E2E_ABSENT_SECRET_SERVICE_BUS`: a bus on which no Secret Service is
//!   activatable (required);
//! - `UC_E2E_PRESENT_SECRET_SERVICE_BUS`: a bus with an unlocked, isolated
//!   Secret Service (only for the test that needs one).
//!
//! `scripts/e2e/linux-legacy-file-key-store.sh` prepares both buses.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::Value;
use uc_e2e_tests::{
    get_session_token, NodeBinarySet, TestCli, TestDaemon, TestProfile, UpgradeUserdataFixture,
};
use uc_platform::ports::SecureStorageProvider;
use uc_platform::system_secure_storage::SystemSecureStorage;

const LEGACY_KEK_KEY: &str = "kek:v1:profile:default";
const LEGACY_KEK_FILE: &str = "6b656b3a76313a70726f66696c653a64656661756c74.bin";
/// `iroh-identity:v1`, which v0.20.0-alpha.6 kept next to the KEK.
const LEGACY_IDENTITY_FILE: &str = "69726f682d6964656e746974793a7631.bin";

fn fixture_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/upgrades/v0.19.3/macos-aarch64/single-node-mixed-content")
}

fn expected() -> Value {
    serde_json::from_slice(
        &std::fs::read(fixture_directory().join("expected.json")).expect("read expectations"),
    )
    .expect("decode expectations")
}

fn bus(variable: &str) -> String {
    std::env::var(variable).unwrap_or_else(|_| {
        panic!("{variable} is not set; run scripts/e2e/linux-legacy-file-key-store.sh")
    })
}

fn session(bus: String) -> impl FnOnce(&mut Command) {
    move |command: &mut Command| {
        command
            .env("DBUS_SESSION_BUS_ADDRESS", bus)
            .env("DISPLAY", ":99")
            .env_remove("WAYLAND_DISPLAY");
    }
}

fn restored_profile(label: &str) -> TestProfile {
    let fixture = UpgradeUserdataFixture::load(fixture_directory()).expect("load v0.19.3 fixture");
    let profile = TestProfile::for_upgrade_fixture(&format!(
        "dev-legacy-file-kek-{label}-{}",
        uuid::Uuid::new_v4().as_simple()
    ))
    .expect("create isolated profile");
    fixture
        .restore_into(profile.data_dir(), profile.cache_dir(), &profile.name)
        .expect("restore v0.19.3 fixture");
    assert!(
        profile
            .data_dir()
            .join("keyring")
            .join(LEGACY_KEK_FILE)
            .is_file(),
        "fixture must keep its KEK in the legacy file key store"
    );
    profile
}

fn read_kek(profile: &TestProfile) -> Vec<u8> {
    std::fs::read(profile.data_dir().join("keyring").join(LEGACY_KEK_FILE))
        .expect("read legacy KEK file")
}

async fn authorized_get(daemon: &TestDaemon, path: &str) -> Value {
    let client = reqwest::Client::new();
    let session = get_session_token(daemon, &client).await;
    let response: Value = client
        .get(format!("{}{path}", daemon.base_url()))
        .header("Authorization", format!("Session {session}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("GET {path} failed: {error}"))
        .error_for_status()
        .unwrap_or_else(|error| panic!("GET {path} returned an error: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("GET {path} was not JSON: {error}"));
    response.get("data").cloned().unwrap_or(response)
}

async fn assert_unlocked_history(daemon: &TestDaemon) {
    let state = authorized_get(daemon, "/encryption/state").await;
    assert_eq!(
        state["sessionReady"], true,
        "the background session must open from the stored KEK"
    );
    let recovery = authorized_get(daemon, "/encryption/recovery").await;
    assert_eq!(
        recovery["state"], "not_required",
        "a readable KEK must not ask for the passphrase"
    );
    assert_history_decrypts(daemon);
}

fn assert_history_decrypts(daemon: &TestDaemon) {
    let cli = TestCli::new(&daemon.profile);
    for record in expected()["records"].as_array().expect("expected records") {
        let Some(text) = record["text"].as_str() else {
            continue;
        };
        let id = record["id"].as_str().expect("record id");
        let output = cli.run_ok(&["--json", "get", "--id", id]);
        let value: Value = serde_json::from_str(output.trim()).expect("entry JSON");
        assert_eq!(value["text"], text, "record {id} was not decrypted intact");
    }
}

fn daemon_log(profile: &TestProfile) -> String {
    let mut log = String::new();
    if let Ok(entries) = std::fs::read_dir(profile.log_dir()) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("uniclipboard-daemon")
            {
                log.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
            }
        }
    }
    log
}

async fn startup_status(profile: &TestProfile) -> Option<Value> {
    let connection: Value = serde_json::from_slice(
        &std::fs::read(profile.data_dir().join("daemon-startup.conn")).ok()?,
    )
    .ok()?;
    let port = connection["port"].as_u64()?;
    let token = connection["token"].as_str()?;
    reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/startup"))
        .bearer_auth(token)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()
}

async fn wait_for_failed_startup(profile: &TestProfile) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = startup_status(profile).await {
            if status["progress"]["state"] == "failed" {
                return status;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "startup did not report a failure within 30s"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// The isolated Secret Service as the daemon for `profile` sees it.
///
/// The service name is derived from `UC_PROFILE`, so each test profile owns a
/// separate namespace in the isolated keyring. Tests run one at a time.
fn secret_service_for(profile: &TestProfile) -> SystemSecureStorage {
    std::env::set_var(
        "DBUS_SESSION_BUS_ADDRESS",
        bus("UC_E2E_PRESENT_SECRET_SERVICE_BUS"),
    );
    std::env::set_var("UC_PROFILE", &profile.name);
    std::env::set_var("UNICLIPBOARD_ENV", "development");
    SystemSecureStorage::new()
}

const SOURCE_RECORD: &str = "secure-storage-source.json";

/// Compares key material without printing it, even for synthetic fixtures.
#[track_caller]
fn assert_same_secret<T: PartialEq>(actual: &T, expected: &T, message: &str) {
    assert!(actual == expected, "{message} (key bytes withheld)");
}

fn source_record(profile: &TestProfile) -> Option<String> {
    std::fs::read_to_string(profile.data_dir().join(SOURCE_RECORD)).ok()
}

/// The daemon confirms the source after its background session recovery.
async fn wait_for_source_record(profile: &TestProfile, store: &str) {
    let expected = format!(r#""store":"{store}""#);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        if source_record(profile).is_some_and(|record| record.contains(&expected)) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the key store source was not recorded as {store}: {:?}",
            source_record(profile)
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn move_aside(path: &Path) -> PathBuf {
    let retained = path.with_file_name("keyring-retained-by-test");
    std::fs::rename(path, &retained).expect("retain legacy key store");
    retained
}

#[tokio::test]
#[ignore = "requires Linux, built binaries and a private session bus"]
async fn legacy_file_key_store_starts_without_a_secret_service() {
    let profile = restored_profile("absent");
    let kek = read_kek(&profile);

    let daemon = TestDaemon::start_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_ABSENT_SECRET_SERVICE_BUS")),
    )
    .await
    .unwrap_or_else(|error| panic!("v0.19.3 file-KEK profile did not start: {error}"));

    assert_unlocked_history(&daemon).await;
    assert_same_secret(
        &read_kek(&daemon.profile),
        &kek,
        "the legacy KEK must stay intact",
    );
    assert!(
        daemon_log(&daemon.profile).contains("using the legacy file key store"),
        "the daemon log must name the selected key store"
    );
    wait_for_source_record(&daemon.profile, "legacy_file").await;
}

#[tokio::test]
#[ignore = "requires Linux, built binaries and two private session buses"]
async fn legacy_file_key_store_stays_authoritative_when_a_secret_service_appears() {
    let profile = restored_profile("present");
    let kek = read_kek(&profile);
    let mut daemon = TestDaemon::start_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_ABSENT_SECRET_SERVICE_BUS")),
    )
    .await
    .unwrap_or_else(|error| panic!("first start without a secret service failed: {error}"));
    assert_unlocked_history(&daemon).await;
    daemon.stop_gracefully().await.expect("stop first daemon");

    daemon
        .restart_preserving_configured_with(session(bus("UC_E2E_PRESENT_SECRET_SERVICE_BUS")))
        .await
        .unwrap_or_else(|error| {
            panic!(
                "restart with a secret service failed: {error}\n{}",
                daemon.diagnostic_log()
            )
        });

    assert_unlocked_history(&daemon).await;
    assert_same_secret(
        &read_kek(&daemon.profile),
        &kek,
        "the legacy KEK must stay intact",
    );
    assert!(
        daemon_log(&daemon.profile).contains("using the recorded secure storage source"),
        "the restart must use the source recorded by the first run"
    );
    assert!(
        source_record(&daemon.profile)
            .is_some_and(|record| record.contains(r#""store":"legacy_file""#)),
        "a secret service that appears later must not replace the recorded file key store"
    );
}

#[tokio::test]
#[ignore = "requires Linux, built binaries and an isolated secret service"]
async fn an_empty_secret_service_without_a_record_keeps_the_file_key_store() {
    let profile = restored_profile("empty-service");
    let kek = read_kek(&profile);

    let daemon = TestDaemon::start_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_PRESENT_SECRET_SERVICE_BUS")),
    )
    .await
    .unwrap_or_else(|error| panic!("start with an empty secret service failed: {error}"));

    assert_unlocked_history(&daemon).await;
    assert_same_secret(
        &read_kek(&daemon.profile),
        &kek,
        "the legacy KEK must stay intact",
    );
    assert!(
        daemon_log(&daemon.profile)
            .contains("system secure store holds none of the legacy file entries"),
        "an available but empty secret service must not replace the file key store"
    );
    wait_for_source_record(&daemon.profile, "legacy_file").await;
}

#[tokio::test]
#[ignore = "requires Linux, built binaries and a private session bus"]
async fn missing_legacy_key_store_fails_closed_without_an_empty_file_store() {
    let profile = restored_profile("missing");
    let keyring = profile.data_dir().join("keyring");
    let retained = move_aside(&keyring);

    let daemon = TestDaemon::spawn_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_ABSENT_SECRET_SERVICE_BUS")),
    )
    .expect("spawn daemon");
    let status = wait_for_failed_startup(&daemon.profile).await;

    assert_eq!(status["progress"]["failure"]["reason"], "startup_failed");
    assert!(
        !keyring.exists(),
        "an unavailable secret service must not be replaced by an empty file store"
    );
    assert!(retained.join(LEGACY_KEK_FILE).is_file());
    assert!(
        daemon_log(&daemon.profile).contains("engine startup failed"),
        "the daemon log must record the startup failure"
    );
}

#[tokio::test]
#[ignore = "requires Linux, built binaries and a private session bus"]
async fn a_wrong_legacy_kek_asks_for_the_passphrase_without_being_overwritten() {
    let profile = restored_profile("wrong");
    let replaced = vec![0x5a_u8; 32];
    std::fs::write(
        profile.data_dir().join("keyring").join(LEGACY_KEK_FILE),
        &replaced,
    )
    .expect("replace the KEK with a wrong one");

    let daemon = TestDaemon::start_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_ABSENT_SECRET_SERVICE_BUS")),
    )
    .await
    .unwrap_or_else(|error| panic!("wrong-KEK profile did not reach recovery: {error}"));

    let recovery = authorized_get(&daemon, "/encryption/recovery").await;
    assert_eq!(recovery["state"], "awaiting_passphrase");
    assert_same_secret(
        &read_kek(&daemon.profile),
        &replaced,
        "the stored KEK must not change before the passphrase is authenticated",
    );

    let client = reqwest::Client::new();
    let session_token = get_session_token(&daemon, &client).await;
    let unlock = client
        .post(format!(
            "{}/encryption/unlock-with-passphrase",
            daemon.base_url()
        ))
        .header("Authorization", format!("Session {session_token}"))
        .json(&serde_json::json!({ "passphrase": expected()["passphrase"] }))
        .send()
        .await
        .expect("unlock request");
    assert!(
        unlock.status().is_success(),
        "the original passphrase must recover"
    );
    let state = authorized_get(&daemon, "/encryption/state").await;
    assert_eq!(state["sessionReady"], true);
    assert_history_decrypts(&daemon);
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires Linux, built binaries and a private session bus"]
async fn an_unreadable_legacy_key_store_fails_closed_and_stays_intact() {
    use std::os::unix::fs::PermissionsExt;

    let profile = restored_profile("unreadable");
    let kek = read_kek(&profile);
    let keyring = profile.data_dir().join("keyring");
    std::fs::set_permissions(&keyring, std::fs::Permissions::from_mode(0o000))
        .expect("make key store unreadable");
    if std::fs::read_dir(&keyring).is_ok() {
        std::fs::set_permissions(&keyring, std::fs::Permissions::from_mode(0o700)).unwrap();
        eprintln!("skipped: privileged runners bypass directory permissions");
        return;
    }

    let daemon = TestDaemon::spawn_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_ABSENT_SECRET_SERVICE_BUS")),
    )
    .expect("spawn daemon");
    let status = wait_for_failed_startup(&daemon.profile).await;
    std::fs::set_permissions(&keyring, std::fs::Permissions::from_mode(0o700))
        .expect("restore key store permissions");

    assert_eq!(status["progress"]["failure"]["reason"], "startup_failed");
    assert_same_secret(
        &read_kek(&daemon.profile),
        &kek,
        "the legacy KEK must stay intact",
    );
    assert!(
        daemon_log(&daemon.profile).contains("legacy file key store cannot be listed"),
        "the daemon log must explain why no key store was chosen"
    );
}

#[tokio::test]
#[ignore = "requires Linux, built binaries and an isolated secret service"]
async fn entries_split_between_the_file_store_and_a_secret_service_fail_closed() {
    let profile = restored_profile("split");
    let keyring = profile.data_dir().join("keyring");
    std::fs::write(keyring.join(LEGACY_IDENTITY_FILE), [7_u8; 32])
        .expect("add a file-only legacy entry");
    let kek = read_kek(&profile);
    let secret_service = secret_service_for(&profile);
    secret_service
        .set(LEGACY_KEK_KEY, &kek)
        .expect("seed the secret service with the KEK only");

    let daemon = TestDaemon::spawn_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_PRESENT_SECRET_SERVICE_BUS")),
    )
    .expect("spawn daemon");
    let status = wait_for_failed_startup(&daemon.profile).await;

    assert_eq!(status["progress"]["failure"]["reason"], "startup_failed");
    assert_same_secret(
        &read_kek(&daemon.profile),
        &kek,
        "the legacy KEK must stay intact",
    );
    assert_same_secret(
        &std::fs::read(keyring.join(LEGACY_IDENTITY_FILE)).expect("read file-only entry"),
        &vec![7_u8; 32],
        "a stored key changed",
    );
    assert_same_secret(
        &secret_service.get(LEGACY_KEK_KEY).expect("read seeded KEK"),
        &Some(kek.clone()),
        "the secret service copy must stay intact",
    );
    assert!(
        daemon_log(&daemon.profile).contains("hold different entries"),
        "the daemon log must explain why no key store was chosen"
    );
}

#[tokio::test]
#[ignore = "requires Linux, built binaries and an isolated secret service"]
async fn a_secret_service_holding_every_legacy_entry_keeps_authority() {
    let profile = restored_profile("both");
    let kek = read_kek(&profile);
    let secret_service = secret_service_for(&profile);
    secret_service
        .set(LEGACY_KEK_KEY, &kek)
        .expect("seed the secret service with the same KEK");

    let daemon = TestDaemon::start_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_PRESENT_SECRET_SERVICE_BUS")),
    )
    .await
    .unwrap_or_else(|error| panic!("profile with both copies did not start: {error}"));

    assert_unlocked_history(&daemon).await;
    assert_same_secret(
        &read_kek(&daemon.profile),
        &kek,
        "the file copy must stay intact",
    );
    assert!(
        daemon_log(&daemon.profile).contains("holds every legacy file entry"),
        "the daemon log must name the selected key store"
    );
    wait_for_source_record(&daemon.profile, "system").await;
}

#[tokio::test]
#[ignore = "requires Linux, built binaries and an isolated secret service"]
async fn a_stale_secret_service_kek_asks_for_the_passphrase_and_leaves_the_file_store_intact() {
    let profile = restored_profile("stale");
    let kek = read_kek(&profile);
    let stale = vec![0x33_u8; 32];
    let secret_service = secret_service_for(&profile);
    secret_service
        .set(LEGACY_KEK_KEY, &stale)
        .expect("seed the secret service with a stale KEK");

    let daemon = TestDaemon::start_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_PRESENT_SECRET_SERVICE_BUS")),
    )
    .await
    .unwrap_or_else(|error| panic!("stale-KEK profile did not reach recovery: {error}"));

    let recovery = authorized_get(&daemon, "/encryption/recovery").await;
    assert_eq!(recovery["state"], "awaiting_passphrase");
    assert_eq!(
        recovery["canSubmitPassphrase"], true,
        "no legacy material may be reported lost"
    );
    assert_same_secret(
        &secret_service.get(LEGACY_KEK_KEY).expect("read stale KEK"),
        &Some(stale.clone()),
        "the stored KEK must not change before the passphrase is authenticated",
    );

    let client = reqwest::Client::new();
    let session_token = get_session_token(&daemon, &client).await;
    let unlock = client
        .post(format!(
            "{}/encryption/unlock-with-passphrase",
            daemon.base_url()
        ))
        .header("Authorization", format!("Session {session_token}"))
        .json(&serde_json::json!({ "passphrase": expected()["passphrase"] }))
        .send()
        .await
        .expect("unlock request");
    assert!(
        unlock.status().is_success(),
        "the original passphrase must recover"
    );
    let state = authorized_get(&daemon, "/encryption/state").await;
    assert_eq!(state["sessionReady"], true);
    assert_history_decrypts(&daemon);
    assert_same_secret(
        &read_kek(&daemon.profile),
        &kek,
        "the file store must stay intact",
    );
    wait_for_source_record(&daemon.profile, "system").await;
}

#[tokio::test]
#[ignore = "requires Linux, built binaries and an isolated secret service"]
async fn a_confirmed_system_source_is_not_replaced_when_the_secret_service_disappears() {
    let profile = restored_profile("confirmed-system");
    let kek = read_kek(&profile);
    let secret_service = secret_service_for(&profile);
    secret_service
        .set(LEGACY_KEK_KEY, &kek)
        .expect("seed the secret service with the valid KEK");
    let stale = vec![0x44_u8; 32];
    std::fs::write(
        profile.data_dir().join("keyring").join(LEGACY_KEK_FILE),
        &stale,
    )
    .expect("leave a stale KEK in the file store");

    let mut daemon = TestDaemon::start_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_PRESENT_SECRET_SERVICE_BUS")),
    )
    .await
    .unwrap_or_else(|error| panic!("start with the secret service failed: {error}"));
    assert_unlocked_history(&daemon).await;
    wait_for_source_record(&daemon.profile, "system").await;
    daemon.stop_gracefully().await.expect("stop first daemon");
    let record = source_record(&daemon.profile);

    daemon
        .respawn_preserving_configured_with(session(bus("UC_E2E_ABSENT_SECRET_SERVICE_BUS")))
        .expect("respawn daemon without the secret service");
    let status = wait_for_failed_startup(&daemon.profile).await;

    assert_eq!(status["progress"]["failure"]["reason"], "startup_failed");
    assert_same_secret(
        &read_kek(&daemon.profile),
        &stale,
        "the stale file KEK must not change",
    );
    assert_eq!(source_record(&daemon.profile), record);
    assert_same_secret(
        &secret_service.get(LEGACY_KEK_KEY).expect("read system KEK"),
        &Some(kek.clone()),
        "the recorded store must stay intact",
    );
}

#[tokio::test]
#[ignore = "requires Linux, built binaries and an isolated secret service"]
async fn a_confirmed_file_source_is_kept_when_the_secret_service_holds_a_different_key() {
    let profile = restored_profile("confirmed-file");
    let kek = read_kek(&profile);
    let mut daemon = TestDaemon::start_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_ABSENT_SECRET_SERVICE_BUS")),
    )
    .await
    .unwrap_or_else(|error| panic!("start without the secret service failed: {error}"));
    assert_unlocked_history(&daemon).await;
    wait_for_source_record(&daemon.profile, "legacy_file").await;
    daemon.stop_gracefully().await.expect("stop first daemon");
    let stale = vec![0x55_u8; 32];
    let secret_service = secret_service_for(&daemon.profile);
    secret_service
        .set(LEGACY_KEK_KEY, &stale)
        .expect("seed the secret service with a different KEK");

    daemon
        .restart_preserving_configured_with(session(bus("UC_E2E_PRESENT_SECRET_SERVICE_BUS")))
        .await
        .unwrap_or_else(|error| {
            panic!(
                "restart with the secret service failed: {error}\n{}",
                daemon.diagnostic_log()
            )
        });

    assert_unlocked_history(&daemon).await;
    assert_same_secret(
        &read_kek(&daemon.profile),
        &kek,
        "the file KEK must stay intact",
    );
    assert_same_secret(
        &secret_service.get(LEGACY_KEK_KEY).expect("read system KEK"),
        &Some(stale.clone()),
        "the unselected store must not change",
    );
}

#[tokio::test]
#[ignore = "requires Linux, built binaries and an isolated secret service"]
async fn an_unconfirmed_run_records_nothing_and_changes_no_key() {
    let profile = restored_profile("unconfirmed");
    let kek = read_kek(&profile);
    let secret_service = secret_service_for(&profile);
    secret_service
        .set(LEGACY_KEK_KEY, &kek)
        .expect("seed the secret service with the valid KEK");
    let stale = vec![0x66_u8; 32];
    std::fs::write(
        profile.data_dir().join("keyring").join(LEGACY_KEK_FILE),
        &stale,
    )
    .expect("leave a stale KEK in the file store");

    let mut daemon = TestDaemon::start_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_ABSENT_SECRET_SERVICE_BUS")),
    )
    .await
    .unwrap_or_else(|error| panic!("stale file KEK did not reach recovery: {error}"));
    let recovery = authorized_get(&daemon, "/encryption/recovery").await;
    assert_eq!(recovery["state"], "awaiting_passphrase");
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        source_record(&daemon.profile),
        None,
        "a locked run must not record"
    );
    assert_same_secret(
        &read_kek(&daemon.profile),
        &stale,
        "no key may change without authentication",
    );
    daemon.stop_gracefully().await.expect("stop locked daemon");

    daemon
        .restart_preserving_configured_with(session(bus("UC_E2E_PRESENT_SECRET_SERVICE_BUS")))
        .await
        .unwrap_or_else(|error| {
            panic!(
                "restart with the secret service failed: {error}\n{}",
                daemon.diagnostic_log()
            )
        });
    assert_unlocked_history(&daemon).await;
    wait_for_source_record(&daemon.profile, "system").await;
    assert_same_secret(
        &read_kek(&daemon.profile),
        &stale,
        "the unselected file store must not change",
    );
}

#[tokio::test]
#[ignore = "requires Linux, built binaries and a private session bus"]
async fn an_unrecognized_source_record_fails_closed() {
    let profile = restored_profile("bad-record");
    let kek = read_kek(&profile);
    std::fs::write(
        profile.data_dir().join(SOURCE_RECORD),
        b"{\"store\":\"elsewhere\"}",
    )
    .expect("write an unrecognized record");

    let daemon = TestDaemon::spawn_preserving_configured_with(
        profile,
        &NodeBinarySet::current(),
        None,
        session(bus("UC_E2E_ABSENT_SECRET_SERVICE_BUS")),
    )
    .expect("spawn daemon");
    let status = wait_for_failed_startup(&daemon.profile).await;

    assert_eq!(status["progress"]["failure"]["reason"], "startup_failed");
    assert_same_secret(
        &read_kek(&daemon.profile),
        &kek,
        "the legacy KEK must stay intact",
    );
    assert_eq!(
        source_record(&daemon.profile).as_deref(),
        Some(r#"{"store":"elsewhere"}"#)
    );
    assert!(daemon_log(&daemon.profile).contains("source record is not recognized"));
}

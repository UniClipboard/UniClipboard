use std::path::PathBuf;

use serde_json::Value;
use uc_e2e_tests::{
    get_session_token, verify_upgrade_userdata_archive, NodeBinarySet, TestCli, TestDaemon,
    TestProfile, UpgradeUserdataFixture, UPGRADE_RELEASES,
};

fn fixture_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/upgrades/v0.19.1/macos-aarch64/single-node-empty")
}

#[test]
fn tracked_v0191_fixture_validates_and_extracts_without_runtime_files() {
    let fixture = UpgradeUserdataFixture::load(fixture_directory()).unwrap();
    assert_eq!(fixture.manifest().source_version, "0.19.1");
    assert_eq!(fixture.manifest().environment, "development");

    let restored = tempfile::tempdir().unwrap();
    let data = restored.path().join("data");
    let cache = restored.path().join("cache");
    fixture
        .restore_into(&data, &cache, "fixture-restored")
        .unwrap();

    for required in [
        "uniclipboard.db",
        "vault/.setup_status",
        "vault/keyslot.json",
        "vault/device_id.txt",
    ] {
        assert!(data.join(required).is_file(), "missing {required}");
    }
    let database_size = std::fs::metadata(data.join("uniclipboard.db"))
        .unwrap()
        .len();
    let wal_size = std::fs::metadata(data.join("uniclipboard.db-wal"))
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    assert!(
        database_size > 4096 || wal_size > 0,
        "tracked SQLite fixture contains neither a checkpoint nor a WAL"
    );
    assert!(
        data.join("iroh-identity_fixture-restored").is_dir(),
        "legacy network identity was not relocated for the fresh profile"
    );
    assert!(
        data.join("iroh-blobs_fixture-restored").is_dir(),
        "legacy blob store was not relocated for the fresh profile"
    );
    let settings: Value =
        serde_json::from_slice(&std::fs::read(data.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["general"]["telemetry_enabled"], false);
    assert_eq!(settings["general"]["usage_analytics_enabled"], false);
    for forbidden in [
        "daemon.conn",
        ".daemon-token",
        ".daemon-pid",
        ".uniclipd.lock",
        "daemon-run.json",
        "daemon-last-exit.json",
        "e2e-daemon-process.log",
        "uniclipboard.db-shm",
    ] {
        assert!(
            !data.join(forbidden).exists(),
            "tracked runtime file {forbidden}"
        );
    }
}

#[test]
fn modified_v0191_fixture_is_rejected_before_extraction() {
    let directory = fixture_directory();
    let manifest = std::fs::read(directory.join("manifest.json")).unwrap();
    let mut archive = std::fs::read(directory.join("userdata.tar.gz")).unwrap();
    let last = archive.last_mut().expect("fixture archive is not empty");
    *last ^= 0x01;

    let error = verify_upgrade_userdata_archive(&manifest, &archive).unwrap_err();
    assert!(
        error.contains("SHA-256 mismatch"),
        "unexpected error: {error}"
    );
}

#[test]
fn selected_breaking_release_fixtures_validate_and_extract() {
    for release in UPGRADE_RELEASES {
        let directory = selected_fixture_directory(release.version);
        let fixture = UpgradeUserdataFixture::load(&directory)
            .unwrap_or_else(|error| panic!("load {} fixture failed: {error}", release.version));
        assert_eq!(fixture.manifest().source_version, release.version);
        assert_eq!(
            fixture.manifest().source_asset_sha256,
            release.macos_aarch64_asset_sha256
        );
        assert_eq!(fixture.manifest().environment, "development");

        let restored = tempfile::tempdir().unwrap();
        let data = restored.path().join("data");
        let cache = restored.path().join("cache");
        fixture
            .restore_into(
                &data,
                &cache,
                &format!("fixture-{}", release.version.replace('.', "-")),
            )
            .unwrap_or_else(|error| panic!("restore {} fixture failed: {error}", release.version));
        let settings: Value =
            serde_json::from_slice(&std::fs::read(data.join("settings.json")).unwrap()).unwrap();
        assert_eq!(settings["general"]["telemetry_enabled"], false);
        assert_eq!(settings["general"]["usage_analytics_enabled"], false);
        for forbidden in [
            "daemon.conn",
            ".daemon-token",
            ".daemon-pid",
            ".uniclipd.lock",
            "daemon-run.json",
            "daemon-last-exit.json",
            "e2e-daemon-process.log",
            "uniclipboard.db-shm",
        ] {
            assert!(
                !data.join(forbidden).exists(),
                "{} restored forbidden file {forbidden}",
                release.version
            );
        }
    }
}

#[test]
fn selected_breaking_release_fixtures_reject_archive_tampering() {
    for release in UPGRADE_RELEASES {
        let directory = selected_fixture_directory(release.version);
        let manifest = std::fs::read(directory.join("manifest.json")).unwrap();
        let mut archive = std::fs::read(directory.join("userdata.tar.gz")).unwrap();
        let last = archive.last_mut().expect("fixture archive is not empty");
        *last ^= 0x01;

        let error = verify_upgrade_userdata_archive(&manifest, &archive).unwrap_err();
        assert!(
            error.contains("SHA-256 mismatch"),
            "{} returned an unexpected error: {error}",
            release.version
        );
    }
}

#[tokio::test]
#[ignore]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
async fn selected_breaking_release_fixtures_upgrade_to_current_dev_runtime() {
    let selected = std::env::var("UC_E2E_UPGRADE_VERSION").ok();
    for release in UPGRADE_RELEASES {
        if selected
            .as_deref()
            .is_some_and(|version| version != release.version)
        {
            continue;
        }
        let fixture = UpgradeUserdataFixture::load(selected_fixture_directory(release.version))
            .unwrap_or_else(|error| panic!("load {} fixture failed: {error}", release.version));
        let profile = TestProfile::for_upgrade_fixture(&format!(
            "dev-upgrade-{}-matrix-{}",
            release.version.replace('.', "-"),
            uuid::Uuid::new_v4().as_simple()
        ))
        .unwrap();
        fixture
            .restore_into(profile.data_dir(), profile.cache_dir(), &profile.name)
            .unwrap_or_else(|error| panic!("restore {} fixture failed: {error}", release.version));
        let daemon = TestDaemon::start_preserving_with(profile, &NodeBinarySet::current(), None)
            .await
            .unwrap_or_else(|error| panic!("upgrade {} fixture failed: {error}", release.version));
        let cli = TestCli::new(&daemon.profile);

        let status = cli.run_capture(&["--json", "status"]);
        assert!(
            status.success(),
            "{} status failed: {}",
            release.version,
            status.stderr
        );
        let status: Value = serde_json::from_str(status.stdout.trim()).unwrap();
        assert_eq!(
            status["device_trust"]["local_membership"], "active",
            "{} local membership",
            release.version
        );
        let client = reqwest::Client::new();
        let session = get_session_token(&daemon, &client).await;
        let setup_state = client
            .get(format!("{}/v2/setup/state", daemon.base_url()))
            .header("Authorization", format!("Session {session}"))
            .send()
            .await
            .unwrap_or_else(|error| {
                panic!("{} setup state request failed: {error}", release.version)
            });
        assert!(
            setup_state.status().is_success(),
            "{} setup state returned {}",
            release.version,
            setup_state.status()
        );
        let setup_state: Value = setup_state.json().await.unwrap_or_else(|error| {
            panic!("{} setup state was not JSON: {error}", release.version)
        });
        let setup_state = setup_state.get("data").unwrap_or(&setup_state);
        assert_eq!(
            setup_state["rePairingRequired"], true,
            "{} must require explicit re-pairing after upgrade",
            release.version
        );
        let members = cli.run_capture(&["--json", "members"]);
        assert!(
            members.success(),
            "{} members failed: {}",
            release.version,
            members.stderr
        );
        assert_eq!(
            serde_json::from_str::<Value>(members.stdout.trim())
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1,
            "{} member count",
            release.version
        );
    }
    if let Some(selected) = selected {
        assert!(
            UPGRADE_RELEASES
                .iter()
                .any(|release| release.version == selected),
            "selected upgrade fixture is not in the matrix: {selected}"
        );
    }
}

fn selected_fixture_directory(version: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/upgrades")
        .join(format!("v{version}"))
        .join("macos-aarch64/single-node-empty")
}

#[tokio::test]
#[ignore]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
async fn tracked_v0191_fixture_upgrades_to_current_in_a_fresh_dev_profile() {
    let fixture = UpgradeUserdataFixture::load(fixture_directory()).unwrap();
    let profile = TestProfile::new_v0_19_1_upgrade("tracked-fixture-single-node");
    fixture
        .restore_into(profile.data_dir(), profile.cache_dir(), &profile.name)
        .unwrap();
    let legacy_identity = regular_files(
        &profile
            .data_dir()
            .join(format!("iroh-identity_{}", profile.name)),
    );
    assert!(!legacy_identity.is_empty());
    let daemon = TestDaemon::start_preserving_with(profile, &NodeBinarySet::current(), None)
        .await
        .unwrap();
    let cli = TestCli::new(&daemon.profile);

    let status = cli.run_capture(&["--json", "status"]);
    assert!(
        status.success(),
        "fixture upgrade status failed: {}",
        status.stderr
    );
    let status: Value = serde_json::from_str(status.stdout.trim()).unwrap();
    assert_eq!(status["device_trust"]["local_membership"], "active");
    let members = cli.run_capture(&["--json", "members"]);
    assert!(
        members.success(),
        "fixture members failed: {}",
        members.stderr
    );
    assert_eq!(
        serde_json::from_str::<Value>(members.stdout.trim())
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        regular_files(&daemon.profile.data_dir().join("iroh-identity")),
        legacy_identity
    );
}

fn regular_files(root: &std::path::Path) -> Vec<Vec<u8>> {
    let mut files = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| std::fs::read(entry.path()).unwrap())
        .collect::<Vec<_>>();
    files.sort();
    files
}

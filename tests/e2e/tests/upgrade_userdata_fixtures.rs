use std::path::PathBuf;

use serde_json::Value;
use uc_e2e_tests::{
    verify_upgrade_userdata_archive, NodeBinarySet, TestCli, TestDaemon, TestProfile,
    UpgradeUserdataFixture,
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

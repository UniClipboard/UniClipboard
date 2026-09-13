use std::path::{Path, PathBuf};

use serde_json::Value;
use uc_e2e_tests::{
    get_session_token, NodeBinarySet, TestDaemon, TestProfile, UpgradeUserdataFixture,
};

fn fixture_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/upgrades/v0.19.3/macos-aarch64/single-node-mixed-content")
}

fn regular_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).expect("read backup directory") {
            let entry = entry.expect("read backup entry");
            let kind = entry.file_type().expect("read backup entry type");
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    files
}

#[tokio::test]
#[ignore]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
async fn v0193_upgrade_preserves_verified_backup_before_startup_changes() {
    let fixture = UpgradeUserdataFixture::load(fixture_directory()).expect("load v0.19.3 fixture");
    let expected: Value = serde_json::from_slice(
        &std::fs::read(fixture_directory().join("expected.json")).expect("read expectations"),
    )
    .expect("decode expectations");
    let passphrase = expected["passphrase"]
        .as_str()
        .expect("expected fixture passphrase");
    let profile = TestProfile::for_upgrade_fixture(&format!(
        "dev-upgrade-backup-v0193-{}",
        uuid::Uuid::new_v4().as_simple()
    ))
    .expect("create isolated profile");
    fixture
        .restore_into(profile.data_dir(), profile.cache_dir(), &profile.name)
        .expect("restore v0.19.3 fixture");
    let original_settings =
        std::fs::read(profile.data_dir().join("settings.json")).expect("read original settings");

    let mut daemon = TestDaemon::start_preserving_with(profile, &NodeBinarySet::current(), None)
        .await
        .expect("start current daemon on v0.19.3 data");
    let client = reqwest::Client::new();
    let session = get_session_token(&daemon, &client).await;
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

    let history = client
        .get(format!("{}/clipboard/entries?limit=100", daemon.base_url()))
        .header("Authorization", format!("Session {session}"))
        .send()
        .await
        .expect("read upgraded history");
    assert!(history.status().is_success(), "upgraded history was unavailable");
    let history: Value = history.json().await.expect("decode upgraded history");
    let entries = history
        .get("data")
        .unwrap_or(&history)
        .as_array()
        .expect("history data array");
    for record in expected["records"].as_array().expect("expected records") {
        let id = record["id"].as_str().expect("expected record id");
        assert!(
            entries
                .iter()
                .any(|entry| entry["id"].as_str() == Some(id)),
            "upgraded history lost record {id}"
        );
    }

    let first_backup = regular_files(daemon.profile.upgrade_backup_dir());
    assert!(
        first_backup.iter().any(|path| path.ends_with("current")),
        "verified file backup record was not published"
    );
    assert!(
        first_backup
            .iter()
            .any(|path| path.ends_with("security-current")),
        "protected security material record was not published"
    );
    let archives: Vec<_> = first_backup
        .iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "archive"))
        .cloned()
        .collect();
    assert!(!archives.is_empty(), "verified file archive was not published");

    daemon
        .restart_preserving()
        .await
        .expect("restart upgraded daemon");
    assert_eq!(
        regular_files(daemon.profile.upgrade_backup_dir()),
        first_backup,
        "ordinary restart replaced or duplicated the upgrade backup"
    );

    daemon
        .stop_gracefully()
        .await
        .expect("stop upgraded daemon cleanly");
    std::fs::remove_dir_all(daemon.profile.data_dir()).expect("remove simulated userdata");
    assert!(daemon.profile.upgrade_backup_dir().is_dir());
    assert!(archives.iter().any(|archive| {
        std::fs::read(archive)
            .expect("read backup after userdata removal")
            .windows(original_settings.len())
            .any(|part| part == original_settings)
    }));
}

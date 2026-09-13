use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;
use uc_e2e_tests::{
    get_session_token, NodeBinarySet, TestDaemon, TestProfile, UpgradeUserdataFixture,
};

fn fixture_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/upgrades/v0.19.3/macos-aarch64/single-node-mixed-content")
}

fn large_history_fixture_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/upgrades/v0.19.3/macos-aarch64/single-node-5000-records")
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
    let startup_conn: Value = serde_json::from_slice(
        &std::fs::read(daemon.profile.data_dir().join("daemon-startup.conn"))
            .expect("read startup connection"),
    )
    .expect("decode startup connection");
    let startup_port = startup_conn["port"].as_u64().expect("startup port");
    let startup_token = startup_conn["token"].as_str().expect("startup token");
    let startup_status: Value = client
        .get(format!("http://127.0.0.1:{startup_port}/startup"))
        .bearer_auth(startup_token)
        .send()
        .await
        .expect("read startup progress")
        .json()
        .await
        .expect("decode startup progress");
    assert_eq!(startup_status["service_ready"], true);
    let upgrade_steps = startup_status["progress"]["upgrade"]["steps"]
        .as_array()
        .expect("startup upgrade steps");
    assert_eq!(upgrade_steps[0]["step"], "backing_up");
    assert_eq!(upgrade_steps[0]["completed"], true);

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

#[tokio::test]
#[ignore]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
async fn v0193_upgrade_backs_up_and_preserves_5000_history_records() {
    let fixture_directory = large_history_fixture_directory();
    let fixture = UpgradeUserdataFixture::load(&fixture_directory)
        .expect("load v0.19.3 large-history fixture");
    let expected: Value = serde_json::from_slice(
        &std::fs::read(fixture_directory.join("expected.json")).expect("read expectations"),
    )
    .expect("decode expectations");
    let expected_count = expected["recordCount"].as_u64().expect("record count") as usize;
    let expected_generated_count = expected["generatedRecordCount"]
        .as_u64()
        .expect("generated record count") as usize;
    let generated_prefix = expected["generatedTextPrefix"]
        .as_str()
        .expect("generated text prefix");
    let passphrase = expected["passphrase"].as_str().expect("fixture passphrase");
    let baseline_ids: HashSet<_> = expected["baselineRecordIds"]
        .as_array()
        .expect("baseline record ids")
        .iter()
        .map(|value| value.as_str().expect("baseline record id").to_string())
        .collect();

    let profile = TestProfile::for_upgrade_fixture(&format!(
        "dev-upgrade-backup-v0193-large-{}",
        uuid::Uuid::new_v4().as_simple()
    ))
    .expect("create isolated profile");
    fixture
        .restore_into(profile.data_dir(), profile.cache_dir(), &profile.name)
        .expect("restore v0.19.3 large-history fixture");

    let mut daemon = TestDaemon::start_preserving_with(profile, &NodeBinarySet::current(), None)
        .await
        .expect("start current daemon on large v0.19.3 data");
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
        .expect("unlock upgraded large-history profile");
    assert!(unlock.status().is_success());

    let mut ids = HashSet::new();
    let mut generated_count = 0;
    for offset in (0..expected_count).step_by(1000) {
        let response = client
            .get(format!(
                "{}/clipboard/entries?limit=1000&offset={offset}",
                daemon.base_url()
            ))
            .header("Authorization", format!("Session {session}"))
            .send()
            .await
            .expect("read upgraded history page");
        assert!(response.status().is_success());
        let response: Value = response.json().await.expect("decode upgraded history page");
        let entries = response
            .get("data")
            .unwrap_or(&response)
            .as_array()
            .expect("history data array");
        assert_eq!(entries.len(), 1000, "history page at offset {offset}");
        for entry in entries {
            ids.insert(entry["id"].as_str().expect("history entry id").to_string());
            if entry["preview"]
                .as_str()
                .is_some_and(|preview| preview.starts_with(generated_prefix))
            {
                generated_count += 1;
            }
        }
    }
    assert_eq!(ids.len(), expected_count);
    assert_eq!(generated_count, expected_generated_count);
    assert!(baseline_ids.is_subset(&ids));

    let backup_files = regular_files(daemon.profile.upgrade_backup_dir());
    assert!(backup_files.iter().any(|path| path.ends_with("current")));
    assert!(backup_files.iter().any(|path| path
        .extension()
        .is_some_and(|extension| extension == "archive")));

    daemon
        .stop_gracefully()
        .await
        .expect("stop upgraded large-history daemon");
}

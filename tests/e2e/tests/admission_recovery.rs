use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use uc_e2e_tests::{NodeBinarySet, TestCli, TestDaemon, TestProfile};

const FIXTURE_ENV: &str = "UC_E2E_ADMISSION_RECOVERY_FIXTURE";
const HEALTHY_FIXTURE_ENV: &str = "UC_E2E_HEALTHY_PROFILE_FIXTURE";

#[tokio::test]
async fn bad_metadata_stays_read_only_across_three_restricted_starts() {
    let source = fixture_path();
    let binaries = NodeBinarySet::current();
    let mut observed_guidance = None;
    let profile = TestProfile::new("admission-recovery-three-starts");
    copy_dir(&source, profile.data_dir());
    let before = protected_snapshot(profile.data_dir());
    let mut daemon = TestDaemon::start_preserving_with(profile, &binaries, None)
        .await
        .expect("restricted daemon start");

    for round in 1..=3 {
        let cli = TestCli::new(&daemon.profile);
        let status = cli.run_capture(&["--json", "space", "status"]);
        assert_eq!(status.exit_code, 0, "status failed: {}", status.stderr);
        let payload: Value = serde_json::from_str(&status.stdout).expect("status JSON");
        assert_eq!(
            payload["profile_recovery"]["state"],
            "admission_recovery_required"
        );
        assert!(payload["device_trust"].is_null());
        let guidance = payload["profile_recovery"]["admission"].clone();
        assert_eq!(guidance["category"], "legacy_fallback_invalid");
        assert_eq!(guidance["stage"], "legacy_repository");
        assert_eq!(guidance["action"], "choose_backup");
        if let Some(previous) = &observed_guidance {
            assert_eq!(
                previous, &guidance,
                "recovery guidance changed on round {round}"
            );
        } else {
            observed_guidance = Some(guidance);
        }

        let invite = cli.run_capture(&["--json", "space", "invite"]);
        assert_ne!(
            invite.exit_code, 0,
            "restricted invite unexpectedly succeeded"
        );
        assert!(invite.stdout.trim().is_empty(), "an invitation was emitted");
        assert_eq!(
            invite.stderr.trim(),
            "✗  pairing invitation service unavailable",
            "restricted invite returned an unstable public error"
        );
        for command in [
            &["--json", "member", "list"][..],
            &["--json", "get", "--list"][..],
            &["--json", "send", "--text", "restricted-write"][..],
            &["--json", "mobile", "status"][..],
        ] {
            let denied = cli.run_capture(command);
            assert_ne!(
                denied.exit_code, 0,
                "restricted command unexpectedly succeeded: {command:?}"
            );
            assert!(
                denied.stdout.trim().is_empty(),
                "restricted command emitted business data: {command:?}"
            );
        }

        let after = protected_snapshot(daemon.profile.data_dir());
        assert_eq!(
            before, after,
            "protected profile bytes changed on round {round}"
        );
        assert!(
            !daemon.profile.upgrade_backup_dir().exists(),
            "upgrade unexpectedly created another backup on round {round}"
        );

        if round < 3 {
            daemon
                .restart_preserving_as_gui_with(&binaries)
                .await
                .unwrap_or_else(|error| panic!("restricted restart {round} failed: {error}"));
        }
    }

    daemon.kill();
}

#[test]
fn cli_auto_start_exposes_restricted_recovery() {
    let source = fixture_path();
    let profile = TestProfile::new("admission-recovery-auto-start");
    copy_dir(&source, profile.data_dir());
    let before = protected_snapshot(profile.data_dir());
    let cli = TestCli::new(&profile);

    let status = cli.run_capture(&["--json", "space", "status"]);
    assert_eq!(status.exit_code, 0, "status failed: {}", status.stderr);
    let payload: Value = serde_json::from_str(&status.stdout).expect("status JSON");
    assert_eq!(
        payload["profile_recovery"]["admission"],
        serde_json::json!({
            "category": "legacy_fallback_invalid",
            "stage": "legacy_repository",
            "action": "choose_backup"
        })
    );

    assert_eq!(before, protected_snapshot(profile.data_dir()));
}

#[tokio::test]
async fn healthy_saved_profile_stays_healthy_after_restart() {
    let source = fixture_path_from(HEALTHY_FIXTURE_ENV);
    let profile = TestProfile::new("healthy-profile-two-starts");
    copy_dir(&source, profile.data_dir());
    let mut daemon = TestDaemon::start_preserving_with(profile, &NodeBinarySet::current(), None)
        .await
        .expect("healthy daemon start");

    let first = healthy_cli_snapshot(&TestCli::new(&daemon.profile));
    daemon
        .restart_preserving_as_gui_with(&NodeBinarySet::current())
        .await
        .expect("healthy daemon restart");
    let second = healthy_cli_snapshot(&TestCli::new(&daemon.profile));

    assert_eq!(first, second, "healthy profile changed after restart");
    daemon.kill();
}

fn healthy_cli_snapshot(cli: &TestCli) -> (Value, Value, Value) {
    let status = cli.run_capture(&["--json", "space", "status"]);
    assert_eq!(status.exit_code, 0, "status failed: {}", status.stderr);
    let status: Value = serde_json::from_str(&status.stdout).expect("status JSON");
    assert_eq!(status["profile_recovery"]["state"], "not_required");
    assert_eq!(status["device_trust"]["local_membership"], "active");

    let members = cli.run_capture(&["--json", "member", "list"]);
    assert_eq!(members.exit_code, 0, "members failed: {}", members.stderr);
    let members: Value = serde_json::from_str(&members.stdout).expect("members JSON");

    let history = cli.run_capture(&["--json", "get", "--list", "--limit", "100"]);
    assert_eq!(history.exit_code, 0, "history failed: {}", history.stderr);
    let history: Value = serde_json::from_str(&history.stdout).expect("history JSON");

    (status["device_trust"].clone(), members, history)
}

fn fixture_path() -> PathBuf {
    fixture_path_from(FIXTURE_ENV)
}

fn fixture_path_from(environment: &str) -> PathBuf {
    let path = std::env::var_os(environment)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{environment} must point to a saved profile"));
    assert!(
        path.is_dir(),
        "fixture directory does not exist: {}",
        path.display()
    );
    path
}

fn copy_dir(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create fixture destination");
    for entry in fs::read_dir(source).expect("read fixture directory") {
        let entry = entry.expect("read fixture entry");
        let target = destination.join(entry.file_name());
        let file_type = entry.file_type().expect("read fixture entry type");
        if file_type.is_dir() {
            copy_dir(&entry.path(), &target);
        } else if file_type.is_file() {
            fs::copy(entry.path(), target).expect("copy fixture file");
        }
    }
}

fn protected_snapshot(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut snapshot = BTreeMap::new();
    collect_protected_files(root, root, &mut snapshot);
    snapshot
}

fn collect_protected_files(root: &Path, current: &Path, output: &mut BTreeMap<PathBuf, String>) {
    for entry in fs::read_dir(current).expect("read profile directory") {
        let entry = entry.expect("read profile entry");
        let path = entry.path();
        let relative = path.strip_prefix(root).expect("profile-relative path");
        let file_type = entry.file_type().expect("read profile entry type");
        if file_type.is_dir() {
            collect_protected_files(root, &path, output);
        } else if file_type.is_file() && is_protected(relative) {
            let bytes = fs::read(&path).expect("read protected profile file");
            output.insert(
                relative.to_path_buf(),
                format!("{:x}", Sha256::digest(bytes)),
            );
        }
    }
}

fn is_protected(relative: &Path) -> bool {
    let first = relative.components().next().map(|part| part.as_os_str());
    matches!(
        first.and_then(|part| part.to_str()),
        Some("keyring" | "vault" | "space-control-generations")
    ) || matches!(
        relative.to_str(),
        Some(
            ".engine-upgrade-cursor.json"
                | "upgrade-cursor.json"
                | "device_id.txt"
                | "settings.json"
        )
    )
}

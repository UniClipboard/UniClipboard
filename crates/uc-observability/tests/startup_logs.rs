use chrono::{TimeZone, Utc};
use std::{fs, io::Read};
use uc_observability::startup_logs::{
    export_diagnostic_logs, export_startup_logs, export_startup_logs_with_status,
    DiagnosticArchiveMode, DiagnosticArchiveRequest, HostManifest,
};

#[test]
fn exports_managed_logs_without_a_running_service() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    fs::write(
        logs.join("uniclipboard-gui.json.2026-09-09"),
        "gui failure\n",
    )
    .unwrap();
    fs::write(
        logs.join("uniclipboard-daemon.json.2026-09-09"),
        "daemon failure\n",
    )
    .unwrap();
    fs::write(
        logs.join("engine.2026-09-09.jsonl"),
        "engine connection failure\n",
    )
    .unwrap();
    fs::write(logs.join("secret.txt"), "do not export").unwrap();
    fs::create_dir(logs.join("uniclipboard-cli.json.2026-09-09")).unwrap();
    let output = root.path().join("support.zip");
    export_startup_logs(&logs, &output).unwrap();
    let mut archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    assert_eq!(archive.len(), 4);
    let mut content = String::new();
    archive
        .by_name("logs/uniclipboard-daemon.json.2026-09-09")
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();
    assert_eq!(content, "daemon failure\n");
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["mode"], "offline");
    assert!(manifest["enginePreparation"].is_null());
    assert_eq!(
        manifest["collection"]["includedFiles"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn empty_logs_do_not_replace_an_existing_destination() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("support.zip");
    fs::write(&output, "existing").unwrap();
    assert!(export_startup_logs(root.path(), &output).is_err());
    assert_eq!(fs::read_to_string(output).unwrap(), "existing");
}

#[cfg(unix)]
#[test]
fn ignores_symlinks_to_unrelated_files() {
    let root = tempfile::tempdir().unwrap();
    let secret = root.path().join("secret");
    fs::write(&secret, "secret").unwrap();
    std::os::unix::fs::symlink(secret, root.path().join("uniclipboard-gui.json.2026-09-09"))
        .unwrap();
    assert!(export_startup_logs(root.path(), &root.path().join("out.zip")).is_err());
}

#[test]
fn online_export_embeds_engine_preparation_and_reports_actual_files() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    let engine_record = serde_json::json!({
        "local_schema_version": 2,
        "run_id": "run-1",
        "peer_ref": "peer-1",
        "fields": {
            "event.name": "connection.finished",
            "outcome": "failed",
            "error.phase": "establish",
            "error.reason": "timed_out",
            "attempt_count": 3
        }
    });
    fs::write(
        logs.join("engine.2026-09-11.jsonl"),
        format!("{engine_record}\n"),
    )
    .unwrap();
    fs::write(logs.join("engine.latest.jsonl"), b"ignored").unwrap();
    let output = root.path().join("support.zip");
    let preparation = serde_json::json!({
        "flush": "completed",
        "status": { "runId": "run-1" },
        "otherProcessesFlushed": false
    });

    let report = export_diagnostic_logs(
        &logs,
        &output,
        DiagnosticArchiveRequest {
            mode: DiagnosticArchiveMode::Online,
            since: Some(Utc.with_ymd_and_hms(2026, 9, 11, 0, 0, 0).unwrap()),
            engine_preparation: Some(preparation),
            startup_status: None,
            host: None,
            network: Default::default(),
        },
    )
    .unwrap();

    assert_eq!(report.included_files, ["engine.2026-09-11.jsonl"]);
    assert!(report.unreadable_files.is_empty());
    assert!(report.truncated_files.is_empty());
    let mut archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    let mut engine_log = String::new();
    archive
        .by_name("logs/engine.2026-09-11.jsonl")
        .unwrap()
        .read_to_string(&mut engine_log)
        .unwrap();
    let archived_record: serde_json::Value = serde_json::from_str(engine_log.trim()).unwrap();
    assert_eq!(archived_record, engine_record);
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["mode"], "online");
    assert_eq!(manifest["enginePreparation"]["status"]["runId"], "run-1");
    assert_eq!(
        manifest["collection"]["includedFiles"][0],
        "engine.2026-09-11.jsonl"
    );
}

#[cfg(unix)]
#[test]
fn online_export_keeps_a_partial_package_when_a_managed_file_is_unreadable() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    let unreadable = logs.join("engine.2026-09-11.jsonl");
    fs::write(&unreadable, b"{}\n").unwrap();
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
    let output = root.path().join("support.zip");

    let report = export_diagnostic_logs(
        &logs,
        &output,
        DiagnosticArchiveRequest {
            mode: DiagnosticArchiveMode::Online,
            since: None,
            engine_preparation: Some(serde_json::json!({ "flush": "completed" })),
            startup_status: None,
            host: None,
            network: Default::default(),
        },
    )
    .unwrap();

    assert!(report.included_files.is_empty());
    assert_eq!(report.unreadable_files, ["engine.2026-09-11.jsonl"]);
    let archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    assert_eq!(archive.len(), 1);
}

#[test]
fn offline_export_embeds_terminal_startup_status() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    fs::write(
        logs.join("uniclipboard-daemon.json.2026-09-16"),
        "backup failure\n",
    )
    .unwrap();
    let output = root.path().join("support.zip");
    let startup_status = serde_json::json!({
        "service_failed": true,
        "progress": {
            "state": "failed",
            "failure": { "reason": "backup_failed", "retryable": true }
        }
    });

    export_startup_logs_with_status(&logs, &output, Some(startup_status.clone())).unwrap();

    let mut archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["mode"], "offline");
    assert_eq!(manifest["startupStatus"], startup_status);
}

#[test]
fn offline_export_fails_when_every_log_is_outside_the_window() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    fs::write(
        logs.join("uniclipboard-daemon.json.2026-09-09"),
        "old failure\n",
    )
    .unwrap();
    let output = root.path().join("support.zip");
    fs::write(&output, "existing").unwrap();

    let result = export_diagnostic_logs(
        &logs,
        &output,
        DiagnosticArchiveRequest {
            mode: DiagnosticArchiveMode::Offline,
            since: Some(Utc::now() + chrono::Duration::days(1)),
            engine_preparation: None,
            startup_status: None,
            host: None,
            network: Default::default(),
        },
    );

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(output).unwrap(), "existing");
}

/// Keeps a log unreadable to the exporter for as long as the guard lives.
struct UnreadableLog {
    #[cfg(windows)]
    _exclusive: fs::File,
}

/// Write an earlier-run log whose name and modification time both fall
/// before `2026-09-02`, then make it unreadable.
#[allow(clippy::unwrap_used)]
fn unreadable_log(path: &std::path::Path, content: &str) -> UnreadableLog {
    fs::write(path, content).unwrap();
    let earlier = std::time::SystemTime::from(Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap());
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(earlier)
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
        UnreadableLog {}
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let exclusive = fs::File::options()
            .read(true)
            .share_mode(0)
            .open(path)
            .unwrap();
        UnreadableLog {
            _exclusive: exclusive,
        }
    }
}

#[allow(clippy::unwrap_used)]
fn since_2026_09_02() -> Option<chrono::DateTime<Utc>> {
    Some(Utc.with_ymd_and_hms(2026, 9, 2, 0, 0, 0).unwrap())
}

#[cfg(any(unix, windows))]
#[test]
fn offline_export_fails_when_unreadable_logs_are_outside_the_window() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    let _unreadable = unreadable_log(
        &logs.join("uniclipboard-daemon.json.2026-09-01"),
        "earlier failure\n",
    );
    let output = root.path().join("support.zip");
    fs::write(&output, "existing").unwrap();

    let result = export_diagnostic_logs(
        &logs,
        &output,
        DiagnosticArchiveRequest {
            mode: DiagnosticArchiveMode::Offline,
            since: since_2026_09_02(),
            engine_preparation: None,
            startup_status: None,
            host: None,
            network: Default::default(),
        },
    );

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(output).unwrap(), "existing");
}

#[cfg(any(unix, windows))]
#[test]
fn online_export_does_not_report_unreadable_logs_outside_the_window() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    let _unreadable = unreadable_log(
        &logs.join("uniclipboard-daemon.json.2026-09-01"),
        "earlier failure\n",
    );
    fs::write(logs.join("engine.2026-09-11.jsonl"), "{}\n").unwrap();
    let output = root.path().join("support.zip");

    let report = export_diagnostic_logs(
        &logs,
        &output,
        DiagnosticArchiveRequest {
            mode: DiagnosticArchiveMode::Online,
            since: since_2026_09_02(),
            engine_preparation: None,
            startup_status: None,
            host: None,
            network: Default::default(),
        },
    )
    .unwrap();

    assert_eq!(report.included_files, ["engine.2026-09-11.jsonl"]);
    assert!(report.unreadable_files.is_empty());
}

#[cfg(any(unix, windows))]
#[test]
fn offline_export_keeps_startup_status_when_every_log_in_the_window_is_unreadable() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    let _unreadable = unreadable_log(
        &logs.join("uniclipboard-daemon.json.2026-09-01"),
        "startup failure\n",
    );
    let output = root.path().join("support.zip");
    let startup_status = serde_json::json!({ "service_failed": true });

    export_startup_logs_with_status(&logs, &output, Some(startup_status.clone())).unwrap();

    let mut archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    assert_eq!(archive.len(), 1);
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["startupStatus"], startup_status);
    assert_eq!(
        manifest["collection"]["unreadableFiles"][0],
        "uniclipboard-daemon.json.2026-09-01"
    );
}

#[test]
fn export_failures_are_classified_by_where_they_happened() {
    use uc_observability::startup_logs::ExportFailureCause;

    // Nothing to export.
    let root = tempfile::tempdir().unwrap();
    let error = export_startup_logs(root.path(), &root.path().join("out.zip")).unwrap_err();
    assert_eq!(error.cause(), ExportFailureCause::NoLogs);

    // The log directory cannot be listed: a file stands where it should be.
    let not_a_directory = root.path().join("logs");
    fs::write(&not_a_directory, "file").unwrap();
    let error = export_startup_logs(&not_a_directory, &root.path().join("out.zip")).unwrap_err();
    assert_eq!(error.cause(), ExportFailureCause::SourceRead);

    // The archive cannot be created next to a destination that has no parent.
    let logs = root.path().join("real-logs");
    fs::create_dir(&logs).unwrap();
    fs::write(logs.join("uniclipboard-gui.json.2026-09-09"), "x\n").unwrap();
    let error =
        export_startup_logs(&logs, &root.path().join("missing").join("out.zip")).unwrap_err();
    assert_eq!(error.cause(), ExportFailureCause::ArchiveWrite);
}

fn read_manifest(output: &std::path::Path) -> serde_json::Value {
    let mut archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    serde_json::from_str(&manifest).unwrap()
}

#[test]
fn the_manifest_names_the_sources_that_contributed_nothing() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    fs::write(logs.join("uniclipboard-gui.json.2026-09-09"), "gui\n").unwrap();
    fs::write(logs.join("engine.2026-09-09.jsonl"), "engine\n").unwrap();
    let output = root.path().join("support.zip");

    export_startup_logs(&logs, &output).unwrap();

    let manifest = read_manifest(&output);
    let missing: Vec<(String, String)> = manifest["collection"]["missingSources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["source"].as_str().unwrap().to_owned(),
                entry["reason"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        missing,
        [
            ("daemon".to_owned(), "missing".to_owned()),
            ("cli".to_owned(), "missing".to_owned()),
            ("quick-panel".to_owned(), "missing".to_owned()),
        ]
    );
}

#[test]
fn the_manifest_describes_the_exporting_host() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    fs::write(logs.join("uniclipboard-daemon.json.2026-09-09"), "daemon\n").unwrap();
    let output = root.path().join("support.zip");

    export_diagnostic_logs(
        &logs,
        &output,
        DiagnosticArchiveRequest {
            mode: DiagnosticArchiveMode::Online,
            since: None,
            engine_preparation: None,
            startup_status: None,
            host: Some(HostManifest {
                app_version: "1.2.0".to_owned(),
                build_commit: "unknown".to_owned(),
                os: "macos".to_owned(),
                arch: "aarch64".to_owned(),
                profile: Some("dev".to_owned()),
                portable: false,
                installed_log_profile: "debug".to_owned(),
                telemetry_enabled: true,
                timezone_offset_minutes: 480,
            }),
            network: Default::default(),
        },
    )
    .unwrap();

    let host = read_manifest(&output)["host"].clone();
    assert_eq!(
        host,
        serde_json::json!({
            "appVersion": "1.2.0",
            "buildCommit": "unknown",
            "os": "macos",
            "arch": "aarch64",
            "profile": "dev",
            "portable": false,
            "installedLogProfile": "debug",
            "telemetryEnabled": true,
            "timezoneOffsetMinutes": 480
        })
    );
}

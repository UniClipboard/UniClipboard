use chrono::{TimeZone, Utc};
use std::{fs, io::Read};
use uc_observability::startup_logs::{
    export_diagnostic_logs, export_startup_logs, DiagnosticArchiveMode, DiagnosticArchiveRequest,
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
    fs::write(logs.join("engine.2026-09-11.jsonl"), b"{}\n").unwrap();
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
        },
    )
    .unwrap();

    assert_eq!(report.included_files, ["engine.2026-09-11.jsonl"]);
    assert!(report.unreadable_files.is_empty());
    assert!(report.truncated_files.is_empty());
    let mut archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
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
        },
    )
    .unwrap();

    assert!(report.included_files.is_empty());
    assert_eq!(report.unreadable_files, ["engine.2026-09-11.jsonl"]);
    let archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    assert_eq!(archive.len(), 1);
}

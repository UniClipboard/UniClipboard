//! Bounded record of a daemon start that failed before its logs were usable,
//! and its presence in the diagnostic export.

use std::fs;
use std::io::Read;

use uc_observability::startup_failure::{
    read_startup_failure, write_startup_failure, StartupFailure, StartupFailureClass, StartupStep,
    MAX_STARTUP_FAILURE_BYTES, STARTUP_FAILURE_FILE,
};
use uc_observability::startup_logs::export_startup_logs;

fn failure(step: StartupStep, class: StartupFailureClass) -> StartupFailure {
    StartupFailure::now(step, class)
}

#[test]
fn a_written_failure_is_read_back_with_its_step_and_class() {
    let logs = tempfile::tempdir().unwrap();
    let written = failure(StartupStep::InstanceLock, StartupFailureClass::LockTimeout);

    write_startup_failure(logs.path(), &written).unwrap();
    let read = read_startup_failure(logs.path()).unwrap().unwrap();

    assert_eq!(read, written);
    let raw: serde_json::Value =
        serde_json::from_slice(&fs::read(logs.path().join(STARTUP_FAILURE_FILE)).unwrap()).unwrap();
    assert_eq!(raw["step"], "instance_lock");
    assert_eq!(raw["class"], "lock_timeout");
    assert_eq!(raw["pid"], u64::from(std::process::id()));
    assert!(raw["runId"].is_string());
    assert!(raw["recordedAt"].is_string());
}

#[test]
fn the_record_is_replaced_not_accumulated() {
    let logs = tempfile::tempdir().unwrap();
    write_startup_failure(
        logs.path(),
        &failure(StartupStep::InstanceLock, StartupFailureClass::LockTimeout),
    )
    .unwrap();
    write_startup_failure(
        logs.path(),
        &failure(StartupStep::Tracing, StartupFailureClass::Io),
    )
    .unwrap();

    let read = read_startup_failure(logs.path()).unwrap().unwrap();
    assert_eq!(read.step, StartupStep::Tracing);
    let size = fs::metadata(logs.path().join(STARTUP_FAILURE_FILE))
        .unwrap()
        .len();
    assert!(size <= MAX_STARTUP_FAILURE_BYTES, "{size} bytes");
    let entries: Vec<_> = fs::read_dir(logs.path()).unwrap().collect();
    assert_eq!(entries.len(), 1, "no temporary file is left behind");
}

#[test]
fn writing_creates_the_log_directory() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("not").join("yet").join("there");

    write_startup_failure(
        &logs,
        &failure(StartupStep::PathResolution, StartupFailureClass::Internal),
    )
    .unwrap();

    assert!(logs.join(STARTUP_FAILURE_FILE).is_file());
}

#[test]
fn a_missing_record_is_none_and_an_oversized_one_is_refused() {
    let logs = tempfile::tempdir().unwrap();
    assert!(read_startup_failure(logs.path()).unwrap().is_none());

    fs::write(
        logs.path().join(STARTUP_FAILURE_FILE),
        vec![b'x'; MAX_STARTUP_FAILURE_BYTES as usize + 1],
    )
    .unwrap();
    assert!(read_startup_failure(logs.path()).is_err());
}

#[test]
fn the_offline_export_includes_the_record_even_without_any_other_log() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    write_startup_failure(
        &logs,
        &failure(StartupStep::InstanceLock, StartupFailureClass::LockTimeout),
    )
    .unwrap();
    let output = root.path().join("support.zip");

    export_startup_logs(&logs, &output).unwrap();

    let mut archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    let mut content = String::new();
    archive
        .by_name(&format!("logs/{STARTUP_FAILURE_FILE}"))
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();
    assert!(content.contains("instance_lock"));
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(
        manifest["collection"]["includedFiles"][0],
        STARTUP_FAILURE_FILE
    );
}

#[test]
fn an_unreadable_record_is_reported_instead_of_dropped() {
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    fs::create_dir(&logs).unwrap();
    fs::write(logs.join("uniclipboard-daemon.json.2026-09-09"), "x\n").unwrap();
    // A directory with the record's name cannot be read as a file.
    fs::create_dir(logs.join(STARTUP_FAILURE_FILE)).unwrap();
    let output = root.path().join("support.zip");

    export_startup_logs(&logs, &output).unwrap();

    let mut archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(
        manifest["collection"]["unreadableFiles"][0],
        STARTUP_FAILURE_FILE
    );
}

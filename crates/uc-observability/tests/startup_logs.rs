use std::{fs, io::Read};
use uc_observability::startup_logs::export_startup_logs;

#[test]
fn exports_only_role_logs_without_a_running_service() {
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
    fs::write(logs.join("secret.txt"), "do not export").unwrap();
    fs::create_dir(logs.join("uniclipboard-cli.json.2026-09-09")).unwrap();
    let output = root.path().join("support.zip");
    export_startup_logs(&logs, &output).unwrap();
    let mut archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
    assert_eq!(archive.len(), 2);
    let mut content = String::new();
    archive
        .by_name("uniclipboard-daemon.json.2026-09-09")
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();
    assert_eq!(content, "daemon failure\n");
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

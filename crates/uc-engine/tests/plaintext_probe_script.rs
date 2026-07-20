use std::path::PathBuf;
use std::process::{Command, Output};

fn scanner_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("scripts/security/scan-plaintext-probe.sh")
}

fn run_scanner(probe_file: &std::path::Path, roots: &[&std::path::Path]) -> Output {
    let mut command = Command::new("bash");
    command.arg(scanner_path()).arg(probe_file);
    command.args(roots);
    command.output().expect("failed to run plaintext scanner")
}

#[test]
fn scanner_accepts_clean_roots_without_revealing_the_probe() {
    let temp = tempfile::tempdir().unwrap();
    let probe = "runtime-probe-value-123456";
    let probe_file = temp.path().join("probe.txt");
    let root = temp.path().join("private");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&probe_file, probe).unwrap();
    std::fs::write(root.join("encrypted.db"), b"ciphertext-only").unwrap();

    let output = run_scanner(&probe_file, &[root.as_path()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "scanner failed: {stderr}");
    assert!(stdout.contains("Scanned root:"));
    assert!(stdout.contains("File types:"));
    assert!(!stdout.contains(probe));
    assert!(!stderr.contains(probe));
}

#[test]
fn scanner_reports_leaking_files_without_revealing_the_probe() {
    let temp = tempfile::tempdir().unwrap();
    let probe = "runtime-probe-value-654321";
    let probe_file = temp.path().join("probe.txt");
    let root = temp.path().join("cache");
    let leak = root.join(format!("leaked-{probe}.bin"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&probe_file, probe).unwrap();
    std::fs::write(&leak, format!("prefix-{probe}-suffix")).unwrap();

    let output = run_scanner(&probe_file, &[root.as_path()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.contains("leaked-[REDACTED].bin"));
    assert!(!stdout.contains(probe));
    assert!(!stderr.contains(probe));
}

#[test]
fn scanner_rejects_missing_roots() {
    let temp = tempfile::tempdir().unwrap();
    let probe_file = temp.path().join("probe.txt");
    let missing = temp.path().join("missing");
    std::fs::write(&probe_file, "runtime-probe-value").unwrap();

    let output = run_scanner(&probe_file, &[missing.as_path()]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Scan root does not exist:"));
}

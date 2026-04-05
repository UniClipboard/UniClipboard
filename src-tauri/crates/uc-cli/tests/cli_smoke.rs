//! CLI smoke tests — validates binary invocation, help output, exit codes, and version flag.

use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn cli_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_uniclipboard-cli"))
}

fn smoke_test_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn test_help_output() {
    let _guard = smoke_test_guard();
    let output = cli_binary()
        .arg("--help")
        .output()
        .expect("failed to execute uniclipboard-cli");

    assert!(
        output.status.success(),
        "Expected exit code 0 for --help, got {:?}",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("status"),
        "Help output should mention 'status' subcommand, got: {}",
        stdout
    );
    assert!(
        stdout.contains("devices"),
        "Help output should mention 'devices' subcommand, got: {}",
        stdout
    );
    assert!(
        stdout.contains("space-status"),
        "Help output should mention 'space-status' subcommand, got: {}",
        stdout
    );
}

#[test]
fn test_version_flag() {
    let _guard = smoke_test_guard();
    let output = cli_binary()
        .arg("--version")
        .output()
        .expect("failed to execute uniclipboard-cli");

    assert!(
        output.status.success(),
        "Expected exit code 0 for --version, got {:?}",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("uniclipboard-cli"),
        "Version output should contain binary name, got: {}",
        stdout
    );
}

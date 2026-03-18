//! CLI smoke tests — validates binary invocation, help output, exit codes, and version flag.

use std::process::Command;

fn cli_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_uniclipboard-cli"))
}

#[test]
fn test_help_output() {
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

#[test]
fn test_status_daemon_unreachable() {
    let output = cli_binary()
        .arg("status")
        .output()
        .expect("failed to execute uniclipboard-cli");

    let exit_code = output.status.code().expect("process terminated by signal");
    assert_eq!(
        exit_code, 5,
        "Expected exit code 5 (daemon unreachable), got {}",
        exit_code
    );
}

#[test]
fn test_status_json_daemon_unreachable() {
    let output = cli_binary()
        .args(["--json", "status"])
        .output()
        .expect("failed to execute uniclipboard-cli");

    let exit_code = output.status.code().expect("process terminated by signal");
    assert_eq!(
        exit_code, 5,
        "Expected exit code 5 (daemon unreachable) with --json, got {}",
        exit_code
    );
}

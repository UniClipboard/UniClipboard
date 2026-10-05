//! TestCli — ergonomic command builder for `uniclip` with profile isolation.

use std::path::PathBuf;
use std::process::{Command, Output};

use crate::{NodeBinarySet, TestProfile};

/// Builder for running `uniclip` commands against a specific test profile.
pub struct TestCli {
    binary: PathBuf,
    pub profile_name: String,
}

impl TestCli {
    /// Create a CLI helper bound to the given profile.
    pub fn new(profile: &TestProfile) -> Self {
        Self::with_binaries(profile, &NodeBinarySet::current())
    }

    pub fn with_binaries(profile: &TestProfile, binaries: &NodeBinarySet) -> Self {
        Self {
            binary: binaries.cli.clone(),
            profile_name: profile.name.clone(),
        }
    }

    /// Path to the `uniclip` binary.
    pub fn binary_path(&self) -> &std::path::Path {
        &self.binary
    }

    /// Run a uniclip command with the test profile automatically set.
    /// Returns the raw Output for assertions.
    ///
    /// Panics with a build hint when the binary under test is missing: the
    /// suite runs a prebuilt CLI (the Go `uniclip`, or `uc-dev-cli` for the
    /// development commands) and a bare "No such file" is hard to act on.
    pub fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        assert!(
            self.binary.is_file(),
            "CLI binary not found at {}. Build the user-facing CLI with \
             `scripts/e2e/build-cli.sh` (set UC_E2E_CLI to use another binary), or the \
             development CLI with `cargo build -p uc-dev-cli --features uc-dev-cli/dev-tools` \
             (UC_E2E_DEV_CLI overrides its path).",
            self.binary.display()
        );
        Command::new(&self.binary)
            .env("UC_PROFILE", &self.profile_name)
            .env("UNICLIPBOARD_ENV", "development")
            .args(args)
            .output()
    }

    /// Run a command and assert it succeeded (exit code 0), returning stdout.
    pub fn run_ok(&self, args: &[&str]) -> String {
        let output = self.run(args).expect("failed to execute uniclip");
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!(
                "uniclip {:?} failed (exit={:?}):\nstderr: {}",
                args,
                output.status.code(),
                stderr
            );
        }
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    /// Run a command and return exit code + stdout + stderr without asserting.
    pub fn run_capture(&self, args: &[&str]) -> CapturedOutput {
        let output = self.run(args).expect("failed to execute uniclip");
        CapturedOutput {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        }
    }
}

/// Captured output from a CLI invocation.
#[derive(Debug)]
pub struct CapturedOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CapturedOutput {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

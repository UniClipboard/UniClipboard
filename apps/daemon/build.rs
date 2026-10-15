//! Stamps the commit the daemon is built from into `UC_BUILD_COMMIT`.
//!
//! `UC_BUILD_COMMIT` from the environment wins (CI, source archives without a
//! `.git`); otherwise the checkout's HEAD is used. A build with neither is
//! stamped `unknown` rather than guessed. Uncommitted working-tree changes are
//! not detected.

use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=UC_BUILD_COMMIT");
    let commit = std::env::var("UC_BUILD_COMMIT")
        .ok()
        .filter(|value| is_commit(value))
        .or_else(|| git(&["rev-parse", "HEAD"]).filter(|value| is_commit(value)));
    println!(
        "cargo:rustc-env=UC_BUILD_COMMIT={}",
        commit.as_deref().unwrap_or("unknown")
    );
    // Rebuild when HEAD moves; in a worktree the git directory is private to it.
    if let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        for relative in ["HEAD", "logs/HEAD"] {
            let path = Path::new(&git_dir).join(relative);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
        .map(|text| text.trim().to_owned())
}

fn is_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

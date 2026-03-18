---
status: diagnosed
phase: 41-daemon-and-cli-skeletons
source: 41-01-SUMMARY.md, 41-02-SUMMARY.md, 41-03-SUMMARY.md
started: 2026-03-18T14:15:00Z
updated: 2026-03-18T14:30:00Z
---

## Current Test

[testing complete]

## Tests

### 1. Daemon binary builds successfully

expected: Run `cd src-tauri && cargo build -p uc-daemon`. Build completes without errors. Binary exists at `target/debug/uniclipboard-daemon`.
result: pass

### 2. CLI binary builds successfully

expected: Run `cd src-tauri && cargo build -p uc-cli`. Build completes without errors. Binary exists at `target/debug/uniclipboard-cli`.
result: pass

### 3. CLI --help shows subcommands

expected: Run `cd src-tauri && cargo run -p uc-cli -- --help`. Output shows "status", "devices", and "space-status" as available subcommands, plus a global `--json` flag.
result: pass

### 4. CLI --version shows version

expected: Run `cd src-tauri && cargo run -p uc-cli -- --version`. Output shows the crate version string (e.g. "uniclipboard-cli 0.1.0").
result: pass

### 5. CLI status returns exit code 5 when daemon not running

expected: With no daemon running, execute `cd src-tauri && cargo run -p uc-cli -- status; echo "exit: $?"`. The command prints an error about daemon being unreachable and exits with code 5.
result: pass

### 6. Daemon starts and responds to ping

expected: Start daemon with `cd src-tauri && cargo run -p uc-daemon &`, wait 2 seconds, then send a JSON-RPC ping: `echo '{"jsonrpc":"2.0","method":"ping","id":1}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/uniclipboard-daemon.sock` (or the platform socket path). Response should contain `"result":"pong"`. Kill daemon afterwards.
result: issue
reported: "Daemon fails to start with error: path must be shorter than SUN_LEN"
severity: blocker

### 7. Daemon graceful shutdown on SIGTERM

expected: Start daemon, note its PID, send `kill -TERM <pid>`. Daemon exits cleanly (exit code 0), socket file is removed from disk.
result: issue
reported: "Cannot test because daemon fails to start (blocked by test 6 SUN_LEN issue)"
severity: blocker

### 8. CLI smoke tests pass

expected: Run `cd src-tauri && cargo test -p uc-cli`. All 4 smoke tests (help, version, exit codes) pass.
result: pass

### 9. Daemon unit tests pass

expected: Run `cd src-tauri && cargo test -p uc-daemon`. All unit tests (handler dispatch, RPC type serde, runtime state) pass.
result: pass

## Summary

total: 9
passed: 7
issues: 2
pending: 0
skipped: 0

## Gaps

- truth: "Daemon starts and accepts JSON-RPC connections on Unix socket"
  status: failed
  reason: "User reported: Daemon fails to start with error: path must be shorter than SUN_LEN"
  severity: blocker
  test: 6
  root_cause: "Socket path app_data_root/uniclipboard-daemon.sock resolves to ~/Library/Application Support/app.uniclipboard.desktop/uniclipboard-daemon.sock (~91 bytes for short usernames), exceeding macOS sockaddr_un.sun_path 104-byte limit for longer usernames or deep worktree paths"
  artifacts:
  - path: "src-tauri/crates/uc-daemon/src/main.rs"
    issue: "Lines 20-23 construct socket path from app_data_root which is too long on macOS"
  - path: "src-tauri/crates/uc-cli/src/commands/status.rs"
    issue: "CLI status command uses same long socket path pattern"
    missing:
  - "Use /tmp/uniclipboard-daemon.sock or $TMPDIR/uc-daemon.sock instead of app_data_root for socket path in both daemon and CLI"
    debug_session: ""

- truth: "Daemon exits cleanly on SIGTERM and removes socket file"
  status: failed
  reason: "User reported: Cannot test because daemon fails to start (blocked by test 6 SUN_LEN issue)"
  severity: blocker
  test: 7
  root_cause: "Blocked by gap 1 — same root cause (socket path too long). Once socket path is fixed, graceful shutdown should be retestable."
  artifacts:
  - path: "src-tauri/crates/uc-daemon/src/app.rs"
    issue: "Shutdown logic exists but untestable until socket bind succeeds"
    missing:
  - "Fix socket path (gap 1), then retest shutdown behavior"
    debug_session: ""

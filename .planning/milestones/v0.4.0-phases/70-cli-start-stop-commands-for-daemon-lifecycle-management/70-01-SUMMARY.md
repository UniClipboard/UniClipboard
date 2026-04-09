---
phase: 70-cli-start-stop-commands-for-daemon-lifecycle-management
plan: "01"
subsystem: cli
tags:
  - cli
  - daemon-lifecycle
  - start-stop
  - process-management
dependency_graph:
  requires:
    - uc-cli/local_daemon.rs (ensure_local_daemon_running, resolve_daemon_binary_path)
    - uc-daemon/process_metadata.rs (read_pid_file)
  provides:
    - uniclipboard-cli start (background + foreground modes)
    - uniclipboard-cli stop (SIGTERM + polling)
  affects:
    - CLI command surface (two new top-level subcommands)
tech_stack:
  added:
    - libc = "0.2" (uc-cli dependency for POSIX signal operations)
  patterns:
    - Injectable closure pattern for testable command logic (run_start_background_with, run_stop_with)
    - Idempotent CLI commands (already-running/not-running = exit 0)
key_files:
  created:
    - src-tauri/crates/uc-cli/src/commands/start.rs
    - src-tauri/crates/uc-cli/src/commands/stop.rs
  modified:
    - src-tauri/crates/uc-cli/src/commands/mod.rs
    - src-tauri/crates/uc-cli/src/main.rs
    - src-tauri/crates/uc-cli/src/local_daemon.rs
    - src-tauri/crates/uc-cli/Cargo.toml
decisions:
  - "Background start reuses ensure_local_daemon_running() for probe-spawn-poll pattern consistency"
  - "Foreground mode checks for already-running daemon first before spawning with inherited stdio"
  - "stop uses libc::kill(pid, 0) for stale PID detection before sending SIGTERM"
  - "SIGKILL not used -- user warned if daemon does not stop within 10s timeout"
  - "libc added directly to uc-cli (not workspace) since no other crate needs it"
metrics:
  duration: "~8min"
  completed: "2026-03-28"
  tasks_completed: 2
  files_changed: 6
---

# Phase 70 Plan 01: CLI start/stop Commands for Daemon Lifecycle Management Summary

**One-liner:** `uniclipboard-cli start/stop` commands with background/foreground modes, SIGTERM-based stop polling, and injectable closure testing pattern.

## What Was Built

Two new top-level CLI subcommands for daemon lifecycle management:

### `uniclipboard-cli start`

- **Background mode** (default): Reuses `ensure_local_daemon_running()` to probe-spawn-poll. Returns `{"status": "started", "pid": N}` or `{"status": "already_running", "pid": N}`.
- **Foreground mode** (`--foreground` / `-f`): Checks for already-running first, then spawns daemon with `Stdio::inherit()` for log streaming. Blocks until daemon exits.
- **Idempotent**: Already-running daemon returns exit 0.
- `StartOutput` struct with `#[derive(Serialize)]` and `impl fmt::Display`.

### `uniclipboard-cli stop`

- Reads PID via `uc_daemon::process_metadata::read_pid_file()`.
- Stale PID guard: `libc::kill(pid, 0)` check before sending SIGTERM.
- SIGTERM via `libc::kill(pid, SIGTERM)` (Unix) / `taskkill` (Windows).
- 10-second polling loop with 200ms interval.
- **Idempotent**: No PID file or stale PID returns exit 0 with `"not_running"`.
- No SIGKILL escalation — user warned if timeout exceeded.
- `StopOutput` struct with `#[derive(Serialize)]` and `impl fmt::Display`.

### Supporting Changes

- `local_daemon::resolve_daemon_binary_path()` changed to `pub(crate)` for foreground mode access.
- `Commands` enum extended with `Start { foreground: bool }` and `Stop` variants.
- Both commands wired in `main.rs` match block.
- `libc = "0.2"` added to `uc-cli/Cargo.toml`.

## Tests

12 unit tests added (6 in `start::tests`, 6 in `stop::tests`):

**start::tests:**
- `start_background_already_running` — session.spawned=false returns "already_running" with pid
- `start_background_spawned` — session.spawned=true returns "started" with pid
- `start_background_spawn_failure` — LocalDaemonError::Spawn returns Err
- `json_output_start_already_running` — JSON contains "status" and "pid" fields
- `json_output_start_started` — JSON serializes correctly
- `display_output_start` — Display produces "Daemon started (pid N)" and "Daemon already running (pid N)"

**stop::tests:**
- `stop_no_pid_file` — read_pid returns None, exits 0
- `stop_pid_file_stale` — read_pid returns Some but process not running, exits 0
- `stop_success` — process running first check, stopped after SIGTERM, exits 0
- `stop_timeout` — SIGTERM fails returns EXIT_ERROR immediately
- `json_output_stop` — JSON serializes with "status" field
- `display_output_stop` — Display produces "Daemon stopped" and "Daemon is not running"

## Deviations from Plan

None — plan executed exactly as written.

## Known Stubs

None — both commands are fully wired with real implementations.

## Self-Check: PASSED

- `src-tauri/crates/uc-cli/src/commands/start.rs`: FOUND
- `src-tauri/crates/uc-cli/src/commands/stop.rs`: FOUND
- Commit d33625af: FOUND
- Commit fe01ce3d: FOUND
- `cargo check -p uc-cli`: clean (0 warnings)
- `cargo test -p uc-cli start::`: 6 passed
- `cargo test -p uc-cli tests::`: 45 passed (includes stop unit tests)

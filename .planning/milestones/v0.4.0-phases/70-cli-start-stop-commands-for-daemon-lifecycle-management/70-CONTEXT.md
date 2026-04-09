# Phase 70: CLI start/stop commands for daemon lifecycle management - Context

**Gathered:** 2026-03-28
**Status:** Ready for planning

<domain>
## Phase Boundary

Add `start` and `stop` subcommands to the CLI for daemon lifecycle management. `start` launches the daemon (background by default, foreground optional with log streaming). `stop` gracefully terminates a running daemon. No restart, reload, or daemon-as-service features.

</domain>

<decisions>
## Implementation Decisions

### Command interface

- **D-01:** `start` and `stop` are top-level `Commands` enum variants (same level as `Status`, `Devices`, `Setup`, `SpaceStatus`)
- **D-02:** Both commands accept the existing global `--json` and `--verbose` flags
- **D-03:** `start` accepts a `--foreground` (short: `-f`) flag to run in foreground mode instead of background

### Start behavior

- **D-04:** Default mode is background: spawn daemon process detached, poll health endpoint until healthy, print success and exit
- **D-05:** Reuse existing `local_daemon::ensure_local_daemon_running()` logic for the background path (probe health, spawn if needed, wait for healthy)
- **D-06:** If daemon is already running (health probe succeeds), print "daemon already running" and exit 0 (idempotent)
- **D-07:** Foreground mode (`--foreground`): spawn daemon process with stdout/stderr inherited (not piped to null), CLI process waits for daemon to exit or Ctrl+C. No detach.
- **D-08:** Foreground mode should NOT pass `--gui-managed` to the daemon binary (stdin tether is a GUI concept, not CLI)

### Stop behavior

- **D-09:** Read PID from profile-aware PID file via `process_metadata::read_pid_file()`
- **D-10:** Send SIGTERM to the PID (Unix) / TerminateProcess on Windows
- **D-11:** Poll until process exits or timeout (reuse similar polling pattern as `wait_for_daemon_health`)
- **D-12:** If no PID file exists or daemon is not running, print "daemon is not running" and exit 0 (idempotent)
- **D-13:** If daemon doesn't stop within timeout after SIGTERM, warn user (do NOT escalate to SIGKILL automatically — leave that to user)

### Output and feedback

- **D-14:** Non-JSON mode: human-friendly status messages ("Starting daemon...", "Daemon started (pid 12345)", "Daemon stopped")
- **D-15:** JSON mode: structured `{"status": "started", "pid": 12345}` / `{"status": "stopped"}` / `{"status": "already_running", "pid": ...}`
- **D-16:** Exit codes: EXIT_SUCCESS (0) for success/already-running/not-running, EXIT_ERROR (1) for spawn failure or stop failure, EXIT_DAEMON_UNREACHABLE (5) not used (start/stop don't require existing daemon)

### Claude's Discretion

- Exact timeout values for health polling after start and process exit polling after stop
- Whether to add a `--timeout` flag for customizable wait durations
- Internal error message wording

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### CLI structure

- `src-tauri/crates/uc-cli/src/main.rs` — CLI entry point, Commands enum, clap setup
- `src-tauri/crates/uc-cli/src/commands/mod.rs` — Command module registration
- `src-tauri/crates/uc-cli/src/exit_codes.rs` — Exit code constants

### Daemon spawn and health

- `src-tauri/crates/uc-cli/src/local_daemon.rs` — `ensure_local_daemon_running()`, `spawn_daemon_process()`, `probe_daemon_health()`, health polling loop
- `src-tauri/crates/uc-daemon/src/process_metadata.rs` — PID file read/write/remove, profile-aware path resolution

### Daemon shutdown

- `src-tauri/crates/uc-daemon/src/app.rs` — `DaemonApp::run()` shutdown sequence, `wait_for_shutdown_signal()`, SIGTERM handling

### Daemon binary

- `src-tauri/crates/uc-daemon/src/main.rs` — Daemon binary entry, `--gui-managed` flag, stdin EOF tether pattern

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `local_daemon::ensure_local_daemon_running()` — Already handles probe-then-spawn-then-poll pattern, can be reused directly for `start` background mode
- `local_daemon::spawn_daemon_process()` — Returns `Child`, currently discards it. Foreground mode needs a variant that inherits stdio
- `local_daemon::resolve_daemon_binary_path()` — Daemon binary resolution, reusable
- `process_metadata::read_pid_file()` / `remove_pid_file()` — PID management for `stop`
- `daemon_client::DaemonHttpClient` — Not needed for start/stop (PID-based, not API-based)

### Established Patterns

- All commands return `i32` exit code, use `(json: bool, verbose: bool)` signature
- `local_daemon.rs` uses testable function injection pattern (`ensure_local_daemon_running_with`)
- Error types are custom enums with Display impl (not anyhow at command level)

### Integration Points

- `Commands` enum in `main.rs` — add `Start` and `Stop` variants
- `commands/mod.rs` — register new `start` and `stop` modules
- `exit_codes.rs` — may need new codes if distinct exit semantics needed

</code_context>

<specifics>
## Specific Ideas

- Start command defaults to background (daemon化), foreground mode prints daemon logs — matches standard daemon CLI conventions (systemctl start / launchctl load pattern)
- Foreground mode is primarily for debugging: user wants to see daemon log output in their terminal

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 70-cli-start-stop-commands-for-daemon-lifecycle-management_
_Context gathered: 2026-03-28_

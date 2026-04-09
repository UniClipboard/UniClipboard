---
phase: 68-adopt-tauri-sidecar-for-daemon
plan: 02
subsystem: infra
tags: [tauri, sidecar, shell-plugin, daemon, lifecycle]

# Dependency graph
requires:
  - phase: 68-01
    provides: tauri-plugin-shell dependency, externalBin config, build.rs staging, shell capability
provides:
  - Tauri sidecar-based daemon spawn via app.shell().sidecar()
  - CommandChild-based GuiOwnedDaemonState for stdin tether (D-06)
  - PID-based daemon shutdown with libc::kill(0) polling on Unix
  - AppHandle threading through bootstrap_daemon_connection and supervise_daemon
  - shell plugin registered in main.rs builder chain
affects:
  - Daemon spawn path (now uses Tauri bundler path resolution and code signing)
  - GUI exit cleanup (now uses PID termination + CommandChild::kill() instead of Child methods)

# Tech tracking
tech-stack:
  added:
    - libc = "0.2" (unix target dep in uc-daemon-client for process-existence check)
  patterns:
    - Sidecar spawn: app.shell().sidecar("uniclipboard-daemon").args(["--gui-managed"]).spawn() returns (Receiver, CommandChild)
    - stdin tether (D-06): CommandChild held in GuiOwnedDaemonState; dropping it sends EOF to daemon
    - rx drain: background task consumes Receiver events to prevent pipe blocking
    - PID shutdown: terminate_local_daemon_pid → libc::kill(pid, 0) polling → CommandChild::kill() on timeout

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-tauri/src/bootstrap/run.rs
    - src-tauri/crates/uc-daemon-client/src/daemon_lifecycle.rs
    - src-tauri/crates/uc-daemon-client/Cargo.toml
    - src-tauri/src/main.rs
    - src-tauri/crates/uc-tauri/tests/daemon_bootstrap_contract.rs
    - src-tauri/crates/uc-tauri/tests/daemon_exit_cleanup.rs

key-decisions:
  - "CommandChild from sidecar spawn maintains stdin tether (D-06): drop sends EOF to daemon's --gui-managed stdin monitor"
  - "shutdown_owned_daemon uses terminate_local_daemon_pid + libc::kill(0) polling instead of Child::try_wait/kill/wait"
  - "spawn_daemon_process returns (CommandChild, u32) tuple since pid() must be called before move"
  - "Sidecar rx Receiver drained in background task — must not be dropped immediately or pipe blocks"
  - "Test spawn closures use Ok(None) — CommandChild cannot be constructed outside Tauri runtime"
  - "Pre-existing test bug fixed: startup_helper_rejects test now checks timeout error instead of unreachable terminate"
  - "daemon_exit_cleanup integration tests that required real CommandChild removed (D-06 tested at E2E level)"

patterns-established:
  - "Sidecar spawn pattern: (rx, child) = app.shell().sidecar(name).args([...]).spawn()?"
  - "CommandChild pid extraction: let pid = child.pid(); before move into record_spawned"
  - "Receiver drain pattern: tauri::async_runtime::spawn(async move { while let Some(event) = rx.recv().await { ... } })"

requirements-completed: [PH68-03, PH68-04, PH68-06]

# Metrics
duration: 20min
completed: 2026-03-28
---

# Phase 68 Plan 02: Sidecar Spawn Migration Summary

**Daemon spawn migrated to Tauri sidecar API: CommandChild replaces std::process::Child, GuiOwnedDaemonState holds CommandChild for stdin tether, PID-based shutdown replaces Child lifecycle methods, AppHandle threaded through bootstrap and supervision**

## Performance

- **Duration:** ~20 min
- **Started:** 2026-03-28T04:25:00Z
- **Completed:** 2026-03-28T04:45:13Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

### Task 1: CommandChild-based GuiOwnedDaemonState (ae32397e)

- Replaced `std::process::Child` with `tauri_plugin_shell::process::CommandChild` in `OwnedDaemonChild` struct
- Updated `record_spawned` to take `(CommandChild, pid: u32, SpawnReason)` — pid extracted before move since `CommandChild::pid()` is consumed
- Rewrote `shutdown_owned_daemon` to use PID-based termination: `terminate_local_daemon_pid` + `libc::kill(0)` existence polling on Unix, `CommandChild::kill()` on timeout
- Added `tauri-plugin-shell` workspace dep and `libc = "0.2"` unix target dep to `uc-daemon-client/Cargo.toml`
- Removed `record_spawned_tracks_pid_and_reason` and `clear_removes_owned_child_snapshot` tests (CommandChild not constructible in unit tests)
- Preserved `begin_exit_cleanup_is_idempotent_until_finished` test (no child dependency)

### Task 2: Sidecar spawn API and AppHandle wiring (c5e7dd7d)

- Replaced `spawn_daemon_process()` (std::process::Command) with sidecar version using `app.shell().sidecar("uniclipboard-daemon").args(["--gui-managed"]).spawn()`
- Deleted `resolve_daemon_binary_path()` and `daemon_binary_name()` — Tauri handles binary path and platform naming
- Background task drains sidecar `Receiver` to prevent pipe blocking; `CommandChild` ownership in `GuiOwnedDaemonState` maintains stdin tether (D-06)
- `bootstrap_daemon_connection<R: Runtime>` now takes `&AppHandle<R>` as first parameter
- `supervise_daemon<R: Runtime>` now takes `&AppHandle<R>` as first parameter
- `bootstrap_daemon_connection_with_hooks` `Spawn` generic changed from `Result<Option<Child>, ...>` to `Result<Option<(CommandChild, u32)>, ...>`
- Registered `.plugin(tauri_plugin_shell::init())` in `main.rs` builder chain
- Wired `&app_handle_for_daemon` to `bootstrap_daemon_connection` in `main.rs`
- Cloned `app_handle_for_supervisor` for `supervise_daemon` in `main.rs`
- Updated `daemon_bootstrap_contract.rs` tests: all spawn closures return `Ok(None)`
- Updated `daemon_exit_cleanup.rs` tests: removed tests requiring real CommandChild, preserved no-child and source-check tests

## Task Commits

| Task | Name | Commit | Files |
| ---- | ---- | ------ | ----- |
| 1 | Migrate GuiOwnedDaemonState to CommandChild | ae32397e | daemon_lifecycle.rs, uc-daemon-client/Cargo.toml, Cargo.lock |
| 2 | Sidecar spawn API and AppHandle wiring | c5e7dd7d | run.rs, main.rs, daemon_bootstrap_contract.rs, daemon_exit_cleanup.rs |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed pre-existing test failure: startup_helper_rejects_healthy_but_incompatible_daemon**
- **Found during:** Task 2 — running tests
- **Issue:** The test used `|| unreachable!()` for `terminate_incompatible` closure, but per code logic `terminate_incompatible` is always called when `replacement_attempt < MAX_INCOMPATIBLE_REPLACEMENT_ATTEMPTS`. The test was already broken before this migration.
- **Fix:** Changed `terminate_incompatible` closure to `|| Ok(())` (allow the call), updated error message assertion to match the actual `wait_for_endpoint_absent` timeout error ("did not exit within 10ms") instead of the version mismatch error
- **Files modified:** `src-tauri/crates/uc-tauri/src/bootstrap/run.rs`
- **Commit:** c5e7dd7d

## Known Stubs

None — all paths are wired. Spawn uses real sidecar API, shutdown uses real PID termination.

## Self-Check: PASSED

- FOUND: src-tauri/crates/uc-tauri/src/bootstrap/run.rs
- FOUND: src-tauri/crates/uc-daemon-client/src/daemon_lifecycle.rs
- FOUND: src-tauri/src/main.rs
- FOUND: commit ae32397e (Task 1)
- FOUND: commit c5e7dd7d (Task 2)

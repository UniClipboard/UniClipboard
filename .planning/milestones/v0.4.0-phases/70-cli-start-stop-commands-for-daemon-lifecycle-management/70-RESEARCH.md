# Phase 70: CLI start/stop commands for daemon lifecycle management - Research

**Researched:** 2026-03-28
**Domain:** Rust CLI process management — daemon spawn, PID-based termination, health polling
**Confidence:** HIGH

## Summary

Phase 70 adds `start` and `stop` subcommands to the existing `uc-cli` crate. Both commands use patterns already established in the codebase: `ensure_local_daemon_running()` for background start, `read_pid_file()` + `libc::kill` for stop. The implementation is well-scoped: no new dependencies are needed (libc is already in the workspace), no new ports or use cases required, and all required infrastructure already exists.

The key insight is that `start` in background mode is essentially a thin wrapper around `ensure_local_daemon_running()` which already handles the probe-spawn-poll cycle. The only new code needed is the `--foreground` mode (spawn with inherited stdio, wait for child) and the `stop` command (read PID, send SIGTERM, poll for exit with timeout, warn if timeout).

**Primary recommendation:** Implement `start` as a one-file command that reuses `ensure_local_daemon_running()` for background mode and adds a `spawn_daemon_foreground()` helper for foreground mode. Implement `stop` as a separate one-file command using `process_metadata::read_pid_file()` with platform-conditional SIGTERM/TerminateProcess.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**D-01:** `start` and `stop` are top-level `Commands` enum variants (same level as `Status`, `Devices`, `Setup`, `SpaceStatus`)
**D-02:** Both commands accept the existing global `--json` and `--verbose` flags
**D-03:** `start` accepts a `--foreground` (short: `-f`) flag to run in foreground mode instead of background
**D-04:** Default mode is background: spawn daemon process detached, poll health endpoint until healthy, print success and exit
**D-05:** Reuse existing `local_daemon::ensure_local_daemon_running()` logic for the background path (probe health, spawn if needed, wait for healthy)
**D-06:** If daemon is already running (health probe succeeds), print "daemon already running" and exit 0 (idempotent)
**D-07:** Foreground mode (`--foreground`): spawn daemon process with stdout/stderr inherited (not piped to null), CLI process waits for daemon to exit or Ctrl+C. No detach.
**D-08:** Foreground mode should NOT pass `--gui-managed` to the daemon binary (stdin tether is a GUI concept, not CLI)
**D-09:** Read PID from profile-aware PID file via `process_metadata::read_pid_file()`
**D-10:** Send SIGTERM to the PID (Unix) / TerminateProcess on Windows
**D-11:** Poll until process exits or timeout (reuse similar polling pattern as `wait_for_daemon_health`)
**D-12:** If no PID file exists or daemon is not running, print "daemon is not running" and exit 0 (idempotent)
**D-13:** If daemon doesn't stop within timeout after SIGTERM, warn user (do NOT escalate to SIGKILL automatically)
**D-14:** Non-JSON mode: human-friendly status messages ("Starting daemon...", "Daemon started (pid 12345)", "Daemon stopped")
**D-15:** JSON mode: structured `{"status": "started", "pid": 12345}` / `{"status": "stopped"}` / `{"status": "already_running", "pid": ...}`
**D-16:** Exit codes: EXIT_SUCCESS (0) for success/already-running/not-running, EXIT_ERROR (1) for spawn failure or stop failure, EXIT_DAEMON_UNREACHABLE (5) not used

### Claude's Discretion

- Exact timeout values for health polling after start and process exit polling after stop
- Whether to add a `--timeout` flag for customizable wait durations
- Internal error message wording

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

## Standard Stack

### Core (already in uc-cli/Cargo.toml — no new dependencies needed)

| Library                | Version   | Purpose                          | Why Standard                                                                                 |
| ---------------------- | --------- | -------------------------------- | -------------------------------------------------------------------------------------------- |
| `libc`                 | workspace | SIGTERM on Unix                  | Already in workspace via uc-daemon; `libc::kill(pid, libc::SIGTERM)` is the standard pattern |
| `tokio`                | 1         | Async runtime, sleep for polling | Already in uc-cli                                                                            |
| `serde` / `serde_json` | 1         | JSON output mode                 | Already in uc-cli                                                                            |
| `clap`                 | 4.5       | `--foreground` flag on `start`   | Already in uc-cli                                                                            |

### Existing Internal APIs (no new ports/use cases needed)

| Asset                                    | Location                            | Used By                                                      |
| ---------------------------------------- | ----------------------------------- | ------------------------------------------------------------ |
| `ensure_local_daemon_running()`          | `uc-cli/src/local_daemon.rs`        | `start` background mode                                      |
| `spawn_daemon_process()` (private)       | `uc-cli/src/local_daemon.rs`        | `start` foreground variant needs similar logic               |
| `resolve_daemon_binary_path()` (private) | `uc-cli/src/local_daemon.rs`        | `start` foreground mode                                      |
| `process_metadata::read_pid_file()`      | `uc-daemon/src/process_metadata.rs` | `stop` command                                               |
| `probe_daemon_health()` (private)        | `uc-cli/src/local_daemon.rs`        | Process exit poll after stop needs health probe or pid check |
| `exit_codes::{EXIT_SUCCESS, EXIT_ERROR}` | `uc-cli/src/exit_codes.rs`          | Both commands                                                |
| `output::print_result()`                 | `uc-cli/src/output.rs`              | Both commands                                                |

### New Items Required

- `libc` dependency in `uc-cli/Cargo.toml` — needed for `libc::kill(pid as i32, libc::SIGTERM)` and `libc::kill(pid as i32, 0)` (process existence check)
- Two new command files: `src/commands/start.rs` and `src/commands/stop.rs`
- Minor helpers to expose from `local_daemon.rs` if foreground spawn is needed there, OR duplicate the binary resolution inline in `start.rs`

**Version verification:** `libc` is already a workspace dependency (used by `uc-daemon` for `shutdown_owned_daemon`'s PID-based termination, Phase 68 note in STATE.md). No new external dependency required — just add `libc.workspace = true` to `uc-cli/Cargo.toml`.

## Architecture Patterns

### Recommended Project Structure

```
src-tauri/crates/uc-cli/src/
├── commands/
│   ├── mod.rs            # add: pub mod start; pub mod stop;
│   ├── start.rs          # NEW: start command
│   ├── stop.rs           # NEW: stop command
│   ├── devices.rs
│   ├── setup.rs
│   ├── space_status.rs
│   └── status.rs
├── local_daemon.rs       # expose resolve_daemon_binary_path (pub) + spawn foreground helper
├── main.rs               # add Start { foreground: bool } and Stop variants
└── exit_codes.rs         # no changes needed
```

### Pattern 1: Command Module Structure (from existing commands)

**What:** Each command is a module in `commands/` with a `pub async fn run(json: bool, verbose: bool) -> i32` entry point.
**When to use:** All CLI commands follow this pattern.

```rust
// Source: src-tauri/crates/uc-cli/src/commands/space_status.rs (existing)
pub async fn run(json: bool, verbose: bool) -> i32 {
    // ...
    exit_codes::EXIT_SUCCESS
}
```

### Pattern 2: Start Background Mode (reuse ensure_local_daemon_running)

**What:** Call `ensure_local_daemon_running()` which returns `LocalDaemonSession { base_url, spawned }`. If `spawned == false`, daemon was already running.
**When to use:** Default `start` invocation (no `--foreground`).

```rust
// Pseudocode for start.rs background path
match ensure_local_daemon_running().await {
    Ok(session) if !session.spawned => {
        // D-06: already running — get PID from pid file for output
        let pid = process_metadata::read_pid_file().ok().flatten();
        print_output(StartOutput { status: "already_running", pid }, json);
        EXIT_SUCCESS
    }
    Ok(session) => {
        let pid = process_metadata::read_pid_file().ok().flatten();
        print_output(StartOutput { status: "started", pid }, json);
        EXIT_SUCCESS
    }
    Err(e) => {
        eprintln!("Error: {}", e);
        EXIT_ERROR
    }
}
```

### Pattern 3: Start Foreground Mode

**What:** Spawn daemon with inherited stdio (no Stdio::null()), wait for process exit.
**When to use:** `start --foreground` / `start -f`.

Key difference from `spawn_daemon_process()`: use `Stdio::inherit()` instead of `Stdio::null()`. Do NOT pass `--gui-managed` (D-08). The CLI process blocks until daemon exits or Ctrl+C.

```rust
// Pseudocode for foreground spawn
let mut child = Command::new(&daemon_binary)
    .stdin(Stdio::null())      // stdin null even in foreground (no gui-managed tether)
    .stdout(Stdio::inherit())  // inherit: daemon stdout → CLI terminal
    .stderr(Stdio::inherit())  // inherit: daemon stderr → CLI terminal
    .spawn()?;

// Wait — daemon runs until SIGTERM from user Ctrl+C or natural exit
let status = child.wait()?;
```

Note: `Ctrl+C` in a terminal sends SIGINT to the entire process group. When the daemon inherits stdio from the CLI, Ctrl+C will deliver SIGINT to the daemon (which handles `ctrl_c()` in `wait_for_shutdown_signal()`), causing graceful shutdown. The CLI `child.wait()` then returns naturally. This is the correct behavior — no special signal handling needed in the CLI for foreground mode.

### Pattern 4: Stop Command (PID-based SIGTERM)

**What:** Read PID file, send SIGTERM, poll for process exit, warn on timeout.
**When to use:** `stop` command.

```rust
// Pseudocode for stop.rs
// 1. Read PID file
let pid = match process_metadata::read_pid_file() {
    Ok(Some(pid)) => pid,
    Ok(None) => { print "daemon is not running"; return EXIT_SUCCESS; }  // D-12
    Err(e) => { eprintln!("Error: {}", e); return EXIT_ERROR; }
};

// 2. Check if process exists
#[cfg(unix)]
let process_exists = unsafe { libc::kill(pid as i32, 0) } == 0;

// 3. If process doesn't exist — pid file stale
if !process_exists {
    print "daemon is not running"; return EXIT_SUCCESS;  // D-12
}

// 4. Send SIGTERM
#[cfg(unix)]
unsafe { libc::kill(pid as i32, libc::SIGTERM); }
#[cfg(windows)]
terminate_process_windows(pid);

// 5. Poll for exit
let deadline = Instant::now() + STOP_TIMEOUT;
loop {
    tokio::time::sleep(POLL_INTERVAL).await;
    if !process_is_running(pid) { break; }
    if Instant::now() >= deadline {
        eprintln!("Warning: daemon (pid {}) did not stop within timeout", pid);  // D-13
        return EXIT_ERROR;
    }
}

print_output(StopOutput { status: "stopped" }, json);
EXIT_SUCCESS
```

### Anti-Patterns to Avoid

- **Do not reimplement health polling for stop:** The stop command checks process existence via `libc::kill(pid, 0)` — NOT via HTTP health probe. Health probe after SIGTERM might return errors or hang; the `kill(pid, 0)` pattern is authoritative.
- **Do not pass `--gui-managed` in foreground mode:** D-08 explicitly forbids this.
- **Do not SIGKILL on timeout:** D-13 explicitly says only warn, don't escalate.
- **Do not call `process::exit()` inside command functions:** The pattern returns `i32`, and `main.rs` calls `std::process::exit(exit_code)`.

## Don't Hand-Roll

| Problem                           | Don't Build            | Use Instead                               | Why                                                                               |
| --------------------------------- | ---------------------- | ----------------------------------------- | --------------------------------------------------------------------------------- |
| Daemon health polling after start | Custom polling loop    | `ensure_local_daemon_running()`           | Already handles probe-spawn-poll with configurable timeout and proper error types |
| PID file read                     | Custom file parsing    | `process_metadata::read_pid_file()`       | Profile-aware, handles missing file gracefully, already tested                    |
| Daemon binary path resolution     | Custom path logic      | `resolve_daemon_binary_path()` (make pub) | Handles Windows .exe suffix, sibling binary detection, fallback to $PATH          |
| SIGTERM on Unix                   | Custom signal handling | `libc::kill(pid, SIGTERM)`                | Standard pattern, already used in uc-daemon (Phase 68 STATE.md note)              |
| JSON output formatting            | Custom JSON print      | `output::print_result()`                  | Shared utility for all CLI commands                                               |

**Key insight:** This phase is almost entirely composition of existing infrastructure. The only genuinely new logic is the foreground spawn variant and the stop polling loop.

## Common Pitfalls

### Pitfall 1: PID stale after daemon crash

**What goes wrong:** PID file exists but the process is gone (daemon crashed without cleanup). `read_pid_file()` returns Some(pid), but `kill(pid, 0)` returns -1.
**Why it happens:** `DaemonPidFileGuard::drop()` removes the PID file on clean shutdown, but not on SIGKILL or panic.
**How to avoid:** Always verify process existence via `kill(pid, 0)` before sending SIGTERM. If `kill(pid, 0)` fails with ESRCH (no such process), treat as "daemon not running" and return EXIT_SUCCESS (D-12).
**Warning signs:** User reports "daemon is running" when it's not.

### Pitfall 2: Process group SIGINT in foreground mode

**What goes wrong:** Developers assume Ctrl+C in terminal only kills the CLI, leaving daemon orphaned.
**Why it happens:** Misunderstanding of Unix terminal process groups.
**How to avoid:** No action needed — inherited stdio means daemon is in the same process group as CLI. Ctrl+C delivers SIGINT to both. Daemon's `wait_for_shutdown_signal()` handles SIGINT via `ctrl_c()`. The CLI's `child.wait()` returns after daemon exits.
**Warning signs:** Would manifest as "zombie daemon" reports after Ctrl+C — not actually a problem.

### Pitfall 3: start prints PID from PID file written BEFORE health is ready

**What goes wrong:** The daemon writes its PID file in `DaemonPidFileGuard::activate()` which is called early in `DaemonApp::run()`. So `read_pid_file()` after `ensure_local_daemon_running()` succeeds will have the correct PID. However, timing is: PID file written → HTTP health becomes ready → `ensure_local_daemon_running()` returns. The PID file is available by the time we read it.
**How to avoid:** Read PID file AFTER `ensure_local_daemon_running()` returns (not before). This is the natural implementation order.

### Pitfall 4: Windows process termination

**What goes wrong:** `libc::kill()` doesn't work on Windows. Need `windows_sys` or `winapi` for `TerminateProcess`.
**Why it happens:** Unix-only API.
**How to avoid:** Use `#[cfg(unix)]` / `#[cfg(windows)]` conditional compilation. For Windows: open process handle via `OpenProcess`, then call `TerminateProcess`. Alternatively, use `taskkill /PID {pid} /F` via `std::process::Command` to avoid adding `windows-sys` dependency — but this is fragile. Best approach: add `#[cfg(windows)]` block using `windows-sys` or document as Unix-only MVP with `#[cfg(not(unix))]` unimplemented stub.
**Warning signs:** CI failure on Windows build if not handled.

### Pitfall 5: Timeout values

**What goes wrong:** Stop timeout too short → daemon doing cleanup gets warned about. Start timeout inherited from `STARTUP_TIMEOUT` (8 seconds) is reasonable.
**How to avoid:** Use 10 seconds for stop (generous for graceful shutdown of all services). Use existing `STARTUP_TIMEOUT = 8s` from `local_daemon.rs` for start. The discretion area in CONTEXT.md allows choosing values.

## Code Examples

### Checking process existence on Unix

```rust
// Source: POSIX kill(2) — signal 0 tests process existence without delivering signal
#[cfg(unix)]
fn is_process_running(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}
```

### Sending SIGTERM on Unix

```rust
// Source: POSIX kill(2) — standard graceful termination
#[cfg(unix)]
fn send_sigterm(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) == 0 }
}
```

### clap variant with flag for Start command

```rust
// Source: clap 4.5 derive API — consistent with existing Commands enum in main.rs
Commands::Start {
    #[arg(long, short = 'f', help = "Run daemon in foreground (log output to terminal)")]
    foreground: bool,
}
```

### JSON output structs

```rust
// start command output
#[derive(Serialize)]
struct StartOutput {
    status: &'static str,  // "started" | "already_running"
    pid: Option<u32>,
}

// stop command output
#[derive(Serialize)]
struct StopOutput {
    status: &'static str,  // "stopped" | "not_running"
}

// Display impl for human-readable mode
impl fmt::Display for StartOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.status, self.pid) {
            ("started", Some(pid)) => write!(f, "Daemon started (pid {})", pid),
            ("already_running", Some(pid)) => write!(f, "Daemon already running (pid {})", pid),
            ("already_running", None) => write!(f, "Daemon already running"),
            _ => write!(f, "Daemon started"),
        }
    }
}
```

## Validation Architecture

### Test Framework

| Property           | Value                                  |
| ------------------ | -------------------------------------- |
| Framework          | Rust built-in test + tokio::test       |
| Config file        | none (standard cargo test)             |
| Quick run command  | `cd src-tauri && cargo test -p uc-cli` |
| Full suite command | `cd src-tauri && cargo test -p uc-cli` |

### Phase Requirements → Test Map

| Behavior                                                                      | Test Type                               | Automated Command                     | File Exists? |
| ----------------------------------------------------------------------------- | --------------------------------------- | ------------------------------------- | ------------ |
| start background — already running → EXIT_SUCCESS, "already_running" output   | unit                                    | `cargo test -p uc-cli start::tests::` | ❌ Wave 0    |
| start background — spawn + health ok → EXIT_SUCCESS, "started" output         | unit (mock spawn/probe)                 | `cargo test -p uc-cli start::tests::` | ❌ Wave 0    |
| start background — spawn failure → EXIT_ERROR                                 | unit                                    | `cargo test -p uc-cli start::tests::` | ❌ Wave 0    |
| stop — no PID file → EXIT_SUCCESS, "not_running"                              | unit                                    | `cargo test -p uc-cli stop::tests::`  | ❌ Wave 0    |
| stop — PID file exists, process not running → EXIT_SUCCESS, idempotent        | unit                                    | `cargo test -p uc-cli stop::tests::`  | ❌ Wave 0    |
| stop — PID file exists, SIGTERM sent, process exits → EXIT_SUCCESS, "stopped" | unit (mock process check)               | `cargo test -p uc-cli stop::tests::`  | ❌ Wave 0    |
| stop — SIGTERM sent but timeout → EXIT_ERROR, warning message                 | unit (mock process check stays-running) | `cargo test -p uc-cli stop::tests::`  | ❌ Wave 0    |
| JSON output shape for start/stop                                              | unit                                    | `cargo test -p uc-cli`                | ❌ Wave 0    |

### Testing Strategy for Process Operations

Stop command tests need to mock the "is process running" check. Use the testable injection pattern already established in `local_daemon.rs` (function injection via generic parameters). Extract the process-existence check and SIGTERM send into injectable function parameters for unit testing, similar to how `ensure_local_daemon_running_with` separates probe and spawn.

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-cli`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-cli`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-cli/src/commands/start.rs` — covers start background and foreground paths
- [ ] `src-tauri/crates/uc-cli/src/commands/stop.rs` — covers stop with PID-based termination

## Environment Availability

Step 2.6: No external dependencies beyond the project's own code. `libc` is already a workspace dependency. All infrastructure (daemon binary, PID file, health endpoint) is project-owned.

## Open Questions

1. **Windows TerminateProcess implementation**
   - What we know: `libc::kill` is Unix-only. Windows needs `OpenProcess` + `TerminateProcess` from `windows-sys` crate.
   - What's unclear: Is `windows-sys` already in the workspace? (Not checked — uc-daemon's Phase 68 termination used `libc::kill` + polling, which suggests Windows may not be fully implemented yet.)
   - Recommendation: Check workspace Cargo.toml for `windows-sys`. If absent, implement a `#[cfg(windows)]` stub that calls `taskkill /PID {pid} /F` via `std::process::Command` as a lightweight alternative to avoid adding a new dependency.

2. **Should `--timeout` flag be added?**
   - What we know: CONTEXT.md lists this as Claude's discretion.
   - Recommendation: Do NOT add `--timeout` in this phase. Hard-code sensible defaults (10s stop timeout, reuse existing 8s start timeout). Adding a flag increases test surface and clap complexity for minimal user value in this initial implementation.

3. **Foreground mode: should CLI print "Starting daemon in foreground..." before spawn?**
   - What we know: D-14 says human-friendly messages for non-JSON mode.
   - Recommendation: Print "Starting daemon in foreground... (press Ctrl+C to stop)" before the spawn, then let daemon stdout/stderr flow naturally. Print nothing after daemon exits (its own shutdown logs are visible).

## Sources

### Primary (HIGH confidence)

- Direct code inspection of `src-tauri/crates/uc-cli/src/local_daemon.rs` — spawn patterns, health polling, error types
- Direct code inspection of `src-tauri/crates/uc-daemon/src/process_metadata.rs` — PID file read/write/remove
- Direct code inspection of `src-tauri/crates/uc-daemon/src/app.rs` — SIGTERM handling, `wait_for_shutdown_signal()`
- Direct code inspection of `src-tauri/crates/uc-cli/src/commands/space_status.rs` — command module pattern
- Direct code inspection of `src-tauri/crates/uc-cli/src/main.rs` — Commands enum, routing pattern
- `70-CONTEXT.md` — all locked decisions (D-01 through D-16)

### Secondary (MEDIUM confidence)

- POSIX `kill(2)` semantics: signal 0 tests process existence — standard Unix knowledge
- Tokio terminal process group behavior for foreground mode — standard Unix process group semantics

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all dependencies already present, directly verified from Cargo.toml
- Architecture: HIGH — patterns directly observed from existing command files
- Pitfalls: HIGH (Unix) / MEDIUM (Windows) — Unix well-understood, Windows path needs workspace check

**Research date:** 2026-03-28
**Valid until:** 90 days (stable internal codebase, no external API dependencies)

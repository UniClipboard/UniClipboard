---
phase: 41-daemon-and-cli-skeletons
plan: 04
type: execute
wave: 1
depends_on: []
files_modified:
  - src-tauri/crates/uc-daemon/src/lib.rs
  - src-tauri/crates/uc-daemon/src/socket.rs
  - src-tauri/crates/uc-daemon/src/main.rs
  - src-tauri/crates/uc-cli/src/commands/status.rs
autonomous: true
gap_closure: true
requirements: [DAEM-01, DAEM-02, CLI-02]

must_haves:
  truths:
    - 'Daemon starts and accepts JSON-RPC connections on Unix socket'
    - 'Daemon exits cleanly on SIGTERM and removes socket file'
    - 'CLI status command connects to daemon via same socket path'
  artifacts:
    - path: 'src-tauri/crates/uc-daemon/src/socket.rs'
      provides: 'Shared socket path resolution function'
      exports: ['resolve_daemon_socket_path']
    - path: 'src-tauri/crates/uc-daemon/src/main.rs'
      provides: 'Daemon entry point using short socket path'
    - path: 'src-tauri/crates/uc-cli/src/commands/status.rs'
      provides: 'CLI status using shared socket path'
  key_links:
    - from: 'src-tauri/crates/uc-daemon/src/main.rs'
      to: 'src-tauri/crates/uc-daemon/src/socket.rs'
      via: 'resolve_daemon_socket_path()'
      pattern: 'uc_daemon::socket::resolve_daemon_socket_path'
    - from: 'src-tauri/crates/uc-cli/src/commands/status.rs'
      to: 'src-tauri/crates/uc-daemon/src/socket.rs'
      via: 'resolve_daemon_socket_path()'
      pattern: 'uc_daemon::socket::resolve_daemon_socket_path'
---

<objective>
Fix socket path too long for macOS sockaddr_un.sun_path limit (104 bytes).

Purpose: Daemon fails to start because `app_data_root/uniclipboard-daemon.sock` resolves to ~91+ bytes which exceeds the macOS SUN_LEN limit for longer usernames or deep paths. Both daemon and CLI must use a short, shared socket path.
Output: Daemon starts successfully on macOS, CLI connects to same socket, graceful shutdown works.
</objective>

<execution_context>
@/Users/mark/.claude/get-shit-done/workflows/execute-plan.md
@/Users/mark/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/41-daemon-and-cli-skeletons/41-UAT.md

<interfaces>
<!-- From uc-daemon/src/lib.rs — current public modules -->
pub mod app;
pub mod rpc;
pub mod state;
pub mod worker;
pub mod workers;

<!-- From uc-daemon/src/main.rs lines 19-23 — the broken socket path -->

let socket_path = ctx
.storage_paths
.app_data_root
.join("uniclipboard-daemon.sock");

<!-- From uc-cli/src/commands/status.rs lines 39-45 — correct pattern already in CLI -->

fn resolve*socket_path() -> std::path::PathBuf {
let dir = std::env::var("XDG_RUNTIME_DIR")
.map(std::path::PathBuf::from)
.unwrap_or_else(|*| std::env::temp_dir());
dir.join("uniclipboard-daemon.sock")
}
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Extract shared resolve_daemon_socket_path to uc-daemon lib</name>
  <files>src-tauri/crates/uc-daemon/src/socket.rs, src-tauri/crates/uc-daemon/src/lib.rs</files>
  <action>
Create `src-tauri/crates/uc-daemon/src/socket.rs` with a public `resolve_daemon_socket_path()` function that returns `PathBuf`.

Logic (Unix):

- Check `$XDG_RUNTIME_DIR` env var first. If set and non-empty, use it as candidate base directory.
- Otherwise fall back to `PathBuf::from("/tmp")` (NOT `std::env::temp_dir()` which on macOS returns `$TMPDIR` — a long `/var/folders/.../T/` path that may exceed SUN_LEN).
- Join with `"uniclipboard-daemon.sock"`.
- **Length guard**: After joining, check byte length <= 103 (path bytes + NUL terminator <= 104, the sockaddr_un.sun_path limit). Use a helper `fn socket_path_byte_len(path: &Path) -> usize` that on `#[cfg(unix)]` uses `std::os::unix::ffi::OsStrExt::as_bytes().len()` and on other platforms uses `path.as_os_str().len()`. If the XDG_RUNTIME_DIR-based path exceeds this, log a warning and fall back to `/tmp/uniclipboard-daemon.sock`.
- **Testability**: Expose a pure internal function `resolve_daemon_socket_path_from(base: Option<&Path>) -> PathBuf` that accepts the base directory as a parameter. The public `resolve_daemon_socket_path()` calls it with the env-var-resolved base. Tests use the `_from` variant directly, avoiding global env var mutation and parallel test pollution.
- This produces paths like `/tmp/uniclipboard-daemon.sock` (~35 bytes) — well within the 104-byte SUN_LEN limit.

Logic (non-Unix): return `std::env::temp_dir().join("uniclipboard-daemon.sock")`.

Add a `#[cfg(test)]` module with:

- `test_socket_path_length`: call `resolve_daemon_socket_path_from(None)` and assert `socket_path_byte_len(&path) <= 103` (byte length via helper; path + NUL must fit in 104-byte sun_path).
- `test_socket_path_ends_with_sock`: assert filename is `uniclipboard-daemon.sock`.
- `test_xdg_runtime_dir_override`: call `resolve_daemon_socket_path_from(Some(Path::new("/run/user/1000")))`, verify base directory is used. No global env mutation needed.
- `test_xdg_runtime_dir_too_long`: call `resolve_daemon_socket_path_from(Some(Path::new("/a]very/long/path/...")))` with a base that would exceed 103 bytes after joining, verify fallback to `/tmp` is used.
- `test_socket_path_boundary`: call `resolve_daemon_socket_path_from` with a base that produces exactly 103 bytes, verify it succeeds. Call with base that produces 104 bytes, verify fallback.

All tests use the pure `resolve_daemon_socket_path_from()` function directly — no global env var mutation, safe for parallel execution.

Update `src-tauri/crates/uc-daemon/src/lib.rs` to add `pub mod socket;`.
</action>
<verify>
<automated>cd /Users/mark/.superset/worktrees/uniclipboard-desktop.aaaa/gsddiscuss-41-auto/src-tauri && cargo test -p uc-daemon socket</automated>
</verify>
<done>resolve_daemon_socket_path() is public in uc-daemon, returns a short path under 104 bytes, all tests pass.</done>
</task>

<task type="auto">
  <name>Task 2: Wire daemon and CLI to use shared socket path</name>
  <files>src-tauri/crates/uc-daemon/src/main.rs, src-tauri/crates/uc-cli/src/commands/status.rs</files>
  <action>
In `src-tauri/crates/uc-daemon/src/main.rs`:
- Replace lines 19-23 (the `ctx.storage_paths.app_data_root.join(...)` socket path) with:
  `let socket_path = uc_daemon::socket::resolve_daemon_socket_path();`
- Add `use uc_daemon::socket::resolve_daemon_socket_path;` (or inline the crate path).
- Remove the now-unused `storage_paths` field access if it was only used for socket path. Keep `ctx` if other fields are used (they are not currently, but `build_daemon_app()` return is used for tracing init side effects).

In `src-tauri/crates/uc-cli/src/commands/status.rs`:

- Delete the local `resolve_socket_path()` function (lines 34-45).
- Replace `let socket_path = resolve_socket_path();` on line 58 with:
  `let socket_path = uc_daemon::socket::resolve_daemon_socket_path();`

Both binaries now resolve to the same short socket path, ensuring daemon bind and CLI connect use identical paths.
</action>
<verify>
<automated>cd /Users/mark/.superset/worktrees/uniclipboard-desktop.aaaa/gsddiscuss-41-auto/src-tauri && cargo build -p uc-daemon -p uc-cli && cargo test -p uc-daemon && cargo test -p uc-cli</automated>
</verify>
<done>Daemon binary uses resolve_daemon_socket_path() for socket bind. CLI status command uses the same function. Both crates compile and all tests pass. Socket path is short enough for macOS SUN_LEN limit.</done>
</task>

</tasks>

<verification>
1. `cd src-tauri && cargo build -p uc-daemon -p uc-cli` — both binaries compile
2. `cd src-tauri && cargo test -p uc-daemon` — all tests pass including new socket path tests
3. `cd src-tauri && cargo test -p uc-cli` — all CLI smoke tests still pass
4. Manual: Start daemon (`cargo run -p uc-daemon`), verify it starts without SUN_LEN error
5. Manual: With daemon running, `cargo run -p uc-cli -- status` returns running status
6. Manual: Send SIGTERM to daemon, verify clean shutdown and socket file removed
</verification>

<success_criteria>

- Daemon starts successfully on macOS without "path must be shorter than SUN_LEN" error
- CLI status command connects to daemon via the same socket path
- Daemon graceful shutdown removes socket file
- All existing tests pass (uc-daemon and uc-cli)
- Socket path byte length is always <= 103 (+ NUL = 104, the sun_path limit)
  </success_criteria>

<output>
After completion, create `.planning/phases/41-daemon-and-cli-skeletons/41-04-SUMMARY.md`
</output>

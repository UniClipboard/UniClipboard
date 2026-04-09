# Phase 68: Adopt Tauri Sidecar for daemon binary management - Context

**Gathered:** 2026-03-28
**Status:** Ready for planning

<domain>
## Phase Boundary

Migrate daemon binary building, bundling, and path resolution from manual management to Tauri's externalBin sidecar mechanism. GUI launches daemon via Tauri sidecar API instead of raw std::process::Command. Dev builds auto-compile and place daemon binary. CLI path resolution is out of scope (future phase).

</domain>

<decisions>
## Implementation Decisions

### Bundling Strategy

- **D-01:** Use Tauri `externalBin` in `tauri.conf.json` to declare daemon as a sidecar binary. Tauri handles platform-specific target-triple naming and inclusion in app bundle (.dmg/.msi/.AppImage).
- **D-02:** Binary naming follows Tauri sidecar convention: `uniclipboard-daemon-{target-triple}` (e.g., `uniclipboard-daemon-aarch64-apple-darwin`).

### Launch Mechanism

- **D-03:** Replace `std::process::Command` in `run.rs` with Tauri sidecar API (`app.shell().sidecar("uniclipboard-daemon")`). Delete custom `resolve_daemon_binary_path()` and `daemon_binary_name()` functions.
- **D-04:** Pass `--gui-managed` argument via sidecar `.args()`.

### Dev Build Automation

- **D-05:** Use `build.rs` in uc-tauri to copy the compiled daemon binary from `target/{profile}/uniclipboard-daemon` to `src-tauri/binaries/uniclipboard-daemon-{target-triple}` after build. Tauri's workspace cargo build already compiles all workspace members including uc-daemon.

### stdin pipe Tether

- **D-06:** Migrate existing stdin pipe tether (GUI-managed daemon shutdown) to sidecar `CommandChild` stdin. The sidecar API supports `.write(bytes)` for async stdin communication. Daemon-side stdin monitoring logic (`--gui-managed` mode) remains unchanged.

### CLI Path Resolution

- **D-07:** CLI (`uc-cli`) path resolution stays as-is in this phase. CLI continues using `resolve_daemon_binary_path_from(current_exe)` sibling lookup. Unified CLI distribution (brew install, single entry point) is a separate future phase.

### Claude's Discretion

- Exact `build.rs` implementation details (how to detect target triple, copy logic)
- Whether to use `tauri-plugin-shell` permissions model or bypass for sidecar
- Supervision loop adaptation details (sidecar `CommandChild` vs std `Child` for health monitoring)

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Tauri Sidecar Documentation

- Tauri v2 sidecar guide (use context7 to fetch `tauri` docs on sidecar/externalBin)

### Current Implementation

- `src-tauri/crates/uc-tauri/src/bootstrap/run.rs` -- spawn_daemon_process(), resolve_daemon_binary_path(), supervise_daemon(), bootstrap_daemon_connection()
- `src-tauri/tauri.conf.json` -- bundle and build configuration (currently no externalBin)
- `src-tauri/crates/uc-daemon/Cargo.toml` -- daemon binary name: `uniclipboard-daemon`
- `src-tauri/crates/uc-cli/src/local_daemon.rs` -- CLI daemon spawning (NOT modified in this phase)
- `src-tauri/crates/uc-daemon-client/src/daemon_lifecycle.rs` -- daemon lifecycle management

### Build Pipeline

- `.github/workflows/build.yml` -- current build workflow (daemon binary not bundled)
- `.github/workflows/release.yml` -- release workflow (needs externalBin artifacts)

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `bootstrap_daemon_connection()` in `run.rs:80` -- health probe, spawn, poll loop. Core logic reusable, only spawn mechanism changes.
- `supervise_daemon()` in `run.rs:117` -- supervision with exponential backoff. Needs adaptation from `std::process::Child` to sidecar `CommandChild`.
- `DaemonConnectionState` -- existing state management for WS bridge reconnection, stays as-is.

### Established Patterns

- `--gui-managed` flag + stdin pipe tether (Phase 46.6) -- daemon detects parent exit via stdin EOF
- Daemon binary name constant `DAEMON_BINARY_NAME = "uniclipboard-daemon"` in `run.rs`
- `tauri-plugin-shell` already in dependencies (needed for sidecar API)

### Integration Points

- `run.rs:spawn_daemon_process()` -- primary replacement target
- `run.rs:resolve_daemon_binary_path()` -- delete, replaced by sidecar path resolution
- `tauri.conf.json` -- add `bundle.externalBin` array
- `build.rs` -- add daemon binary copy logic for dev builds
- `.github/workflows/build.yml` -- may need pre-build step to place binary with target-triple name

</code_context>

<specifics>
## Specific Ideas

- User envisions future `brew install uniclipboard` with a single `uniclipboard` command entry point. Daemon should be invisible to users. This is out of Phase 68 scope but informs direction.

</specifics>

<deferred>
## Deferred Ideas

- **Unified CLI distribution via Homebrew** -- single `uniclipboard` command, daemon binary bundled inside. Separate phase for CLI packaging and distribution strategy.
- **CLI daemon path resolution unification** -- extract shared resolve module to uc-daemon-client when CLI distribution is addressed.

</deferred>

---

_Phase: 68-adopt-tauri-sidecar-for-daemon_
_Context gathered: 2026-03-28_

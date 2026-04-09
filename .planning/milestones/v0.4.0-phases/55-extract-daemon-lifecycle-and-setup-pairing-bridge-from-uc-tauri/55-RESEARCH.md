# Phase 55: extract-daemon-lifecycle-and-setup-pairing-bridge-from-uc-tauri - Research

**Researched:** 2026-03-24
**Domain:** Rust crate extraction — move Tauri-free daemon lifecycle code from `uc-tauri` to `uc-daemon-client`
**Confidence:** HIGH

## Summary

Phase 55 extracts `daemon_lifecycle.rs` (340 lines, zero Tauri deps) from `uc-tauri/bootstrap/` into `uc-daemon-client/src/`, moves `terminate_local_daemon_pid()` from `run.rs` into the same module, and deletes dead `setup_pairing_bridge.rs`. This is a pure refactor with no behavioral changes.

The key technical decision is handling the error type for `terminate_local_daemon_pid`. Currently it returns `DaemonBootstrapError` (defined in `run.rs`). Since `daemon_lifecycle.rs` must be self-contained (D-05: no cross-module deps), the function's error type must also live in `daemon_lifecycle.rs`. The plan defines a simple `TerminateDaemonError(String)` in `daemon_lifecycle.rs`; `run.rs` maps it to `DaemonBootstrapError::IncompatibleDaemon` when re-importing.

**Primary recommendation:** Copy `daemon_lifecycle.rs` to `uc-daemon-client/src/`, inline `terminate_local_daemon_pid` into it as a `pub(crate)` fn returning `Result<(), TerminateDaemonError>`, add `pub mod daemon_lifecycle;` and public re-exports to `uc-daemon-client/src/lib.rs`, update 5 files' import paths, delete 2 source files.

---

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** `daemon_lifecycle.rs` migrates to `uc-daemon-client/src/daemon_lifecycle.rs`
- **D-02:** `setup_pairing_bridge.rs` deleted from `uc-tauri/bootstrap/`
- **D-03:** No new crate — reuse existing `uc-daemon-client`
- **D-04:** `terminate_local_daemon_pid()` moves from `uc-tauri/bootstrap/run.rs` (lines 590-627) to `daemon_lifecycle.rs`
- **D-05:** Migrated `daemon_lifecycle.rs` is fully self-contained, zero cross-module dependencies
- **D-06:** `run.rs` re-imports `terminate_local_daemon_pid` from `uc-daemon-client` after the move
- **D-07:** One commit per logical step
- **D-08:** No re-export stubs (Phase 54 precedent — verified one-pass cutover works)
- **D-09:** Dead re-export lines for `setup_pairing_bridge` removed from `bootstrap/mod.rs`
- **D-10:** 3 `#[cfg(test)]` unit tests migrate with `daemon_lifecycle.rs` to `uc-daemon-client`
- **D-11:** `uc-tauri/tests/daemon_exit_cleanup.rs` and `daemon_bootstrap_contract.rs` update imports

### Claude's Discretion

- Whether `#[cfg(test)]` inline test `spawn_test_child()` needs adaptation for `uc-daemon-client` test env
- Whether `daemon_lifecycle.rs` needs sub-module拆分 (kept as single file)

### Deferred Ideas (OUT OF SCOPE)

- setup_pairing_bridge.rs dead code成因复盘
- Todo fix-setup-pairing-confirmation-toast-missing

---

## Standard Stack

This is an intra-crate refactor with no new dependencies.

| Item                      | Status             | Notes                                             |
| ------------------------- | ------------------ | ------------------------------------------------- |
| `uc-daemon-client`        | Existing           | Already in workspace, already has `[lib]` section |
| `daemon_lifecycle.rs`     | Move from uc-tauri | Zero Tauri deps confirmed                         |
| `setup_pairing_bridge.rs` | Delete             | Dead code, no callers                             |

**No new dependencies needed.** All required types (`thiserror`, `tokio`) are already in `uc-daemon-client/Cargo.toml`.

---

## Architecture Patterns

### Crate Extraction Pattern (Phase 40, 54 precedent)

**Step 1 — Copy source to destination crate:**
Source `uc-tauri/bootstrap/daemon_lifecycle.rs` → `uc-daemon-client/src/daemon_lifecycle.rs`

**Step 2 — Add module declaration + public re-exports in `uc-daemon-client/src/lib.rs`:**

```rust
pub mod daemon_lifecycle;
pub use daemon_lifecycle::{GuiOwnedDaemonState, OwnedDaemonChild, SpawnReason, DaemonExitCleanupError};
```

**Step 3 — Update all callers' import paths** (5 files):

```rust
// Before
use uc_tauri::bootstrap::{GuiOwnedDaemonState, SpawnReason};
// After
use uc_daemon_client::daemon_lifecycle::{GuiOwnedDaemonState, SpawnReason};
```

**Step 4 — Delete source files and mod.rs entries:**

- `uc-tauri/bootstrap/daemon_lifecycle.rs`
- `uc-tauri/bootstrap/setup_pairing_bridge.rs`
- Remove from `uc-tauri/bootstrap/mod.rs`

**Step 5 — Migrate inline unit tests** (handled by the file copy)

### Error Type Design Decision

`terminate_local_daemon_pid` currently returns `Result<(), DaemonBootstrapError>` (defined in `run.rs`). Since `daemon_lifecycle.rs` must be self-contained (D-05), the function's error type must live in the migrated file.

**Solution:** Define `TerminateDaemonError(String)` in `daemon_lifecycle.rs`:

```rust
#[derive(Debug)]
pub struct TerminateDaemonError(pub String);

impl std::fmt::Display for TerminateDaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TerminateDaemonError {}
```

`run.rs` re-imports `terminate_local_daemon_pid` and maps to `DaemonBootstrapError`:

```rust
use uc_daemon_client::daemon_lifecycle::terminate_local_daemon_pid;
// ...
terminate_local_daemon_pid(pid).map_err(|e| DaemonBootstrapError::IncompatibleDaemon {
    details: e.to_string(),
})?;
```

The `daemon_lifecycle.rs` caller uses only the `Display` impl (for log message), so `TerminateDaemonError` satisfies the contract.

---

## Don't Hand-Roll

| Problem                            | Don't Build                   | Use Instead                                 | Why                                                                    |
| ---------------------------------- | ----------------------------- | ------------------------------------------- | ---------------------------------------------------------------------- |
| Cross-platform process termination | Custom platform-specific code | `std::process::Command` + `kill`/`taskkill` | Already implemented in `terminate_local_daemon_pid`, no changes needed |
| Thread-safe daemon child tracking  | Raw `Mutex` + `AtomicBool`    | `Arc<GuiOwnedDaemonStateInner>` pattern     | Already implemented, no changes needed                                 |

---

## Files Affected

### Source files to CREATE

| File                                       | Purpose                                                       |
| ------------------------------------------ | ------------------------------------------------------------- |
| `uc-daemon-client/src/daemon_lifecycle.rs` | Migrated module (copy + `terminate_local_daemon_pid` inlined) |

### Source files to DELETE

| File                                             | Purpose       |
| ------------------------------------------------ | ------------- |
| `uc-tauri/src/bootstrap/daemon_lifecycle.rs`     | Migrated away |
| `uc-tauri/src/bootstrap/setup_pairing_bridge.rs` | Dead code     |

### Files to MODIFY (import path updates)

| File                                          | Change                                                                                                                                                                |
| --------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `uc-daemon-client/src/lib.rs`                 | Add `pub mod daemon_lifecycle;` + re-exports                                                                                                                          |
| `uc-tauri/src/main.rs`                        | Change `GuiOwnedDaemonState` import path                                                                                                                              |
| `uc-tauri/src/bootstrap/mod.rs`               | Remove `daemon_lifecycle` and `setup_pairing_bridge` mod/re-exports                                                                                                   |
| `uc-tauri/src/bootstrap/run.rs`               | Remove `terminate_local_daemon_pid` def, add `pub use uc_daemon_client::daemon_lifecycle::terminate_local_daemon_pid;`, update internal `use super::daemon_lifecycle` |
| `uc-tauri/src/bootstrap/run.rs`               | Change `use super::daemon_lifecycle::{GuiOwnedDaemonState, SpawnReason}` → `use uc_daemon_client::daemon_lifecycle::{GuiOwnedDaemonState, SpawnReason}`               |
| `uc-tauri/tests/daemon_exit_cleanup.rs`       | Change `use uc_tauri::bootstrap::{GuiOwnedDaemonState, SpawnReason}` → `use uc_daemon_client::daemon_lifecycle::{GuiOwnedDaemonState, SpawnReason}`                   |
| `uc-tauri/tests/daemon_bootstrap_contract.rs` | Same import path update                                                                                                                                               |

### Inline test migration

The 3 `#[cfg(test)]` tests in `daemon_lifecycle.rs` (lines 276-340) use only `std::process::{Command, Stdio}` — no Tauri or `uc-tauri` imports. They migrate as-is with the file copy. The `spawn_test_child()` helper calls `std::env::current_exe()` which works in any test context.

---

## Common Pitfalls

### Pitfall 1: Circular dependency through error types

**What goes wrong:** `terminate_local_daemon_pid` returns `DaemonBootstrapError` (in `run.rs`). If `daemon_lifecycle.rs` imports from `run.rs`, and `run.rs` imports from `daemon_lifecycle.rs`, circular deps form at compile time.
**How to avoid:** Define `TerminateDaemonError` in `daemon_lifecycle.rs` itself. `run.rs` maps it to `DaemonBootstrapError` at the call site.
**Verification:** `cd src-tauri && cargo check -p uc-daemon-client` must succeed before moving to `cd src-tauri && cargo check -p uc-tauri`.

### Pitfall 2: Incomplete re-export deletion

**What goes wrong:** `bootstrap/mod.rs` still has `pub mod daemon_lifecycle` after file deletion — Rust compile error "file not found".
**How to avoid:** Remove all 3 lines from `bootstrap/mod.rs` in the same commit that deletes `daemon_lifecycle.rs`: `pub mod daemon_lifecycle`, `pub use daemon_lifecycle::GuiOwnedDaemonState`, etc.
**Verification:** `cargo check -p uc-tauri` must show zero references to `uc_tauri::bootstrap::daemon_lifecycle`.

### Pitfall 3: Test process cleanup leak

**What goes wrong:** Tests spawn child processes but don't clean them up, causing PID leaks or hanging processes on subsequent runs.
**How to avoid:** The 3 unit tests in `daemon_lifecycle.rs` all call `cleanup_owned_child()` or `child.kill()/wait()` — pattern is already correct. Confirm the tests pass after migration.

---

## Code Examples

### Existing `terminate_local_daemon_pid` (to be migrated)

```rust
// Current in run.rs:590-627
pub(crate) fn terminate_local_daemon_pid(pid: u32) -> Result<(), DaemonBootstrapError> {
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("kill");
        command.arg("-TERM").arg(pid.to_string());
        command
    };

    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("taskkill");
        command.arg("/PID").arg(pid.to_string()).arg("/T").arg("/F");
        command
    };

    let output = command.output()
        .map_err(|error| DaemonBootstrapError::IncompatibleDaemon { ... })?;

    if output.status.success() {
        return Ok(());
    }
    Err(DaemonBootstrapError::IncompatibleDaemon { ... })
}
```

### New in `daemon_lifecycle.rs`

```rust
pub struct TerminateDaemonError(pub String);

impl std::fmt::Display for TerminateDaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TerminateDaemonError {}

pub(crate) fn terminate_local_daemon_pid(pid: u32) -> Result<(), TerminateDaemonError> {
    #[cfg(unix)]
    let mut command = Command::new("kill");
    #[cfg(windows)]
    let mut command = Command::new("taskkill");

    // Platform-specific args...
    let output = command.output()
        .map_err(|e| TerminateDaemonError(format!("failed to launch terminator: {e}")))?;

    if output.status.success() {
        return Ok(());
    }
    Err(TerminateDaemonError(format!(
        "failed to terminate pid {pid}: status={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}
```

### Updated re-export in `run.rs` (after migration)

```rust
// Remove local definition; add re-export at top of run.rs
pub use uc_daemon_client::daemon_lifecycle::terminate_local_daemon_pid;

// Internal caller in run.rs maps to DaemonBootstrapError:
terminate_local_daemon_pid(pid)
    .map_err(|e| DaemonBootstrapError::IncompatibleDaemon {
        details: e.to_string(),
    })?;
```

### Updated caller in migrated `daemon_lifecycle.rs`

```rust
// In shutdown_owned_daemon(), line ~139:
if let Err(error) = terminate_local_daemon_pid(daemon_pid) {
    // error is TerminateDaemonError, Display used for log only
    let cleanup_error = DaemonExitCleanupError::Terminate {
        pid: daemon_pid,
        details: error.to_string(),  // Display impl, no new error type needed
    };
    // ...
}
```

---

## State of the Art

| Old Approach                                       | Current Approach                                                           | When Changed | Impact                                                                             |
| -------------------------------------------------- | -------------------------------------------------------------------------- | ------------ | ---------------------------------------------------------------------------------- |
| `daemon_lifecycle.rs` in `uc-tauri/bootstrap/`     | `daemon_lifecycle.rs` in `uc-daemon-client/src/`                           | Phase 55     | Removes Tauri dep from daemon lifecycle code                                       |
| `terminate_local_daemon_pid` in `run.rs`           | `terminate_local_daemon_pid` in `uc-daemon-client/src/daemon_lifecycle.rs` | Phase 55     | Self-contained module                                                              |
| `setup_pairing_bridge.rs` in `uc-tauri/bootstrap/` | DELETED                                                                    | Phase 55     | Dead code removed (Phase 54 created replacement in `uc-daemon-client/realtime.rs`) |

---

## Open Questions

1. **Can `terminate_local_daemon_pid` keep `pub(crate)` visibility after migration?**
   - What we know: `run.rs` calls it internally. `daemon_lifecycle.rs` calls it. No external crate uses it.
   - What's unclear: Should it be `pub(crate)` (accessible within `uc-daemon-client`) or `pub` (if any other crate in the workspace needs it)?
   - Recommendation: Keep `pub(crate)` — no known external callers.

2. **Sub-module拆分的必要性:**
   - What we know: `daemon_lifecycle.rs` has ~340 lines, `terminate_local_daemon_pid` is ~40 lines.
   - What's unclear: Is separating `terminate_local_daemon_pid` into a sub-module (`daemon_lifecycle::process.rs`) worth the added complexity?
   - Recommendation: Keep as single file (Claude's Discretion D-05 favors simplicity).

---

## Environment Availability

Step 2.6: SKIPPED — no external dependencies. This is a pure Rust intra-workspace refactor. All tools (Rust toolchain, cargo) are assumed available.

---

## Validation Architecture

### Test Framework

| Property           | Value                                                                                                              |
| ------------------ | ------------------------------------------------------------------------------------------------------------------ |
| Framework          | Rust `#[test]` / `#[tokio::test]` (built-in)                                                                       |
| Config file        | None — uses Cargo.toml dev-dependencies                                                                            |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon-client daemon_lifecycle -- --test-threads=1`                              |
| Full suite command | `cd src-tauri && cargo test -p uc-daemon-client -- --test-threads=1 && cargo test -p uc-tauri -- --test-threads=1` |

### Phase Requirements Map

No explicit requirement IDs for Phase 55 (TBD). Validation is compile + test only.

| Behavior                                                       | Test Type   | Automated Command                                                                             | File Exists?  |
| -------------------------------------------------------------- | ----------- | --------------------------------------------------------------------------------------------- | ------------- |
| `uc-daemon-client` compiles with new `daemon_lifecycle` module | build check | `cd src-tauri && cargo check -p uc-daemon-client`                                             | ✅            |
| `uc-daemon-client` unit tests pass after migration             | unit        | `cd src-tauri && cargo test -p uc-daemon-client daemon_lifecycle -- --test-threads=1`         | ✅ (existing) |
| `uc-tauri` compiles after import path updates                  | build check | `cd src-tauri && cargo check -p uc-tauri`                                                     | ✅            |
| `daemon_exit_cleanup` integration tests pass                   | integration | `cd src-tauri && cargo test -p uc-tauri --test daemon_exit_cleanup -- --test-threads=1`       | ✅            |
| `daemon_bootstrap_contract` integration tests pass             | integration | `cd src-tauri && cargo test -p uc-tauri --test daemon_bootstrap_contract -- --test-threads=1` | ✅            |
| No `uc_tauri::bootstrap::daemon_lifecycle` references remain   | grep check  | `rg 'uc_tauri::bootstrap.*daemon_lifecycle' src-tauri/src/ src-tauri/crates/uc-tauri/`        | ✅            |
| `setup_pairing_bridge` fully removed from `uc-tauri`           | grep check  | `rg 'setup_pairing_bridge' src-tauri/crates/uc-tauri/src/`                                    | ✅ empty      |

### Wave 0 Gaps

None — existing test infrastructure covers all phase requirements. The 3 unit tests in `daemon_lifecycle.rs` migrate with the file.

---

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-tauri/src/bootstrap/daemon_lifecycle.rs` — source of truth for the module being migrated
- `src-tauri/crates/uc-tauri/src/bootstrap/run.rs` — `terminate_local_daemon_pid` definition (lines 590-627) and callers
- `src-tauri/crates/uc-daemon-client/src/lib.rs` — target crate structure, existing module pattern
- `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` — re-export entries to be removed
- `src-tauri/src/main.rs` — `GuiOwnedDaemonState` usage sites (lines 26, 332, 375)

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-tauri/tests/daemon_exit_cleanup.rs` — import paths to update
- `src-tauri/crates/uc-tauri/tests/daemon_bootstrap_contract.rs` — import paths to update
- `.planning/phases/54-extract-daemon-client-and-realtime-infrastructure-from-uc-tauri/54-CONTEXT.md` — extraction pattern precedent

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — pure intra-workspace refactor, no new deps
- Architecture: HIGH — extraction pattern already established by Phase 40, 54
- Pitfalls: HIGH — known pitfalls (circular deps, incomplete re-export deletion) are predictable and have clear preventions

**Research date:** 2026-03-24
**Valid until:** 2026-04-24 (stable refactor pattern, no external dependencies)

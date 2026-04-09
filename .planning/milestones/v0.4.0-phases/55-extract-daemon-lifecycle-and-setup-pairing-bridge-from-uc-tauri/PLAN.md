---
gsd_plan_version: 1.0
phase: 55
wave: coordination
autonomous: false
depends_on: []
files_modified:
  - src-tauri/crates/uc-daemon-client/src/daemon_lifecycle.rs
  - src-tauri/crates/uc-daemon-client/src/lib.rs
  - src-tauri/crates/uc-tauri/src/main.rs
  - src-tauri/crates/uc-tauri/src/bootstrap/mod.rs
  - src-tauri/crates/uc-tauri/src/bootstrap/run.rs
  - src-tauri/crates/uc-tauri/src/bootstrap/daemon_lifecycle.rs
  - src-tauri/crates/uc-tauri/src/bootstrap/setup_pairing_bridge.rs
  - src-tauri/crates/uc-tauri/tests/daemon_exit_cleanup.rs
  - src-tauri/crates/uc-tauri/tests/daemon_bootstrap_contract.rs
---

# Phase 55: Extract Daemon Lifecycle and Setup Pairing Bridge from uc-tauri

## Goal

Move `daemon_lifecycle.rs` (GUI-owned daemon process lifecycle management) from `uc-tauri/bootstrap/` to `uc-daemon-client/src/`, move `terminate_local_daemon_pid` from `run.rs` into the same module with a self-contained error type, delete dead `setup_pairing_bridge.rs`, and update all import paths.

## Background

Phase 54 extracted daemon HTTP client, WebSocket bridge, realtime runtime, and connection state from `uc-tauri` into `uc-daemon-client`. Phase 55 continues this extraction by moving the daemon lifecycle management code, which has zero Tauri dependencies and belongs in the daemon client crate.

## Decisions (locked)

- **D-01:** `daemon_lifecycle.rs` migrates to `uc-daemon-client/src/daemon_lifecycle.rs`
- **D-02:** `setup_pairing_bridge.rs` deleted from `uc-tauri/bootstrap/`
- **D-03:** No new crate — reuse existing `uc-daemon-client`
- **D-04:** `terminate_local_daemon_pid()` moves from `run.rs` to `daemon_lifecycle.rs`
- **D-05:** Migrated `daemon_lifecycle.rs` is fully self-contained, zero cross-module dependencies
- **D-06:** `run.rs` re-imports `terminate_local_daemon_pid` from `uc-daemon-client`
- **D-07:** One commit per logical step
- **D-08:** No re-export stubs (Phase 54 verified one-pass cutover works)
- **D-09:** Dead re-export lines for `setup_pairing_bridge` removed from `bootstrap/mod.rs`
- **D-10:** 3 `#[cfg(test)]` unit tests migrate with `daemon_lifecycle.rs` to `uc-daemon-client`
- **D-11:** `uc-tauri/tests/daemon_exit_cleanup.rs` and `daemon_bootstrap_contract.rs` update imports

## Error Type Design

`terminate_local_daemon_pid` currently returns `Result<(), DaemonBootstrapError>` (defined in `run.rs`). Since `daemon_lifecycle.rs` must be self-contained (D-05), define `TerminateDaemonError(String)` in `daemon_lifecycle.rs`:

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

`run.rs` re-imports and maps to `DaemonBootstrapError` at the call site:

```rust
pub use uc_daemon_client::daemon_lifecycle::terminate_local_daemon_pid;
// ...
terminate_local_daemon_pid(pid).map_err(|e| DaemonBootstrapError::IncompatibleDaemon {
    details: e.to_string(),
})?;
```

## Plans

| #   | Plan                             | Wave | Depends | Summary                                                                                                                                                  |
| --- | -------------------------------- | ---- | ------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 01  | [55-01-PLAN.md](./55-01-PLAN.md) | 1    | —       | Create `uc-daemon-client/src/daemon_lifecycle.rs` with migrated code + `TerminateDaemonError`, update `lib.rs`, verify `cargo check -p uc-daemon-client` |
| 02  | [55-02-PLAN.md](./55-02-PLAN.md) | 2    | 55-01   | Update all `uc-tauri` call sites (import paths, mod.rs cleanup, file deletions), verify `cargo check -p uc-tauri`                                        |

## Must-Haves (Goal-Backward Verification)

- [ ] `uc-daemon-client` compiles with new `daemon_lifecycle` module
- [ ] `uc-daemon-client` unit tests pass after migration
- [ ] `uc-tauri` compiles after import path updates
- [ ] `daemon_exit_cleanup` integration tests pass
- [ ] `daemon_bootstrap_contract` integration tests pass
- [ ] No `uc_tauri::bootstrap::daemon_lifecycle` references remain in `uc-tauri`
- [ ] `setup_pairing_bridge` fully removed from `uc-tauri`

## Out of Scope

- Creating a new crate (reuse `uc-daemon-client`)
- Modifying daemon business logic (code move only)
- Phase 56 refactor work

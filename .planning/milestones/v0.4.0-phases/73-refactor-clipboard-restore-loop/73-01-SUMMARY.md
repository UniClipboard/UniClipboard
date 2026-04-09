---
phase: 73-refactor-clipboard-restore-loop-prevention
plan: 01
subsystem: clipboard
tags: [clipboard, coordinator, refactor, origin-guard, bootstrap, architecture]
dependency_graph:
  requires: []
  provides: [ClipboardWriteCoordinator, ClipboardWriteIntent, new_clipboard_change_origin]
  affects: [uc-app, uc-bootstrap, uc-infra, uc-daemon, uc-tauri]
tech_stack:
  added: []
  patterns: [coordinator-pattern, factory-function, builder-pattern]
key_files:
  created:
    - src-tauri/crates/uc-app/src/usecases/clipboard/clipboard_write_coordinator.rs
  modified:
    - src-tauri/crates/uc-app/src/usecases/clipboard/mod.rs
    - src-tauri/crates/uc-app/src/usecases/mod.rs
    - src-tauri/crates/uc-app/src/runtime.rs
    - src-tauri/crates/uc-bootstrap/src/assembly.rs
    - src-tauri/crates/uc-bootstrap/src/lib.rs
    - src-tauri/crates/uc-bootstrap/src/background_tasks.rs
    - src-tauri/crates/uc-infra/src/clipboard/change_origin.rs
    - src-tauri/crates/uc-infra/src/clipboard/mod.rs
    - src-tauri/crates/uc-daemon/src/entrypoint.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/crates/uc-tauri/src/commands/encryption.rs
    - src-tauri/crates/uc-tauri/src/commands/clipboard.rs
    - src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs
    - src-tauri/src/main.rs
decisions:
  - 'ClipboardWriteIntent enum defined in ClipboardWriteCoordinator (not uc-core) to keep coordinator self-contained'
  - 'CoreRuntime.clipboard_write_coordinator is Option to allow CLI runtimes (no clipboard writes) to pass None'
  - 'InMemoryClipboardChangeOrigin locked to pub(crate) via factory function new_clipboard_change_origin() in uc-infra'
  - 'AppRuntime::with_clipboard_write_coordinator() uses Arc::get_mut before sharing to avoid cross-crate field access'
  - 'set_clipboard_write_coordinator(&mut self) added alongside builder method for mutable-reference mutation path'
metrics:
  duration_minutes: 14
  completed_date: '2026-03-29'
  tasks_completed: 2
  files_modified: 14
---

# Phase 73 Plan 01: ClipboardWriteCoordinator Implementation Summary

**One-liner:** ClipboardWriteCoordinator centralises guard-registration + write + cleanup-on-error into a single write boundary, with InMemoryClipboardChangeOrigin locked to pub(crate) via factory function.

## What Was Built

### ClipboardWriteCoordinator (Task 1)

New file `clipboard_write_coordinator.rs` implements `ClipboardWriteCoordinator::write(snapshot, intent)` as the sole clipboard write entry point:

- `ClipboardWriteIntent` enum: `LocalRestore`, `RemotePush`, `LocalCapture`
- Per-intent TTL: `Duration::from_secs(2)` for local intents, `Duration::from_secs(60)` for `RemotePush`
- `LocalRestore`/`LocalCapture`: calls `remember_local_snapshot_hash(key, 2s)`, writes, cleans up on error
- `RemotePush`: calls `remember_remote_snapshot_hash(key, 60s)`, writes, then calls `set_next_origin(RemotePush, 60s)` for OS re-encoding loopback guard
- Error path: calls `consume_origin_for_snapshot_or_default` to prevent stale guard accumulation
- 5 unit tests covering all intent variants and error paths (all passing)
- Registered in `clipboard/mod.rs` and re-exported from `usecases/mod.rs`

### Bootstrap Wiring (Task 2)

- `build_clipboard_write_coordinator()` added to `assembly.rs` as a pure builder function
- `BackgroundRuntimeDeps.clipboard_write_coordinator` field added
- Coordinator constructed and stored in `WiredDependencies.background` during `wire_dependencies_with_identity_store()`
- `CoreRuntime.clipboard_write_coordinator: Option<Arc<ClipboardWriteCoordinator>>` field added (defaults to `None`)
- `CoreRuntime::set_clipboard_write_coordinator()` and `with_clipboard_write_coordinator()` builder
- `CoreUseCases::clipboard_write_coordinator()` accessor returns `Option<Arc<ClipboardWriteCoordinator>>`
- Daemon entrypoint extracts coordinator from background and chains `.with_clipboard_write_coordinator()`
- GUI main.rs chains `.with_clipboard_write_coordinator(background.clipboard_write_coordinator.clone())`
- `AppRuntime::with_clipboard_write_coordinator()` added (builder, uses `Arc::get_mut` before sharing)
- `build_clipboard_write_coordinator` re-exported from `uc-bootstrap/src/lib.rs`

### InMemoryClipboardChangeOrigin Lockdown

- `pub struct InMemoryClipboardChangeOrigin` changed to `pub(crate)` in `change_origin.rs`
- `pub use change_origin::InMemoryClipboardChangeOrigin` removed from `uc-infra/src/clipboard/mod.rs`
- `pub fn new_clipboard_change_origin()` factory added to `uc-infra/src/clipboard/mod.rs`
- All 5 external usages (uc-app tests, uc-tauri bootstrap/commands tests) migrated to `new_clipboard_change_origin()`
- `assembly.rs` migrated from `Arc::new(InMemoryClipboardChangeOrigin::new())` to `new_clipboard_change_origin()`

## Verification

- `cargo test -p uc-app clipboard_write_coordinator`: 5/5 tests pass
- `cargo test -p uc-infra`: 16 tests pass (existing change_origin tests green)
- `cargo build -p uc-daemon -p uc-tauri`: compiles without error
- `grep "pub struct InMemoryClipboardChangeOrigin"`: no matches (only `pub(crate)`)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Import path correction for ClipboardChangeOrigin/SystemClipboardSnapshot**

- **Found during:** Task 1 initial compilation
- **Issue:** Used `uc_core::clipboard::change::ClipboardChangeOrigin` and `uc_core::clipboard::system::SystemClipboardSnapshot` (private submodules)
- **Fix:** Changed to `uc_core::clipboard::ClipboardChangeOrigin` and `uc_core::clipboard::SystemClipboardSnapshot` (public re-exports)
- **Files modified:** clipboard_write_coordinator.rs

**2. [Rule 1 - Bug] RepresentationId/MimeType constructor API correction in tests**

- **Found during:** Task 1 test compilation
- **Issue:** `RepresentationId::new("rep-1")` (takes no args), `MimeType::from("text/plain")` (wrong From impl)
- **Fix:** Changed to `RepresentationId::from_str("rep-1")` and `MimeType("text/plain".to_string())`
- **Files modified:** clipboard_write_coordinator.rs tests

**3. [Rule 2 - Missing critical functionality] set_clipboard_write_coordinator(&mut self) method**

- **Found during:** Task 2, AppRuntime cross-crate wiring
- **Issue:** `CoreRuntime.clipboard_write_coordinator` is `pub(crate)` so cannot be accessed from uc-tauri; needed a `&mut self` method for `Arc::get_mut()` pattern
- **Fix:** Added `set_clipboard_write_coordinator(&mut self, ...)` alongside the builder method
- **Files modified:** runtime.rs

**4. [Rule 2 - Missing critical functionality] Migrate all external InMemoryClipboardChangeOrigin usages**

- **Found during:** Task 2, test compilation after lockdown
- **Issue:** 5 external usages (uc-app integration tests, uc-tauri command/bootstrap tests) broke when `InMemoryClipboardChangeOrigin` became `pub(crate)`
- **Fix:** Replaced all `Arc::new(InMemoryClipboardChangeOrigin::new())` with `new_clipboard_change_origin()`
- **Files modified:** clipboard_sync_e2e_test.rs, runtime.rs (tests), encryption.rs (tests), clipboard.rs (tests)

## Known Stubs

None. All coordinator functionality is fully implemented and tested.

## Self-Check: PASSED

Files exist:

- `src-tauri/crates/uc-app/src/usecases/clipboard/clipboard_write_coordinator.rs` ✓

Commits exist:

- `63d406e2` feat(73-01): add ClipboardWriteCoordinator with write() method and unit tests ✓
- `5ab40afe` feat(73-01): wire ClipboardWriteCoordinator into bootstrap and CoreUseCases ✓

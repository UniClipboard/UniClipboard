---
phase: 60-extract-file-transfer-wiring-orchestration-from-uc-tauri-to-uc-app
plan: 02
subsystem: infra
tags: [rust, file-transfer, orchestrator, uc-tauri, wiring]

# Dependency graph
requires:
  - phase: 60-01
    provides: FileTransferOrchestrator in uc-app, BackgroundRuntimeDeps.file_transfer_orchestrator field
provides:
  - wiring.rs fully migrated to FileTransferOrchestrator method calls
  - file_transfer_wiring.rs deleted from uc-tauri (no stubs)
  - FileTransferStatusPayload re-exported from uc-app::usecases::file_sync
affects:
  - uc-tauri/bootstrap/wiring.rs (all standalone function call sites replaced)
  - uc-tauri/bootstrap/mod.rs (pub mod file_transfer_wiring removed)
  - uc-app/usecases/file_sync/mod.rs (FileTransferStatusPayload added to re-exports)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "FileTransferOrchestrator::now_ms() accessor: exposes internal clock for clipboard receive loop PendingInboundTransfer.created_at_ms"
    - "Arc clone before closure: file_transfer_orchestrator cloned before clipboard_receive spawn to allow reuse in reconcile/sweep tasks"

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/mod.rs
    - src-tauri/crates/uc-app/src/usecases/file_sync/file_transfer_orchestrator.rs
    - src-tauri/crates/uc-app/src/usecases/file_sync/mod.rs
    - src-tauri/crates/uc-tauri/tests/models_serialization_test.rs
    - src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs
  deleted:
    - src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs

key-decisions:
  - "FileTransferOrchestrator::now_ms() accessor added to orchestrator — clipboard receive loop needs timestamp for PendingInboundTransfer.created_at_ms without requiring a separate clock parameter"
  - "clipboard_receive_orchestrator clone created before the registry.spawn('clipboard_receive') closure — prevents move conflict with later reconcile/sweep uses of the same Arc"
  - "FileTransferStatusPayload added to uc-app::usecases::file_sync re-exports — test file models_serialization_test.rs depends on it and needs stable import path after file_transfer_wiring.rs deletion"

requirements-completed:
  - PH60-04
  - PH60-05

# Metrics
duration: 9min
completed: 2026-03-25
---

# Phase 60 Plan 02: Rewire wiring.rs to FileTransferOrchestrator, delete file_transfer_wiring.rs

**All file_transfer_wiring.rs standalone functions replaced by FileTransferOrchestrator method calls; file deleted with no stubs; full workspace compiles**

## Performance

- **Duration:** 9 min
- **Started:** 2026-03-25T13:33:42Z
- **Completed:** 2026-03-25T13:42:51Z
- **Tasks:** 1 (+ 1 auto-fix deviation)
- **Files modified:** 6 (1 deleted)

## Accomplishments

- Rewired `start_background_tasks` to destructure `file_transfer_orchestrator` from `BackgroundRuntimeDeps` (direct `Arc`, no expect/unwrap)
- Updated `register_pairing_background_tasks` signature: replaced `inbound_file_transfer_repo`, `inbound_clock`, `early_completion_cache` params with `Arc<FileTransferOrchestrator>`
- Updated `run_network_realtime_loop` signature: replaced `file_transfer_repo`, `clock`, `early_completion_cache` params with `Arc<FileTransferOrchestrator>` — eliminates 2nd `TrackInboundTransfersUseCase` construction
- Updated `run_clipboard_receive_loop` signature: replaced `transfer_tracker`, `clock`, `early_completion_cache` params with `Option<Arc<FileTransferOrchestrator>>` — eliminates 3rd and 4th constructions
- All 6 `super::file_transfer_wiring::*` call sites replaced with orchestrator method calls: `emit_pending_status`, `handle_transfer_progress`, `handle_transfer_completed`, `handle_transfer_failed`, `reconcile_on_startup`, `spawn_timeout_sweep`
- `file_transfer_wiring.rs` deleted; `pub mod file_transfer_wiring` removed from `mod.rs` (no re-export stubs per D-08)
- Added `FileTransferOrchestrator::now_ms()` accessor for timestamp access in clipboard receive loop
- Added `FileTransferStatusPayload` to `uc-app::usecases::file_sync` re-exports

## Task Commits

1. **Task 1: Rewire wiring.rs to use FileTransferOrchestrator, delete file_transfer_wiring.rs** - `205df84c` (feat)
2. **[Rule 1 - Bug Fix] Update remaining file_transfer_wiring references after deletion** - `c407d588` (fix)

**Plan metadata:** *(this commit)*

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — All call sites migrated to orchestrator methods; BackgroundRuntimeDeps destructure updated; function signatures simplified
- `src-tauri/crates/uc-tauri/src/bootstrap/mod.rs` — `pub mod file_transfer_wiring` removed
- `src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs` — **DELETED** (638 lines removed)
- `src-tauri/crates/uc-app/src/usecases/file_sync/file_transfer_orchestrator.rs` — `now_ms()` accessor added
- `src-tauri/crates/uc-app/src/usecases/file_sync/mod.rs` — `FileTransferStatusPayload` added to re-exports
- `src-tauri/crates/uc-tauri/tests/models_serialization_test.rs` — Import updated from `uc_tauri::bootstrap::file_transfer_wiring` to `uc_app::usecases::file_sync`
- `src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs` — Comment updated to reference correct module path

## Decisions Made

- **now_ms() accessor on orchestrator**: Clipboard receive loop needs a timestamp for `PendingInboundTransfer.created_at_ms`. Instead of passing a separate clock parameter, exposed `now_ms()` on the orchestrator to keep the API unified.
- **Clone before closure**: `file_transfer_orchestrator` is moved into `clipboard_receive` closure; separate clones (`clipboard_receive_orchestrator`) created before closure to allow reuse in reconcile and sweep tasks.
- **No re-export stubs**: Per D-08/D-09, `pub mod file_transfer_wiring` is simply deleted — no stub wrapper left behind.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Missing file_transfer_wiring references in tests and comment**

- **Found during:** Post-compile verification (`cargo test -p uc-tauri`)
- **Issue:** `tests/models_serialization_test.rs` imported `FileTransferStatusPayload` via `uc_tauri::bootstrap::file_transfer_wiring::FileTransferStatusPayload`, which no longer exists after deletion. Also, `FileTransferStatusPayload` was not re-exported from `uc-app::usecases::file_sync`.
- **Fix:** Added `FileTransferStatusPayload` to `uc-app/src/usecases/file_sync/mod.rs` re-exports; updated test import to `uc_app::usecases::file_sync::FileTransferStatusPayload`; updated comment in `host_event_emitter.rs`.
- **Files modified:** `mod.rs`, `models_serialization_test.rs`, `host_event_emitter.rs`
- **Committed in:** `c407d588`

**2. [Rule 2 - Missing Critical] FileTransferOrchestrator::now_ms() accessor not in plan**

- **Found during:** Task 1 (clipboard receive loop needs timestamp for PendingInboundTransfer)
- **Issue:** The plan's step 6 said "use `deps.system.clock.clone()` which is available at the call site" but `run_clipboard_receive_loop` doesn't have access to `deps` — it's called from within the spawn closure.
- **Fix:** Added `pub fn now_ms(&self) -> i64` to `FileTransferOrchestrator`, exposing the internal clock. This is a small, self-contained addition that keeps the API unified.
- **Files modified:** `file_transfer_orchestrator.rs`
- **Committed in:** `205df84c` (Task 1)

---

**Total deviations:** 2 (1 bug fix, 1 missing critical)

## Issues Encountered

- Pre-existing test failure: `bootstrap::run::tests::startup_helper_rejects_healthy_but_incompatible_daemon` panics with `unreachable code` at `run.rs:798` — confirmed pre-existing at commit `e28ff554` (Plan 01). Not caused by our changes.

## Next Phase Readiness

- Phase 60 is now complete: `FileTransferOrchestrator` lives in `uc-app`, is wired via `BackgroundRuntimeDeps`, and all `uc-tauri` call sites use it
- `file_transfer_wiring.rs` is fully deleted with no re-export stubs
- `cargo check -p uc-tauri` and `cargo test -p uc-app file_transfer` both pass clean

## Self-Check: PASSED

- `file_transfer_wiring.rs`: NOT FOUND (deleted as expected)
- `pub mod file_transfer_wiring` in mod.rs: NOT FOUND
- `super::file_transfer_wiring` in wiring.rs: NOT FOUND
- `EarlyCompletionCache::default()` standalone in wiring.rs: NOT FOUND
- `.handle_transfer_progress(` in wiring.rs: FOUND
- `.handle_transfer_completed(` in wiring.rs: FOUND
- `.handle_transfer_failed(` in wiring.rs: FOUND
- `.reconcile_on_startup()` in wiring.rs: FOUND
- `.spawn_timeout_sweep(` in wiring.rs: FOUND
- `.emit_pending_status(` in wiring.rs: FOUND
- `.tracker()` for record_pending in wiring.rs: FOUND
- Commit `205df84c` (Task 1): FOUND
- Commit `c407d588` (Fix): FOUND
- `cargo check -p uc-tauri`: PASSED (0 errors, pre-existing warnings only)
- `cargo test -p uc-app file_transfer`: 8 passed
- `cargo test -p uc-tauri`: 70 passed, 1 pre-existing failure

---
*Phase: 60-extract-file-transfer-wiring-orchestration-from-uc-tauri-to-uc-app*
*Completed: 2026-03-25*

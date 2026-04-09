---
phase: 37-wiring-decomposition
plan: 02
subsystem: infra
tags: [rust, tauri, event-emitter, hexagonal-architecture, refactoring]

requires:
  - phase: 37-wiring-decomposition
    provides: Plan 01 — PairingHostEvent, SetupHostEvent, SpaceAccessHostEvent sub-enums and extended TauriEventEmitter/LoggingEventEmitter

provides:
  - wiring.rs event loops decoupled from Tauri AppHandle for event emission (all 13 app.emit() calls replaced)
  - file_transfer_wiring.rs has zero tauri imports (5 functions migrated)
  - HostEventSetupPort replacing TauriSetupEventPort
  - All background task loops use HostEventEmitterPort exclusively

affects: [37-03-PLAN, wiring-decomposition]

tech-stack:
  added: []
  patterns:
    - 'emitter.emit(HostEvent::...) with best-effort warn! pattern throughout all event loops'
    - 'HostEventSetupPort wrapping Arc<dyn HostEventEmitterPort> as SetupEventPort adapter'

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs

key-decisions:
  - 'handle_pairing_message keeps _app_handle parameter (unused, prefixed _) — removal deferred to Plan 03'
  - 'run_pairing_action_loop keeps _app_handle parameter — removal deferred to Plan 03'
  - 'Test fixtures use TauriEventEmitter or NoopEmitter based on whether they verify Tauri wire format'
  - 'HostEventSetupPort replaces TauriSetupEventPort: simpler design with no RwLock needed'

patterns-established:
  - 'background tasks capture event_emitter.clone() not app_handle.clone() for emit purposes'
  - 'TODO(plan-03) comment marks deferred _app_handle parameters for atomic removal in Plan 03'

requirements-completed:
  - RNTM-02

duration: 35min
completed: 2026-03-17
---

# Phase 37 Plan 02: Wiring.rs & file_transfer_wiring.rs HostEventEmitterPort Migration Summary

**All 13 app.emit() calls in wiring.rs event loops and all 5 AppHandle functions in file_transfer_wiring.rs replaced with HostEventEmitterPort, achieving zero tauri imports in file_transfer_wiring.rs**

## Performance

- **Duration:** ~35 min
- **Started:** 2026-03-17T14:00:00Z
- **Completed:** 2026-03-17T14:35:00Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments

- Migrated all 13 `app.emit()` call sites in wiring.rs: setup-state-changed (1), space-access-completed (2), p2p-pairing-verification (7), pairing-subscribe-failure/recovered (2), inbound-clipboard-subscribe-error/retry (2 — via `clipboard_emitter`)
- Replaced `TauriSetupEventPort` (RwLock-based) with `HostEventSetupPort` (emitter-based, simpler)
- Updated `runtime.rs` `build_setup_orchestrator` to pass `event_emitter` instead of `app_handle`
- Migrated 5 functions in `file_transfer_wiring.rs`: `handle_transfer_progress`, `handle_transfer_completed`, `handle_transfer_failed`, `spawn_timeout_sweep`, `reconcile_on_startup` — all now take `&dyn HostEventEmitterPort` or `Arc<dyn HostEventEmitterPort>`
- Removed `use tauri::{AppHandle, Emitter}` from `file_transfer_wiring.rs` — zero tauri imports
- Updated all callers in wiring.rs (7 call sites in pairing event loop, file transfer loop)
- All 211 tests passing (including existing contract tests from Plan 37-01)

## Task Commits

1. **Task 1: Migrate wiring.rs app.emit calls to HostEventEmitterPort** - `c31a4702` (refactor)
2. **Task 2: Migrate file transfer functions from AppHandle to HostEventEmitterPort** - `56e8f870` (refactor)

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` - Replaced TauriSetupEventPort, 13 app.emit() → HostEvent emitter calls, updated callers
- `src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs` - 5 functions migrated, zero tauri imports
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - `build_setup_orchestrator` signature updated to accept `event_emitter`

## Decisions Made

- `_app_handle` parameters kept in `handle_pairing_message` and `run_pairing_action_loop` with `// TODO(plan-03)` comments. Removal requires updating `main.rs` caller at the same time — atomic with Plan 03.
- Test fixtures use `TauriEventEmitter::new(app_handle)` when the test verifies the Tauri wire format (JSON field names/casing), and `NoopEmitter` when only business logic is under test.
- Removed intermediate payload structs (`InboundClipboardSubscribeErrorPayload`, `PairingEventsSubscribeFailurePayload`, etc.) since serialization is now owned by `TauriEventEmitter` adapter.

## Deviations from Plan

None - plan executed exactly as written. Tasks 1 and 2 were committed atomically in the correct order (Task 2 file_transfer_wiring.rs changes required for Task 1 callers in wiring.rs to compile, so both were developed in a single pass).

## Issues Encountered

- Test code used `P2PPairingVerificationEvent` (now removed from production code) and called `handle_pairing_message` with old 6-arg signature. Fixed by: (a) replacing probe with `serde_json::json!`, (b) adding `NoopEmitter` struct in test module, (c) passing `TauriEventEmitter` or `NoopEmitter` to all test call sites.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- All event emission in wiring.rs and file_transfer_wiring.rs now uses HostEventEmitterPort
- Plan 03 can safely split wiring.rs into separate files and remove the deferred `_app_handle` parameters from `handle_pairing_message` and `run_pairing_action_loop`
- `start_background_tasks` still has `app_handle: Option<AppHandle<R>>` parameter — Plan 03 will remove it together with the `main.rs` caller update

---

_Phase: 37-wiring-decomposition_
_Completed: 2026-03-17_

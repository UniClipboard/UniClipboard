---
phase: 64-tauri-sync-retirement
plan: 02
subsystem: uc-tauri/commands
tags: [cleanup, daemon-retirement, passive-mode, sync-guard]

dependency_graph:
  requires:
    - phase: 64-01
      provides: removed-daemon-duplicated-sync-loops-from-wiring-rs
  provides:
    - restore_clipboard_entry-skips-outbound-sync-in-Passive-mode
    - dead-sync_inbound_clipboard-accessor-removed
  affects: [uc-tauri/commands/clipboard.rs, uc-tauri/bootstrap/runtime.rs]

tech-stack:
  added: []
  patterns:
    - "Passive mode guard: check ClipboardIntegrationMode::Passive before direct sync to avoid double-send with daemon"

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-tauri/src/commands/clipboard.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs

key-decisions:
  - "restore_clipboard_entry outbound sync skipped entirely in Passive mode; daemon ClipboardWatcherWorker owns it after detecting OS clipboard write"
  - "snapshot.clone() moved inside Passive-mode guard so the clone is skipped when guard prevents outbound sync"
  - "sync_inbound_clipboard accessor deleted from AppUseCases doc comment count updated (5->4)"

patterns-established:
  - "Passive mode guard pattern: !matches!(runtime.clipboard_integration_mode(), ClipboardIntegrationMode::Passive)"

requirements-completed:
  - PH64-04
  - PH64-06

duration: 6min
completed: "2026-03-26"
---

# Phase 64 Plan 02: Gate restore_clipboard_entry Outbound Sync and Remove Dead Accessor Summary

**Passive-mode double-sync eliminated: restore_clipboard_entry now skips direct outbound sync when daemon is running, and dead sync_inbound_clipboard accessor removed from AppUseCases**

## Performance

- **Duration:** ~6 min
- **Started:** 2026-03-26T04:18:00Z
- **Completed:** 2026-03-26T04:24:23Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- `restore_clipboard_entry_impl` now skips the `sync_outbound_clipboard()` call in Passive mode — daemon's ClipboardWatcherWorker detects the OS clipboard write and handles outbound sync, preventing double-send
- `snapshot.clone()` moved inside the guard so the clone allocation is also skipped in Passive mode
- `sync_inbound_clipboard()` accessor deleted from `AppUseCases` in runtime.rs — it had zero callers after 64-01 removed the clipboard_receive loop
- Doc comment on `AppUseCases` updated to reflect 4 non-core accessors (was 5)

## Task Commits

1. **Task 1: Gate restore_clipboard_entry outbound sync on Full mode** - `275704e2` (feat)
2. **Task 2: Remove dead sync_inbound_clipboard accessor from AppUseCases** - `34b70809` (refactor)

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` - Added Passive mode guard around outbound sync block in `restore_clipboard_entry_impl`
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - Removed `sync_inbound_clipboard()` method and updated doc comment

## Decisions Made

- `restore_clipboard_entry` outbound sync skipped in Passive mode; daemon ClipboardWatcherWorker owns it after detecting OS clipboard write
- `snapshot.clone()` moved inside Passive-mode guard so the clone is skipped when the guard prevents outbound sync
- `sync_inbound_clipboard` accessor deleted — zero callers confirmed after 64-01 cleanup

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- uc-daemon test suite showed 5 pre-existing failures (pairing_api_* and pid_path_tracks_uc_profile) unrelated to this plan's changes. Confirmed by running baseline test before applying changes — same failures existed before.

## Known Stubs

None.

## Next Phase Readiness

- Phase 64 plans complete: wiring.rs reduced from 1378 to 482 lines (Plan 01), double-sync eliminated (Plan 02)
- Daemon fully owns inbound clipboard sync; uc-tauri GUI retains outbound sync for standalone (Full) mode only
- `sync_outbound_clipboard` accessor retained — still used by `sync_clipboard_items`, `restore_clipboard_entry` (Full mode), and `on_clipboard_changed`

## Self-Check: PASSED

- [x] `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` exists
- [x] `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` exists
- [x] `64-02-SUMMARY.md` exists
- [x] Commit `275704e2` exists (Task 1: Passive mode guard)
- [x] Commit `34b70809` exists (Task 2: Remove dead accessor)

---
*Phase: 64-tauri-sync-retirement*
*Completed: 2026-03-26*

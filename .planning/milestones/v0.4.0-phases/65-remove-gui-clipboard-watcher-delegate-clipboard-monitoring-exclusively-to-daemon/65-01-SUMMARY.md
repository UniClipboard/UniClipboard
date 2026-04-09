---
phase: 65-remove-gui-clipboard-watcher-delegate-clipboard-monitoring-exclusively-to-daemon
plan: 01
subsystem: platform
tags: [rust, dead-code-removal, hexagonal-architecture, clipboard, daemon]

requires:
  - phase: 57
    provides: daemon clipboard watcher integration making GUI watcher obsolete
provides:
  - uc-platform crate without runtime/ipc/watcher infrastructure
  - uc-core without StartClipboardWatcherPort
  - Minimal PlatformEvent inlined into clipboard/watcher.rs
affects: [65-02, uc-tauri wiring, app-lifecycle]

tech-stack:
  added: []
  patterns:
    - 'Inline domain events into consuming module when full event bus deleted'

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-platform/src/lib.rs
    - src-tauri/crates/uc-platform/src/ports/mod.rs
    - src-tauri/crates/uc-platform/src/adapters/mod.rs
    - src-tauri/crates/uc-platform/src/usecases/mod.rs
    - src-tauri/crates/uc-platform/src/clipboard/watcher.rs
    - src-tauri/crates/uc-platform/src/clipboard/mod.rs
    - src-tauri/crates/uc-core/src/ports/mod.rs
    - src-tauri/crates/uc-app/src/usecases/mod.rs

key-decisions:
  - 'Inlined PlatformEvent (ClipboardChanged only) into clipboard/watcher.rs rather than keeping separate ipc module'
  - 'Re-exported PlatformEvent and PlatformEventSender from clipboard/mod.rs for external consumers'

patterns-established:
  - 'When deleting event bus infrastructure, inline only the variants needed by preserved code'

requirements-completed: [D-01, D-02, D-03, D-04, D-05, D-06, D-07, D-08, D-13, D-15]

duration: 5min
completed: 2026-03-26
---

# Phase 65 Plan 01: Remove GUI Clipboard Watcher Infrastructure Summary

**Deleted PlatformRuntime, IPC, event bus, watcher control, and StartClipboardWatcherPort -- 12 files removed, uc-platform stripped to clipboard-only platform crate**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-26T13:15:39Z
- **Completed:** 2026-03-26T13:20:59Z
- **Tasks:** 2
- **Files modified:** 19

## Accomplishments

- Deleted entire runtime/ directory (PlatformRuntime, event bus)
- Deleted entire ipc/ directory (PlatformCommand, PlatformEvent)
- Removed WatcherControlPort, PlatformCommandExecutorPort, ClipboardRuntimePort from ports
- Removed InMemoryWatcherControl from adapters
- Removed StartClipboardWatcher use case from uc-platform
- Deleted StartClipboardWatcherPort trait from uc-core
- Removed re-exports from uc-app usecases/mod.rs
- Preserved ClipboardWatcher and LocalClipboard in clipboard module

## Task Commits

Each task was committed atomically:

1. **Task 1: Delete uc-platform runtime, ipc, watcher, and executor infrastructure** - `bd558783` (refactor)
2. **Task 2: Delete StartClipboardWatcherPort from uc-core and remove re-exports from uc-app** - `69fd6465` (refactor)

## Files Created/Modified

- `src-tauri/crates/uc-platform/src/lib.rs` - Removed ipc and runtime module declarations
- `src-tauri/crates/uc-platform/src/ports/mod.rs` - Removed watcher_control, command_executor, clipboard_runtime
- `src-tauri/crates/uc-platform/src/adapters/mod.rs` - Removed in_memory_watcher_control
- `src-tauri/crates/uc-platform/src/usecases/mod.rs` - Removed start_clipboard_watcher, kept apply_autostart only
- `src-tauri/crates/uc-platform/src/clipboard/watcher.rs` - Inlined PlatformEvent and PlatformEventSender
- `src-tauri/crates/uc-platform/src/clipboard/mod.rs` - Added re-exports for PlatformEvent/PlatformEventSender
- `src-tauri/crates/uc-core/src/ports/mod.rs` - Removed start_clipboard_watcher module and re-exports
- `src-tauri/crates/uc-app/src/usecases/mod.rs` - Removed StartClipboardWatcherPort/Error re-export

### Deleted Files (12)

- `src-tauri/crates/uc-platform/src/runtime/runtime.rs`
- `src-tauri/crates/uc-platform/src/runtime/event_bus.rs`
- `src-tauri/crates/uc-platform/src/runtime/mod.rs`
- `src-tauri/crates/uc-platform/src/ipc/command.rs`
- `src-tauri/crates/uc-platform/src/ipc/event.rs`
- `src-tauri/crates/uc-platform/src/ipc/mod.rs`
- `src-tauri/crates/uc-platform/src/ports/watcher_control.rs`
- `src-tauri/crates/uc-platform/src/ports/command_executor.rs`
- `src-tauri/crates/uc-platform/src/ports/clipboard_runtime.rs`
- `src-tauri/crates/uc-platform/src/adapters/in_memory_watcher_control.rs`
- `src-tauri/crates/uc-platform/src/usecases/start_clipboard_watcher.rs`
- `src-tauri/crates/uc-core/src/ports/start_clipboard_watcher.rs`
- `src-tauri/crates/uc-platform/tests/runtime_test.rs`
- `src-tauri/crates/uc-platform/tests/watcher_control_test.rs`

## Decisions Made

- Inlined PlatformEvent (ClipboardChanged variant only) into clipboard/watcher.rs because the full event type had 7 variants but the clipboard watcher only uses ClipboardChanged. This avoids keeping an entire ipc module for one variant.
- Re-exported PlatformEvent and PlatformEventSender from clipboard/mod.rs so that uc-tauri wiring (Plan 02 scope) can import them from a stable path.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Inlined PlatformEvent into clipboard/watcher.rs**

- **Found during:** Task 1 (cargo check after deleting ipc module)
- **Issue:** ClipboardWatcher in clipboard/watcher.rs imported PlatformEvent from crate::ipc and PlatformEventSender from crate::runtime::event_bus, both deleted
- **Fix:** Defined minimal PlatformEvent enum (ClipboardChanged only) and PlatformEventSender type alias directly in watcher.rs; re-exported from clipboard/mod.rs
- **Files modified:** src-tauri/crates/uc-platform/src/clipboard/watcher.rs, src-tauri/crates/uc-platform/src/clipboard/mod.rs
- **Verification:** cargo check -p uc-platform passes
- **Committed in:** bd558783 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary to preserve clipboard watcher functionality after ipc module deletion. No scope creep.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 02 can now remove uc-tauri references to PlatformCommandSender and update wiring.rs to use the new import paths
- ClipboardChangeHandler trait preserved in uc-core for daemon integration
- clipboard_rs dependency preserved in uc-platform per D-13

---

_Phase: 65-remove-gui-clipboard-watcher-delegate-clipboard-monitoring-exclusively-to-daemon_
_Completed: 2026-03-26_

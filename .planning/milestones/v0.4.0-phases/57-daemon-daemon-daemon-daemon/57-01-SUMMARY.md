---
phase: 57-daemon-daemon-daemon-daemon
plan: 01
subsystem: daemon
tags: [rust, daemon, clipboard, clipboard_rs, websocket, capture-usecase]

# Dependency graph
requires:
  - phase: 56.1-eliminate-hardcoded-strings-in-pairing-setup-flow
    provides: daemon_api_strings WS constants pattern used by this plan
provides:
  - Real ClipboardWatcherWorker with DaemonClipboardChangeHandler in uc-daemon
  - ws_topic::CLIPBOARD and ws_event::CLIPBOARD_NEW_CONTENT constants in uc-core
  - Daemon clipboard capture pipeline: OS change -> ClipboardWatcher dedup -> CaptureClipboardUseCase -> WS broadcast
affects: [57-02, 57-03, daemon-client, uc-daemon-client]

# Tech tracking
tech-stack:
  added: [clipboard-rs 0.3.3 as runtime dep in uc-daemon, uc-platform as runtime dep in uc-daemon]
  patterns:
    - DaemonClipboardChangeHandler implementing ClipboardChangeHandler for daemon-side capture
    - tokio::task::spawn_blocking for clipboard_rs blocking watcher loop
    - WatcherShutdown::stop() on CancellationToken for clean watcher exit

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-core/src/network/daemon_api_strings.rs
    - src-tauri/crates/uc-daemon/Cargo.toml
    - src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs
    - src-tauri/crates/uc-daemon/src/main.rs

key-decisions:
  - "Use ClipboardChangeOrigin::LocalCapture directly in daemon (no consume_origin_for_snapshot_or_default) -- write-back loop prevention deferred to Plan 03"
  - "ClipboardWatcherWorker holds mpsc channel to bridge blocking watcher thread to async select! loop"
  - "WatcherShutdown stays within start() async fn -- never crosses await boundary to another task (WatcherShutdown is !Send)"
  - "broadcast::send Err on no receivers logged at debug level (expected when no WS clients connected)"
  - "clipboard-rs added as direct runtime dep to uc-daemon (not just transitive via uc-platform)"

patterns-established:
  - "Daemon clipboard capture: ClipboardWatcher(dedup) -> mpsc channel -> DaemonClipboardChangeHandler -> CaptureClipboardUseCase -> broadcast WS event"

requirements-completed: [PH57-01, PH57-02, PH57-03]

# Metrics
duration: 8min
completed: 2026-03-25
---

# Phase 57 Plan 01: Daemon Clipboard Watcher Integration Summary

**Real daemon clipboard watcher using clipboard_rs ClipboardWatcherContext with dedup, CaptureClipboardUseCase persistence, and clipboard.new_content WS broadcast**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-25T04:49:46Z
- **Completed:** 2026-03-25T04:58:05Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Added `ws_topic::CLIPBOARD` and `ws_event::CLIPBOARD_NEW_CONTENT` constants to `daemon_api_strings.rs` for shared WS protocol contracts
- Replaced the placeholder `ClipboardWatcherWorker` with a real implementation using `clipboard_rs::ClipboardWatcherContext` and `spawn_blocking`
- Implemented `DaemonClipboardChangeHandler` that calls `CaptureClipboardUseCase` for persistence and broadcasts `clipboard.new_content` WS events
- Wired `LocalClipboard` + `ClipboardWatcherWorker` + `DaemonClipboardChangeHandler` in daemon `main.rs`

## Task Commits

Each task was committed atomically:

1. **Task 1: Add clipboard WS constants and uc-platform dependency** - `7d16e335` (feat)
2. **Task 2: Implement real ClipboardWatcherWorker with DaemonClipboardChangeHandler** - `2944ad64` (feat)

## Files Created/Modified
- `src-tauri/crates/uc-core/src/network/daemon_api_strings.rs` - Added ws_topic::CLIPBOARD and ws_event::CLIPBOARD_NEW_CONTENT with test assertions
- `src-tauri/crates/uc-daemon/Cargo.toml` - Moved uc-platform to runtime deps, added clipboard-rs 0.3.3
- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` - Full rewrite with real implementation
- `src-tauri/crates/uc-daemon/src/main.rs` - Wired LocalClipboard and DaemonClipboardChangeHandler

## Decisions Made
- Used `ClipboardChangeOrigin::LocalCapture` directly instead of `consume_origin_for_snapshot_or_default` — write-back loop prevention is deferred to Plan 03 when inbound sync is wired
- `WatcherShutdown` (which is `!Send`) stays within the `start()` async fn body, never crossing an `await` boundary to another task — same task creates and calls `shutdown.stop()`
- `broadcast::send` errors when no WS receivers are connected are logged at `debug` level (not `warn`) since no clients is expected when the daemon is running without GUI

## Deviations from Plan

**1. [Rule 3 - Blocking] Added clipboard-rs as direct runtime dependency**
- **Found during:** Task 2 (clipboard_watcher.rs rewrite)
- **Issue:** Plan said to check if clipboard_rs is transitively available via uc-platform re-export; it is not re-exported, so `use clipboard_rs::...` in uc-daemon code fails
- **Fix:** Added `clipboard-rs = { version = "0.3.3", features = ["default"] }` to uc-daemon Cargo.toml (matching uc-platform's version)
- **Files modified:** src-tauri/crates/uc-daemon/Cargo.toml
- **Verification:** `cargo check -p uc-daemon` exits 0
- **Committed in:** 2944ad64 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** The fix was necessary for compilation. No scope creep.

## Issues Encountered
- Pre-existing flaky test `daemon_pid_guard_removes_pid_file_on_drop` (parsing empty string) -- passes in isolation, fails intermittently in parallel run. Confirmed pre-existing via stash check.
- Pre-existing `pairing_api` integration test failures -- confirmed the same failures exist on original codebase before any changes.

## Known Stubs
None - the clipboard watcher is fully wired. `ClipboardChangeOrigin::LocalCapture` is used directly (intentional stub deferred to Plan 03 per plan specification).

## Next Phase Readiness
- Daemon now captures clipboard changes via `ClipboardWatcherWorker` and broadcasts `clipboard.new_content` WS events
- Plan 02 can build on this to handle inbound sync notification to GUI via DaemonWsBridge
- Plan 03 will add `ClipboardChangeOriginPort` shared across daemon and GUI for write-back loop prevention

---
*Phase: 57-daemon-daemon-daemon-daemon*
*Completed: 2026-03-25*

---
phase: 61-daemon-outbound-clipboard-sync
plan: 01
subsystem: daemon
tags: [clipboard, sync, outbound, spawn_blocking, OutboundSyncPlanner, file-sync, url]

# Dependency graph
requires:
  - phase: 57-daemon-clipboard-integration
    provides: DaemonClipboardChangeHandler with ClipboardChangeOriginPort write-back loop prevention
  - phase: 60-extract-file-transfer-wiring-orchestration-from-uc-tauri-to-uc-app
    provides: OutboundSyncPlanner, SyncOutboundClipboardUseCase, FileTransferOrchestrator in uc-app
provides:
  - Daemon outbound clipboard sync: after LocalCapture, dispatches SyncOutboundClipboardUseCase via spawn_blocking
  - File path extraction from snapshot representations (text/uri-list, file/uri-list, public.file-url)
  - FileCandidate construction with metadata pre-fetching
  - File sync dispatch via SyncOutboundFileUseCase per planner output
  - RemotePush origin guard: no outbound sync for remote-originated changes
affects: [62-daemon-inbound-clipboard-sync, 63-daemon-file-transfer-orchestration, 64-tauri-sync-retirement]

# Tech tracking
tech-stack:
  added: [url = "2" (file:// URI parsing in uc-daemon)]
  patterns:
    - extract_file_paths_from_snapshot ported from uc-tauri/runtime.rs into uc-daemon
    - SyncOutboundClipboardUseCase constructed via build_sync_outbound_clipboard_use_case helper
    - SyncOutboundFileUseCase built inline from wiring_deps (CoreUseCases not available on Arc<CoreRuntime> directly)

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-daemon/Cargo.toml
    - src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs

key-decisions:
  - "SyncOutboundFileUseCase built inline from wiring_deps — CoreUseCases<'a> lifetime complexity in async fn resolved by direct construction"
  - "Outbound sync dispatch placed inside Ok(Some(entry_id)) arm only — failed captures and deduped captures do not trigger sync"
  - "extract_file_paths_from_snapshot duplicated from uc-tauri/runtime.rs into uc-daemon (not extracted to shared crate) to keep phase scope minimal"
  - "APFS file reference resolution is a no-op stub on macOS — APFS resolution deferred to future phase as in uc-tauri"

patterns-established:
  - "spawn_blocking wrapping SyncOutboundClipboardUseCase::execute() — uses executor::block_on internally, must not run inside async context"
  - "outbound_snapshot = snapshot.clone() placed immediately before execute_with_origin() which takes ownership"
  - "FileCandidate construction: extracted_paths_count captured BEFORE metadata filtering, enabling all_files_excluded detection in planner"

requirements-completed: [PH61-01, PH61-02, PH61-03, PH61-04]

# Metrics
duration: 8min
completed: 2026-03-25
---

# Phase 61 Plan 01: Daemon Outbound Clipboard Sync Summary

**DaemonClipboardChangeHandler now dispatches outbound clipboard sync via OutboundSyncPlanner and SyncOutboundClipboardUseCase (spawn_blocking) after successful LocalCapture, with file URI extraction and per-intent SyncOutboundFileUseCase dispatch**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-25T14:57:00Z
- **Completed:** 2026-03-25T15:05:08Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- Added `extract_file_paths_from_snapshot` free function parsing `text/uri-list`, `file/uri-list`, `files`, and `public.file-url` representations into `Vec<PathBuf>` with deduplication
- Added `build_sync_outbound_clipboard_use_case` method wiring all 8 required ports from `wiring_deps()`
- Extended `on_clipboard_changed` to dispatch full outbound sync pipeline after successful capture: planner → clipboard dispatch → file dispatch
- RemotePush origin guard: `extracted_paths_count` and `file_candidates` are always empty for RemotePush, and `OutboundSyncPlanner::plan()` returns `OutboundSyncPlan { clipboard: None, files: [] }` for RemotePush
- Added `url = "2"` dependency for `url::Url::parse` file:// URI parsing
- 3 unit tests: URI list extraction, non-file rep returns empty, dedup

## Task Commits

Each task was committed atomically:

1. **Task 1: Add outbound sync helpers and file path extraction** - `a3f71bba` (feat)
2. **Task 2: Extend on_clipboard_changed to dispatch outbound sync** - `831c1bc8` (feat)

## Files Created/Modified
- `src-tauri/crates/uc-daemon/Cargo.toml` - Added `url = "2"` dependency
- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` - Added helpers and outbound sync dispatch

## Decisions Made
- **SyncOutboundFileUseCase inline construction**: `CoreUseCases<'a>` has lifetime parameter that is complex to use inside `async fn`. Direct construction from `wiring_deps()` is simpler and avoids lifetime issues.
- **Stub extraction location**: `extract_file_paths_from_snapshot` duplicated from `uc-tauri/runtime.rs` into `uc-daemon` rather than extracting to a shared crate. Minimal scope to avoid phase creep.
- **APFS stub**: `resolve_apfs_file_reference` is `#[cfg(target_os = "macos")]` no-op matching the uc-tauri pattern; APFS resolution deferred to a future phase.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] CoreRuntime has no usecases() method**
- **Found during:** Task 2 (dispatch outbound sync)
- **Issue:** Plan said `self.runtime.usecases().sync_outbound_file()` but `Arc<CoreRuntime>` has no `usecases()` method — only `AppUseCases` in `uc-tauri` does
- **Fix:** Build `SyncOutboundFileUseCase` inline from `wiring_deps()` matching the same field accesses as `CoreUseCases::sync_outbound_file()` in `uc-app/src/usecases/mod.rs`
- **Files modified:** clipboard_watcher.rs
- **Verification:** `cargo check -p uc-daemon` exits 0
- **Committed in:** 831c1bc8 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 - Bug)
**Impact on plan:** Required correction. No scope creep. Identical behavior to plan intent.

## Issues Encountered
- Pre-existing flaky test `process_metadata::tests::pid_path_tracks_uc_profile` in uc-daemon fails intermittently due to race conditions unrelated to this plan. Not caused by our changes (passes when run in isolation).

## Next Phase Readiness
- Daemon outbound clipboard sync is complete — daemon now syncs to peers after local captures
- Phase 62 (daemon inbound clipboard sync) can proceed: daemon receives peer clipboard and writes to local system
- Phase 63 (daemon file transfer orchestration) builds on this foundation

---
*Phase: 61-daemon-outbound-clipboard-sync*
*Completed: 2026-03-25*

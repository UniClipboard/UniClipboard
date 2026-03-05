---
phase: 06-dashboard-image-display
plan: 01
subsystem: ui
tags: [tauri, custom-protocol, convertFileSrc, image-display, cross-platform]

# Dependency graph
requires:
  - phase: 05-windows
    provides: Working image capture on all platforms (images stored with uc:// URLs)
provides:
  - resolveUcUrl helper for platform-correct uc:// URL resolution
  - Cross-platform image display in dashboard (thumbnails + expanded)
  - Manual URL construction for uc:// protocol (avoids convertFileSrc slash encoding)
affects: [dashboard, clipboard-display, protocol-handler]

# Tech tracking
tech-stack:
  added: []
  patterns: [manual-platform-url-construction, uc-protocol-resolver]

key-files:
  created:
    - src/lib/protocol.ts
    - src/lib/__tests__/protocol.test.ts
  modified:
    - src/components/clipboard/ClipboardItem.tsx
    - src/api/clipboardItems.ts
    - src/components/clipboard/__tests__/ClipboardItem.test.tsx
    - src-tauri/crates/uc-tauri/src/protocol.rs
    - vite.config.ts

key-decisions:
  - 'Manual URL construction instead of convertFileSrc to avoid slash encoding (%2F) on Windows'
  - 'Backend parse_uc_request handles both direct scheme and localhost proxy URL formats'

patterns-established:
  - 'resolveUcUrl pattern: all uc:// URLs must be resolved before use in img src or fetch'
  - 'Dual URL format support in backend protocol handler for cross-platform compatibility'

requirements-completed: [IMG-DISPLAY-01, IMG-DISPLAY-02]

# Metrics
duration: 21min
completed: 2026-03-05
---

# Phase 6 Plan 01: Dashboard Image Display Fix Summary

**Platform-aware uc:// URL resolver with manual URL construction replacing convertFileSrc to fix image display on all platforms**

## Performance

- **Duration:** 21 min
- **Started:** 2026-03-05T06:22:06Z
- **Completed:** 2026-03-05T06:43:31Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments

- Created resolveUcUrl helper that converts uc:// URLs to platform-correct format (macOS/Linux: uc://localhost/{path}, Windows: http://uc.localhost/{path})
- Wired URL resolution into ClipboardItem image rendering and fetchClipboardResourceText
- Fixed convertFileSrc slash encoding bug by switching to manual URL construction
- Updated backend parse_uc_request to handle both direct scheme and localhost proxy URL formats
- All 70 frontend tests pass, TypeScript build clean

## Task Commits

Each task was committed atomically:

1. **Task 1: Create resolveUcUrl helper and wire into ClipboardItem + API layer** - `b75e712` (feat)
2. **Task 2: Verify image display in dashboard** - checkpoint:human-verify (approved)

**Bug fix during verification:** `54fe292` (fix) - Replace convertFileSrc with manual URL construction

## Files Created/Modified

- `src/lib/protocol.ts` - Platform-aware URL resolver for uc:// custom protocol URLs
- `src/lib/__tests__/protocol.test.ts` - 4 test cases for resolveUcUrl (uc:// resolve, passthrough, empty)
- `src/components/clipboard/ClipboardItem.tsx` - Image rendering uses resolveUcUrl for thumbnail + detail URLs
- `src/api/clipboardItems.ts` - fetchClipboardResourceText resolves URL before fetch
- `src/components/clipboard/__tests__/ClipboardItem.test.tsx` - Updated with @tauri-apps/api/core mock
- `src-tauri/crates/uc-tauri/src/protocol.rs` - Backend handles both URL formats for uc:// protocol
- `vite.config.ts` - Added worktrees/ to vitest exclude pattern

## Decisions Made

- **Manual URL construction over convertFileSrc:** Tauri's convertFileSrc encodes slashes (/ to %2F) in the path, breaking URL routing on Windows. Replaced with manual platform detection and URL construction.
- **Dual URL format in backend:** parse_uc_request now handles both direct scheme format (host=resource_type, e.g., uc://thumbnail/rep-1) and localhost proxy format (host=localhost, resource type in path, e.g., http://uc.localhost/thumbnail/rep-1) for full cross-platform support.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed vitest worktrees/ directory exclusion**

- **Found during:** Task 1 (test verification)
- **Issue:** Git worktree directory `worktrees/` was being picked up by vitest, causing duplicate test file discovery and test failures from a different branch's code
- **Fix:** Added `**/worktrees/**` to vitest exclude pattern in vite.config.ts
- **Files modified:** vite.config.ts
- **Verification:** All 29 test files (70 tests) pass
- **Committed in:** b75e712 (Task 1 commit)

**2. [Rule 1 - Bug] Replaced convertFileSrc with manual URL construction**

- **Found during:** Task 2 (human verification on Windows)
- **Issue:** convertFileSrc was encoding slashes (/ to %2F) in the path parameter, producing URLs like `http://uc.localhost/thumbnail%2Frep-1` instead of `http://uc.localhost/thumbnail/rep-1`, breaking URL routing
- **Fix:** Frontend: replaced convertFileSrc with manual platform detection (navigator.userAgent Windows check) and direct URL string construction. Backend: updated parse_uc_request to handle both direct scheme format and localhost proxy format.
- **Files modified:** src/lib/protocol.ts, src/lib/**tests**/protocol.test.ts, src/components/clipboard/**tests**/ClipboardItem.test.tsx, src-tauri/crates/uc-tauri/src/protocol.rs
- **Verification:** Images display correctly in dashboard on Windows
- **Committed in:** 54fe292

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Both fixes necessary for correct cross-platform operation. The convertFileSrc bug was a Tauri API behavior that could not be predicted from documentation alone.

## Issues Encountered

- vitest was discovering test files from the git worktree directory, causing test failures that masked the real test results. Resolved by adding worktrees/ to the exclude pattern.
- Tauri's convertFileSrc API encodes path separators, making it unsuitable for multi-segment paths like "thumbnail/rep-1". Required switching to manual URL construction.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Dashboard image display working on all platforms (macOS, Linux, Windows)
- End-to-end flow complete: clipboard capture -> storage -> display
- Phase 6 complete - no further plans needed

---

_Phase: 06-dashboard-image-display_
_Completed: 2026-03-05_

---
phase: 05-windows
plan: 01
subsystem: clipboard
tags: [image, bmp, png, clipboard-rs, clipboard-win, windows, dib]

# Dependency graph
requires: []
provides:
  - 'clipboard-rs 0.3.3 upgrade with Windows image handling fixes'
  - 'Cross-platform dib_to_png converter (BmpDecoder::new_without_file_header -> PNG)'
  - 'read_image_windows_as_png using CF_DIB raw data'
affects: [05-02]

# Tech tracking
tech-stack:
  added: [clipboard-rs 0.3.3]
  patterns: [CF_DIB-to-PNG conversion via image crate BmpDecoder without file header]

key-files:
  created:
    - src-tauri/crates/uc-platform/src/clipboard/image_convert.rs
  modified:
    - src-tauri/crates/uc-platform/Cargo.toml
    - src-tauri/crates/uc-platform/src/clipboard/mod.rs
    - src-tauri/crates/uc-platform/src/clipboard/platform/windows.rs
    - src-tauri/Cargo.lock

key-decisions:
  - 'Used BmpDecoder::new_without_file_header for CF_DIB data (no 14-byte BMP header present in Windows clipboard data)'
  - 'Module image_convert is not cfg-gated so tests run on all platforms (macOS/Linux CI)'
  - 'read_image_windows_as_png reads CF_DIB via RawData(CF_DIB) instead of formats::Bitmap for correct headerless data'

patterns-established:
  - 'Cross-platform image conversion in separate module from platform-specific clipboard access'

requirements-completed: [WIN-IMG-01, WIN-IMG-02, WIN-IMG-03]

# Metrics
duration: 3min
completed: 2026-03-05
---

# Phase 05 Plan 01: Windows Image Capture Building Blocks Summary

**Upgraded clipboard-rs to 0.3.3 and created cross-platform CF_DIB-to-PNG converter with 4 unit tests replacing buggy RGBA-returning read_image_windows**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-05T05:39:34Z
- **Completed:** 2026-03-05T05:43:02Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- Upgraded clipboard-rs from 0.3.1 to 0.3.3 with clean compilation
- Created `dib_to_png` cross-platform converter using `BmpDecoder::new_without_file_header`
- Replaced buggy `read_image_windows()` (returned raw RGBA bytes) with correct `read_image_windows_as_png()`
- Added 4 unit tests that run on all platforms: PNG magic bytes, roundtrip decode, empty input error, truncated header error

## Task Commits

Each task was committed atomically:

1. **Task 1: Upgrade clipboard-rs to 0.3.3** - `125ce73` (chore)
2. **Task 2 RED: Failing tests for dib_to_png** - `fdec761` (test)
3. **Task 2 GREEN: Implement dib_to_png and read_image_windows_as_png** - `fb366b0` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-platform/Cargo.toml` - Bumped clipboard-rs to 0.3.3
- `src-tauri/Cargo.lock` - Updated lockfile for clipboard-rs 0.3.3
- `src-tauri/crates/uc-platform/src/clipboard/image_convert.rs` - New cross-platform dib_to_png converter with 4 tests
- `src-tauri/crates/uc-platform/src/clipboard/mod.rs` - Added non-cfg-gated image_convert module declaration
- `src-tauri/crates/uc-platform/src/clipboard/platform/windows.rs` - Replaced read_image_windows with read_image_windows_as_png

## Decisions Made

- Used `BmpDecoder::new_without_file_header` because CF_DIB clipboard data lacks the 14-byte BMP file header
- Kept `image_convert` module non-cfg-gated so unit tests run on macOS/Linux CI hosts
- Used `formats::RawData(formats::CF_DIB)` instead of `formats::Bitmap` to get raw DIB data without clipboard API prepending a file header

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `dib_to_png` converter ready for Plan 02 to wire into production clipboard capture path
- `read_image_windows_as_png` ready to be called from the snapshot reading flow
- All existing tests remain green

---

_Phase: 05-windows_
_Completed: 2026-03-05_

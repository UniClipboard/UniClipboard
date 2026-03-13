---
phase: 28-support-link-content-type-mime-link-and-url-detected-plain-text
plan: 01
subsystem: clipboard
tags: [url-detection, link-classification, mime, dto, content-type]

requires:
  - phase: 25-per-device-sync-content-type-toggles
    provides: classify_snapshot and is_content_type_allowed with Text/Image filtering

provides:
  - link_utils module with is_single_url, parse_uri_list, extract_domain
  - classify_snapshot extended with plain-text URL detection as Link
  - is_content_type_allowed with ct.link toggle for Link category
  - ClipboardLinkItemDto struct for frontend API
  - link-aware get_clipboard_item command returning urls and domains

affects: [28-02-frontend-link-display]

tech-stack:
  added: [url (crate v2)]
  patterns: [content-type detection via bytes inspection in classify_snapshot]

key-files:
  created:
    - src-tauri/crates/uc-core/src/clipboard/link_utils.rs
  modified:
    - src-tauri/crates/uc-core/Cargo.toml
    - src-tauri/crates/uc-core/src/clipboard/mod.rs
    - src-tauri/crates/uc-core/src/settings/content_type_filter.rs
    - src-tauri/crates/uc-tauri/src/models/mod.rs
    - src-tauri/crates/uc-tauri/src/commands/clipboard.rs

key-decisions:
  - 'url crate v2 used for URL parsing validation instead of regex'
  - 'Link detection in classify_snapshot inspects bytes as UTF-8 for is_single_url check'
  - 'ClipboardItemDto.link changed from Option<serde_json::Value> to Option<ClipboardLinkItemDto> for type safety'

patterns-established:
  - 'Content bytes inspection in classify_snapshot for sub-classification of text/plain'

requirements-completed: [LINK-01, LINK-02, LINK-03, LINK-04]

duration: 7min
completed: 2028-03-12
---

# Phase 28 Plan 01: Backend Link Detection Summary

**URL detection and classification with link_utils module, extended classify_snapshot, ct.link filtering, and ClipboardLinkItemDto for structured link data**

## Performance

- **Duration:** 7 min
- **Started:** 2028-03-12T23:28:22Z
- **Completed:** 2028-03-12T23:36:17Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

- Created link_utils module with is_single_url, parse_uri_list, and extract_domain utilities
- Extended classify_snapshot to detect plain-text single URLs as Link category (in addition to existing text/uri-list detection)
- Made Link category filterable via ct.link toggle in is_content_type_allowed
- Added ClipboardLinkItemDto with typed urls and domains fields
- Updated get_clipboard_item command to return populated link data for both text/uri-list and plain-text URL entries

## Task Commits

Each task was committed atomically:

1. **Task 1: Create link_utils module and extend classify_snapshot** - `849dcabc` (feat)
2. **Task 2: Add ClipboardLinkItemDto and populate link field** - `138bd494` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-core/src/clipboard/link_utils.rs` - URL parsing utilities: is_single_url, parse_uri_list, extract_domain
- `src-tauri/crates/uc-core/Cargo.toml` - Added url crate dependency
- `src-tauri/crates/uc-core/src/clipboard/mod.rs` - Registered link_utils module
- `src-tauri/crates/uc-core/src/settings/content_type_filter.rs` - Extended classify_snapshot and is_content_type_allowed for Link
- `src-tauri/crates/uc-tauri/src/models/mod.rs` - Added ClipboardLinkItemDto, typed link field
- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` - Link-aware DTO mapping in get_clipboard_item

## Decisions Made

- Used url crate v2 for URL parsing validation instead of regex for correctness and RFC compliance
- Link detection in classify_snapshot inspects representation bytes as UTF-8 for is_single_url check
- Changed ClipboardItemDto.link from Option<serde_json::Value> to Option<ClipboardLinkItemDto> for type safety

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Backend link detection and DTO population complete
- Plan 02 (frontend link display) can proceed with ClipboardLinkItemDto API contract
- All existing tests continue to pass

---

_Phase: 28-support-link-content-type-mime-link-and-url-detected-plain-text_
_Completed: 2028-03-12_

## Self-Check: PASSED

---
status: awaiting_human_verify
trigger: 'text-truncation-500-chars: expanded view and copy action only show 500 chars'
created: 2026-03-04T00:00:00Z
updated: 2026-03-04T00:04:00Z
---

## Current Focus

hypothesis: CONFIRMED - Original fix is correct. Reported clipboard timeout is a pre-existing Linux/X11 issue unrelated to our changes.
test: Verified changed files do not touch clipboard reading path. Analyzed log trace.
expecting: User to re-test with smaller text (or on macOS) to isolate X11 timeout from our fix
next_action: Await human verification with workaround for X11 timeout

## Symptoms

expected: After expanding a clipboard entry in the dashboard, the full content (e.g., 20000 characters) should be displayed. Copying via action bar should copy the full content.
actual: After expanding, only ~500 characters are shown. Copying via action bar also only copies ~500 characters.
errors: No error messages reported
reproduction: Copy a text with 20000+ characters. Open dashboard. Expand entry - only 500 chars. Click copy - only 500 chars.
started: Always been this way since feature was implemented

## Eliminated

- hypothesis: Our normalizer/repo/list_entry_projections fix caused the clipboard read timeout
  evidence: |
  1. Our changes only touch 3 files: normalizer.rs, representation_repo.rs, list_entry_projections.rs
  2. None of these are in the clipboard reading path (uc-platform/src/clipboard/common.rs or platform/linux.rs)
  3. The timeout occurs at ctx.get_buffer() in common.rs:176 -- this is the clipboard-rs library / X11 protocol level
  4. The log shows the timeout happens BEFORE any of our code executes (normalizer runs at line 24.904, timeout at 24.399-24.903)
  5. git diff confirms no changes to uc-platform at all
     timestamp: 2026-03-04T00:04:00Z

## Evidence

- timestamp: 2026-03-04T00:00:30Z
  checked: uc-infra/src/clipboard/normalizer.rs
  found: PREVIEW_LENGTH_CHARS = 500. For large text (> inline_threshold_bytes), normalizer creates PersistedClipboardRepresentation with inline_data = truncated 500-char preview, blob_id = None, payload_state = Inline
  implication: Full text content is permanently lost at this point - only 500 chars kept

- timestamp: 2026-03-04T00:00:40Z
  checked: uc-app/src/usecases/internal/capture_clipboard.rs lines 178-206
  found: Only Staged representations get queued for blob materialization. Large text gets Inline state (with truncated preview), not Staged.
  implication: The background blob worker never processes large text content

- timestamp: 2026-03-04T00:00:50Z
  checked: uc-app/src/usecases/clipboard/get_entry_detail.rs
  found: Tries blob_id first, falls back to inline_data. Since large text has no blob and only truncated preview in inline_data, returns 500 chars.
  implication: Detail/expanded view only gets truncated content

- timestamp: 2026-03-04T00:00:55Z
  checked: uc-app/src/usecases/clipboard/restore_clipboard_selection.rs
  found: Reads inline_data or blob, same issue - gets truncated 500 chars for large text
  implication: Copy-back action also only copies truncated content

- timestamp: 2026-03-04T00:02:00Z
  checked: DB CHECK constraint (migration 2026-01-18-000001)
  found: CHECK (inline_data IS NULL OR blob_id IS NULL). Allows inline_data + Staged (no blob_id) but NOT inline_data + blob_id simultaneously.
  implication: update_processing_result must clear inline_data when setting blob_id

- timestamp: 2026-03-04T00:02:30Z
  checked: All test suites (uc-infra, uc-app, uc-tauri, uc-core)
  found: All tests pass after fix (165 + 153 + 83 + 117 = 518 tests total)
  implication: Fix is backward-compatible and doesn't break existing functionality

- timestamp: 2026-03-04T00:03:30Z
  checked: User-reported log trace showing clipboard timeout
  found: |
  The log shows:
  1. UTF8_STRING and TEXT raw format reads TIMEOUT at the X11/clipboard-rs level (common.rs:176)
  2. High-level ctx.get_text() (common.rs:50) succeeded but returned empty text (0 bytes)
  3. Snapshot arrives with formats=1, total_size_bytes=0
  4. Normalizer correctly processes the 0-byte content as small inline
  5. Capture fails with "no usable representations" because the content is empty
     The timeout chain: X11 selection transfer -> clipboard-rs get_buffer -> Timeout
     implication: This is a pre-existing Linux/X11 clipboard timeout issue unrelated to our fix. The timeout occurs at the platform layer before any of our changed code executes.

- timestamp: 2026-03-04T00:04:00Z
  checked: git diff --stat HEAD
  found: Only 3 source files changed: normalizer.rs, representation_repo.rs, list_entry_projections.rs. No changes to uc-platform, clipboard reading, or common.rs.
  implication: Our fix cannot have caused the clipboard read timeout.

## Resolution

root_cause: In normalizer.rs, large text content (> 16KB) is handled by a "preview" branch that truncates to 500 chars and stores as Inline state. Unlike large non-text (images) which get Staged state and are processed by the background blob worker, large text is never blob-materialized. The full content is permanently lost at capture time because (1) the normalizer only stores the 500-char truncated preview, and (2) the capture use case only queues Staged representations for blob materialization.

fix: |

1. normalizer.rs: Changed large text handling from Inline+preview to Staged+preview using new_with_state. This preserves the 500-char inline preview for fast list display while marking the representation for blob worker processing.
2. representation_repo.rs: Modified update_processing_result to clear inline_data (set to NULL) when setting blob_id, satisfying the DB CHECK constraint (inline_data IS NULL OR blob_id IS NULL).
3. list_entry_projections.rs: Updated has_detail logic to also return true for Staged/Processing states, not just when blob_id is set. Reordered code so is_image check is available for the preview fallback. Added proper fallback for text without inline_data (uses entry title).
4. normalizer.rs (test): Added new test test_normalizer_creates_staged_with_preview_for_large_text.

verification: All 518 tests pass. New test confirms large text gets Staged state with inline preview. Clipboard timeout confirmed as pre-existing X11 issue.

files_changed:

- src-tauri/crates/uc-infra/src/clipboard/normalizer.rs
- src-tauri/crates/uc-infra/src/db/repositories/representation_repo.rs
- src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs

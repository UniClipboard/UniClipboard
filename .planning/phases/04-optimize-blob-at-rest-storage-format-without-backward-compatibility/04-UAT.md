---
status: complete
phase: 04-optimize-blob-at-rest-storage-format-without-backward-compatibility
source: 04-01-SUMMARY.md, 04-02-SUMMARY.md
started: 2026-03-04T03:30:00Z
updated: 2026-03-04T03:35:00Z
---

## Current Test

[testing complete]

## Tests

### 1. App launches without errors after V2 migration

expected: Run `bun tauri dev`. App starts normally without crashes or errors related to blob migration. The spool directory cleanup runs silently on first launch.
result: pass

### 2. Copy image to clipboard and verify capture

expected: Copy an image (e.g., screenshot or image from browser) to system clipboard. The app captures it and shows a new clipboard entry in the history list with an image thumbnail.
result: pass

### 3. Paste back a captured image entry

expected: Click on a previously captured image entry in clipboard history to restore it. Paste into an app (e.g., a text editor or image viewer). The pasted image matches the original.
result: pass

### 4. Copy text to clipboard still works

expected: Copy some text to system clipboard. App captures it and shows a new text entry in clipboard history. Clicking the entry restores it to clipboard and pasting produces the original text.
result: pass

### 5. Blob files use binary format on disk

expected: After capturing an image, check the blob spool directory. New blob files should start with "UCBL" magic bytes (not JSON `{`). You can verify with: `xxd <blob_file> | head -1` — first bytes should show `5543 424c` (UCBL).
result: pass

## Summary

total: 5
passed: 5
issues: 0
pending: 0
skipped: 0

## Gaps

[none yet]

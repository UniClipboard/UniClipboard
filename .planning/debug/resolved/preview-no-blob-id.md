---
status: resolved
trigger: "preview-no-blob-id: get_entry_resource fails with 'Preview representation has no blob_id' for synced entries"
created: 2026-03-08T00:00:00Z
updated: 2026-03-08T00:00:00Z
---

## Current Focus

hypothesis: CONFIRMED - GetEntryResourceUseCase unconditionally requires blob_id but inline representations (small content < inline_threshold) have blob_id=None by design
test: Traced full data flow from normalizer -> storage -> get_entry_resource
expecting: Fix get_entry_resource to handle inline representations
next_action: Implement fix in GetEntryResourceUseCase to handle inline content

## Symptoms

expected: Clipboard entries from history should be restorable to system clipboard without errors
actual: get_entry_resource fails with ERROR "Preview representation has no blob_id" for certain entries
errors:

- Failed to get entry resource error=Preview representation has no blob_id entry_id=52283345-4e46-44f9-8750-22038fa9ab4b
- Failed to get entry resource error=Preview representation has no blob_id entry_id=f08f2427-f9cd-4589-b471-47baa700e388
- The same entries' inline_data IS successfully decrypted (4 bytes, 8 bytes) but the operation fails because blob_id is None
  reproduction: peerA + peerB in full mode sync, from clipboard history dashboard, click restore on a synced entry
  started: Current behavior, discovered during testing

## Eliminated

## Evidence

- timestamp: 2026-03-08T00:01:00Z
  checked: GetEntryResourceUseCase::execute() at get_entry_resource.rs:65-68
  found: Unconditionally does `preview_rep.blob_id.clone().ok_or(...)` - fails when blob_id is None
  implication: Any representation with PayloadAvailability::Inline will fail here since inline reps have blob_id=None by design

- timestamp: 2026-03-08T00:02:00Z
  checked: ClipboardRepresentationNormalizer at normalizer.rs:89-106
  found: Small content (< inline_threshold_bytes) creates Inline state with inline_data=Some, blob_id=None
  implication: This is correct behavior - the normalizer stores small content inline. The bug is in get_entry_resource not handling this case.

- timestamp: 2026-03-08T00:03:00Z
  checked: PersistedClipboardRepresentation::new() at snapshot.rs:40-50
  found: When inline_data=Some and blob_id=None, payload_state=Inline. This is the expected state for small clipboard content.
  implication: Confirms root cause is in GetEntryResourceUseCase, not in how data is stored.

- timestamp: 2026-03-08T00:04:00Z
  checked: Frontend usage at ClipboardItem.tsx:68,129 and ClipboardPreview.tsx:47,61
  found: Frontend calls getClipboardEntryResource to get blob URL, then fetches content via `uc://blob/{blob_id}`. For inline content there is no blob, so this entire approach fails.
  implication: The use case needs to return inline_data directly when blob_id is absent, or the EntryResourceResult needs to support inline content.

## Resolution

root_cause: GetEntryResourceUseCase::execute() unconditionally requires blob_id (line 65-68) but inline representations (small content < inline_threshold) have blob_id=None by design. The use case was written assuming all content goes through blob storage, but the normalizer stores small content inline. The error "Preview representation has no blob_id" is the direct manifestation.
fix: Modify GetEntryResourceUseCase to handle both inline and blob content. When blob_id exists, return blob URL. When inline_data exists (and blob_id is None), return inline_data directly in the result. Also update EntryResourceResult to support both modes, the command DTO, and frontend handling.
verification: Rust tests pass (2 get_entry_resource tests including new inline test), all 4 Rust test suites pass, cargo check clean. Frontend test for text expansion passes. Pre-existing image test failure confirmed unrelated.
files_changed:

- src-tauri/crates/uc-app/src/usecases/clipboard/get_entry_resource.rs
- src-tauri/crates/uc-tauri/src/commands/clipboard.rs
- src-tauri/crates/uc-tauri/src/models/mod.rs
- src-tauri/crates/uc-tauri/Cargo.toml
- src/api/clipboardItems.ts
- src/components/clipboard/ClipboardItem.tsx
- src/components/clipboard/ClipboardPreview.tsx

---
phase: 58-extract-dto-models-and-pairing-event-types-from-uc-tauri-to-uc-app-and-uc-core
plan: 01
subsystem: backend-rust
tags: [dto-unification, serde, uc-app, uc-tauri, refactor]
dependency_graph:
  requires: []
  provides: [EntryProjectionDto-serde, ClipboardStats-serde, unified-clipboard-dto]
  affects: [uc-tauri/commands, uc-tauri/models, uc-tauri/tests]
tech_stack:
  added: []
  patterns: [single-source-of-truth-dto, direct-import-from-uc-app]
key_files:
  created: []
  modified:
    - src-tauri/crates/uc-app/src/usecases/clipboard/list_entry_projections/list_entry_projections.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/mod.rs
    - src-tauri/crates/uc-tauri/src/models/mod.rs
    - src-tauri/crates/uc-tauri/src/commands/clipboard.rs
    - src-tauri/crates/uc-tauri/tests/models_serialization_test.rs
    - src-tauri/crates/uc-tauri/tests/clipboard_commands_stats_favorites_test.rs
decisions:
  - "[Phase 58]: EntryProjectionDto gains serde in uc-app; uc-tauri ClipboardEntryProjection deleted with no re-export stub (D-05)"
  - "[Phase 58]: link_domains populated inline at command layer via mut reference loop, not in a separate mapping step"
  - "[Phase 58]: file_transfer_ids marked #[serde(skip)] as internal-only field not part of frontend wire contract"
metrics:
  duration: 31min
  completed: 2026-03-25
  tasks: 2
  files: 6
---

# Phase 58 Plan 01: Extract and Unify Clipboard DTOs from uc-tauri to uc-app Summary

Unified clipboard DTO types by adding serde derives to uc-app's EntryProjectionDto and ClipboardStats, then deleting the duplicate definitions from uc-tauri/models and updating all consumers to import directly from uc-app.

## What Was Built

### Task 1: Add serde derives to uc-app types

**`list_entry_projections.rs`** — Added `use serde::{Deserialize, Serialize};` and changed `EntryProjectionDto` derive from `#[derive(Debug, Clone, PartialEq)]` to `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`. Added serde annotations:
- `#[serde(skip_serializing_if = "Option::is_none")]` on: `thumbnail_url`, `file_transfer_status`, `file_transfer_reason`, `link_urls`, `file_sizes`
- `#[serde(skip)]` on `file_transfer_ids` (internal field, not in frontend JSON contract)
- New field `link_domains: Option<Vec<String>>` with `#[serde(skip_serializing_if = "Option::is_none")]`
- Updated all struct literals in `execute()`, `execute_single()`, and tests to include `link_domains: None`

**`clipboard/mod.rs`** — Added `use serde::{Deserialize, Serialize};` and changed `ClipboardStats` derive to `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]`.

### Task 2: Delete duplicates from uc-tauri, update consumers

**`uc-tauri/src/models/mod.rs`** — Deleted `ClipboardEntryProjection` struct, deleted `ClipboardStats` struct, added `use uc_app::usecases::clipboard::EntryProjectionDto`, updated `ClipboardEntriesResponse::Ready` to use `Vec<EntryProjectionDto>`, deleted two internal unit tests that covered the now-deleted types.

**`uc-tauri/src/commands/clipboard.rs`** — Updated imports to remove `ClipboardEntryProjection` and `ClipboardStats` from `crate::models`, added `use uc_app::usecases::clipboard::{ClipboardStats, EntryProjectionDto}`. Replaced the `dtos.into_iter().map(|dto| ClipboardEntryProjection {...})` mapping in `get_clipboard_entries()` and `get_clipboard_entry()` with inline `dto.link_domains = ...` mutation. Simplified `get_clipboard_stats()` to return `ClipboardUseCases::compute_stats(&dtos)` directly.

**`tests/models_serialization_test.rs`** — Updated import to remove `ClipboardEntryProjection` from `uc_tauri::models`, added `use uc_app::usecases::clipboard::EntryProjectionDto`. Updated all test struct constructions to use `EntryProjectionDto` with `file_transfer_ids: vec![]` and `link_domains: None`. Added assertion that `file_transfer_ids` is absent from serialized JSON.

**`tests/clipboard_commands_stats_favorites_test.rs`** — Replaced `ClipboardStats` import from `uc_tauri::models` with `use uc_app::usecases::clipboard::ClipboardStats`.

## Verification Results

- `cargo test -p uc-app` — 277 passed, 2 ignored
- `cargo test -p uc-tauri` — 189 passed, 3 ignored
- `cargo check` (full workspace) — 0 errors
- `grep -rn "\bClipboardEntryProjection\b" src-tauri/crates/uc-tauri/src/` — NOT FOUND (correct)
- `grep -r "pub struct ClipboardStats" src-tauri/crates/uc-tauri/src/` — NOT FOUND (correct)

## Deviations from Plan

None - plan executed exactly as written.

## Known Stubs

None.

## Commits

| Task | Name | Commit | Files |
| ---- | ---- | ------ | ----- |
| 1 | Add serde derives and link_domains to EntryProjectionDto and ClipboardStats in uc-app | 4266a6d7 | list_entry_projections.rs, mod.rs |
| 2 | Delete duplicate DTOs from uc-tauri models, update commands and tests | 25afd028 | models/mod.rs, commands/clipboard.rs, tests/* |

## Self-Check: PASSED

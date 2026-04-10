---
phase: 89-use-cases-and-delete-integration
plan: 01
subsystem: search
tags: [rust, search, use-cases, hexagonal-architecture, tracing, tokio, async-trait]

# Dependency graph
requires:
  - phase: 88-search-domain-model
    provides: SearchIndexPort trait, SearchDocument, SearchPosting, SearchQuery, SearchResult, SearchError, RebuildProgress domain types

provides:
  - IndexClipboardEntry use case (SIDX-01) — thin orchestrator over SearchIndexPort::index_entry
  - RemoveIndexedEntry use case (D-05) — thin orchestrator over SearchIndexPort::remove_entry
  - SearchClipboardEntries use case (D-06) — thin orchestrator over SearchIndexPort::search
  - RebuildSearchIndex use case (D-04) — thin orchestrator over SearchIndexPort::rebuild with caller-supplied mpsc::Sender
  - Public re-exports at uc_app::usecases::{IndexClipboardEntry, RebuildSearchIndex, RemoveIndexedEntry, SearchClipboardEntries}

affects: [phase-90-tokenizer, phase-91-infra, phase-92-daemon-integration, phase-89-02-delete-integration]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - 'Search use case pattern: struct { Arc<dyn SearchIndexPort> } + from_port constructor + tracing::instrument execute'
    - 'Mock port pattern for unit tests: MockSearchIndex with Arc<Mutex<Option<_>>> captured args and fail_next error injection'
    - 'Sender-forwarding test pattern: mock sends RebuildProgress via forwarded Sender, test receives to prove non-drop'

key-files:
  created:
    - src-tauri/crates/uc-app/src/usecases/search/mod.rs
    - src-tauri/crates/uc-app/src/usecases/search/index_clipboard_entry.rs
    - src-tauri/crates/uc-app/src/usecases/search/remove_indexed_entry.rs
    - src-tauri/crates/uc-app/src/usecases/search/search_clipboard_entries.rs
    - src-tauri/crates/uc-app/src/usecases/search/rebuild_search_index.rs
  modified:
    - src-tauri/crates/uc-app/src/usecases/mod.rs

key-decisions:
  - 'Rule 3 deviation: pub mod search added in Task 1 (not Task 2) because module cannot compile without declaration'
  - 'No anyhow wrapping: all four use cases preserve SearchError at execute() boundary per D-03/D-04/D-05'
  - 'No tokenizer port injection: callers supply pre-built SearchDocument/Vec<SearchPosting> (D-02, D-03)'
  - 'Caller-supplied mpsc::Sender: RebuildSearchIndex forwards Sender directly, no no-op convenience channel'

patterns-established:
  - 'Search use case shape: Arc<dyn SearchIndexPort> field, from_port constructor, single tracing::instrument execute, no anyhow wrapping'
  - 'Per-use-case MockSearchIndex in #[cfg(test)] mod tests with Arc<Mutex<Option<_>>> captured args'

requirements-completed: [SIDX-01]

# Metrics
duration: 4min
completed: 2026-04-10
---

# Phase 89 Plan 01: Search Use Cases Summary

**Four thin search use cases over SearchIndexPort (index/remove/query/rebuild) with tracing instruments, 9 passing unit tests, wired into uc_app::usecases public API**

## Performance

- **Duration:** 4 min
- **Started:** 2026-04-10T14:34:57Z
- **Completed:** 2026-04-10T14:39:23Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

- Created five new files under `uc-app/src/usecases/search/`: mod.rs + one file per use case
- Each use case is a struct holding `Arc<dyn SearchIndexPort>` with a `from_port` constructor and a `#[tracing::instrument]` execute method
- 9 unit tests pass across all four use cases: happy path + error propagation for each; rebuild test verifies Sender forwarding via mock-emitted RebuildProgress
- Exposed `pub mod search` and `pub use search::{...}` re-exports in `usecases/mod.rs` — downstream consumers use `uc_app::usecases::IndexClipboardEntry` etc.
- `cargo check --workspace` clean with no warnings

## Task Commits

1. **Task 1: Create search submodule with four use case files and mod.rs wiring** - `f2237f72` (feat)
2. **Task 2: Wire search submodule into usecases/mod.rs public API** - `6a0e7726` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-app/src/usecases/search/mod.rs` — search submodule declarations and pub use re-exports
- `src-tauri/crates/uc-app/src/usecases/search/index_clipboard_entry.rs` — IndexClipboardEntry use case with 2 tests
- `src-tauri/crates/uc-app/src/usecases/search/remove_indexed_entry.rs` — RemoveIndexedEntry use case with 2 tests
- `src-tauri/crates/uc-app/src/usecases/search/search_clipboard_entries.rs` — SearchClipboardEntries use case with 3 tests
- `src-tauri/crates/uc-app/src/usecases/search/rebuild_search_index.rs` — RebuildSearchIndex use case with 2 tests
- `src-tauri/crates/uc-app/src/usecases/mod.rs` — added pub mod search + pub use search::{...} re-exports

## Decisions Made

- No anyhow wrapping: all use cases return `Result<_, SearchError>` preserving typed error boundary (D-03, D-04, D-05)
- No tokenizer port injected: callers build `SearchDocument`/`Vec<SearchPosting>` themselves (D-02, D-03)
- No convenience no-op channel in RebuildSearchIndex: caller-supplied Sender is the contract (D-04)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added pub mod search to usecases/mod.rs in Task 1 instead of Task 2**

- **Found during:** Task 1 (search use case files)
- **Issue:** Plan said "Do NOT touch usecases/mod.rs in this task — Task 2 handles wiring" but the module cannot be compiled (and tests cannot run) without the `pub mod search` declaration. Task 1's verify command `cargo test -p uc-app -- usecases::search` would exit non-zero without it.
- **Fix:** Added `pub mod search;` to `usecases/mod.rs` in Task 1. Task 2 only added the `pub use search::{...}` re-export block.
- **Files modified:** src-tauri/crates/uc-app/src/usecases/mod.rs
- **Verification:** All 9 tests pass with the declaration present; workspace check clean.
- **Committed in:** f2237f72 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary structural fix. Task 2 still had meaningful work (adding the re-export block). No scope creep.

## Issues Encountered

None beyond the Rule 3 deviation above.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- All four search use cases are ready for Phase 92 daemon integration
- Phase 89 Plan 02 (delete integration) can wire RemoveIndexedEntry into DeleteClipboardEntry
- Phase 90 tokenizer pipeline can call IndexClipboardEntry::execute with pre-built SearchDocument/Vec<SearchPosting>
- Phase 91 infra adapter implements SearchIndexPort and can be injected via Arc<dyn SearchIndexPort>

## Self-Check

- [x] `src-tauri/crates/uc-app/src/usecases/search/mod.rs` exists
- [x] `src-tauri/crates/uc-app/src/usecases/search/index_clipboard_entry.rs` exists
- [x] `src-tauri/crates/uc-app/src/usecases/search/remove_indexed_entry.rs` exists
- [x] `src-tauri/crates/uc-app/src/usecases/search/search_clipboard_entries.rs` exists
- [x] `src-tauri/crates/uc-app/src/usecases/search/rebuild_search_index.rs` exists
- [x] Commit f2237f72 exists (Task 1)
- [x] Commit 6a0e7726 exists (Task 2)

## Self-Check: PASSED

---

_Phase: 89-use-cases-and-delete-integration_
_Completed: 2026-04-10_

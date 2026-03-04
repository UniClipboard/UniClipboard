---
phase: 04-optimize-blob-at-rest-storage-format-without-backward-compatibility
plan: 01
subsystem: database, security, blob
tags: [zstd, diesel, aad, blob-storage, xchacha20-poly1305, compression]

# Dependency graph
requires: []
provides:
  - BlobStorePort::put returning (PathBuf, Option<i64>) for compressed size tracking
  - AAD v2 function (for_blob_v2) with "uc:blob:v2|{blob_id}" format
  - Blob domain model with compressed_size: Option<i64> field
  - Diesel migration adding compressed_size column to blob table
  - zstd dependency available in uc-infra
  - PlaceholderBlobStorePort dead code removed
affects: [04-02, blob-storage, encryption]

# Tech tracking
tech-stack:
  added: [zstd 0.13]
  patterns: [BlobStorePort returning tuple with optional compressed size metadata]

key-files:
  created:
    - src-tauri/crates/uc-infra/migrations/2026-03-04-000001_blob_v2_binary_format/up.sql
    - src-tauri/crates/uc-infra/migrations/2026-03-04-000001_blob_v2_binary_format/down.sql
  modified:
    - src-tauri/crates/uc-core/src/security/aad.rs
    - src-tauri/crates/uc-core/src/blob/mod.rs
    - src-tauri/crates/uc-core/src/ports/blob_store.rs
    - src-tauri/crates/uc-infra/src/db/schema.rs
    - src-tauri/crates/uc-infra/src/db/models/blob.rs
    - src-tauri/crates/uc-infra/src/db/mappers/blob_mapper.rs
    - src-tauri/crates/uc-infra/Cargo.toml
    - src-tauri/crates/uc-infra/src/security/encrypted_blob_store.rs
    - src-tauri/crates/uc-infra/src/blob/blob_writer.rs
    - src-tauri/crates/uc-platform/src/adapters/blob_store.rs
    - src-tauri/crates/uc-platform/src/adapters/mod.rs

key-decisions:
  - 'Kept for_blob (v1) unchanged alongside new for_blob_v2 for backward compatibility with inline data and network clipboard'
  - 'BlobStorePort::put returns (PathBuf, Option<i64>) tuple where None means store does not track compression'
  - 'Removed PlaceholderBlobStorePort dead code to reduce implementor count from 3 to 2'

patterns-established:
  - 'AAD versioning: separate functions per version (for_blob, for_blob_v2) rather than parameterized version'
  - 'BlobStorePort::put returns metadata tuple for storage metrics'

requirements-completed: [BLOB-01, BLOB-02, BLOB-03]

# Metrics
duration: 13min
completed: 2026-03-04
---

# Phase 04 Plan 01: Domain Contracts Summary

**AAD v2 function, Blob compressed_size field, BlobStorePort tuple return type, Diesel migration for compressed_size column, and zstd dependency added**

## Performance

- **Duration:** 13 min
- **Started:** 2026-03-04T01:49:22Z
- **Completed:** 2026-03-04T02:02:54Z
- **Tasks:** 2
- **Files modified:** 21

## Accomplishments

- Added `for_blob_v2()` AAD function with "uc:blob:v2|{blob_id}" format and 4 unit tests (TDD)
- Updated `BlobStorePort::put` return type to `Result<(PathBuf, Option<i64>)>` across all 11 implementations (2 real + 9 mock/noop)
- Added `compressed_size: Option<i64>` to Blob domain model, Diesel schema, BlobRow, NewBlobRow, and BlobRowMapper
- Created Diesel migration that deletes old blob records and adds compressed_size column
- Removed PlaceholderBlobStorePort dead code
- Added zstd 0.13 dependency to uc-infra

## Task Commits

Each task was committed atomically:

1. **Task 1: Update domain contracts -- AAD v2, Blob model, BlobStorePort signature** - `8064675` (feat, TDD)
2. **Task 2: Diesel migration + schema + model updates for compressed_size column** - `e49fecb` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-core/src/security/aad.rs` - Added for_blob_v2() function with v2 AAD format
- `src-tauri/crates/uc-core/src/blob/mod.rs` - Added compressed_size field to Blob struct
- `src-tauri/crates/uc-core/src/ports/blob_store.rs` - Changed put() return to (PathBuf, Option<i64>)
- `src-tauri/crates/uc-platform/src/adapters/blob_store.rs` - Updated FilesystemBlobStore, removed PlaceholderBlobStorePort
- `src-tauri/crates/uc-platform/src/adapters/mod.rs` - Removed PlaceholderBlobStorePort re-export
- `src-tauri/crates/uc-infra/src/security/encrypted_blob_store.rs` - Updated put() return, mock
- `src-tauri/crates/uc-infra/src/blob/blob_writer.rs` - Destructure compressed_size, pass to Blob::new
- `src-tauri/crates/uc-infra/Cargo.toml` - Added zstd = "0.13"
- `src-tauri/crates/uc-infra/src/db/schema.rs` - Added compressed_size column to blob table
- `src-tauri/crates/uc-infra/src/db/models/blob.rs` - Added compressed_size to BlobRow and NewBlobRow
- `src-tauri/crates/uc-infra/src/db/mappers/blob_mapper.rs` - Map compressed_size in both directions
- `src-tauri/crates/uc-infra/migrations/2026-03-04-000001_blob_v2_binary_format/up.sql` - Migration: delete old blobs + add column
- `src-tauri/crates/uc-infra/migrations/2026-03-04-000001_blob_v2_binary_format/down.sql` - Migration rollback

## Decisions Made

- Kept `for_blob` (v1) unchanged alongside new `for_blob_v2` -- v1 is still used by inline data and network clipboard AAD
- `BlobStorePort::put` returns `(PathBuf, Option<i64>)` tuple where `None` means the store does not track compressed size
- Removed `PlaceholderBlobStorePort` dead code to simplify future trait changes (reduced implementors from 3 to 2)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated blob_mapper.rs to_domain in Task 1**

- **Found during:** Task 1 (BlobStorePort signature changes)
- **Issue:** blob_mapper.rs calls Blob::new which now requires compressed_size parameter; compilation blocked
- **Fix:** Added `None` placeholder for compressed_size in to_domain, later updated to `row.compressed_size` in Task 2
- **Files modified:** src-tauri/crates/uc-infra/src/db/mappers/blob_mapper.rs
- **Verification:** cargo check --workspace passes
- **Committed in:** 8064675 (Task 1 commit)

**2. [Rule 3 - Blocking] Updated 9 additional BlobStorePort mock/noop implementations**

- **Found during:** Task 1 (trait signature change)
- **Issue:** Plan listed 3 implementors but codebase had 11 total (2 real + 9 mock/noop in test code)
- **Fix:** Updated all mock and noop BlobStorePort implementations across uc-tauri, uc-app, and uc-infra test files
- **Files modified:** uc-tauri/src/bootstrap/runtime.rs, uc-tauri/src/commands/clipboard.rs, uc-tauri/src/commands/encryption.rs, uc-app/usecases/clipboard/resolve_blob_resource.rs, resolve_thumbnail_resource.rs, restore_clipboard_selection.rs, uc-app/tests/stress_test.rs, snapshot_cache_integration_test.rs, uc-infra/tests/blob_repo_test.rs, uc-infra/src/clipboard/background_blob_worker.rs
- **Verification:** cargo check --workspace passes
- **Committed in:** 8064675 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking)
**Impact on plan:** Both auto-fixes necessary for compilation. Scope change: plan underestimated the number of BlobStorePort implementors. No scope creep.

## Issues Encountered

- Pre-existing test failure in uc-platform: `business_command_timeouts_cover_stream_operation_budgets` -- unrelated to blob changes, libp2p network timeout budget assertion. Not fixed (out of scope).

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- All contracts in place for Plan 02 to implement the V2 binary blob format
- BlobStorePort::put signature ready for compressed_size tracking
- AAD v2 ready for use in new encryption flow
- zstd crate available for compression implementation
- Diesel schema ready with compressed_size column

---

_Phase: 04-optimize-blob-at-rest-storage-format-without-backward-compatibility_
_Completed: 2026-03-04_

---
phase: 04-optimize-blob-at-rest-storage-format-without-backward-compatibility
plan: 02
subsystem: security
tags: [zstd, binary-format, blob-storage, xchacha20, compression]

# Dependency graph
requires:
  - phase: 04-01
    provides: AAD v2, BlobStorePort (PathBuf, Option<i64>) signature, compressed_size migration
provides:
  - UCBL binary blob format (4B magic + 1B version + 24B nonce + ciphertext)
  - zstd compress-before-encrypt pipeline in EncryptedBlobStore
  - Runtime spool directory cleanup with V2 migration sentinel
affects: []

# Tech tracking
tech-stack:
  added: [zstd 0.13]
  patterns: [binary-header-format, compress-then-encrypt, sentinel-file-migration]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-infra/src/security/encrypted_blob_store.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs

key-decisions:
  - 'zstd level 3 for compression (default, good speed/ratio balance)'
  - '500MB max decompressed size to prevent zip bombs'
  - 'Sentinel file (.v2_migrated) for one-time spool cleanup instead of per-startup purge'

patterns-established:
  - 'UCBL binary format: magic(4B) + version(1B) + nonce(24B) + ciphertext(NB)'
  - 'Compress-then-encrypt pipeline for blob storage'
  - 'Sentinel-file migration pattern for one-time filesystem cleanup'

requirements-completed: [BLOB-01, BLOB-02, BLOB-04]

# Metrics
duration: 18min
completed: 2026-03-04
---

# Phase 04 Plan 02: V2 Binary Blob Format Summary

**UCBL binary format with zstd compress-before-encrypt pipeline replacing JSON serialization, plus runtime spool cleanup**

## Performance

- **Duration:** 18 min
- **Started:** 2026-03-04T02:58:13Z
- **Completed:** 2026-03-04T03:17:11Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- Replaced JSON blob serialization with 29-byte UCBL binary header format (magic + version + nonce + ciphertext)
- Added zstd compression at level 3 before encryption, decompression after decryption with 500MB zip-bomb limit
- EncryptedBlobStore::put now returns `(PathBuf, Some(on_disk_size))` for compressed size tracking
- Added sentinel-based one-time spool directory cleanup on startup for old JSON-format blob files
- 13 comprehensive tests covering binary format serialization, parsing, roundtrip, error cases, and AAD v2 usage

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement V2 binary format with zstd compression in EncryptedBlobStore** - `c0050ed` (feat)
2. **Task 2: Wire runtime spool cleanup and verify full workspace compilation** - `a66a8c4` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-infra/src/security/encrypted_blob_store.rs` - UCBL binary format serializer/parser, zstd compression, V2 blob read/write with 13 tests
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` - One-time spool directory cleanup with .v2_migrated sentinel file

## Decisions Made

- Used zstd compression level 3 (default) for good speed/ratio balance
- Set 500MB max decompressed size limit to prevent zip bombs on untrusted data
- Used sentinel file (.v2_migrated) approach for one-time spool cleanup rather than purging on every startup

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- Pre-existing test failure in `uc-platform` (`business_command_timeouts_cover_stream_operation_budgets`) unrelated to Phase 04 changes. Logged to deferred-items.md. Not fixed (out of scope).

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 04 is now complete: domain contracts (Plan 01) and binary format implementation (Plan 02) are both done
- All new blob writes use UCBL binary format with zstd compression
- Old JSON-format blob files are purged on first startup after upgrade
- `cargo check --workspace` compiles cleanly; all 13 encrypted_blob_store tests pass

## Self-Check: PASSED

- FOUND: `src-tauri/crates/uc-infra/src/security/encrypted_blob_store.rs`
- FOUND: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`
- FOUND: `04-02-SUMMARY.md`
- FOUND: `c0050ed` (Task 1 commit)
- FOUND: `a66a8c4` (Task 2 commit)

---

_Phase: 04-optimize-blob-at-rest-storage-format-without-backward-compatibility_
_Completed: 2026-03-04_

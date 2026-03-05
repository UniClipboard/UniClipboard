---
phase: 08-optimize-large-image-sync-pipeline-v3-binary-protocol-compression-zero-copy-fanout
plan: 01
subsystem: network
tags: [binary-protocol, zstd, compression, wire-format, codec, xchacha20]

# Dependency graph
requires:
  - phase: 04-optimize-blob-at-rest-storage-format
    provides: zstd compression pattern (level 3), chunked AEAD V2 encoder/decoder
provides:
  - ClipboardBinaryPayload and BinaryRepresentation V3 binary codec in uc-core
  - ChunkedEncoderV3/DecoderV3 with zstd compression in uc-infra
  - V3 wire format with 37-byte UC3 header
  - TransferPayloadEncryptorAdapter producing V3 format
  - TransferPayloadDecryptorAdapter supporting both V2 and V3
affects: [08-02-PLAN, sync_outbound, sync_inbound]

# Tech tracking
tech-stack:
  added: []
  patterns:
    [V3 binary wire protocol, compression-before-encryption pipeline, magic-based format detection]

key-files:
  created:
    - src-tauri/crates/uc-core/src/network/protocol/clipboard_payload_v3.rs
  modified:
    - src-tauri/crates/uc-core/src/network/protocol/mod.rs
    - src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs

key-decisions:
  - 'V3 binary codec uses pure std::io Read/Write with to_le_bytes/from_le_bytes, no serde dependency'
  - 'thiserror source field renamed to reason for String-typed error variants (String does not implement std::error::Error)'
  - 'TransferPayloadDecryptorAdapter detects V2/V3 by magic bytes for transition period'

patterns-established:
  - 'V3 binary payload: ts_ms(8B) + rep_count(2B) + per-rep length-prefixed fields'
  - 'V3 wire header: magic(4) + compression_algo(1) + uncompressed_len(4) + transfer_id(16) + total_chunks(4) + chunk_size_hint(4) + total_plaintext_len(4)'
  - 'Magic-based format detection in decryptor adapter for protocol versioning'

requirements-completed: [V3-CODEC, V3-WIRE, V3-COMPRESS, V3-LARGE]

# Metrics
duration: 6min
completed: 2026-03-05
---

# Phase 08 Plan 01: V3 Binary Payload Codec and Chunked Encoder Summary

**V3 binary payload codec eliminating JSON+base64 overhead with zstd compression for payloads > 8KB, producing 37-byte UC3 wire headers**

## Performance

- **Duration:** 6 min
- **Started:** 2026-03-05T14:36:36Z
- **Completed:** 2026-03-05T14:42:22Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments

- ClipboardBinaryPayload and BinaryRepresentation types with pure binary encode/decode (no serde), eliminating ~33% JSON+base64 overhead
- V3 chunked encoder/decoder with zstd compression before encryption for payloads > 8KB threshold
- TransferPayloadEncryptorAdapter now produces V3 format; DecryptorAdapter detects V2/V3 by magic bytes
- 27 total new tests including 10MB payload round-trips, compression flag verification, and V2 backward compatibility

## Task Commits

Each task was committed atomically:

1. **Task 1: V3 binary payload codec in uc-core** - `2d9b2b77` (feat)
2. **Task 2: V3 chunked encoder/decoder with zstd compression** - `4699331a` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-core/src/network/protocol/clipboard_payload_v3.rs` - V3 binary payload codec with ClipboardBinaryPayload and BinaryRepresentation types
- `src-tauri/crates/uc-core/src/network/protocol/mod.rs` - Module declaration and re-exports for V3 types
- `src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs` - V3 encoder/decoder, compression logic, updated adapters, V3 constants and error variants

## Decisions Made

- Used `reason` instead of `source` for String-typed error variant fields in thiserror (String does not implement std::error::Error, causing thiserror `#[source]` auto-derive conflict)
- Kept V2 ChunkedEncoder/ChunkedDecoder and all V2 tests intact for transition period (removal in Plan 02)
- Compression decision made in adapter layer (not in encoder) for clean separation of concerns

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed thiserror source field naming**

- **Found during:** Task 2 (V3 chunked encoder)
- **Issue:** `CompressionFailed { source: String }` caused compile error because thiserror interprets `source` field specially and requires it to implement `std::error::Error`
- **Fix:** Renamed `source` to `reason` in both CompressionFailed and DecompressionFailed variants
- **Files modified:** src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs
- **Verification:** All 26 chunked_transfer tests pass, cargo check clean
- **Committed in:** 4699331a (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Naming change only, no functional impact. No scope creep.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- V3 binary codec and chunked encoder/decoder ready for use in Plan 02 (rewiring use cases)
- V2 code preserved for transition; Plan 02 will remove V1/V2 legacy paths
- All 128 uc-core and 184 uc-infra tests pass

---

_Phase: 08-optimize-large-image-sync-pipeline-v3-binary-protocol-compression-zero-copy-fanout_
_Completed: 2026-03-05_

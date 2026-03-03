---
phase: 02-unified-transfer-layer
plan: '01'
subsystem: network
tags: [rust, serde, serde_with, base64, aead, clipboard-protocol, v2-protocol]

# Dependency graph
requires: []
provides:
  - ClipboardPayloadVersion enum (V1/V2) with serde into/try_from u8
  - ClipboardMessage with payload_version field and base64-encoded encrypted_content
  - ClipboardMultiRepPayloadV2 and WireRepresentation types for V2 transfers
  - for_chunk_transfer AAD helper for chunk-level AEAD encryption
affects:
  - 02-02-PLAN (V2 encoder/decoder depends on these types)
  - 02-03-PLAN (use cases depend on ClipboardPayloadVersion routing)
  - sync_outbound.rs (payload_version field added)
  - sync_inbound.rs (payload_version routing will be added in later plans)

# Tech tracking
tech-stack:
  added:
    - serde_bytes = "0.11" (added but not used for Base64; kept per plan spec)
    - serde_with base64 feature enabled (for Base64 JSON encoding)
  patterns:
    - ClipboardMessage uses serde_with Base64 for encrypted_content (compact JSON, not integer array)
    - ClipboardPayloadVersion uses serde(into = "u8", try_from = "u8") for numeric JSON representation
    - serde(default) on payload_version for backward compatibility with V1 senders
    - AAD for chunk transfers is binary (16 bytes transfer_id || 4 bytes chunk_index LE) not text

key-files:
  created:
    - src-tauri/crates/uc-core/src/network/protocol/clipboard_payload_v2.rs
  modified:
    - src-tauri/crates/uc-core/src/network/protocol/clipboard.rs
    - src-tauri/crates/uc-core/src/network/protocol/mod.rs
    - src-tauri/crates/uc-core/src/security/aad.rs
    - src-tauri/crates/uc-core/Cargo.toml
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
    - src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs

key-decisions:
  - 'Used serde_with Base64 instead of serde_bytes for JSON base64 encoding (serde_bytes only optimizes binary formats like bincode/CBOR, not JSON)'
  - 'ClipboardPayloadVersion serializes as numeric u8 in JSON for compact wire format and forward compatibility'
  - 'for_chunk_transfer uses binary AAD format (not text) consistent with AEAD standard practices'
  - 'WireRepresentation.bytes also uses Base64 encoding matching encrypted_content pattern'

patterns-established:
  - 'Pattern 1: Binary fields in network protocol messages use serde_with Base64 for compact JSON encoding'
  - 'Pattern 2: New enum payload versions use serde(default) for backward compatibility with old senders'
  - 'Pattern 3: Chunk AAD is binary concatenation (transfer_id || chunk_index_LE) not text format'

requirements-completed:
  - UTL-01
  - UTL-02

# Metrics
duration: 45min
completed: 2026-03-03
---

# Phase 2 Plan 01: V2 Protocol Type Contracts Summary

**V2 clipboard protocol type contracts established: ClipboardPayloadVersion enum, base64-encoded ClipboardMessage, ClipboardMultiRepPayloadV2/WireRepresentation types, and binary chunk AAD helper**

## Performance

- **Duration:** ~45 min
- **Started:** 2026-03-03T07:00:00Z
- **Completed:** 2026-03-03T07:45:00Z
- **Tasks:** 2 (both TDD)
- **Files modified:** 8

## Accomplishments

- Established backward-compatible `ClipboardPayloadVersion` enum (V1/V2) with `#[serde(default)]` so old senders produce V1 messages automatically
- Updated `ClipboardMessage.encrypted_content` to use base64 JSON encoding (via `serde_with` Base64), replacing the integer array format
- Created `ClipboardMultiRepPayloadV2` and `WireRepresentation` types for bundling all clipboard representations in a single atomic V2 transfer
- Added `for_chunk_transfer` AAD helper producing deterministic 20-byte binary AAD for per-chunk AEAD encryption
- Fixed all downstream callers of `ClipboardMessage` struct initializer to include the new `payload_version` field

## Task Commits

Each task was committed atomically:

1. **Task 1: Add payload_version to ClipboardMessage and create V2 payload types** - `f5b7186` (feat)
2. **Task 2: Add chunk-level AAD helper to aad.rs** - `6c5e01f` (feat)

**Plan metadata:** (see docs commit below)

_Note: Both tasks used TDD (RED → GREEN). 16 new tests added total._

## Files Created/Modified

- `src-tauri/crates/uc-core/src/network/protocol/clipboard.rs` - Added ClipboardPayloadVersion enum, payload_version field with serde(default), base64 encoding for encrypted_content
- `src-tauri/crates/uc-core/src/network/protocol/clipboard_payload_v2.rs` - NEW: ClipboardMultiRepPayloadV2 and WireRepresentation types with base64 bytes field
- `src-tauri/crates/uc-core/src/network/protocol/mod.rs` - Exported ClipboardPayloadVersion, ClipboardMultiRepPayloadV2, WireRepresentation
- `src-tauri/crates/uc-core/src/security/aad.rs` - Added for_chunk_transfer helper with 5 new tests
- `src-tauri/crates/uc-core/Cargo.toml` - Added serde_bytes 0.11, enabled serde_with base64 feature
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs` - Added payload_version: V1 to ClipboardMessage init
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` - Added payload_version: V1 to test ClipboardMessage init
- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` - Added payload_version: V1 to test ClipboardMessage init

## Decisions Made

- Used `serde_with` Base64 instead of the plan-specified `serde_bytes` for JSON base64 encoding. `serde_bytes` optimizes binary serializers (bincode, CBOR) but with `serde_json` it still outputs integer arrays `[1,2,3]`. Since `serde_with` was already in uc-core's dependencies and provides true base64 JSON encoding, it was the correct tool.
- Both `encrypted_content` in `ClipboardMessage` and `bytes` in `WireRepresentation` use the same Base64 encoding pattern for consistency.
- `ClipboardPayloadVersion` serializes as a bare u8 number in JSON (via `into/try_from`) rather than a string, matching the plan's wire format requirement.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Used serde_with Base64 instead of serde_bytes for JSON base64 encoding**

- **Found during:** Task 1 (implementing encrypted_content field)
- **Issue:** Plan specified `#[serde(with = "serde_bytes")]` for base64 JSON encoding, but `serde_bytes` with `serde_json` actually produces integer arrays `[1,2,3]`, not base64 strings. This is a known limitation — serde_bytes is optimized for binary wire formats, not JSON.
- **Fix:** Used `serde_with` crate's `Base64` annotation (`#[serde_as(as = "Base64")]`) which correctly produces base64 JSON strings. Also enabled the `base64` feature in serde_with dependency. Kept `serde_bytes = "0.11"` in Cargo.toml per plan spec (but not used for the field annotation).
- **Files modified:** clipboard.rs, clipboard_payload_v2.rs, Cargo.toml
- **Verification:** Tests `encrypted_content_serializes_as_base64_not_integer_array` and `wire_representation_bytes_encode_as_base64_in_json` pass
- **Committed in:** f5b7186 (Task 1 commit)

**2. [Rule 1 - Bug] Fixed ClipboardMessage struct initializers missing payload_version field**

- **Found during:** Task 1 (running uc-app tests after modifying ClipboardMessage)
- **Issue:** Adding `payload_version` field to `ClipboardMessage` caused compile errors in `sync_outbound.rs`, `sync_inbound.rs` (test helper), and `libp2p_network.rs` (test) — all struct initializers required updating.
- **Fix:** Added `payload_version: ClipboardPayloadVersion::V1` to all three struct initializers.
- **Files modified:** sync_outbound.rs, sync_inbound.rs, libp2p_network.rs
- **Verification:** `cargo test -p uc-app --lib` passes (145 tests), `cargo check -p uc-platform` passes
- **Committed in:** f5b7186 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (both Rule 1 - Bug)
**Impact on plan:** Both auto-fixes necessary for correctness and compilation. No scope creep. Core intent of plan fully realized (base64 JSON encoding, backward-compatible protocol versioning, chunk AAD helper).

## Issues Encountered

- serde_bytes does not produce base64 in JSON (only integer arrays). Required switching to serde_with Base64. This is a documentation/assumption error in the plan, not a code bug.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- All V2 protocol type contracts are in place for Plans 02 and 03
- `ClipboardPayloadVersion`, `ClipboardMultiRepPayloadV2`, `WireRepresentation` accessible via `uc_core::network::protocol`
- `for_chunk_transfer` accessible via `uc_core::security::aad`
- 110 uc-core tests passing, 145 uc-app tests passing

---

_Phase: 02-unified-transfer-layer_
_Completed: 2026-03-03_

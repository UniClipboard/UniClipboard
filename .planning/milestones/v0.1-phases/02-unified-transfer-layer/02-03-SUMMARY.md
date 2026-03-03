---
phase: 02-unified-transfer-layer
plan: '03'
subsystem: clipboard-sync
tags: [chunked-transfer, xchacha20poly1305, v2-protocol, multi-representation, libp2p]

# Dependency graph
requires:
  - phase: 02-unified-transfer-layer
    provides: ClipboardMultiRepPayloadV2, WireRepresentation, ClipboardPayloadVersion types (Plan 01)
  - phase: 02-unified-transfer-layer
    provides: ChunkedEncoder::encode_to, ChunkedDecoder::decode_from streaming API (Plan 02)
provides:
  - V2 outbound: sync_outbound.rs packs all snapshot representations into ClipboardMultiRepPayloadV2 and chunk-encrypts via ChunkedEncoder::encode_to
  - V2 inbound: sync_inbound.rs routes V2 messages to apply_v2_inbound, selects highest-priority representation (image > html > rtf > plain > other)
  - Transport limits raised: 300MB payload cap, 120s read/write timeouts in libp2p_network.rs
  - V1 backward compatibility: V1 inbound path unchanged; V2 path added as separate branch
affects:
  - uc-app clipboard capture pipeline
  - uc-platform libp2p transport tests

# Tech tracking
tech-stack:
  added:
    - uc-infra added as production dependency in uc-app/Cargo.toml (was dev-only before)
  patterns:
    - Version dispatch: match message.payload_version { V1 => ..., V2 => ... } in execute_with_outcome
    - Priority selection: max_by_key priority function (image > html > rtf > plain > other)
    - Tamper resilience: V2 decode failures return Ok(Skipped) with error log — no panics, no Result propagation
    - V2 dedup by message.id only (OS-clipboard snapshot_hash comparison intentionally skipped for V2)

key-files:
  modified:
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
    - src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs
    - src-tauri/crates/uc-app/Cargo.toml

key-decisions:
  - 'Streaming option B chosen for outbound: ChunkedEncoder::encode_to writes to Vec<u8> in use case, then Vec<u8> passed to ClipboardTransportPort::send_clipboard. The memory guarantee (CHUNK_SIZE x 2) holds during encoding. True stream-pass-through (Option A) would require ClipboardTransportPort interface changes that affect all adapters — deferred.'
  - 'Inbound read_to_end unchanged: libp2p inbound handler still reads full ProtocolMessage JSON via read_to_end, then ChunkedDecoder::decode_from(Cursor::new(&encrypted_content), key) is called in the use case. True stream-level decoding would require separating the outer JSON envelope from the V2 payload — future optimization.'
  - 'V2 dedup by message.id only: OS-clipboard snapshot_hash comparison omitted because the OS clipboard holds only the highest-priority representation, not all reps. Re-computing snapshot_hash from the OS clipboard would be fragile. TTL-bounded recent_ids dedup is sufficient.'
  - 'MimeType construction: MimeType(s.to_string()) used directly — from_str_lossy does not exist in the API. This was explicitly verified from mime.rs source.'
  - 'uc-infra promoted to production dependency in uc-app: ChunkedEncoder/ChunkedDecoder are required in the production outbound and inbound paths (not just tests).'

patterns-established:
  - 'Version dispatch pattern: check message.payload_version before any decryption; route to separate V1/V2 handler functions'
  - 'Tamper-resilient V2 decode: match on ChunkedDecoder result, log error, rollback_recent_id, return Ok(Skipped) — never propagate decode errors as Result::Err'
  - 'Priority selection: fn priority(mime: Option<&str>) -> u8 + max_by_key pattern; reusable for future representation types'

requirements-completed: [UTL-05, UTL-06, UTL-07]

# Metrics
duration: 15min
completed: 2026-03-03
---

# Phase 2 Plan 03: V2 Outbound/Inbound Wire-up Summary

**V2 multi-representation outbound (all formats, chunk-encrypted) and V1/V2 dispatch inbound with image-priority selection, delivering end-to-end clipboard sync across devices with XChaCha20-Poly1305 bounded-memory chunked encoding**

## Performance

- **Duration:** 15 min
- **Started:** 2026-03-03T07:20:09Z
- **Completed:** 2026-03-03T07:35:00Z
- **Tasks:** 2 (both TDD)
- **Files modified:** 4

## Accomplishments

- Replaced text-only V1 outbound with V2 multi-representation path: all snapshot representations packed into ClipboardMultiRepPayloadV2, chunk-encrypted via ChunkedEncoder::encode_to
- Extended inbound with V2 detection branch: V1 path unchanged, V2 path uses ChunkedDecoder::decode_from + priority-based representation selection (image > html > rtf > plain > other)
- V2 tamper resilience: decode failures return Ok(Skipped) with error log; no panics or partial writes
- Raised transport limits: BUSINESS_PAYLOAD_MAX_BYTES 100MB→300MB, read/write timeouts 30s/10s→120s/120s
- E2E test clipboard_sync_e2e_dual_peer_in_process passes end-to-end with V2

## Task Commits

Each task was committed atomically:

1. **Task 1: Rewrite sync_outbound.rs for V2 and raise transport limits** - `6147c8f` (feat)
2. **Task 2: Extend sync_inbound.rs with V2 detection and priority-based repr selection** - `bfe4a64` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs` - V2 outbound path: ClipboardMultiRepPayloadV2, ChunkedEncoder, snapshot_hash as content_hash; removed V1 text-only filtering
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` - V1/V2 dispatch, apply_v1_inbound, apply_v2_inbound, select_highest_priority_repr helper
- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` - Transport constants: 300MB cap, 120s timeouts
- `src-tauri/crates/uc-app/Cargo.toml` - Added uc-infra as production dependency

## Decisions Made

**Option B chosen for outbound streaming:**
ChunkedEncoder::encode_to writes to a Vec<u8> in the use case, then the Vec is passed to ClipboardTransportPort::send_clipboard. The memory guarantee (CHUNK_SIZE x 2) holds during encoding because encode_to processes one chunk at a time. Option A (passing plaintext+key to the transport for true stream-write) would require changing ClipboardTransportPort to add a new method — this would affect all adapters and all call sites, and was considered an architectural change (Rule 4 scope). Deferred to a future plan.

**Inbound read_to_end unchanged:**
The libp2p inbound handler reads the full ProtocolMessage JSON via read_to_end (as V1 did). ChunkedDecoder::decode_from is called with Cursor::new(&encrypted_content) inside the use case. True inbound stream-level chunking would require changing the wire format to separate the outer JSON envelope from the V2 payload — a future optimization.

**V2 dedup by message.id only:**
OS-clipboard snapshot_hash comparison is intentionally omitted for V2. Rationale documented in code: the OS clipboard holds only the highest-priority representation (not all reps), so snapshot_hash computed from the OS clipboard would differ from the V2 snapshot_hash (which covers all representations). TTL-bounded recent_ids dedup by message.id is sufficient.

## Streaming Memory Analysis

**Outbound (current, Option B):**

- Peak during encoding: CHUNK_SIZE (256KB) plaintext slice + CHUNK_SIZE+16 ciphertext Vec = ~512KB
- Full encrypted_content Vec allocated after encoding completes: proportional to payload size
- Better than V1 (which held serialized JSON + encrypted blob simultaneously)
- True streaming deferred to a future plan when ClipboardTransportPort gains a streaming variant

**Inbound:**

- Full ProtocolMessage JSON buffer in memory (read_to_end): proportional to payload size
- ChunkedDecoder processes ciphertext Cursor without additional copies beyond the plaintext output
- The BUSINESS_PAYLOAD_MAX_BYTES cap (300MB) bounds the maximum in-memory buffer

## Deviations from Plan

**1. [Rule 1 - Bug] E2E test clipboard_sync_e2e_dual_peer_in_process failed after Task 1**

- **Found during:** Task 1 GREEN verification
- **Issue:** The E2E test sends V2 from outbound_a to inbound_b. The V2 payload was produced by sync_outbound but inbound was still on the V1 path (serde_json::from_slice as EncryptedBlob), causing deserialization failure.
- **Fix:** Proceeded immediately to Task 2 (sync_inbound V2 path) — this was the planned fix. The E2E test passes after Task 2.
- **Files modified:** sync_inbound.rs (Task 2)
- **Committed in:** bfe4a64

**2. [Rule 3 - Blocking] uc-infra dependency missing from uc-app production dependencies**

- **Found during:** Task 1 implementation (ChunkedEncoder needed in production code)
- **Issue:** uc-infra was only in uc-app dev-dependencies; ChunkedEncoder is needed in production outbound path
- **Fix:** Added uc-infra = { path = "../uc-infra" } to [dependencies] in uc-app/Cargo.toml
- **Files modified:** src-tauri/crates/uc-app/Cargo.toml
- **Committed in:** 6147c8f (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 bug/ordering issue, 1 blocking dependency)
**Impact on plan:** Both auto-fixes necessary for correctness. No scope creep.

## Issues Encountered

- `MimeType::from_str_lossy` does not exist in the API (as the plan warned). Verified from mime.rs source: `MimeType` is a newtype with public field, so `MimeType(s.to_string())` is the correct construction. No `.unwrap()` needed.
- `FormatId::from(&str)` works via `impl From<&str>` from the `impl_id!` macro — verified from id_macro.rs.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- V2 end-to-end clipboard sync is functional: outbound sends all representations, inbound selects highest-priority
- V1 backward compatibility maintained: old devices (payload_version = V1 or missing) still work
- Transport limits raised to support images (300MB cap, 120s timeouts)
- Foundation ready for Phase 3 (if applicable): representation persistence, blob storage for received images, passive-mode V2 dedup improvements

Known limitation: Inbound BUSINESS_STREAM_WRITE_TIMEOUT on the receiving side also affects how long the sender waits for the stream close. With 120s write timeout, large images (up to 300MB) have sufficient time even on slow LAN connections.

---

_Phase: 02-unified-transfer-layer_
_Completed: 2026-03-03_

## Self-Check: PASSED

Files verified:

- FOUND: src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
- FOUND: src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
- FOUND: src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs
- FOUND: .planning/phases/02-unified-transfer-layer/02-03-SUMMARY.md

Commits verified:

- FOUND: 6147c8f (feat(02-03): rewrite sync_outbound to V2 multi-representation and raise transport limits)
- FOUND: bfe4a64 (feat(02-03): extend sync_inbound with V2 detection, priority-based repr selection, and tamper resilience)

Test results: cargo test -p uc-core -p uc-infra -p uc-app — 0 failures across all crates

---
phase: 08-optimize-large-image-sync-pipeline-v3-binary-protocol-compression-zero-copy-fanout
plan: 02
subsystem: network
tags: [binary-protocol, zero-copy, arc, tokio-join, parallelization, outbound-sync]

# Dependency graph
requires:
  - phase: 08-optimize-large-image-sync-pipeline-v3-binary-protocol-compression-zero-copy-fanout
    provides: V3 binary codec (ClipboardBinaryPayload, BinaryRepresentation), ChunkedEncoderV3/DecoderV3
provides:
  - V3 outbound encoding pipeline (binary encode -> encrypt -> Arc<[u8]> fanout)
  - Arc<[u8]> zero-copy multi-peer fanout in ClipboardTransportPort
  - tokio::join! parallel encrypt + ensure_business_path for first peer
  - V1/V2 payload types deleted, ClipboardPayloadVersion V3-only
affects: [08-03-PLAN, sync_inbound]

# Tech tracking
tech-stack:
  added: []
  patterns:
    [Arc<[u8]> zero-copy fanout, tokio::join! parallel encrypt+ensure, V3-only payload version]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-core/src/ports/clipboard_transport.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
    - src-tauri/crates/uc-core/src/network/protocol/clipboard.rs
    - src-tauri/crates/uc-core/src/network/protocol/mod.rs
    - src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs

key-decisions:
  - 'Arc<[u8]> for ClipboardTransportPort send/broadcast signatures eliminates per-peer copy for large payloads'
  - 'tokio::join! parallelizes encryption with first peer ensure_business_path; remaining peers serial'
  - 'V1/V2 removed from ClipboardPayloadVersion enum; old messages fail deserialization (intentional break)'
  - 'Local stub types in sync_inbound.rs preserve compilation during V1/V2 transition (Plan 03 rewrite)'
  - 'MIME constants moved from deleted clipboard_payload.rs to protocol/mod.rs'

patterns-established:
  - 'Arc<[u8]> zero-copy fanout: encrypt once, clone Arc for each peer send'
  - 'tokio::join! for overlapping encryption with network path setup'

requirements-completed: [V3-ARC, V3-OUTBOUND, V3-NOENC, V3-NOLEAK]

# Metrics
duration: 14min
completed: 2026-03-05
---

# Phase 08 Plan 02: V3 Outbound Encoding with Arc Zero-Copy Fanout Summary

**V3 binary encoding pipeline with Arc<[u8]> zero-copy multi-peer fanout and tokio::join! parallel encrypt+ensure, deleting all V1/V2 payload types**

## Performance

- **Duration:** 14 min
- **Started:** 2026-03-05T14:44:34Z
- **Completed:** 2026-03-05T14:59:17Z
- **Tasks:** 2
- **Files modified:** 12

## Accomplishments

- ClipboardTransportPort signatures changed from Vec<u8> to Arc<[u8]> for zero-copy multi-peer fanout
- Outbound sync rewritten: ClipboardBinaryPayload encode_to_vec -> encrypt -> Arc::from(framed.into_boxed_slice()) -> clone per peer
- First peer uses tokio::join! to parallelize encryption with ensure_business_path
- V1/V2 payload files deleted (clipboard_payload.rs, clipboard_payload_v2.rs), ClipboardPayloadVersion V3-only
- All 13 sync_outbound tests pass with V3 verification
- sync_inbound.rs kept compiling with local stub types for Plan 03 rewrite

## Task Commits

Each task was committed atomically:

1. **Task 1: Port signature change Vec to Arc** - `c793cfcc` (feat)
2. **Task 2: V3 outbound encoding + parallelization + zero-copy, delete V1/V2** - `31f8d1d4` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-core/src/ports/clipboard_transport.rs` - Updated trait with Arc<[u8]> signatures
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs` - V3 binary encode + tokio::join! + Arc fanout
- `src-tauri/crates/uc-core/src/network/protocol/clipboard.rs` - ClipboardPayloadVersion V3-only
- `src-tauri/crates/uc-core/src/network/protocol/mod.rs` - Removed V1/V2 modules, moved MIME constants
- `src-tauri/crates/uc-core/src/network/protocol/clipboard_payload.rs` - DELETED (V1)
- `src-tauri/crates/uc-core/src/network/protocol/clipboard_payload_v2.rs` - DELETED (V2)
- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` - Arc<[u8]> in BusinessCommand, V3 inbound match
- `src-tauri/crates/uc-platform/src/adapters/network.rs` - Arc<[u8]> stub signatures
- `src-tauri/crates/uc-core/src/network/mod.rs` - Re-export BinaryRepresentation, ClipboardBinaryPayload
- `src-tauri/crates/uc-infra/src/clipboard/mod.rs` - Export ChunkedDecoderV3
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` - Local V1/V2 stub types for compilation
- `src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs` - Arc<[u8]> in mock signatures

## Decisions Made

- Arc<[u8]> chosen over Arc<Vec<u8>> for trait ergonomics (direct deref to &[u8])
- tokio::join! captures cloned Arc dependencies instead of &self to avoid Send bounds issues
- Local stub types in sync_inbound.rs (without serde_with) preserve compilation; bytes field uses default serde (integer array) which differs from original Base64 encoding but is acceptable since Plan 03 replaces the entire V2 inbound path
- MIME constants moved to protocol/mod.rs as module-level constants (simplest relocation)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed serde_with unavailable in uc-app for stub WireRepresentation**

- **Found during:** Task 2
- **Issue:** Local WireRepresentation stub used `#[serde_with::serde_as]` but uc-app doesn't depend on serde_with
- **Fix:** Removed serde_with attribute, used plain serde Vec<u8> serialization for bytes field
- **Files modified:** src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
- **Verification:** cargo check passes, sync_inbound tests pass

**2. [Rule 3 - Blocking] Fixed e2e test mock missing Arc<[u8]> signatures**

- **Found during:** Task 2
- **Issue:** clipboard_sync_e2e_test.rs InProcessNetwork mock still used Vec<u8> for send/broadcast
- **Fix:** Updated mock signatures to Arc<[u8]>
- **Files modified:** src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs

**3. [Rule 3 - Blocking] Fixed missing ClipboardTextPayloadV1::new in stub**

- **Found during:** Task 2
- **Issue:** sync_inbound.rs tests call ClipboardTextPayloadV1::new which wasn't in the initial stub
- **Fix:** Added new() constructor to the local stub type
- **Files modified:** src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs

---

**Total deviations:** 3 auto-fixed (3 blocking)
**Impact on plan:** All fixes necessary for compilation. No scope creep.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- V3 outbound pipeline complete and tested
- sync_inbound.rs has local V1/V2 stubs ready for Plan 03 rewrite to V3 binary decode
- libp2p inbound handler routes V3 to existing decode path (Plan 03 will add V3-specific streaming)

---

_Phase: 08-optimize-large-image-sync-pipeline-v3-binary-protocol-compression-zero-copy-fanout_
_Completed: 2026-03-05_

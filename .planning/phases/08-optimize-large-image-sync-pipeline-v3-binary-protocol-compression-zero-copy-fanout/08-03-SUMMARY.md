---
phase: 08-optimize-large-image-sync-pipeline-v3-binary-protocol-compression-zero-copy-fanout
plan: 03
subsystem: sync
tags: [v3-protocol, binary-codec, tracing, chunked-transfer, inbound-sync]

requires:
  - phase: 08 plan 01
    provides: V3 binary codec (ClipboardBinaryPayload encode/decode)
  - phase: 08 plan 02
    provides: V3 outbound encoding, Arc zero-copy fanout, ClipboardPayloadVersion V3-only enum
provides:
  - V3-only inbound clipboard sync (no V1/V2 dispatch)
  - V3-only chunked transfer (V2 encoder/decoder removed)
  - Tracing spans on outbound.prepare, outbound.send (per peer), inbound.decode
affects: [sync, clipboard-pipeline, observability]

tech-stack:
  added: []
  patterns: [tracing span instrumentation on critical sync paths]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
    - src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
    - src-tauri/crates/uc-infra/src/clipboard/mod.rs
    - src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs

key-decisions:
  - 'Kept V3_MAGIC constant name (not renamed to MAGIC) for clarity in documentation and grep-ability'
  - 'Removed snapshot_matches_content_hash and first_text_representation_len helpers (V1-only, no longer referenced)'
  - 'Pre-existing uc-tauri test failures and libp2p_network wire roundtrip test failure documented as deferred (not caused by Plan 03)'

patterns-established:
  - 'Tracing spans on sync critical path: outbound.prepare, outbound.send (per peer), inbound.decode'

requirements-completed: [V3-INBOUND, V3-NOENC, V3-NOLEAK]

duration: 19min
completed: 2026-03-05
---

# Phase 08 Plan 03: V3 Inbound Rewrite + V2 Removal + Tracing Spans Summary

**V3-only inbound binary decode replacing V1/V2 dispatch, V2 chunked transfer code removed, tracing spans on outbound.prepare/send and inbound.decode**

## Performance

- **Duration:** 19 min
- **Started:** 2026-03-05T15:01:58Z
- **Completed:** 2026-03-05T15:20:57Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- Rewrote inbound sync path to V3-only binary payload decode via `ClipboardBinaryPayload::decode_from`, removing ~420 lines of V1/V2 code
- Deleted V2 `ChunkedEncoder`/`ChunkedDecoder`, `V2_MAGIC`, and all V2-specific tests from chunked_transfer.rs
- Renamed `ChunkedEncoderV3`/`ChunkedDecoderV3` to `ChunkedEncoder`/`ChunkedDecoder` (V3 is now the only format)
- Added tracing spans: `outbound.prepare` (with raw_bytes), `outbound.send` (per peer with peer_id), `inbound.decode` (with wire_bytes)
- Added debug-level per-representation logging and info-level summary count on inbound
- Added TODO comments at 2 locations for deferred multi-rep persistence

## Task Commits

Each task was committed atomically:

1. **Task 1: Rewrite inbound use case for V3-only decode, remove V1/V2 paths** - `59b85e5f` (feat)
2. **Task 2: Remove V2 chunked transfer code, add tracing spans, final cleanup** - `c3a8221e` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` - V3-only inbound with binary decode, priority selection, tracing spans
- `src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs` - V3-only chunked transfer (V2 code removed, types renamed)
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs` - Added outbound.prepare and outbound.send tracing spans
- `src-tauri/crates/uc-infra/src/clipboard/mod.rs` - Updated re-exports (removed ChunkedDecoderV3)
- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` - Removed Plan 03 TODO comments

## Decisions Made

- Kept `V3_MAGIC` constant name instead of renaming to `MAGIC` -- the V3 prefix provides clarity for documentation and grep-ability
- Removed `snapshot_matches_content_hash` and `first_text_representation_len` helpers that were only used in the deleted V1 full-mode path
- Marked `encryption` field as `#[allow(dead_code)]` since it was only used by the removed V1 path but is still required by the constructor interface

## Deviations from Plan

None - plan executed exactly as written.

## Deferred Issues

- **Pre-existing:** `uc-tauri` workspace tests have compilation errors due to mock types using `Vec<u8>` instead of `Arc<[u8]>` for `send_clipboard`/`broadcast_clipboard` (introduced by Plan 02 trait change, not Plan 03)
- **Pre-existing:** `libp2p_network_clipboard_wire_roundtrip_delivers_clipboard_message` test fails because it sends non-encrypted test data that cannot be decoded by ChunkedDecoder (was also failing before Plan 03)

## Issues Encountered

- `git stash pop` reverted Task 2 file changes due to Cargo.lock conflict -- had to reapply all chunked_transfer.rs, mod.rs, sync_outbound.rs, and libp2p_network.rs edits

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 08 V3 migration is complete: codec (Plan 01), outbound (Plan 02), inbound + cleanup (Plan 03)
- All V1/V2 code paths removed from sync pipeline
- Tracing spans provide latency visibility on critical outbound/inbound paths
- Multi-rep persistence deferred (TODO markers at 2 locations in sync_inbound.rs)

---

_Phase: 08-optimize-large-image-sync-pipeline-v3-binary-protocol-compression-zero-copy-fanout_
_Completed: 2026-03-05_

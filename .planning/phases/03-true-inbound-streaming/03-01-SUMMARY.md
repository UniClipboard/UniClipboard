---
phase: 03-true-inbound-streaming
plan: '01'
subsystem: network
tags: [wire-format, framing, protocol, binary-payload, length-prefix]

# Dependency graph
requires:
  - phase: 02-unified-transfer-layer
    provides: ChunkedEncoder/Decoder, V2 protocol types, sync_outbound V2 path
provides:
  - ProtocolMessage::frame_to_bytes() method for two-segment wire format production
  - sync_outbound produces length-prefixed framed bytes with raw V2 binary trailing payload
  - parse_framed_v2 test helper for two-segment wire format parsing
affects: [03-02-PLAN (inbound streaming parser reads length-prefix then streams V2 remainder)]

# Tech tracking
tech-stack:
  added: []
  patterns: [two-segment wire framing with 4-byte LE length prefix]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-core/src/network/protocol/protocol_message.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
    - src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs
    - src-tauri/crates/uc-app/Cargo.toml

key-decisions:
  - 'frame_to_bytes returns Result<Vec<u8>, serde_json::Error> matching to_bytes signature — no new error types needed'
  - 'V2 JSON header has encrypted_content=vec![] with raw V2 binary as trailing bytes — eliminates ~33% base64 overhead on wire'
  - 'to_bytes and from_bytes remain unchanged for backward compatibility and internal use'

patterns-established:
  - 'Two-segment wire framing: [4-byte JSON len LE][JSON header][optional raw trailing payload]'
  - 'parse_framed_v2 test helper pattern for verifying two-segment wire format in tests'

requirements-completed: []

# Metrics
duration: 5min
completed: 2026-03-03
---

# Phase 3 Plan 01: Two-Segment Wire Framing Summary

**ProtocolMessage gains frame_to_bytes() for [4-byte LE len][JSON header][raw V2 binary payload] wire format, eliminating ~33% base64 overhead on outbound V2 clipboard messages**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-03T09:24:37Z
- **Completed:** 2026-03-03T09:30:20Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Added `ProtocolMessage::frame_to_bytes(trailing_payload: Option<&[u8]>)` method producing [4-byte JSON len LE][JSON bytes][optional raw payload]
- Updated sync_outbound to produce two-segment framed bytes with `frame_to_bytes(Some(&encrypted_content))` and empty `encrypted_content` in JSON header
- Updated all affected tests to parse the new length-prefixed wire format
- Fixed e2e test `InProcessNetwork` to parse framed format and re-attach trailing V2 payload for inbound processing

## Task Commits

Each task was committed atomically:

1. **Task 1: Add frame_to_bytes method to ProtocolMessage** - `099171c` (feat) - TDD: 3 new tests for roundtrip, trailing payload, empty trailing
2. **Task 2: Update sync_outbound to produce two-segment framed wire bytes** - `cc42d3c` (feat) - Updated production code + all tests + e2e test

**Plan metadata:** (pending)

## Files Created/Modified

- `src-tauri/crates/uc-core/src/network/protocol/protocol_message.rs` - Added `frame_to_bytes()` method and 3 unit tests
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs` - Changed outbound framing from `to_bytes()` to `frame_to_bytes(Some(&encrypted_content))`, added `parse_framed_v2` test helper, updated 2 tests
- `src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs` - Updated `InProcessNetwork::send_clipboard` to parse two-segment framed format
- `src-tauri/crates/uc-app/Cargo.toml` - Removed duplicate uc-infra dev-dependency

## Decisions Made

- `frame_to_bytes` returns `Result<Vec<u8>, serde_json::Error>` matching `to_bytes` signature -- no new error types needed
- V2 JSON header has `encrypted_content: vec![]` with raw V2 binary as trailing bytes -- eliminates ~33% base64 overhead on wire
- `to_bytes` and `from_bytes` remain unchanged -- backward compatibility maintained for Plan 03-02 inbound parser and internal use
- No V1 backward-compatibility concerns: V1 receivers already cannot process V2 payloads, and non-clipboard messages use the same framing

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed e2e test InProcessNetwork for new wire format**

- **Found during:** Task 2 (sync_outbound update)
- **Issue:** `clipboard_sync_e2e_test.rs` uses `InProcessNetwork::send_clipboard` which called `ProtocolMessage::from_bytes()` directly on outbound bytes -- fails with length prefix
- **Fix:** Updated to parse two-segment framed format: read 4-byte len, extract JSON header, parse it, re-attach trailing V2 payload to `encrypted_content` for inbound processing
- **Files modified:** `src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs`
- **Verification:** `cargo test -p uc-app --test clipboard_sync_e2e_test` passes
- **Committed in:** `cc42d3c` (Task 2 commit)

**2. [Rule 3 - Blocking] Removed duplicate uc-infra dev-dependency**

- **Found during:** Task 2 (sync_outbound update)
- **Issue:** `uc-infra` was listed in both `[dependencies]` and `[dev-dependencies]` in `uc-app/Cargo.toml`, causing cargo warning
- **Fix:** Removed the duplicate `uc-infra` from `[dev-dependencies]`
- **Files modified:** `src-tauri/crates/uc-app/Cargo.toml`
- **Verification:** `cargo check -p uc-app` passes without warnings
- **Committed in:** `cc42d3c` (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking)
**Impact on plan:** Both auto-fixes necessary for tests to compile and pass. No scope creep.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Two-segment wire framing is in place for all outbound messages
- Plan 03-02 can now implement the inbound streaming parser: read 4-byte length prefix, parse JSON header via `from_bytes()`, then stream remaining V2 bytes directly to `ChunkedDecoder::decode_from`
- `ProtocolMessage::from_bytes()` is unchanged and ready for the inbound reader to parse the JSON header portion

## Self-Check: PASSED

All files exist. All commits verified (099171c, cc42d3c).

---

_Phase: 03-true-inbound-streaming_
_Completed: 2026-03-03_

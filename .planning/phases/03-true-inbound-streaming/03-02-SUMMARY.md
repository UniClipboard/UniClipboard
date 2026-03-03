---
phase: 03-true-inbound-streaming
plan: 02
subsystem: network, clipboard
tags:
  [
    libp2p,
    streaming,
    chunked-transfer,
    sync-inbound,
    SyncIoBridge,
    spawn_blocking,
    two-segment-framing,
  ]

# Dependency graph
requires:
  - phase: 03-true-inbound-streaming
    plan: 01
    provides: two-segment wire format (frame_to_bytes), outbound V2 framing
provides:
  - V2 inbound streaming decode at transport level via SyncIoBridge + spawn_blocking + ChunkedDecoder
  - Channel type (ClipboardMessage, Option<Vec<u8>>) throughout clipboard receive pipeline
  - EncryptionSessionPort threaded into Libp2pNetworkAdapter for V2 transport-level decryption
  - Pre-decoded plaintext fast path in SyncInboundClipboardUseCase
  - Fallback ChunkedDecoder decode path in use case for robustness
affects: [03-true-inbound-streaming, uc-platform, uc-app, uc-tauri]

# Tech tracking
tech-stack:
  added: [uc-infra dependency in uc-platform, tokio-util io-util feature for SyncIoBridge]
  patterns:
    [two-segment framing reader, ProcessedMessage enum dispatch, transport-level streaming decode]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs
    - src-tauri/crates/uc-platform/Cargo.toml
    - src-tauri/crates/uc-core/src/ports/clipboard_transport.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs
    - src-tauri/crates/uc-tauri/src/commands/clipboard.rs
    - src-tauri/crates/uc-tauri/src/test_utils.rs
    - src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
    - src-tauri/crates/uc-platform/src/adapters/network.rs

key-decisions:
  - 'EncryptionSessionPort added as constructor parameter to Libp2pNetworkAdapter — not a runtime field or deferred injection. Production wiring creates encryption_session before adapter.'
  - 'Stream close handled via Drop (SyncIoBridge drops -> tokio reader drops -> compat layer drops -> Take<libp2p::Stream> drops -> stream closes). No explicit .close() needed for V2 path.'
  - "Fallback ChunkedDecoder path preserved in sync_inbound for robustness — if transport layer didn't pre-decode, use case decodes in-process."
  - 'MAX_JSON_HEADER_SIZE set to 64KB — JSON headers exceeding this are discarded at transport layer with warn log.'
  - 'ProcessedMessage enum separates V2 decoded results from standard messages — enables clean dispatch without re-parsing.'

patterns-established:
  - 'Two-segment framing reader: 4-byte LE length prefix + bounded JSON header + optional trailing binary payload'
  - 'Transport-level streaming decode: SyncIoBridge + spawn_blocking bridges async libp2p stream to sync std::io::Read for ChunkedDecoder'
  - '(ClipboardMessage, Option<Vec<u8>>) tuple channel: Option carries pre-decoded V2 plaintext, None for V1'

requirements-completed: []

# Metrics
duration: 20min
completed: 2026-03-03
---

# Phase 03 Plan 02: Inbound V2 Streaming Decode Summary

**Two-segment framing reader with transport-level V2 streaming decode via SyncIoBridge + spawn_blocking, eliminating read_to_end bottleneck and achieving ~1x chunk memory for V2 inbound**

## Performance

- **Duration:** ~20 min
- **Started:** 2026-03-03T08:00:00Z
- **Completed:** 2026-03-03T08:20:00Z
- **Tasks:** 2
- **Files modified:** 12

## Accomplishments

- Replaced read_to_end bottleneck in inbound stream handler with two-segment framing reader (4-byte LE length prefix + bounded JSON header + V2 trailing payload)
- V2 clipboard messages decoded at transport level via SyncIoBridge + spawn_blocking + ChunkedDecoder::decode_from — peak memory is ~1x chunk size (256KB) instead of full payload
- Changed clipboard channel type from Sender<ClipboardMessage> to Sender<(ClipboardMessage, Option<Vec<u8>>)> across entire codebase (9 implementors updated)
- Added pre-decoded plaintext fast path to SyncInboundClipboardUseCase — V2 path skips master key retrieval and ChunkedDecoder when transport already decoded
- Preserved fallback ChunkedDecoder path in use case for robustness
- Added 2 new tests for pre-decoded plaintext handling
- All 82 unit tests pass across affected crates (1 pre-existing failure unrelated to changes)

## Task Commits

Each task was committed atomically:

1. **Task 1: Restructure inbound stream handler for two-segment framing and V2 streaming decode** - `8ca86c4` (feat)
2. **Task 2: Update sync_inbound to use pre-decoded V2 plaintext from transport** - `4e90a1a` (feat)
3. **Task 2 tests: Pre-decoded V2 plaintext tests** - `66a6a3d` (test)

## Files Created/Modified

- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` - Two-segment framing reader, ProcessedMessage enum, handle_standard_message/handle_v2_clipboard split, EncryptionSessionPort threading
- `src-tauri/crates/uc-platform/Cargo.toml` - Added uc-infra dependency and tokio-util io-util feature
- `src-tauri/crates/uc-core/src/ports/clipboard_transport.rs` - subscribe_clipboard return type changed to (ClipboardMessage, Option<Vec<u8>>)
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` - Pre-decoded plaintext fast path, fallback decode, 2 new tests
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` - Receive loop destructures tuple, encryption_session created before adapter
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - Test mock updated
- `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` - Test mock updated
- `src-tauri/crates/uc-tauri/src/test_utils.rs` - Test mock updated
- `src-tauri/crates/uc-app/tests/clipboard_sync_e2e_test.rs` - InProcessNetwork mock updated, execute call updated
- `src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs` - Test mock updated
- `src-tauri/crates/uc-platform/src/adapters/network.rs` - Placeholder mock updated

## Decisions Made

- **EncryptionSessionPort threading:** Added as constructor parameter to Libp2pNetworkAdapter rather than runtime injection. Production wiring in wiring.rs creates encryption_session before the adapter — required reordering initialization order.
- **Stream close via Drop:** For V2 path, the stream is moved into spawn_blocking via SyncIoBridge. When ChunkedDecoder finishes, SyncIoBridge drops the tokio reader, which drops the compat layer, which drops Take<libp2p::Stream>. No explicit .close() needed.
- **Fallback decode path preserved:** sync_inbound retains ChunkedDecoder::decode_from fallback for robustness — if transport didn't pre-decode (e.g., in e2e tests), the use case handles it.
- **64KB JSON header cap:** MAX_JSON_HEADER_SIZE = 64KB prevents oversized header allocation attacks.
- **ProcessedMessage enum:** Clean separation of V2 decoded results vs standard messages — enables type-safe dispatch.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added io-util feature to tokio-util for SyncIoBridge**

- **Found during:** Task 1
- **Issue:** SyncIoBridge is gated behind tokio-util's `io-util` feature, not the `io` feature
- **Fix:** Added `io-util` to tokio-util features in uc-platform/Cargo.toml
- **Files modified:** src-tauri/crates/uc-platform/Cargo.toml
- **Verification:** Compilation succeeds
- **Committed in:** 8ca86c4 (Task 1 commit)

**2. [Rule 3 - Blocking] Reordered encryption_session creation in wiring.rs**

- **Found during:** Task 1
- **Issue:** encryption_session was created after Libp2pNetworkAdapter but now needs to be passed as constructor parameter
- **Fix:** Moved encryption_session creation before Libp2pNetworkAdapter::new() call
- **Files modified:** src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
- **Verification:** Compilation succeeds, all uc-tauri tests pass
- **Committed in:** 8ca86c4 (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (2 blocking)
**Impact on plan:** Both auto-fixes necessary for compilation. No scope creep.

## Issues Encountered

- Pre-existing test failure `business_command_timeouts_cover_stream_operation_budgets` in uc-platform — confirmed it fails identically on main branch, not related to this plan's changes.

## Next Phase Readiness

- Inbound V2 streaming decode is complete — true end-to-end streaming for V2 clipboard sync is now operational
- The full pipeline: outbound (frame_to_bytes with chunked encoding) -> wire (two-segment framing) -> inbound (two-segment reader with streaming ChunkedDecoder decode) -> use case (pre-decoded plaintext fast path)
- Phase 03 goals are met: true inbound streaming with ~1x chunk memory, no read_to_end bottleneck

---

_Phase: 03-true-inbound-streaming_
_Completed: 2026-03-03_

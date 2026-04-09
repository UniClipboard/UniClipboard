---
phase: 45-daemon-api-foundation-add-local-http-and-websocket-transport-with-read-only-runtime-queries
plan: 01
subsystem: api
tags: [axum, websocket, bearer-token, daemon, query-service, json-rpc]
requires:
  - phase: 41-daemon-and-cli-skeletons
    provides: daemon lifecycle, runtime state, and unix-socket RPC baseline
  - phase: 43-unify-gui-and-cli-business-flows
    provides: shared peer and paired-device use cases
provides:
  - daemon-local auth token persistence and connection metadata
  - read-only daemon HTTP and WebSocket DTO contracts
  - daemon query service backed by CoreRuntime and transport-neutral runtime snapshots
affects: [45-02, 45-03, daemon-http-server, cli-daemon-client, tauri-daemon-bootstrap]
tech-stack:
  added: [axum, chrono, rand, tempfile]
  patterns:
    [
      daemon-owned transport DTOs,
      transport-neutral runtime snapshots,
      shared CoreRuntime query service,
    ]
key-files:
  created:
    - src-tauri/crates/uc-daemon/src/api/mod.rs
    - src-tauri/crates/uc-daemon/src/api/auth.rs
    - src-tauri/crates/uc-daemon/tests/api_auth.rs
    - src-tauri/crates/uc-daemon/tests/api_query.rs
  modified:
    - src-tauri/crates/uc-daemon/src/api/types.rs
    - src-tauri/crates/uc-daemon/src/api/query.rs
    - src-tauri/crates/uc-daemon/src/state.rs
    - src-tauri/crates/uc-daemon/src/rpc/handler.rs
    - src-tauri/crates/uc-daemon/src/app.rs
key-decisions:
  - 'Daemon auth uses a dedicated local token file with permission repair instead of embedding auth in URLs or frontend storage.'
  - 'RuntimeState now stores daemon-owned worker snapshots so RPC and HTTP layers map from one internal state source without sharing transport DTOs.'
  - 'api_query tests reuse a single CoreRuntime instance to prevent SQLite migration lock contention during concurrent test bootstrap.'
patterns-established:
  - 'Daemon transport contracts live under uc-daemon::api and do not borrow JSON-RPC framing types.'
  - 'Daemon read models are built from CoreUseCases plus daemon-owned runtime snapshots, not from Tauri commands.'
requirements-completed: [PH45-01, PH45-02]
duration: 18min
completed: 2026-03-19
---

# Phase 45 Plan 01: Daemon API Contract And Auth Foundation Summary

**Daemon auth token persistence, loopback connection metadata, read-only HTTP and WebSocket DTOs, and a CoreRuntime-backed query service for status and device metadata**

## Performance

- **Duration:** 18 min
- **Started:** 2026-03-19T09:57:00Z
- **Completed:** 2026-03-19T10:14:54Z
- **Tasks:** 3
- **Files modified:** 10

## Accomplishments

- Added `uc_daemon::api` as the transport-facing boundary with auth, DTO, and query modules.
- Implemented daemon-local bearer token creation/loading, token-path helpers, and loopback connection metadata generation.
- Added read-only daemon DTOs plus a query service for `health`, `status`, `peers`, `paired-devices`, and pairing-session summaries.
- Refactored daemon runtime snapshots away from JSON-RPC worker DTOs so RPC and future HTTP routes map from the same internal state.

## Task Commits

Each task was committed atomically:

1. **Task 1: Create daemon API module, token persistence, and connection metadata types** - `0322f5de` (impl)
2. **Task 2: Define read-only DTOs and daemon query service for status, peers, paired devices, and pairing session summaries** - `fbc92d17` (impl)
3. **Task 3: Decouple RuntimeState from JSON-RPC transport types and add phase-local contract tests** - `d798a786` (refactor)

## Files Created/Modified

- `src-tauri/crates/uc-daemon/src/api/auth.rs` - daemon bearer-token persistence, parsing, and connection info helpers
- `src-tauri/crates/uc-daemon/src/api/types.rs` - transport DTOs for health, status, peers, paired devices, pairing sessions, and WebSocket events
- `src-tauri/crates/uc-daemon/src/api/query.rs` - CoreRuntime-backed read-only query service for daemon APIs
- `src-tauri/crates/uc-daemon/src/state.rs` - transport-neutral daemon runtime snapshots for workers and pairing sessions
- `src-tauri/crates/uc-daemon/src/rpc/handler.rs` - JSON-RPC status mapping from daemon-owned worker snapshots
- `src-tauri/crates/uc-daemon/tests/api_auth.rs` - auth contract coverage
- `src-tauri/crates/uc-daemon/tests/api_query.rs` - DTO/query contract coverage

## Decisions Made

- Used a dedicated daemon token file named `uniclipboard-daemon.token` and repaired Unix permissions to `0o600` when necessary.
- Serialized daemon transport DTOs in camelCase and reserved `type` / `sessionId` explicitly for WebSocket compatibility.
- Kept pairing-session reads daemon-owned and nullable rather than proxying into Tauri-owned state before Phase 46.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Reused a single CoreRuntime in `api_query` tests**

- **Found during:** Task 2
- **Issue:** Building multiple CLI runtimes concurrently in the same test binary caused SQLite/WAL initialization to fail with `database is locked`.
- **Fix:** Replaced per-test runtime construction with a shared `OnceLock<Arc<CoreRuntime>>` helper.
- **Files modified:** `src-tauri/crates/uc-daemon/tests/api_query.rs`
- **Verification:** `cargo test -p uc-daemon --test api_query` and `cargo test -p uc-daemon --quiet`
- **Committed in:** `d798a786`

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** The fix was required for deterministic verification and did not expand scope beyond the planned query contract.

## Issues Encountered

- Cargo verification was temporarily blocked by stale cargo test processes holding the build lock; clearing those processes allowed the final crate-wide verification run to complete.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `uc-daemon` now exposes stable auth and read-model foundations for the HTTP/WebSocket server work in Plan 45-02.
- RPC status and future HTTP status both derive from daemon-owned worker snapshots, so Plan 45-02 can add transport handlers without reworking runtime state again.

---

_Phase: 45-daemon-api-foundation-add-local-http-and-websocket-transport-with-read-only-runtime-queries_
_Completed: 2026-03-19_

## Self-Check: PASSED

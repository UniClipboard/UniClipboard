---
phase: 45-daemon-api-foundation-add-local-http-and-websocket-transport-with-read-only-runtime-queries
plan: 03
subsystem: api
tags: [daemon-api, reqwest, tauri, websocket, local-http]
requires:
  - phase: 45-02
    provides: daemon HTTP and WebSocket read-only endpoints plus local bearer auth
provides:
  - shared daemon HTTP client for `uc-cli` status and paired-device queries
  - Tauri-side daemon probe/start bootstrap
  - runtime-only `daemon://connection-info` event injection for the main webview
affects:
  [phase-46-daemon-pairing-host-migration, phase-47-frontend-daemon-cutover, cli, tauri-shell]
tech-stack:
  added: [reqwest]
  patterns:
    [
      shared daemon HTTP client in CLI,
      startup-time daemon probe/spawn helper in uc-tauri bootstrap,
      in-memory daemon connection state with post-page-load event delivery,
    ]
key-files:
  created: [src-tauri/crates/uc-cli/src/daemon_client.rs]
  modified:
    [
      src-tauri/crates/uc-cli/src/commands/status.rs,
      src-tauri/crates/uc-cli/src/commands/devices.rs,
      src-tauri/crates/uc-tauri/src/bootstrap/run.rs,
      src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs,
      src-tauri/crates/uc-tauri/src/commands/startup.rs,
      src-tauri/src/main.rs,
    ]
key-decisions:
  - 'CLI daemon reads reuse uc-daemon path/address helpers and treat a missing local token as daemon-unreachable so existing exit-code guarantees stay intact.'
  - 'Tauri stores daemon connection info only in memory and emits it through a single `daemon://connection-info` event after the main webview reaches `PageLoadEvent::Finished`.'
patterns-established:
  - 'CLI daemon reads: transport/auth concerns live in one shared client module, command files only map output and exit semantics.'
  - 'Daemon shell bootstrap: probe `/health`, spawn `uniclipboard-daemon` if needed, cache connection info in managed state, then emit after page readiness.'
requirements-completed: [PH45-05, PH45-06]
duration: 18min
completed: 2026-03-19
---

# Phase 45 Plan 03: CLI Migration And Tauri Daemon Bootstrap Summary

**Shared reqwest CLI daemon client plus Tauri daemon bootstrap and runtime-only connection-info event injection**

## Performance

- **Duration:** 18 min
- **Started:** 2026-03-19T11:14:15Z
- **Completed:** 2026-03-19T11:32:05Z
- **Tasks:** 2
- **Files modified:** 12

## Accomplishments

- Migrated `uc-cli status` and `uc-cli devices` off Unix socket/direct bootstrap reads onto a shared authenticated daemon HTTP client.
- Added Tauri-side daemon probe/spawn bootstrap that caches `DaemonConnectionInfo` in managed in-memory state.
- Delivered daemon connection metadata to the main webview through `daemon://connection-info` only after page readiness, without URL/query/localStorage persistence.

## Task Commits

Each task was committed atomically:

1. **Task 1: Create a shared daemon HTTP client in uc-cli and migrate status/devices to the new API** - `28051faf` (impl)
2. **Task 2: Add Tauri-side daemon startup/probing and runtime-only webview injection** - `b80ef87e` (impl)

## Files Created/Modified

- `src-tauri/crates/uc-cli/src/daemon_client.rs` - Shared daemon HTTP client with bearer auth, loopback URL resolution, and exit-semantic error mapping.
- `src-tauri/crates/uc-cli/src/commands/status.rs` - Status command now renders `/status` HTTP DTOs and preserves human/JSON output behavior.
- `src-tauri/crates/uc-cli/src/commands/devices.rs` - Devices command now renders `/paired-devices` HTTP DTOs and keeps paired-device listing output.
- `src-tauri/crates/uc-tauri/src/bootstrap/run.rs` - Daemon `/health` probe, child-process startup, connection info loading, and main-webview event emission helpers.
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` - Managed `DaemonConnectionState` in-memory holder for runtime-only daemon metadata.
- `src-tauri/crates/uc-tauri/src/commands/startup.rs` - Startup barrier now tracks frontend readiness and one-shot daemon connection delivery.
- `src-tauri/src/main.rs` - Tauri shell now manages daemon connection state, boots/probes daemon at startup, and emits `daemon://connection-info` after page load.

## Decisions Made

- Reused `uc-daemon` socket/address helpers from both CLI and Tauri paths so loopback URL and token-file resolution cannot drift from the daemon contract.
- Kept daemon bootstrap non-invasive to existing frontend business flows: `invoke_handler(...)` and current Tauri commands remain registered, while daemon metadata is delivered alongside them.
- Used a page-ready gate in `StartupBarrier` so the bearer token is emitted exactly through the runtime event path, not via JS string injection or browser storage.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Missing daemon token must still map to unreachable CLI semantics**

- **Found during:** Task 1 (Create a shared daemon HTTP client in uc-cli and migrate status/devices to the new API)
- **Issue:** Existing CLI smoke tests expected exit code `5` when the daemon is not running, but the first HTTP-client implementation returned exit code `1` if the local token file did not exist yet.
- **Fix:** Treated missing token-file reads as `DaemonClientError::Unreachable` so daemon-not-started scenarios preserve the existing CLI contract.
- **Files modified:** `src-tauri/crates/uc-cli/src/daemon_client.rs`, `src-tauri/crates/uc-cli/src/commands/status.rs`, `src-tauri/crates/uc-cli/src/commands/devices.rs`
- **Verification:** `cd src-tauri && cargo test -p uc-cli -- --test-threads=1`
- **Committed in:** `28051faf`

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** The auto-fix was required to preserve existing CLI behavior while switching transports. No scope creep.

## Issues Encountered

- `cargo fmt` reformatted unrelated `uc-core` and `uc-platform` files during task 2 verification. Those incidental changes were reverted before committing so the task commit stayed within planned scope.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 46 can reuse the managed daemon connection state and shell bootstrap path while moving more ownership into the daemon host.
- Phase 47 can consume `daemon://connection-info` from the main webview and cut UI reads over to daemon HTTP/WebSocket without introducing storage persistence for the bearer token.
- Manual follow-up still needed for the plan's desktop-shell checks: verify in devtools that the token never lands in `localStorage` or URL state and that the webview uses the emitted metadata for loopback WebSocket connection setup.

## Self-Check

PASSED

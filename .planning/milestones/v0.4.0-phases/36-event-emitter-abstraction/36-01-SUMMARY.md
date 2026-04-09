---
phase: 36-event-emitter-abstraction
plan: '01'
subsystem: infra
tags: [rust, tauri, event-emitter, ports-adapters, hexagonal-architecture, tracing]

# Dependency graph
requires: []
provides:
  - HostEventEmitterPort trait in uc-core (zero Tauri dependency)
  - HostEvent enum hierarchy covering clipboard, peer discovery, peer connection, and transfer domains
  - TauriEventEmitter adapter mapping 9 HostEvent variants to exact frontend wire contracts
  - LoggingEventEmitter adapter with structured tracing output (always returns Ok)
  - 9 event contract tests + 1 logging test verifying frontend JSON compatibility
affects:
  - 36-02 (AppRuntime integration depends on HostEventEmitterPort and TauriEventEmitter)
  - 36-03 (wiring.rs migration to HostEventEmitterPort replaces direct AppHandle.emit calls)

# Tech tracking
tech-stack:
  added: []
  patterns:
    - HostEvent as pure Rust semantic model with no serde annotations
    - TauriEventEmitter owning the serialization contract via internal module-private payload DTOs
    - LoggingEventEmitter as infallible drop-in for pre-AppHandle and non-GUI modes
    - Contract tests using tauri::test::mock_app() + tokio::sync::mpsc to assert JSON wire shapes

key-files:
  created:
    - src-tauri/crates/uc-core/src/ports/host_event_emitter.rs
    - src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs
  modified:
    - src-tauri/crates/uc-core/src/ports/mod.rs
    - src-tauri/crates/uc-tauri/src/adapters/mod.rs

key-decisions:
  - 'HostEventEmitterPort is synchronous (not async_trait) because tauri::Emitter::emit() is non-async and event delivery is fire-and-forget'
  - 'TauriEventEmitter holds AppHandle<R> by value (not Arc<RwLock<Option<...>>>) - constructed only when AppHandle exists'
  - 'PeerConnectionHostEvent uses Connected/Disconnected (not PeerReady/PeerNotReady) collapsing network states to frontend binary view'
  - 'Internal payload DTOs are module-private — TauriEventEmitter owns the wire contract, not uc-core'
  - 'ClipboardEventPayload uses #[serde(tag = type)] without rename_all to preserve snake_case fields matching existing frontend contract'

patterns-established:
  - 'Port/Adapter split: HostEvent pure Rust type in uc-core, serde DTOs in uc-tauri adapter'
  - 'Contract tests use TauriEventEmitter::new(app.handle().clone()) with MockRuntime generic (not hardcoded tauri::Wry)'
  - 'LoggingEventEmitter as infallible fallback adapter pattern for non-GUI runtime modes'

requirements-completed:
  - EVNT-01
  - EVNT-02
  - EVNT-03

# Metrics
duration: 25min
completed: 2026-03-17
---

# Phase 36 Plan 01: HostEventEmitterPort Trait and Adapters Summary

**HostEventEmitterPort trait in uc-core with TauriEventEmitter and LoggingEventEmitter adapters, 10 contract tests verifying exact frontend JSON wire shapes for 9 event types**

## Performance

- **Duration:** ~25 min
- **Started:** 2026-03-17T07:00:00Z
- **Completed:** 2026-03-17T07:25:00Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Defined HostEventEmitterPort trait + HostEvent enum hierarchy in uc-core with zero Tauri/serde dependency
- Implemented TauriEventEmitter that maps all 9 in-scope HostEvent variants to exact Tauri event name strings and JSON payloads matching current frontend wire contracts
- Implemented LoggingEventEmitter with structured tracing output, always returning Ok — ready for pre-AppHandle and non-GUI usage
- Added 9 event contract tests asserting JSON field casing and structure, plus 1 LoggingEventEmitter test

## Task Commits

Each task was committed atomically:

1. **Task 1: Define HostEventEmitterPort trait and HostEvent type system in uc-core** - `ea3b5a70` (arch)
2. **Task 2: Implement TauriEventEmitter and LoggingEventEmitter adapters with contract tests** - `572fb6c2` (impl)

## Files Created/Modified

- `src-tauri/crates/uc-core/src/ports/host_event_emitter.rs` - HostEventEmitterPort trait, HostEvent enum hierarchy (ClipboardHostEvent, PeerDiscoveryHostEvent, PeerConnectionHostEvent, TransferHostEvent), ClipboardOriginKind, EmitError
- `src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs` - TauriEventEmitter, LoggingEventEmitter, 9 internal payload DTOs, 10 tests
- `src-tauri/crates/uc-core/src/ports/mod.rs` - Added `pub mod host_event_emitter` and `pub use host_event_emitter::*` re-exports
- `src-tauri/crates/uc-tauri/src/adapters/mod.rs` - Added `pub mod host_event_emitter` and re-exports for TauriEventEmitter and LoggingEventEmitter

## Decisions Made

- **Synchronous emit**: HostEventEmitterPort uses `fn emit` not `async fn` because tauri::Emitter::emit() is non-async and event delivery is fire-and-forget
- **TauriEventEmitter holds AppHandle by value**: Simpler than Arc<RwLock<Option<...>>> — constructed only when AppHandle is available
- **PeerConnectionHostEvent collapses network states**: Connected/Disconnected map to the frontend's binary connected:bool view (not PeerReady/PeerNotReady)
- **Module-private payload DTOs**: Internal serialization types stay in the adapter file; uc-core stays free of serde
- **ClipboardEventPayload uses tagged enum without rename_all**: Preserves snake_case fields (entry_id, preview, origin) matching the existing frontend ClipboardEvent type in src/types/events.ts

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed TauriEventEmitter contract tests using hardcoded tauri::Wry type**

- **Found during:** Task 2 verification (cargo test -p uc-tauri host_event_emitter)
- **Issue:** make_emitter helper function had return type `TauriEventEmitter<tauri::Wry>` but tauri::test::mock_app() returns `App<MockRuntime>` — 9 type mismatch errors
- **Fix:** Removed make_emitter helper; replaced all call sites with `TauriEventEmitter::new(app.handle().clone())` which correctly infers `TauriEventEmitter<MockRuntime>` from context
- **Files modified:** src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs
- **Verification:** `cargo test -p uc-tauri host_event_emitter` — 10 passed
- **Committed in:** 572fb6c2 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 - Bug)
**Impact on plan:** Minor test infrastructure fix. No scope creep, no behavioral change.

## Issues Encountered

None beyond the auto-fixed type mismatch in test helpers.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- HostEventEmitterPort and both adapters are ready for wiring into AppRuntime
- Plan 36-02 can proceed: wire TauriEventEmitter into AppRuntime and replace direct AppHandle.emit call sites
- The port is free of Tauri dependency — uc-core crate remains infrastructure-agnostic

---

_Phase: 36-event-emitter-abstraction_
_Completed: 2026-03-17_

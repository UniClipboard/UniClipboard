---
phase: 37-wiring-decomposition
plan: 01
subsystem: infra
tags: [rust, tauri, event-emitter, hexagonal-architecture, domain-events]

requires:
  - phase: 36-event-emitter-abstraction
    provides: HostEvent enum with Clipboard/PeerDiscovery/PeerConnection/Transfer arms, HostEventEmitterPort trait, TauriEventEmitter/LoggingEventEmitter adapters
provides:
  - PairingHostEvent sub-enum with Verification/SubscribeFailure/SubscribeRecovered variants
  - SetupHostEvent sub-enum with StateChanged variant
  - SpaceAccessHostEvent sub-enum with Completed/P2PCompleted variants
  - PairingVerificationKind enum for pairing flow stages
  - Extended TauriEventEmitter with payload DTOs and match arms for all new variants
  - Extended LoggingEventEmitter with structured tracing for all new variants
  - 17 contract tests verifying exact event names and payload field casing
affects: [37-02-PLAN, 37-03-PLAN, wiring-decomposition]

tech-stack:
  added: []
  patterns: [sub-enum domain events with per-category HostEvent variants]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-core/src/ports/host_event_emitter.rs
    - src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs

key-decisions:
  - 'PairingVerificationKind as separate enum (not String) for type safety on kind field'
  - 'SetupStateChangedPayload carries full SetupState enum (not String) preserving data-carrying variants like JoinSpaceConfirmPeer'
  - 'SpaceAccessCompletedPayload.peer_id is String (non-optional) matching existing wire contract'
  - "InboundSubscribeError and InboundSubscribeRetry added as ClipboardHostEvent variants (not separate sub-enum) since they're clipboard-scoped"

patterns-established:
  - 'Sub-enum pattern: each domain area gets its own HostEvent variant wrapping a dedicated sub-enum'
  - 'Contract test pattern: Tauri mock app + channel listener verifying exact JSON field names and casing'

requirements-completed: []

duration: 25min
completed: 2026-03-17
---

# Plan 37-01: HostEvent Sub-enums & Emitter Extensions Summary

**PairingHostEvent, SetupHostEvent, SpaceAccessHostEvent sub-enums with TauriEventEmitter/LoggingEventEmitter match arms and 17 contract tests**

## Performance

- **Duration:** ~25 min
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- Added PairingHostEvent (Verification, SubscribeFailure, SubscribeRecovered), SetupHostEvent (StateChanged), SpaceAccessHostEvent (Completed, P2PCompleted) sub-enums to uc-core
- Extended TauriEventEmitter with payload DTOs (PairingVerificationPayload, PairingSubscribeFailurePayload, PairingSubscribeRecoveredPayload, SetupStateChangedPayload, SpaceAccessCompletedPayload) and match arms
- Extended LoggingEventEmitter with structured tracing fields for all new event variants
- Also added ClipboardHostEvent::InboundSubscribeError and InboundSubscribeRetry variants
- 17 contract tests (10 existing + 7 new) all passing, verifying exact event names and camelCase/snake_case field casing

## Task Commits

1. **Task 1: Add sub-enums to HostEvent in uc-core** - `4b5750b3` (arch)
2. **Task 2: Extend TauriEventEmitter & LoggingEventEmitter** - `21940aca` (impl)

## Files Created/Modified

- `src-tauri/crates/uc-core/src/ports/host_event_emitter.rs` - Added PairingHostEvent, PairingVerificationKind, SetupHostEvent, SpaceAccessHostEvent enums + InboundSubscribeError/InboundSubscribeRetry clipboard variants
- `src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs` - Payload DTOs, map_event_to_json arms, LoggingEventEmitter arms, 7 new contract tests

## Decisions Made

- PairingVerificationKind uses typed enum variants (Request/Verification/Verifying/Complete/Failed) with kind_to_str helper
- SetupState is Serialized as full object (not string) to preserve data-carrying variants
- SpaceAccessCompletedPayload.peer_id is non-optional String matching existing wire contract

## Deviations from Plan

None - plan executed as specified.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- All new HostEvent variants are ready for Plan 37-02 to migrate app.emit() calls in wiring.rs to use HostEventEmitterPort
- Compilation passes with 0 errors, all 22 host_event_emitter tests pass

---

_Phase: 37-wiring-decomposition_
_Completed: 2026-03-17_

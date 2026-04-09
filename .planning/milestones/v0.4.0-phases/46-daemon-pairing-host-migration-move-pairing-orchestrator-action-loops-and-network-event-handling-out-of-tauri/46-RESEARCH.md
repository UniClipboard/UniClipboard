# Phase 46: Daemon Pairing Host Migration - Research

**Researched:** 2026-03-19
**Domain:** Daemon-owned pairing host migration with Tauri compatibility bridge
**Confidence:** HIGH

## User Constraints

- `46-CONTEXT.md` locks the phase boundary: pairing session ownership, pairing action/event loops, and pairing-related network event handling must leave `uc-tauri` and become daemon-owned.
- Phase 46 must preserve the current desktop frontend contract and five-stage pairing UX semantics; direct frontend cutover to daemon HTTP/WebSocket is explicitly deferred to Phase 47.
- Verification codes and fingerprints must stay out of normal session read models and remain available only through authenticated realtime delivery.
- Daemon must keep sessions alive across Tauri/webview disconnects, but the current desktop shell must not auto-resume interrupted sessions in this phase.
- Only one active pairing session is allowed globally. If no local GUI/CLI participant is ready, inbound pairing must be rejected or marked busy rather than parked.

## Summary

The codebase is structurally ready for this migration, but the current host split is still wrong for the Phase 46 goal. `PairingOrchestrator` already lives in `uc-app` and is Tauri-free, yet the actual long-lived host responsibilities remain inside `uc-tauri::bootstrap::wiring`: Tauri owns the pairing action loop, owns the pairing network event subscription/retry loop, emits the frontend-facing pairing events, and directly wires pairing flow side effects into setup/space-access behavior. As a result, pairing session lifetime is effectively tied to the Tauri host process, not to the daemon runtime.

Phase 45 created the transport foundation needed to fix that: `uc-daemon` already has loopback HTTP/WebSocket transport, bearer-token auth, snapshot-first topic subscriptions, and a `RuntimeState` slot for daemon-owned pairing session summaries. What is still missing is the daemon-side pairing host itself: a worker or host service that owns one `PairingOrchestrator`, processes pairing actions, subscribes to pairing network events, projects session summaries into daemon state, and fans out safe realtime updates.

The critical planning implication is that Phase 46 should not be framed as “move a few functions out of `wiring.rs`.” The real unit of work is replacing Tauri as the business host for pairing while keeping Tauri as a compatibility adapter. That means the phase needs three distinct slices:

1. A daemon-owned pairing host and session projection layer.
2. A daemon-owned write/realtime contract for pairing control and updates.
3. A Tauri compatibility bridge that forwards current commands/events to the daemon without cutting the frontend over yet.

If those slices are mixed, the work will either silently absorb Phase 47 or leave pairing lifetime coupled to the desktop shell.

## Current Implementation Map

### What already exists and is reusable

- `src-tauri/crates/uc-app/src/usecases/pairing/orchestrator.rs`
  - `PairingOrchestrator` is already Tauri-free.
  - It exposes mutation methods (`initiate_pairing`, `user_accept_pairing`, `user_reject_pairing`, transport handlers) plus `PairingEventPort::subscribe()`.
  - It already emits domain events for verification, success, failure, and keyslot receipt.
- `src-tauri/crates/uc-daemon/src/app.rs`
  - `DaemonApp` already owns lifecycle, startup, cancellation, and shared runtime state.
  - The daemon already starts HTTP and JSON-RPC transport side-by-side and has a natural place to start a pairing host worker.
- `src-tauri/crates/uc-daemon/src/state.rs`
  - `RuntimeState` already reserves `DaemonPairingSessionSnapshot` storage.
  - That gives Phase 46 a daemon-local read-model landing zone without having to invent new state ownership primitives.
- `src-tauri/crates/uc-daemon/src/api/routes.rs`, `query.rs`, `ws.rs`, `types.rs`
  - Phase 45 already established authenticated HTTP/WS plumbing, snapshot-first websocket semantics, and metadata-only pairing session summaries.
- `src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs`
  - Setup flow already consumes `PairingEventPort` semantics and assumes pairing-domain events arrive quickly enough to drive the setup state machine.

### What is still coupled to Tauri

- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`
  - `start_background_tasks()` spawns `pairing_action` and `pairing_events` long-lived tasks.
  - `run_pairing_action_loop()` owns pairing transport open/send/close behavior and emits frontend-facing pairing verification/completion/failure events.
  - `run_pairing_event_loop()` owns network event subscription retry/recovery and dispatches incoming pairing messages into the orchestrator.
  - `handle_pairing_message()` still maps inbound network events to orchestrator actions, setup triggers, and host-event emissions inside Tauri.
- `src-tauri/crates/uc-tauri/src/commands/pairing.rs`
  - Tauri commands still receive `State<Arc<PairingOrchestrator>>` directly and treat Tauri as the mutation surface for initiate/accept/reject/verify.
- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs`
  - Non-GUI runtime currently uses `LoggingHostEventEmitter`, which is intentionally non-interactive and only logs event type names.
  - That is acceptable for Phase 45 read-only APIs, but insufficient for daemon-owned pairing because Phase 46 needs realtime projection/fanout, not just logging.

## Architecture Findings

### Finding 1: Pairing host ownership is determined by who owns the action loop, not by where `PairingOrchestrator` is defined

`PairingOrchestrator` being in `uc-app` is necessary but not sufficient. Today, Tauri still owns:

- the action receiver
- the transport session open/send/close lifecycle
- the retrying network subscription loop
- the projection from domain/network events into frontend-visible events

That means Phase 46 must migrate the action loop and network event loop together. Moving only command entrypoints would leave the host in the wrong place.

### Finding 2: Daemon already has enough transport foundation; it does not yet have a pairing runtime host

`uc-daemon` can already:

- authenticate local clients
- expose read-only session summaries
- push snapshot-first websocket events

It cannot yet:

- own a `PairingOrchestrator`
- execute pairing write actions
- subscribe to pairing network events as the source of truth
- track discovery visibility separately from pairing readiness
- track participant readiness
- emit pairing incremental updates from daemon-owned state

This is the main missing runtime capability for the phase.

### Finding 3: Phase 46 needs separate discoverability and participant-readiness controls

The context and current product behavior imply two different gates:

- `discoverable`: whether other devices should see this host in peer discovery at all
- `participant_ready`: whether a local GUI/CLI user is actively available to handle pairing steps

Those controls must not collapse into one flag. For CLI-driven usage, the daemon should stay non-discoverable until the user explicitly enters pairing mode. For GUI-driven usage, the host may stay discoverable by default because the GUI is the intended human-facing participant.

This means Phase 46 cannot stop at "reject inbound request when nobody is ready." If the host is headless and no CLI pairing mode is active, the better behavior is to keep it out of discovery visibility in the first place. Busy/reject is still needed, but it is the downstream fallback after a host is intentionally discoverable.

### Finding 4: Phase 46 needs a local participant-readiness mechanism, not autonomous daemon pairing

The context explicitly forbids daemon-autonomous completion. The daemon must reject or busy an inbound request when there is no local participant already engaged in the flow.

That means the daemon pairing host needs one explicit readiness model, such as:

- an in-memory registration/lease tied to an authenticated Tauri/CLI client connection
- a daemon websocket subscriber that explicitly declares pairing participation
- a short-lived readiness token renewed by the compatibility bridge

What must not happen is implicit “daemon will hold the request until someone opens the UI later.” That would violate the locked decision and hide lifecycle bugs.

### Finding 5: Tauri still has to translate daemon events back into the current frontend contract

Phase 46 does not permit direct frontend cutover. Therefore Tauri must keep:

- current invoke command names
- current pairing notification semantics
- current five-stage event mapping: `request`, `verification`, `verifying`, `complete`, `failed`

But Tauri should no longer be the business host. It should become:

- a daemon client for pairing mutations
- a daemon websocket consumer for pairing/discovery updates
- a translator that re-emits existing frontend-facing event names and payload shapes

### Finding 6: Setup flow compatibility is a hard constraint, not a nice-to-have

`SetupActionExecutor` already subscribes to pairing domain events and relies on them for `EnsurePairing`, trust confirmation, and join-space progression. If Phase 46 migrates pairing host ownership but does not preserve those semantics through the bridge, setup will regress even if the standalone pairing dialog still works.

This is why the plan must include setup-oriented regression coverage, not just pairing happy-path coverage.

## Recommended Architecture

### Target Runtime Shape

```text
daemon
├── owns PairingOrchestrator
├── owns pairing action loop
├── owns pairing network event subscription/retry loop
├── owns discovery-visibility gate + active-session gate + participant-readiness gate
├── projects metadata-only session summaries into RuntimeState
└── fans out authenticated realtime pairing updates

tauri
├── forwards pairing write commands to daemon
├── subscribes to daemon pairing/discovery topics
├── maps daemon updates into existing frontend event names/payloads
└── no longer owns pairing business lifetime
```

### Preferred implementation approach

1. Add a daemon-local pairing host module instead of trying to reuse `uc-tauri::bootstrap::wiring` directly.
2. Keep `PairingOrchestrator` in `uc-app`, but instantiate and drive it from the daemon host.
3. Model daemon host mode explicitly:
   - non-discoverable by default for daemon-only / ordinary CLI usage
   - explicit CLI opt-in to become discoverable and pairing-ready
   - GUI-managed default discoverability for the desktop shell
4. Use daemon-owned session projections:
   - metadata-only summaries in `RuntimeState`
   - separate authenticated realtime events for short codes/fingerprints
5. Keep Tauri command shapes stable for Phase 46, but make them daemon clients internally.
6. Keep websocket snapshot-first behavior and extend it for pairing updates instead of inventing a separate polling path.

## Requirement Recommendations

`ROADMAP.md` still marks Phase 46 requirements as `TBD`, so the plan should use explicit derived IDs:

- `PH46-01`: Daemon becomes the single owner of pairing session lifecycle and enforces one active session globally.
- `PH46-01A`: Daemon distinguishes discovery visibility from pairing participant readiness instead of treating them as one flag.
- `PH46-01B`: Daemon-only and ordinary CLI modes are non-discoverable by default until a user explicitly enters pairing mode.
- `PH46-02`: Pairing action execution and pairing-related network event handling move out of `uc-tauri` and stay alive across Tauri/webview disconnects until timeout or terminal result.
- `PH46-03`: Daemon exposes the pairing write/control surface needed by the current desktop flow, with `peerId` only for initiate and `sessionId` for follow-up actions, plus explicit cancel.
- `PH46-03A`: CLI pairing mode explicitly enables discovery visibility and pairing participation; without that opt-in the daemon must stay out of discovery results.
- `PH46-04`: General pairing read models remain metadata-only; verification codes/fingerprints are delivered only through authenticated realtime updates.
- `PH46-05`: Tauri preserves the current desktop command/event contract as a compatibility bridge without introducing the Phase 47 frontend API cutover.
- `PH46-05A`: GUI-hosted daemon remains discoverable by default and registers the desktop shell as the local participant for pairing in Phase 46.
- `PH46-06`: Test coverage proves busy/no-participant rejection, continuity across Tauri disconnect, and setup-flow compatibility.

## Recommended Plan Split

### Plan 46-01: Daemon Pairing Host And Session Projection

Focus:

- instantiate daemon-owned pairing host runtime
- move action loop and network event subscription out of `uc-tauri`
- add daemon-owned discovery-visibility state and default non-discoverable headless behavior
- enforce active-session and participant-readiness gates
- project daemon-owned session summaries into `RuntimeState`

Why first:

- everything else depends on daemon becoming the source of truth
- this isolates the host migration from transport and compatibility concerns

### Plan 46-02: Daemon Pairing Control Surface And Realtime Contract

Focus:

- add daemon pairing mutation endpoints or equivalent daemon control surface
- preserve immediate-ack command semantics
- add pairing incremental websocket events plus distinct discoverability/readiness registration semantics
- keep normal read models metadata-only

Why second:

- once daemon owns the host, Tauri and CLI need a supported client surface
- realtime contract must exist before the Tauri compatibility bridge can consume it

### Plan 46-03: Tauri Compatibility Bridge For Existing Frontend Contract

Focus:

- refactor Tauri commands into daemon clients
- subscribe to daemon pairing/discovery topics
- translate daemon events into current frontend event names/payloads
- remove Tauri-owned pairing background loops from `wiring.rs`

Why third:

- this keeps frontend behavior stable while swapping the backend host
- it avoids mixing daemon-host migration with the later direct frontend cutover

## Validation Architecture

Phase 46 needs both daemon-level and Tauri-bridge regression coverage. The minimum practical validation stack is:

- daemon unit/integration tests for host ownership, session projection, busy gating, and websocket pairing events
- Tauri bridge tests for preserved event names/payloads and command forwarding semantics
- manual desktop verification for disconnect continuity and secret-boundary checks

Recommended verification commands:

- quick daemon host checks:
  - `cd src-tauri && cargo test -p uc-daemon --test pairing_host -- --test-threads=1`
- quick daemon API/WS checks:
  - `cd src-tauri && cargo test -p uc-daemon --test pairing_api --test pairing_ws -- --test-threads=1`
- quick Tauri bridge checks:
  - `cd src-tauri && cargo test -p uc-tauri --test pairing_bridge -- --test-threads=1`
- full suite:
  - `cd src-tauri && cargo test -p uc-daemon -p uc-tauri -- --test-threads=1`

Wave 0 gaps to assume in planning:

- `src-tauri/crates/uc-daemon/tests/pairing_host.rs`
- `src-tauri/crates/uc-daemon/tests/pairing_api.rs`
- `src-tauri/crates/uc-daemon/tests/pairing_ws.rs`
- `src-tauri/crates/uc-tauri/tests/pairing_bridge.rs`

Manual validation still required for:

- session surviving Tauri/webview restart without auto-resume UI
- verification codes/fingerprints absent from `/pairing/sessions/{sessionId}` and present only on authenticated realtime path
- setup flow still transitioning through the expected stages after the daemon-host migration

## Risks And Mitigations

### Risk 1: Re-embedding business ownership in a daemon client helper instead of a daemon host

Mitigation:

- require a daemon-local host module with lifecycle startup in `DaemonApp`
- reject plans that merely move logic from Tauri command handlers into a reusable Tauri-side helper

### Risk 2: Secret leakage through session summaries or snapshot events

Mitigation:

- keep `RuntimeState` pairing summaries metadata-only
- use dedicated authenticated realtime payloads for verification details
- add serialization tests that assert absence of short codes, fingerprints, `KeySlotFile`, and raw challenge bytes

### Risk 3: Setup flow breaks because bridge emits only pairing-dialog events

Mitigation:

- require Tauri bridge tests that drive setup-related pairing transitions
- keep the existing semantic event mapping intact until Phase 47 removes the compatibility layer

### Risk 4: Inbound requests get parked without an active participant

Mitigation:

- make participant readiness explicit in daemon state
- add tests for “busy/no local participant ready” before implementing any reconnect-resume story

### Risk 5: Phase 46 quietly absorbs Phase 47

Mitigation:

- keep frontend HTTP/WebSocket cutover explicitly out of scope
- preserve current Tauri invoke/event contract in plan acceptance criteria

## Planning Guidance

- Prefer daemon-host composition over cross-crate abstraction churn. The repo already has the right app-layer orchestrator; the host is what must move.
- Keep phase tasks concrete about which files stop owning pairing loops. `wiring.rs` should lose `run_pairing_action_loop()` and `run_pairing_event_loop()` ownership by the end of the phase.
- Do not make `RuntimeState` or general websocket snapshots carry verification secrets.
- Do not let direct Tauri-to-frontend event preservation justify keeping session lifetime in Tauri.
- Explicitly test the no-participant path, because that is the easiest locked decision to accidentally violate.

---

_Phase: 46-daemon-pairing-host-migration-move-pairing-orchestrator-action-loops-and-network-event-handling-out-of-tauri_
_Researched: 2026-03-19_

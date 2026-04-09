# Phase 46: Daemon Pairing Host Migration - Context

**Gathered:** 2026-03-19
**Status:** Ready for planning

<domain>
## Phase Boundary

Move pairing session ownership, pairing action/event loops, and pairing-related network event handling out of `uc-tauri` and into the daemon-hosted runtime so Tauri stops being the business host for pairing.

This phase is about changing the host of the pairing flow, not about cutting the frontend directly over to daemon APIs yet.

**In scope:**

- Daemon-owned pairing session lifecycle
- Daemon-owned pairing write/control surface
- Daemon-owned pairing-related network event handling
- Compatibility bridging so the current desktop frontend contract keeps working in Phase 46
- Preserving existing pairing/setup UX semantics while changing the backend host

**Out of scope:**

- Direct frontend cutover to daemon HTTP/WebSocket APIs
- User-facing recovery UI for interrupted pairing sessions
- Multi-session queueing or parallel pairing UX
- Admin/operator-only force-close controls

</domain>

<decisions>
## Implementation Decisions

### Daemon pairing write/control surface

- Phase 46 should go beyond minimal GUI parity and cover the daemon-hosted control surface up to:
  - current pairing actions needed by the desktop flow
  - explicit cancel
  - daemon-side continuity / recoverability semantics
- This phase does **not** need broader admin-style controls such as force-close.
- `initiate` starts from `peerId`; every subsequent write/control action is addressed by `sessionId`.
- Pairing write calls should acknowledge immediately rather than blocking for final outcome.
- Clients learn progress and terminal outcome through realtime events and session updates, not through long-blocking mutation calls.

### Verification data exposure

- Short codes and fingerprints remain restricted to authenticated realtime delivery.
- Normal session read models should stay metadata-oriented and must not expose verification codes or fingerprints.
- Phase 46 should preserve the Phase 45 sensitivity boundary of not leaking raw keyslot files, raw challenge bytes, or other session internals into general read models.

### Disconnect and reconnect behavior

- Once pairing becomes daemon-hosted, a Tauri/webview disconnect must **not** kill the session immediately.
- Daemon-owned pairing sessions stay alive until their normal timeout or terminal result.
- Human-confirmation steps continue using the normal timeout window; daemon does not pause timers just because the current GUI disconnected.
- In Phase 46, "recovery" means daemon-side continuity only.
- The current desktop shell should not auto-resume interrupted pairing sessions in this phase.
- The current desktop shell also does not need an explicit recovery UI in this phase.

### Admission, concurrency, and local participation

- Phase 46 must distinguish **discovery visibility** from **pairing participation readiness**. They are related but not interchangeable controls.
- A daemon-only runtime is **not discoverable by default**. If no local user has explicitly entered pairing mode, the host should stay out of peer discovery results rather than relying on late busy/reject responses.
- Ordinary CLI usage does **not** imply pairing availability. CLI must explicitly opt into pairing mode before the daemon becomes discoverable or accepts pairing work.
- GUI-hosted daemon remains discoverable by default because the GUI shell is the user-facing participant for pairing in this phase.
- Only one active pairing session is allowed globally at a time.
- If a second pairing request arrives while one is active, daemon should respond busy/reject rather than queueing or replacing the active session.
- If a user tries to initiate a new pairing while another session is active, daemon should block the new initiation and report that an active session already exists.
- Daemon must not "complete pairing by itself" without a local participant handling human steps.
- Non-discoverable hosts should block inbound pairing **before session creation** by staying out of the visible discovery set.
- If a new inbound request arrives and there is no local GUI or CLI already participating in the pairing flow, daemon should reject/busy that request rather than parking it for later claim.

### Client event contract for Phase 46

- Existing desktop frontend event contract should remain intact for this phase.
- Pairing realtime semantics should continue to map to the existing five-stage flow:
  - `request`
  - `verification`
  - `verifying`
  - `complete`
  - `failed`
- Peer/discovery updates should use "subscription snapshot first, then fine-grained incremental changes" rather than polling-only or full-table replace on every change.
- Tauri remains the compatibility bridge in Phase 46:
  - daemon becomes the pairing host
  - Tauri translates or forwards daemon events into the current frontend-facing contract
  - direct frontend cutover is deferred to Phase 47

### Claude's Discretion

- Exact HTTP/WS route names and message envelopes for pairing mutation endpoints
- Exact daemon-side state model for `discoverable` vs `participant_ready`, as long as the two concerns remain distinct and observable
- Exact error code taxonomy for "active session exists", "no local client ready", and "busy"
- Exact daemon session summary fields beyond the locked metadata boundary
- Exact Tauri-side bridge implementation that maps daemon events into the current frontend event names/payloads
- Exact representation of local GUI/CLI "participating in pairing flow" readiness

</decisions>

<specifics>
## Specific Ideas

- The daemon is the business host for pairing after this phase, but it is **not** allowed to act as a fully autonomous human decision-maker.
- A host that is not supposed to accept pairing should ideally be invisible upstream, not merely visible and then rejected downstream.
- For CLI-driven pairing, entering pairing mode is an explicit user action that simultaneously enables discovery visibility and local participation for a limited window.
- A local client still needs to handle the human-facing steps of pairing.
- Phase 46 should not silently absorb the frontend API cutover planned for Phase 47.
- Compatibility for the current desktop UI matters more than exposing a polished new daemon-native frontend contract in this phase.

</specifics>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project and roadmap constraints

- `.planning/ROADMAP.md` — Phase 46/47/48 boundary and dependency chain
- `.planning/PROJECT.md` — v0.4.0 runtime-mode-separation goal and daemon-as-business-host direction
- `.planning/REQUIREMENTS.md` — current runtime-separation constraints that Phase 46 must extend without breaking
- `.planning/STATE.md` — latest locked decisions from Phases 36/38/41/45 and current project position

### Prior phase context

- `.planning/phases/45-daemon-api-foundation-add-local-http-and-websocket-transport-with-read-only-runtime-queries/45-CONTEXT.md` — daemon auth, snapshot-first websocket, pairing metadata boundary, Tauri bootstrap role
- `.planning/phases/41-daemon-and-cli-skeletons/41-CONTEXT.md` — daemon runtime shape, worker ownership, local transport baseline
- `.planning/phases/38-coreruntime-extraction/38-CONTEXT.md` — CoreRuntime extraction and non-Tauri runtime ownership patterns
- `.planning/phases/43-unify-gui-and-cli-business-flows-to-eliminate-per-entrypoint-feature-adaptation/43-RESEARCH.md` — pairing should follow shared app-layer facade rather than command-local orchestration

### Current pairing host implementation

- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — current pairing action loop, pairing event loop, and pairing-related network event handling that must leave Tauri
- `src-tauri/crates/uc-tauri/src/commands/pairing.rs` — current Tauri pairing command surface and client-visible mutation semantics
- `src-tauri/src/main.rs` — current Tauri state registration and background task startup

### Current daemon transport/runtime foundation

- `src-tauri/crates/uc-daemon/src/app.rs` — daemon lifecycle host and worker startup/shutdown model
- `src-tauri/crates/uc-daemon/src/api/routes.rs` — existing daemon HTTP route surface
- `src-tauri/crates/uc-daemon/src/api/ws.rs` — current websocket subscription model and snapshot-first behavior
- `src-tauri/crates/uc-daemon/src/api/query.rs` — existing daemon query service patterns
- `src-tauri/crates/uc-daemon/src/state.rs` — daemon runtime snapshot state including reserved pairing session summaries
- `src-tauri/crates/uc-daemon/src/main.rs` — current daemon entrypoint and non-GUI runtime construction
- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` — current non-GUI runtime helper and its placeholder setup wiring limitations

### Shared application-layer pairing logic

- `src-tauri/crates/uc-app/src/usecases/pairing/orchestrator.rs` — Tauri-free pairing orchestrator and session/event subscriptions
- `src-tauri/crates/uc-app/src/usecases/pairing/events.rs` — app-layer pairing domain event contracts
- `src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs` — current setup flow dependence on pairing domain events

### Current frontend compatibility target

- `src/api/p2p.ts` — current frontend pairing/discovery API contract and Tauri event names
- `src/hooks/useDeviceDiscovery.ts` — current discovery/connection/name incremental-consumption model
- `src/components/PairingNotificationProvider.tsx` — current inbound-request and verification event behavior
- `src/components/PairingDialog.tsx` — current initiator-side pairing flow expectations

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `PairingOrchestrator` already exists in `uc-app` and is Tauri-free; it exposes both mutation methods and subscription-based domain events.
- `uc-daemon` already has HTTP/WebSocket auth, snapshot state, and query-service scaffolding from Phase 45.
- `RuntimeState` in `uc-daemon` already reserves daemon-owned pairing session summaries, which is the natural landing spot for daemon-side pairing visibility.
- Existing frontend TypeScript contracts and Tauri event adapters already define the compatibility target for Phase 46.

### Established Patterns

- Long-lived business ownership should move out of `uc-tauri`; Tauri stays as shell/bridge when the frontend contract must be preserved.
- Session-scoped follow-up operations should use `sessionId` once a flow has started.
- Long-running flows should acknowledge writes quickly and stream progress asynchronously.
- Snapshot-first subscription plus incremental updates is already an accepted transport model in the daemon foundation.

### Integration Points

- The daemon should become the owner of pairing session state, pairing transport subscription, and pairing action execution.
- Discovery visibility must be modeled separately from participant readiness so headless daemon and ordinary CLI usage stay non-discoverable by default.
- Tauri pairing commands should shrink into a daemon client/compatibility adapter instead of directly owning orchestrator state.
- Current setup and pairing UI/event consumers must keep receiving the semantic events they already understand, even if the source of truth moves into the daemon.
- Planner/researcher should pay special attention to both how "local GUI/CLI is participating" is detected and how discovery visibility is enabled, because those decisions separately gate visibility and inbound-request acceptance.

</code_context>

<deferred>
## Deferred Ideas

- Direct frontend cutover to daemon HTTP/WebSocket pairing APIs — Phase 47
- User-visible recovery / reclaim UI for interrupted sessions
- Multi-session queueing or parallel pairing UX
- Force-close / operator-only pairing controls

</deferred>

---

_Phase: 46-daemon-pairing-host-migration-move-pairing-orchestrator-action-loops-and-network-event-handling-out-of-tauri_
_Context gathered: 2026-03-19_

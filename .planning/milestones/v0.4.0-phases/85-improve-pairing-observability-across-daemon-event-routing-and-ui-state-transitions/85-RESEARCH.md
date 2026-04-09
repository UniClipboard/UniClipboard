# Phase 85: Improve pairing observability across daemon, event routing, and UI state transitions - Research

**Researched:** 2026-04-02
**Domain:** Pairing flow observability across daemon emitters, realtime routing, and GUI state consumers
**Confidence:** MEDIUM

## User Constraints

- No `CONTEXT.md` exists for this phase.
- Scope must be inferred from:
  - `.planning/ROADMAP.md`
  - pairing debug docs under `.planning/debug/`
  - current source code and current tests
- Focus on current-state research, not implementation planning details.
- Determine what observability gaps remain after previous race-condition fixes.
- Do not create `PLAN.md` files.

## Project Constraints (from CLAUDE.md)

- Use the existing stack and current repo structure; do not introduce parallel architectural paths.
- All Rust-related commands must run from `src-tauri/`.
- Use `tracing` for backend logging, with structured fields and spans rather than ad-hoc text logs.
- Prefer spans for operations with duration and events for discrete state changes.
- Pairing/setup work must respect hexagonal boundaries: `uc-app -> uc-core <- uc-infra / uc-platform`.
- Repository docs must use repo-relative paths only.
- Markdown fenced code blocks must include a language identifier.
- Validation is required before reporting results; do not assume code is correct without running checks where feasible.

<phase_requirements>

## Phase Requirements

| ID         | Description                                                                                                                                                                              | Research Support                                                                                                                                                                     |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| P85-OBS-01 | A single pairing attempt can be followed across daemon mutation, daemon event emission, websocket forwarding, frontend receipt, and UI state transition using stable correlation fields. | Current code already has `session_id` at most backend/realtime boundaries, but trace propagation stops before daemon HTTP and frontend consumers do not emit structured breadcrumbs. |
| P85-OBS-02 | Every pairing/setup state transition produces structured, queryable logs with enough context to explain why the UI advanced, stayed put, or ignored an event.                            | Current daemon logs cover some emit points; frontend consumers mostly stay silent on receive, filter, ignore, or transition decisions.                                               |
| P85-OBS-03 | Event-routing failures become diagnosable without dual-device guesswork.                                                                                                                 | `DaemonWsBridge` logs decode failures, but frontend hook/store layers do not log unsupported payloads, session mismatches, or dropped transitions.                                   |
| P85-OBS-04 | Validation coverage proves observability at the same boundaries the runtime uses now, not through stale pre-refactor test paths.                                                         | Several current tests are stale or failing for harness reasons, so the phase must repair verification first or in parallel.                                                          |
| P85-OBS-05 | Phase scope does not change pairing business behavior or protocol semantics.                                                                                                             | Existing race-condition fixes are already present; this phase should harden visibility and debugging, not re-open pairing protocol logic.                                            |

</phase_requirements>

## Summary

The two historically diagnosed race bugs are no longer the primary missing work. The setup-side lost-subscription bug is structurally fixed in `src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs`: the setup flow now subscribes to pairing events before initiating pairing. The responder-side verification prompt race is also addressed in `src/components/PairingNotificationProvider.tsx`: the accept action writes `activeSessionIdRef.current` synchronously before the async React state commit. In other words, Phase 85 should not be framed as another race-condition repair phase.

The current problem is visibility. The daemon emits useful pairing logs with `session_id` and `peer_id`, and the daemon websocket bridge logs payload decode on the Rust side, but the correlation breaks at the frontend HTTP boundary and again at the UI-consumer boundary. `traceManager` only propagates through Tauri `invokeWithTrace`; pairing and setup now use `daemonClient` fetch calls, which do not send any trace metadata. On the frontend side, `usePairingEvents`, `setupRealtimeStore`, and `PairingNotificationProvider` mostly either mutate UI state silently or drop events silently. When the UI stalls, there is no authoritative timeline explaining whether the daemon failed to emit, the bridge failed to decode, the hook filtered by session, or the UI remained in a waiting phase because `setup.spaceAccessCompleted` never arrived.

**Primary recommendation:** Plan Phase 85 as an observability-and-validation hardening phase with three goals: add end-to-end correlation, instrument every decision boundary that can drop or delay state, and repair stale tests so the new visibility is verifiable.

## Standard Stack

### Core

| Library / Module                                     | Version    | Purpose                                                        | Why Standard                                                                    |
| ---------------------------------------------------- | ---------- | -------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `tracing` in Rust backend                            | workspace  | Structured daemon and bridge logs                              | Already the backend logging standard; pairing host and WS bridge already use it |
| `src-tauri/crates/uc-daemon/src/pairing/host.rs`     | repo-local | Canonical daemon pairing/session event emission                | This is the current source of truth for pairing websocket events                |
| `src-tauri/crates/uc-daemon-client/src/ws_bridge.rs` | repo-local | Canonical Rust-side realtime mapping and backpressure boundary | This is the bridge between daemon websocket envelopes and typed realtime events |
| `src/hooks/useDaemonEvents.ts`                       | repo-local | Canonical frontend subscription entry for pairing events       | Phase 83 made this the intended frontend pairing event path                     |
| `src/store/setupRealtimeStore.ts`                    | repo-local | Canonical setup-state realtime store                           | Setup flow UI derives from this store, not direct pairing callbacks             |

### Supporting

| Library / Tool                    | Version                              | Purpose                                       | When to Use                                                 |
| --------------------------------- | ------------------------------------ | --------------------------------------------- | ----------------------------------------------------------- |
| Vitest                            | 4.0.17                               | Frontend hook/store/component verification    | For frontend event-routing and UI-state assertions          |
| Cargo test                        | 1.92.0                               | Rust use-case and websocket test verification | For daemon and setup listener boundaries                    |
| Seq local server                  | reachable on `http://localhost:5341` | Query current structured logs                 | For manual observability verification and regression triage |
| `.claude/skills/seq/seq-query.sh` | repo-local                           | Repeatable local log queries                  | For operator-facing validation steps                        |

### Alternatives Considered

| Instead of                            | Could Use                                                                      | Tradeoff                                                                       |
| ------------------------------------- | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| New metrics/event bus                 | Existing `tracing` + existing WS/session fields                                | Faster, lower risk, stays inside the current architecture                      |
| Ad-hoc frontend `console.log` scatter | Structured frontend observability adapter or disciplined console-to-Seq events | Ad-hoc logs do not produce a durable, queryable timeline                       |
| New end-to-end harness first          | Repair current stale tests and then add a narrow cross-boundary smoke path     | Reusing the current stack is cheaper, but only after stale tests are corrected |

**Installation:**

```bash
# No new dependencies required for research findings.
```

## Current Architecture

### Relevant Flow

```text
GUI action
  -> src/api/daemon/pairing.ts or src/api/daemon/setup.ts
  -> src/api/daemon/client.ts (HTTP fetch, no trace propagation)
  -> src-tauri/crates/uc-daemon/src/pairing/host.rs
  -> broadcast::Sender<DaemonWsEvent>
  -> daemon websocket server
  -> src/lib/daemon-ws.ts (browser WS client)
  -> src/api/realtime.ts or src/hooks/useDaemonEvents.ts
  -> src/store/setupRealtimeStore.ts / src/components/PairingNotificationProvider.tsx
  -> SetupPage / PairingPinDialog UI state
```

### Ownership Boundaries

- Daemon pairing lifecycle and websocket emission are owned by `src-tauri/crates/uc-daemon/src/pairing/host.rs`.
- Setup join flow translation from pairing domain events to setup state is owned by `src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs`.
- Rust-side typed realtime mapping is owned by `src-tauri/crates/uc-daemon-client/src/ws_bridge.rs`.
- Frontend pairing event subscription is owned by `src/hooks/useDaemonEvents.ts`.
- Frontend setup-state SSOT is owned by `src/store/setupRealtimeStore.ts`.
- Responder pairing UI state is owned by `src/components/PairingNotificationProvider.tsx`.

### Important Split

- Pairing UI and setup UI are no longer the same path.
- Regular pairing UI advances through `usePairingEvents` into `PairingNotificationProvider` or `PairingDialog`.
- Setup UI advances through `setup.stateChanged` into `setupRealtimeStore` and `SetupPage`.
- This split is correct, but debugging it now requires observability on both branches.

## Verified Current Status Of Older Race Bugs

### Older Bug 1: Setup join flow subscribed too late and missed `PairingVerificationRequired`

**Current status:** fixed in code.

**Evidence:**

- `src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs` now calls `setup_pairing_facade.subscribe()` before `initiate_pairing()`.
- The file contains an explicit comment documenting that this ordering prevents the historical `ProcessingJoinSpace` stall.
- Dedicated tests exist in `src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs`:
  - `pairing_verification_listener_emits_join_space_confirm_peer_event`
  - `pairing_verification_listener_emits_join_space_failed_event_on_pairing_failure`

**Confidence:** HIGH on code-level fix, MEDIUM on runtime verification in this session. The targeted `cargo test -p uc-app ...` invocations did not return before research write-up, so this session does not include a fresh green run for those two tests.

### Older Bug 2: Responder accept path lost the first verification event in `PairingNotificationProvider`

**Current status:** fixed in code, but current regression coverage is stale.

**Evidence:**

- `src/components/PairingNotificationProvider.tsx` now sets `activeSessionIdRef.current = sessionId` synchronously inside the accept button handler before `setActiveSessionId(sessionId)`.
- This directly addresses the earlier timing issue where the verification event could arrive before the React state commit.

**Regression coverage status:**

- Current file `src/components/__tests__/PairingNotificationProvider.realtime.test.tsx` is stale.
- It still mocks deleted `@/api/p2p` realtime functions instead of the current `usePairingEvents` path, so it no longer proves the live architecture.
- The file failed in this session for that reason.

**Confidence:** MEDIUM. The code change clearly fixes the specific historical race, but the current test suite no longer proves the real path.

### What This Means For Phase 85

- The historical root causes are no longer the best explanation for current “stuck” debugging sessions.
- Remaining work is mainly about observability, correlation, and validation fidelity.

## Observability Gaps

### Gap 1: Trace propagation stops before daemon HTTP

**What exists now:**

- `src/lib/tauri-command.ts` starts a frontend trace and sends `_trace` into Tauri commands.
- Pairing/setup GUI flows do not use that path anymore; they use `src/api/daemon/client.ts`.

**What is missing:**

- `src/api/daemon/client.ts` does not create or forward any trace/correlation metadata.
- There is no shared frontend-generated `trace_id` on `/pairing/*` or `/setup/*` daemon requests.

**Impact:**

- You can query daemon logs by `session_id` after the session exists.
- You cannot correlate the initiating user action, the HTTP mutation, and the later websocket/UI events as one timeline.

**Confidence:** HIGH

### Gap 2: Frontend receive/filter decisions are mostly silent

**What exists now:**

- `usePairingEvents` routes events.
- `setupRealtimeStore` updates snapshot state.
- `PairingNotificationProvider` and `SetupPage` transition UI.

**What is missing:**

- No structured log when:
  - a pairing event is received
  - a pairing event is ignored because `sessionId` does not match active UI state
  - `setup.spaceAccessCompleted` is received but ignored
  - `PairingNotificationProvider` remains in `verifying` waiting for setup completion
  - `setupRealtimeStore` hydrates, applies realtime update, retries, or drops due to generation mismatch

**Impact:**

- A stuck UI cannot be explained from frontend logs.
- Silent filtering still looks like “event never arrived” from the outside.

**Confidence:** HIGH

### Gap 3: The frontend pairing hook still lacks payload validation and observability on unsupported shapes

**What exists now:**

- `setup.spaceAccessCompleted` has a type guard.
- Pairing payloads in `usePairingEvents` still use a raw structural cast.

**What is missing:**

- No type guard for pairing payload shapes.
- No warning when required fields are missing or when an unsupported `kind` arrives in the browser layer.

**Impact:**

- Browser-side payload mismatches degrade into silent no-op behavior or misleading UI state.
- Debugging malformed events depends entirely on Rust-side logs.

**Confidence:** HIGH

### Gap 4: Bridge backpressure/drop logs are not rich enough

**What exists now:**

- `src-tauri/crates/uc-daemon-client/src/ws_bridge.rs` logs:
  - decode failures
  - generic backpressure warnings

**What is missing:**

- Backpressure/drop logs do not include event topic, event type, session id, or queued event summary.
- There is no per-consumer event timeline; only the consumer name is logged.

**Impact:**

- If a terminal event is delayed or dropped under load, the logs do not say which session or event was affected.

**Confidence:** HIGH

### Gap 5: Seq currently shows backend emit logs, but not the browser receive side

**Verified local evidence:**

- Local Seq is reachable (`http://localhost:5341` returned `200`).
- `.seq-api-key` exists.
- Querying `"broadcasting pairing verification to daemon websocket subscribers"` returned recent backend events with `session_id`, `peer_id`, and fingerprint/code presence flags.
- Querying `"[DaemonWsClient] received WS event"` returned no recent Seq events.

**Impact:**

- The daemon side is queryable today.
- The browser receipt and UI-consumer side is not producing a comparable searchable stream.

**Confidence:** MEDIUM

### Gap 6: Validation coverage is stale exactly where Phase 85 needs confidence

**Verified local evidence from this session:**

- `npx vitest run src/hooks/__tests__/useDaemonEvents.test.ts src/store/__tests__/setupRealtimeStore.test.ts ...`
  - `useDaemonEvents.test.ts`: passed
  - `setupRealtimeStore.test.ts`: passed
  - `PairingNotificationProvider.realtime.test.tsx`: failed because it still mocks obsolete `@/api/p2p` realtime behavior instead of the current hook path
  - `SetupFlow.test.tsx`: failed because `SetupPage` now reads Redux `devicesSlice`, but the test does not mount a Redux provider
- `cargo test -p uc-daemon --test pairing_ws`
  - failed 6/7 tests with `EOF while parsing a value` in the websocket test harness

**Impact:**

- Current test failures are not evidence of live runtime regressions by themselves.
- They are evidence that the verification layer is no longer aligned with the current architecture.

**Confidence:** HIGH

## Architecture Patterns

### Pattern 1: Correlate by `session_id`, then enrich with request-level trace

**What:** Use existing pairing/setup `session_id` as the cross-daemon and cross-realtime correlation key, and layer request-origin trace metadata on top for actions that occur before a session exists.

**When to use:** Any pairing or setup mutation that later causes websocket/UI transitions.

**Why:** `session_id` already exists across daemon host, websocket payloads, and frontend consumers. It is the cheapest reliable anchor. A request-level `trace_id` is still needed to join the initial click/HTTP request to later session creation.

### Pattern 2: Instrument decision boundaries, not just transport boundaries

**What:** Log when code decides to ignore, retry, remap, or wait, not only when it emits or receives.

**Decision points that matter now:**

- `PairingNotificationProvider` session filter
- `setupRealtimeStore` hydration/retry/generation guards
- `usePairingEvents` event-kind routing
- `DaemonWsBridge` backpressure/drop path

**Why:** These are the exact places where user-visible “nothing happened” can occur without transport failure.

### Pattern 3: Keep setup and pairing timelines distinct but queryable together

**What:** Preserve the current architectural split:

- pairing timeline
- setup-state timeline
- optional space-access completion timeline

But make them queryable under the same session correlation.

**Why:** The UI is intentionally split. The observability model should expose that split, not hide it.

### Recommended Project Structure

```text
src/
├── observability/
│   ├── trace.ts                 # existing request-level trace helper
│   ├── seq.ts                   # existing frontend log forwarding
│   └── ...                      # likely place for structured GUI pairing/setup logging helpers
├── hooks/
│   └── useDaemonEvents.ts       # pairing receive + routing instrumentation
├── store/
│   └── setupRealtimeStore.ts    # setup-state transition instrumentation
└── components/
    └── PairingNotificationProvider.tsx  # UI-state transition instrumentation

src-tauri/crates/
├── uc-daemon/src/pairing/host.rs        # daemon emission instrumentation
└── uc-daemon-client/src/ws_bridge.rs    # decode/drop/backpressure instrumentation
```

## Don't Hand-Roll

| Problem                         | Don't Build                                           | Use Instead                                                                                 | Why                                                                             |
| ------------------------------- | ----------------------------------------------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| Cross-layer pairing correlation | A new side channel or separate debug-only state store | Existing `session_id` + existing tracing fields                                             | The runtime already carries `session_id`; duplicating correlation will drift    |
| Frontend observability          | Random `console.log` scatter                          | One consistent frontend observability helper layered on top of current console/Seq pipeline | Scattered logs are hard to query and easy to miss                               |
| Event debugging                 | Manual dual-device guesswork only                     | Structured boundary logs + repaired focused tests + a narrow manual Seq workflow            | Manual repro alone cannot localize daemon vs bridge vs UI                       |
| Validation                      | Reusing stale tests unchanged                         | Repair current tests to match current architecture first                                    | Current failures show some tests are asserting dead paths, not real regressions |

**Key insight:** The system already has a usable identity field (`session_id`) and a usable logging stack (`tracing` + Seq). The missing work is not a new framework; it is disciplined use of the framework at the real decision boundaries.

## Common Pitfalls

### Pitfall 1: Mistaking silent UI filtering for transport failure

**What goes wrong:** The daemon emitted the event, but the frontend ignored it due to session mismatch or state guard.

**Why it happens:** Current UI consumers do not log filter decisions.

**How to avoid:** Add structured logs for accept, ignore, and transition decisions at the consumer layer.

### Pitfall 2: Using stale tests as evidence of a runtime bug

**What goes wrong:** A broken test harness is mistaken for a broken pairing flow.

**Why it happens:** Several tests still target removed or changed architecture:

- old `@/api/p2p` mock path
- no Redux provider for `SetupPage`
- websocket harness EOF failures

**How to avoid:** Repair or replace stale tests before using them as regression signals.

### Pitfall 3: Adding logs only on success paths

**What goes wrong:** The timeline still goes dark when an event is filtered, retried, or dropped.

**Why it happens:** Success logs are easier to add than “decision” logs.

**How to avoid:** Instrument ignored events, retries, backpressure drops, and pending waits.

### Pitfall 4: Trying to solve observability with UI-only logs

**What goes wrong:** Frontend logs tell you the UI was stuck, but not whether the daemon emitted or the bridge decoded.

**How to avoid:** Instrument each boundary consistently:

- daemon mutation
- daemon emit
- websocket forward
- bridge decode
- frontend receive
- UI transition

## Code Examples

Verified current patterns that matter for planning:

### Subscribe Before Initiate In Setup Flow

```rust
// Source: src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs
let event_rx = self.setup_pairing_facade.subscribe().await?;
let session_id = self.setup_pairing_facade.initiate_pairing(peer_id.clone()).await?;
self.start_pairing_verification_listener_with_rx(
    session_id,
    event_rx,
    pairing_session_id,
    joiner_offer,
    context,
).await;
```

This fix is already in place. Phase 85 should observe this path, not redesign it.

### Current Daemon Verification Emit Boundary

```rust
// Source: src-tauri/crates/uc-daemon/src/pairing/host.rs
info!(
    session_id = %session_id,
    peer_id = %peer_id,
    has_short_code = !short_code.is_empty(),
    has_local_fingerprint = !local_fingerprint.is_empty(),
    has_peer_fingerprint = !peer_fingerprint.is_empty(),
    "broadcasting pairing verification to daemon websocket subscribers"
);
emit_pairing_verification(
    &event_tx,
    &session_id,
    pairing_stage::VERIFICATION,
    Some(peer_id.clone()),
    device_name.clone(),
    Some(short_code),
    None,
    Some(local_fingerprint),
    Some(peer_fingerprint),
);
```

This is a good backend boundary log. The planner should preserve this style and extend it downstream.

### Current Frontend Silent Filter Boundary

```typescript
// Source: src/components/PairingNotificationProvider.tsx
const currentSessionId = activeSessionIdRef.current
if (!currentSessionId || sessionId !== currentSessionId) return
```

This guard is correct behaviorally, but it is currently silent. Phase 85 should make this diagnosable.

## State Of The Art

| Old Approach                                                  | Current Approach                                                    | When Changed                               | Impact                                                                    |
| ------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------ | ------------------------------------------------------------------------- |
| Setup listener subscribed after pairing initiation            | Setup listener subscribes before initiate                           | post-Phase 37 bugfix                       | Old lost-event stall should no longer be the main failure mode            |
| Pairing notification accept path relied on async state commit | Accept path updates `activeSessionIdRef` synchronously              | post-2026-03-18 bugfix                     | Old responder verification race should no longer be the main failure mode |
| Tauri command path could propagate `_trace`                   | Pairing/setup GUI uses daemon HTTP client with no trace propagation | after daemon migration phases              | Observability gap moved from backend logic to transport/UI correlation    |
| Pairing tests reflected older event paths                     | Several tests now target stale mocks or stale harness assumptions   | after Phase 83 and related daemon cutovers | Validation no longer cleanly reflects runtime behavior                    |

**Deprecated/outdated for this phase:**

- Treating this as a “pairing race-condition fix” phase.
- Treating failing legacy-style frontend tests as direct proof of a live regression.

## Recommended Plan Slices

### Wave 1: Correlation Contract

- Define the canonical correlation fields for this phase.
- Recommendation:
  - `session_id` for pairing/setup event timelines
  - request-origin `trace_id` for the initial mutation path
- Add the minimal missing propagation from frontend daemon HTTP callers into backend logs where feasible.

### Wave 2: Boundary Instrumentation

- Instrument daemon mutation and emit boundaries that do not yet expose enough context.
- Instrument `DaemonWsBridge` decode, unsupported kind, and backpressure/drop paths with event/session context.
- Instrument frontend receive/filter/transition boundaries in:
  - `usePairingEvents`
  - `setupRealtimeStore`
  - `PairingNotificationProvider`

### Wave 3: Verification Repair

- Repair stale frontend tests to match the current architecture.
- Repair or replace the websocket test harness that now fails with EOF parsing.
- Add one narrow manual Seq-based validation script/checklist for dual-device reproduction.

## Open Questions

1. **How much correlation should be `trace_id` vs `session_id`?**
   - What we know: `session_id` already exists at most important runtime boundaries.
   - What is unclear: whether all initial mutations can cheaply carry a request-origin `trace_id` into daemon HTTP without broad API churn.
   - Recommendation: treat `session_id` as mandatory and `trace_id` as additive where the request starts before session creation.

2. **Should frontend observability go through existing `console` + `seq.ts`, or a more explicit wrapper?**
   - What we know: backend-to-Seq is queryable now; browser receive logs were not found in recent Seq queries.
   - What is unclear: whether the project consistently initializes frontend Seq in the exact environments used for pairing debugging.
   - Recommendation: centralize frontend pairing/setup logs behind a thin helper so the output shape is stable regardless of sink.

3. **How much of Phase 85 should include test-harness repair?**
   - What we know: current stale/failing tests overlap heavily with the phase boundaries.
   - What is unclear: whether planner should make harness repair its own first plan or fold it into each wave.
   - Recommendation: do harness repair in the first verification-focused slice, because otherwise new observability work cannot be trusted.

## Environment Availability

| Dependency       | Required By                | Available | Version                   | Fallback                                  |
| ---------------- | -------------------------- | --------- | ------------------------- | ----------------------------------------- |
| Bun              | Frontend tests and scripts | ✓         | 1.3.4                     | `npx vitest` for direct test invocation   |
| Node / npx       | Vitest execution           | ✓         | v22.19.0                  | —                                         |
| Cargo            | Rust tests                 | ✓         | 1.92.0                    | —                                         |
| Seq local server | Manual log verification    | ✓         | reachable (`200`)         | Raw local logs if Seq becomes unavailable |
| Seq API key      | Local Seq queries          | ✓         | present in `.seq-api-key` | Manual browser/terminal log inspection    |

**Missing dependencies with no fallback:**

- None found.

**Missing dependencies with fallback:**

- None found.

## Validation Architecture

### Test Framework

| Property           | Value                                                                                                       |
| ------------------ | ----------------------------------------------------------------------------------------------------------- |
| Framework          | Vitest 4.0.17 + Rust `cargo test`                                                                           |
| Config file        | `vite.config.ts` for frontend, Cargo workspace tests under `src-tauri/`                                     |
| Quick run command  | `npx vitest run src/hooks/__tests__/useDaemonEvents.test.ts src/store/__tests__/setupRealtimeStore.test.ts` |
| Full suite command | `npx vitest run` and `cd src-tauri && cargo test`                                                           |

### Phase Requirements → Test Map

| Req ID     | Behavior                                                          | Test Type                             | Automated Command                                                                    | File Exists?                           |
| ---------- | ----------------------------------------------------------------- | ------------------------------------- | ------------------------------------------------------------------------------------ | -------------------------------------- |
| P85-OBS-01 | Correlation fields survive daemon -> realtime -> UI path          | Rust + frontend integration           | `cd src-tauri && cargo test ...` plus `npx vitest run ...`                           | ❌ needs refreshed path-specific tests |
| P85-OBS-02 | Boundary logs exist for transition, ignore, and failure decisions | unit / integration / manual Seq query | targeted tests + Seq query script                                                    | ❌ Wave 0                              |
| P85-OBS-03 | Unsupported payloads and drops are diagnosable                    | Rust unit/integration                 | targeted `cargo test` around `ws_bridge` and daemon WS                               | ❌ Wave 0                              |
| P85-OBS-04 | Validation reflects live architecture                             | harness repair                        | `npx vitest run ...` and `cd src-tauri && cargo test -p uc-daemon --test pairing_ws` | ⚠️ existing files fail                 |
| P85-OBS-05 | No pairing behavior regression while observability improves       | focused regression tests              | pairing/setup targeted tests                                                         | ⚠️ partially present, partially stale  |

### Sampling Rate

- **Per task commit:** run the smallest affected frontend or Rust boundary test plus one Seq query if logging changed.
- **Per wave merge:** run repaired frontend event-routing tests and repaired daemon websocket tests.
- **Phase gate:** one dual-device manual pairing check with Seq query evidence, plus green targeted automated tests.

### Wave 0 Gaps

- [ ] `src/components/__tests__/PairingNotificationProvider.realtime.test.tsx` — rewrite to the current `usePairingEvents` architecture
- [ ] `src/pages/__tests__/SetupFlow.test.tsx` — mount with Redux provider or mock the Redux selector boundary explicitly
- [ ] `src-tauri/crates/uc-daemon/tests/pairing_ws.rs` — repair EOF-prone websocket read harness before using it as a signal
- [ ] Add one new test path proving frontend logs or structured decision events for session mismatch / ignored event cases
- [ ] Add one manual Seq verification checklist for pairing request -> verification -> verifying -> complete/failed -> space-access completion timeline

## Sources

### Primary (HIGH confidence)

- `.planning/ROADMAP.md` — Phase 85 placement and stated scope
- `.planning/debug/pairing-verification-prompt-missing.md` — historical responder-side race diagnosis
- `.planning/debug/setup-state-transition-stuck.md` — historical setup-side race diagnosis
- `src/components/PairingNotificationProvider.tsx` — current responder UI consumer behavior
- `src/store/setupRealtimeStore.ts` — current setup-state realtime SSOT
- `src/hooks/useDaemonEvents.ts` — current frontend pairing subscription path
- `src/api/daemon/client.ts` — current daemon HTTP transport path
- `src/lib/tauri-command.ts` — current trace propagation path that pairing/setup no longer uses
- `src-tauri/crates/uc-app/src/usecases/setup/action_executor.rs` — subscribe-before-initiate fix
- `src-tauri/crates/uc-daemon/src/pairing/host.rs` — current daemon emission boundaries
- `src-tauri/crates/uc-daemon-client/src/ws_bridge.rs` — current decode/drop/backpressure boundaries
- `src-tauri/crates/uc-core/src/network/daemon_api_strings.rs` — current pairing/setup wire contract strings

### Secondary (MEDIUM confidence)

- `src/hooks/__tests__/useDaemonEvents.test.ts` — current hook-level routing coverage
- `src/store/__tests__/setupRealtimeStore.test.ts` — current setup store coverage
- `src-tauri/crates/uc-app/src/usecases/setup/orchestrator.rs` — dedicated test definitions for setup listener behavior
- Local Seq query results from `.claude/skills/seq/seq-query.sh`

### Tertiary (LOW confidence)

- Live runtime status of the two targeted `uc-app` pairing listener tests was not confirmed during this session because the specific `cargo test -p uc-app ...` invocations did not complete before write-up.

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - based on current repo modules and locally verified tool availability
- Architecture: HIGH - based on direct source inspection across daemon, bridge, and frontend
- Pitfalls: MEDIUM - strengthened by current failing tests and Seq evidence, but not by a fresh full dual-device manual reproduction in this session

**Research date:** 2026-04-02
**Valid until:** 2026-04-09

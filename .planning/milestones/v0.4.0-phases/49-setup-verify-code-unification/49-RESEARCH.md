# Phase 49: Setup 验证码链路单一化重构 - Research

**Researched:** 2026-03-22
**Domain:** Frontend setup verification-code event path unification; backend-to-frontend setup state delivery contract
**Confidence:** HIGH

## Summary

The single-path goal for setup verification code display is already substantially implemented at the backend and infrastructure layers. The critical missing work is test coverage to prove end-to-end correctness of the unified path, plus targeted frontend cleanup in `SetupPage` and the `PairingNotificationProvider` test suite to remove residual dual-source patterns.

**Primary recommendation:** Add integration tests proving `selectJoinPeer` drives `SetupStateChanged(JoinSpaceConfirmPeer)` on the `setup` topic through the backend, then clean up any remaining frontend dual-source code rather than building new infrastructure.

## User Constraints (from CONTEXT.md)

### Locked Decisions

- setup orchestrator emits `JoinSpaceConfirmPeer` and fires a single `SetupStateChanged` event on the `setup` topic
- daemon websocket uses `setup` topic exclusively for frontend setup UI state — no pairing verification semantic leakage
- `PairingNotificationProvider` handles only regular pairing events (no setup session ownership)
- New independent `useSetupRealtimeStore` with: initial `getSetupState()` hydration, `onSetupStateChanged()` subscription, current state + sessionId + hydrated flag
- SetupPage reads only from the store — no `activeEventSessionIdRef`, no pairing ownership judgment, no local compensation logic
- App top-level setup gate reads from the same store, replacing current approach
- Legacy setup/pairing dual-path code deleted after new path and tests are stable

### Claude's Discretion

- How many plans needed to sequence the work
- Specific test structure within each plan
- Whether `setupRealtimeStore.ts` needs additional error-retries beyond the existing `RETRY_DELAY_MS = 2000`
- Whether `prevStateRef` in `SetupPage` can be eliminated or must remain (it drives animation direction and the `ProcessingJoinSpace` dot-position special case)

### Deferred Ideas

None — phase scope is well-bounded by the original request.

---

## Standard Stack

### Core (already in place)

| Library                                   | Version    | Purpose                                                                      | Status              |
| ----------------------------------------- | ---------- | ---------------------------------------------------------------------------- | ------------------- |
| `src/store/setupRealtimeStore.ts`         | (new file) | Module-level singleton store via `useSyncExternalStore`                      | Already created     |
| `src/api/setup.ts`                        | (existing) | `getSetupState()`, `onSetupStateChanged()` Tauri bridge                      | Already implemented |
| `src/api/p2p.ts`                          | (existing) | `onP2PPairingVerification()`, `onSpaceAccessCompleted()`                     | Already implemented |
| `src/api/realtime.ts`                     | (existing) | `onDaemonRealtimeEvent()` Tauri event listener                               | Already implemented |
| `uc-app/realtime/setup_consumer.rs`       | (existing) | Subscribes to `RealtimeTopic::Pairing`, forwards to `SetupPairingEventHub`   | Already implemented |
| `uc-app/realtime/setup_state_consumer.rs` | (existing) | Subscribes to `RealtimeTopic::Setup`, emits `HostEvent::Setup::StateChanged` | Already implemented |
| `uc-tauri/bootstrap/realtime_runtime.rs`  | (existing) | Owns `DaemonWsBridge`, starts all realtime consumers as tokio tasks          | Already implemented |
| `src-tauri/tests/daemon_ws_bridge.rs`     | (existing) | Bridge routing and contract tests                                            | Partially complete  |

**No new dependencies required.**

---

## Architecture Patterns

### End-to-End Event Path (Already Built)

The full path is already wired. The planner should treat this as gap-filling, not new construction:

```
DaemonPairingHost
  → RealtimeEvent::PairingVerificationRequired
  → RealtimeTopic::Pairing
  → DaemonWsBridge
  → uc-app/realtime/setup_consumer.rs
      → SetupPairingEventHub
      → SetupOrchestrator (advances to JoinSpaceConfirmPeer)
      → RealtimeEvent::SetupStateChanged
      → RealtimeTopic::Setup
  → uc-app/realtime/setup_state_consumer.rs
      → HostEvent::Setup::StateChanged
  → Tauri event system
  → src/api/realtime.ts  (daemon://realtime listener)
  → src/api/setup.ts  (onSetupStateChanged: filters topic==='setup', type==='setup.stateChanged')
  → setupRealtimeStore.ts  (module-level snapshot, useSyncExternalStore)
  → SetupPage  (reads setupState from store)
```

### PairingNotificationProvider Path (Already Decoupled)

Regular pairing events go through a separate, already-decoupled path:

```
DaemonPairingHost
  → RealtimeEvent::PairingVerificationRequired
  → RealtimeTopic::Pairing
  → DaemonWsBridge
  → uc-app/realtime/pairing_consumer.rs
      → HostEvent::Realtime(RealtimeFrontendEvent::pairing.verificationRequired)
  → Tauri event system
  → src/api/p2p.ts  (onP2PPairingVerification: filters topic==='pairing')
  → PairingNotificationProvider  (only acts on kind==='request')
```

**Key invariant:** `PairingNotificationProvider` never reads `getSetupState()`. The dual-path is already prevented at the event-topic level. Tests need to prove this invariant holds.

### Module-Level Singleton Store Pattern

`setupRealtimeStore.ts` uses the module-level singleton + `useSyncExternalStore` pattern — the same pattern already used for the RTK Query API store. This avoids Redux boilerplate for a purpose-built store.

Key implementation details verified:

- `snapshot` is a module-level mutable variable (singleton state)
- `listeners` is a `Set<() => void>` for `useSyncExternalStore` subscription
- `ensureSetupRealtimeSync()` handles initial hydration + subscription with `syncGeneration` guard for teardown races
- `syncSetupStateFromCommand(nextState)` allows command-return-value optimistic updates without double-update
- `resetSetupRealtimeStoreForTests()` resets all state for test isolation

### App Top-Level Gate Pattern

`AppContentWithBar` in `App.tsx` already:

- Calls `useSetupRealtimeStore()` to get `{ setupState, hydrated, sessionId }`
- Uses `previousSetupStateRef` to track `Completed` transition for `showCompletionStep` keeping the SetupPage visible after successful setup
- `isSetupGateActive()` returns `!hydrated || setupState !== 'Completed' || showCompletionStep`
- No polling loop — purely reactive via `useSyncExternalStore`

**No polling to replace.** The polling concern in CONTEXT.md is not present in the current codebase. This was likely identified as a potential anti-pattern to avoid, not an actual existing problem.

---

## Don't Hand-Roll

| Problem                        | Don't Build                    | Use Instead                                                                   | Why                                           |
| ------------------------------ | ------------------------------ | ----------------------------------------------------------------------------- | --------------------------------------------- |
| Cross-component setup state    | Local state in SetupPage + App | `useSyncExternalStore` module singleton                                       | Same store, no prop drilling                  |
| Backend-frontend state sync    | Custom WebSocket wrapper       | `DaemonWsBridge` + Tauri event                                                | Already handles auth, reconnect, backpressure |
| Session dedup                  | ad-hoc sessionId comparisons   | `onSetupStateChanged` already has `activeSessionId` dedup and `seenEventKeys` | Handles event retransmission                  |
| Pairing vs setup event routing | Topic-agnostic event fan-out   | `RealtimeTopic::Pairing` vs `RealtimeTopic::Setup` subscription filtering     | Already enforced by bridge and consumers      |

---

## Common Pitfalls

### Pitfall 1: Stale event handling when both Tauri commands and realtime events arrive

**What goes wrong:** `runAction()` in `SetupPage` calls a Tauri command and passes its return value to `syncSetupStateFromCommand()`. But if the realtime event arrives first, the store is updated, and the command return is stale.
**Why it happens:** Optimistic update from command return + realtime update can race. The `onSetupStateChanged` session dedup guard helps but doesn't eliminate this for non-terminal states.
**How to avoid:** Prefer realtime events as the source of truth. Only use command return value for the initial state when realtime is not yet subscribed. The `ensureSetupRealtimeSync` logic already subscribes before returning.
**Current status:** `setupRealtimeStore.ts` already handles this via `updateSnapshot(nextState, sessionId)` which always overwrites with the latest state. The `sessionId` for non-terminal states is preserved.

### Pitfall 2: Module-level singleton state leaking between tests

**What goes wrong:** `snapshot`, `listeners`, `stopListening`, `startPromise` are module-level. Tests that don't call `resetSetupRealtimeStoreForTests()` get stale state.
**How to avoid:** `resetSetupRealtimeStoreForTests()` exists. Tests must call it in `beforeEach`. The `vi.hoisted` mock hoisting ensures the mock is stable.
**Current status:** `resetSetupRealtimeStoreForTests()` already exists and is called in existing tests.

### Pitfall 3: Missing setup event contract in daemon_ws_bridge tests

**What goes wrong:** `daemon_ws_bridge.rs` tests cover `pairing` topic routing but the setup event contract test only verifies routing to `RealtimeTopic::Setup` subscribers — it doesn't prove the frontend `SetupStateChangedEvent` contract (fields: `sessionId`, `state`, `source`, `ts`).
**How to avoid:** Add explicit test asserting that a `setup.stateChanged` event from daemon arrives at the bridge with the correct `SetupState` enum variant and required fields.
**Current status:** `daemon_ws_bridge_routes_setup_state_only_to_setup_subscribers` covers routing. `install_daemon_setup_pairing_facade_routes_bridge_events_into_setup_subscription` covers pairing-to-hub fanout. Missing: end-to-end `selectJoinPeer → JoinSpaceConfirmPeer` state change.

### Pitfall 4: PairingNotificationProvider treating all pairing events as regular pairing

**What goes wrong:** If the active device is both in a regular pairing flow AND a setup flow simultaneously (different session IDs), the provider shows a toast for the regular pairing while setup is in progress. This is not wrong but could be confusing.
**Why it matters (or not):** The CONTEXT.md intent is to make the separation clean. The current code already doesn't suppress pairing toasts during setup — the separation is by session ID, not by setup-awareness. Since `PairingNotificationProvider` already filters by session ID and doesn't call `getSetupState()`, it is already decoupled.
**Current status:** No `getSetupState()` call in `PairingNotificationProvider.tsx`. Tests validate session-ID filtering. The "suppress/ignore setup" tests mentioned in CONTEXT.md are not present in the test file — tests already focus on regular pairing flows.

### Pitfall 5: Backend SetupOrchestrator not advancing to JoinSpaceConfirmPeer from daemon-side pairing events

**What goes wrong (known Phase 38 bug):** `HostEventSetupPort` captured a stale `LoggingEventEmitter` at `AppRuntime::with_setup` creation time. Setup state changes from `start_pairing_verification_listener_with_rx` only logged to console but never reached the frontend. This was supposed to be fixed in Phase 38.
**Current status:** The fix was "Phase 38 unifies SetupOrchestrator assembly into a single composition point." The `install_daemon_setup_pairing_facade` approach in `realtime_runtime.rs` creates the facade from `SetupAssemblyPorts` before starting consumers. The `setup_hub` is wired into the bridge. This should be verified by the new integration test.

---

## Code Examples

### Existing: setupRealtimeStore.ts public interface (verified)

```typescript
// src/store/setupRealtimeStore.ts
export function useSetupRealtimeStore(): SetupRealtimeStore {
  const currentSnapshot = useSyncExternalStore(subscribe, getSnapshot, getSnapshot)
  useEffect(() => {
    void ensureSetupRealtimeSync()
  }, [])
  return { ...currentSnapshot, syncSetupStateFromCommand }
}

export async function ensureSetupRealtimeSync(): Promise<void> {
  // 1. If not hydrated, call getSetupState() for initial state
  // 2. Subscribe to onSetupStateChanged() (filters topic==='setup')
  // 3. On error, schedule retry with RETRY_DELAY_MS=2000
  // 4. syncGeneration guard prevents stale callbacks after teardown
}

export function syncSetupStateFromCommand(nextState: SetupState) {
  updateSnapshot(nextState)
}
```

### Existing: SetupPage consumption (verified)

```tsx
// src/pages/SetupPage.tsx
const { setupState, hydrated, syncSetupStateFromCommand } = useSetupRealtimeStore()

const runAction = async (action: () => Promise<SetupState>) => {
  setLoading(true)
  try {
    const newState = await action()
    syncSetupStateFromCommand(newState) // optimistic + realtime will overwrite
  } finally {
    setLoading(false)
  }
}
```

### Existing: App gate (verified — no polling)

```tsx
// src/App.tsx
export const AppContentWithBar = () => {
  const { hydrated, setupState } = useSetupRealtimeStore()
  // No setInterval. Purely reactive via useSyncExternalStore.
  const isSetupActive = isSetupGateActive(setupState, hydrated, showCompletionStep)
  // ...
}
```

### Existing: onSetupStateChanged filtering (verified)

```typescript
// src/api/setup.ts
export async function onSetupStateChanged(callback) {
  return onDaemonRealtimeEvent(event => {
    if (event.topic !== 'setup' || event.type !== 'setup.stateChanged') return
    // ... session dedup, enrich with source:'realtime', ts
    callback(enrichedEvent)
  })
}
```

### Missing (gap to fill): Backend integration test for select-peer → JoinSpaceConfirmPeer

```rust
// src-tauri/crates/uc-tauri/tests/daemon_ws_bridge.rs
// Gap: no test that drives the full select-peer → pairing verification arrives
// → SetupStateChanged(JoinSpaceConfirmPeer) path.
// The pairing verification arrives from daemon as pairing.verification_required
// on the pairing topic; the setup consumer forwards to hub; hub → SetupOrchestrator
// advances state; SetupOrchestrator emits SetupStateChanged on setup topic;
// setup_state_consumer forwards to Tauri emitter.
// Need to add a scripted fixture that sends pairing.verification_required,
// then asserts the setup topic subscriber receives SetupStateChanged with
// JoinSpaceConfirmPeer variant and required fields (short_code, peer_fingerprint).
```

---

## State of the Art

| Old Approach                                                                  | Current Approach                                                          | When Changed                            | Impact                                                               |
| ----------------------------------------------------------------------------- | ------------------------------------------------------------------------- | --------------------------------------- | -------------------------------------------------------------------- |
| Frontend reads pairing verification via `onP2PPairingVerification`            | Frontend reads setup verification via `onSetupStateChanged` (setup topic) | Phase 46.1                              | PairingNotificationProvider handles pairing; SetupPage handles setup |
| PairingNotificationProvider called `getSetupState()` to suppress during setup | PairingNotificationProvider has no setup awareness; separation by topic   | Phase 49 (this phase — verify/clean up) | No cross-contamination                                               |
| App gate used periodic polling                                                | App gate uses `useSyncExternalStore` (no polling)                         | Phase 46 (via setupRealtimeStore)       | Reactive, not polled                                                 |
| `activeEventSessionIdRef` in SetupPage for pairing/setup ownership            | Store-based `sessionId` in `setupRealtimeStore`                           | Phase 49 (cleanup target)               | Single source of truth                                               |

**Deprecated/outdated:**

- Legacy `PairingBridge` and setup websocket subscription markers: deleted in Phase 46.1
- `PairingBridge` compatibility tests: deleted in Phase 46.1

---

## Open Questions

1. **Can `prevStateRef` in SetupPage be eliminated?**
   - What we know: `prevStateRef` is used for (a) animation direction via `getStateOrdinal` comparison, and (b) the `ProcessingJoinSpace` dot-position special case where `prevState === JoinSpaceSelectDevice` distinguishes connecting from verifying phases.
   - What's unclear: Whether animation direction can be driven from the store's session transition instead of a ref.
   - Recommendation: Keep `prevStateRef` for now — the ProcessingJoinSpace special case is a genuine display concern that would require store-level session history to eliminate. Do not spend time refactoring this in Phase 49.

2. **Is the Phase 38 stale-emitter bug actually fixed in the daemon wiring?**
   - What we know: The Phase 38 fix was "unify SetupOrchestrator assembly into single composition point." `install_daemon_setup_pairing_facade` creates the facade before consumers start. The `setup_hub` is passed into `start_realtime_runtime`.
   - What's unclear: Whether `SetupOrchestrator` inside the daemon actually subscribes to the hub and receives events. The `SetupPairingFacadePort::subscribe()` returns a channel from the hub.
   - Recommendation: Add the backend integration test to prove the full path works end-to-end. If it fails, the gap is in the `SetupOrchestrator` subscription wiring, not in the bridge.

3. **How many plans are needed?**
   - Recommendation: 3 plans:
     - Plan 01: Backend integration test for select-peer → JoinSpaceConfirmPeer
     - Plan 02: Frontend store tests, SetupPage simplification
     - Plan 03: PairingNotificationProvider test rewrite + App gate verification + legacy cleanup

---

## Validation Architecture

> Included because `workflow.nyquist_validation` is absent from `.planning/config.json` (treated as enabled).

### Test Framework

| Property             | Value                                               |
| -------------------- | --------------------------------------------------- |
| Framework            | Vitest (frontend) + Rust `#[tokio::test]` (backend) |
| Frontend config      | `vitest.config.ts` — already configured             |
| Backend config       | `src-tauri/Cargo.toml` integration test feature     |
| Quick run (frontend) | `bun test src/pages/__tests__/SetupFlow.test.tsx`   |
| Quick run (backend)  | `cd src-tauri && cargo test daemon_ws_bridge`       |
| Full suite           | `bun test` + `cd src-tauri && cargo test`           |

### Phase Requirements to Test Map

No explicit REQ IDs are mapped yet for Phase 49. The requirements are derived from the CONTEXT.md and canonical refs:

| Derived Requirement | Behavior                                                                                                                               | Test Type          | Automated Command                                                                 | File                                                  |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | ------------------ | --------------------------------------------------------------------------------- | ----------------------------------------------------- |
| PH49-BE-01          | `selectJoinPeer` via Tauri command triggers daemon pairing; daemon emits `setup.stateChanged(JoinSpaceConfirmPeer)` on the setup topic | Rust integration   | `cargo test --test daemon_ws_bridge be_select_peer_to_join_space_confirm_peer`    | `src-tauri/crates/uc-tauri/tests/daemon_ws_bridge.rs` |
| PH49-BE-02          | Setup events route to `RealtimeTopic::Setup` subscribers; pairing events do NOT appear in setup subscribers                            | Rust unit          | `cargo test daemon_ws_bridge_routes_setup_state_only_to_setup_subscribers`        | already exists                                        |
| PH49-BE-03          | Daemon `setup.stateChanged` event carries required frontend fields (sessionId, state enum with short_code/peer_fingerprint)            | Rust integration   | `cargo test be_setup_state_changed_payload_fields`                                | `src-tauri/crates/uc-tauri/tests/daemon_ws_bridge.rs` |
| PH49-FE-01          | Store transitions: null + not hydrated → initial hydration → realtime-driven                                                           | Vitest unit        | `bun test src/__tests__/setupRealtimeStore.test.ts`                               | new file                                              |
| PH49-FE-02          | Store receives `JoinSpaceConfirmPeer`: setupState and sessionId correctly updated                                                      | Vitest unit        | `bun test src/__tests__/setupRealtimeStore.test.ts`                               | new file                                              |
| PH49-FE-03          | Store receives `Completed`/`Welcome`: state resets, sessionId nulled                                                                   | Vitest unit        | `bun test src/__tests__/setupRealtimeStore.test.ts`                               | new file                                              |
| PH49-FE-04          | SetupPage renders verification code correctly when store is `JoinSpaceConfirmPeer`                                                     | Vitest integration | `bun test src/pages/__tests__/SetupFlow.test.tsx`                                 | update existing                                       |
| PH49-FE-05          | PairingNotificationProvider shows toast only for kind==='request', not during setup                                                    | Vitest integration | `bun test src/components/__tests__/PairingNotificationProvider.realtime.test.tsx` | already correct                                       |
| PH49-FE-06          | App setup gate reads store without polling; `Completed` transition keeps step visible                                                  | Vitest unit        | `bun test src/__tests__/App.setup-gate-logic.test.ts`                             | already exists                                        |

### Wave 0 Gaps

- [ ] `src/__tests__/setupRealtimeStore.test.ts` — unit tests for store hydration, realtime transitions, reset
- [ ] `src-tauri/crates/uc-tauri/tests/daemon_ws_bridge.rs` — add `be_select_peer_to_join_space_confirm_peer` and `be_setup_state_changed_payload_fields` tests
- [ ] `src/pages/__tests__/SetupFlow.test.tsx` — add test for `JoinSpaceConfirmPeer` rendering using store mock (no pairing mock)
- [ ] Framework install: both Vitest and Rust test infrastructure already present — no new install needed

---

## Sources

### Primary (HIGH confidence)

- `src/store/setupRealtimeStore.ts` — confirmed existing, implementation verified
- `src/pages/SetupPage.tsx` — confirmed existing, store consumption verified
- `src/App.tsx` — confirmed no polling, gate via `useSyncExternalStore`
- `src/api/setup.ts` — confirmed `onSetupStateChanged` filters by topic === 'setup'
- `src/api/p2p.ts` — confirmed `PairingNotificationProvider` path only uses topic === 'pairing'
- `src-tauri/crates/uc-app/src/realtime/setup_consumer.rs` — confirmed topic routing
- `src-tauri/crates/uc-app/src/realtime/setup_state_consumer.rs` — confirmed setup topic → HostEvent mapping
- `src-tauri/crates/uc-tauri/src/bootstrap/realtime_runtime.rs` — confirmed all consumers wired
- `src-tauri/crates/uc-tauri/tests/daemon_ws_bridge.rs` — confirmed existing tests + gaps

### Secondary (MEDIUM confidence)

- Phase 46.1 decisions in STATE.md — confirms PairingBridge deleted, topic separation enforced
- Phase 46 RESEARCH.md — confirms the architectural pattern of hub-based event fanout

### Tertiary (LOW confidence)

- Phase 38 bug description (stale LoggingEventEmitter) — described in STATE.md Known Bugs; fix described but not independently verified

---

## Metadata

**Confidence breakdown:**

- Standard Stack: HIGH — all files confirmed to exist and have correct implementations
- Architecture: HIGH — event paths verified by reading source files
- Pitfalls: HIGH — code patterns confirmed by reading source files
- Backend integration test gaps: MEDIUM — based on reading `daemon_ws_bridge.rs`, confirmed missing test for full select-peer → JoinSpaceConfirmPeer path
- Frontend test gaps: MEDIUM — `setupRealtimeStore.test.ts` confirmed missing by directory listing

**Research date:** 2026-03-22
**Valid until:** 2026-04-21 (30 days — architecture is stable, only test gaps to fill)

# Phase 83: Frontend Pairing Event Architecture — Research

**Researched:** 2026-04-02
**Domain:** React/TypeScript frontend architecture — pairing events, state management, and WS type safety
**Confidence:** HIGH

## Summary

Phase 83 refactors three distinct areas of the frontend pairing system: event subscriptions, state management, and business logic encapsulation. The work is almost entirely mechanical — `p2p.ts` facade removal leaves ~6 call sites, `usePairingEvents` already covers pairing events, and the Redux slice and hook infrastructure already exist. The only non-trivial decision is whether `usePairingEvents` should absorb space-access events (needed by `PairingNotificationProvider`) or stay focused on pairing-only topics.

**Primary recommendation:** Extend `usePairingEvents` to also handle `setup.spaceAccessCompleted` so `PairingNotificationProvider` can use it exclusively, avoiding a second `onSpaceAccessCompleted` subscription. This keeps pairing-related event logic in one hook.

---

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** `src/api/p2p.ts` 中的 `onP2PPairingVerification` 等旧路径（Tauri event bridge）全部替换为 `usePairingEvents` hook
- **D-02:** 前端配对事件订阅的唯一入口是 `usePairingEvents` hook
- **D-03:** `setupRealtimeStore.ts` 的 `onSpaceAccessCompleted` 有幂等去重逻辑，保留在 store 中，不迁移到 hook
- **D-04:** `discoveredPeers` 状态从 `useDeviceDiscovery` hook 迁移到 Redux `devicesSlice`
- **D-05:** `devicesSlice` 作为配对相关状态的唯一来源
- **D-06:** `useDeviceDiscovery` hook 保留用于封装设备发现的副作用（启动扫描、清理），但状态写入 Redux
- **D-07/D-08/D-09:** `SetupPage.tsx` 中的 `getStateOrdinal`、`getStepInfo`、`runAction` 提取为 `useSetupFlow` hook
- **D-10/D-11/D-12:** 为每个 daemon WS event type 创建 typed payload interfaces + 类型守卫函数，替代 `as any`
- **D-13:** `src/api/p2p.ts` 删除
- **D-14:** `getDeviceSyncSettings`/`updateDeviceSyncSettings` 已在 `daemon/device.ts`，保留
- **D-15:** `onP2PPeerDiscoveryChanged` 的 diff 逻辑提取到 `daemon/events.ts` 作为工具函数

### Claude's Discretion

- `useDeviceDiscovery` 具体重构方式（是否需要拆分、或保持 hook 形态但状态写入 Redux）
- `useSetupFlow` hook 的具体 API 设计
- `devicesSlice` 中 discoveredPeers 的 state shape
- 类型守卫函数的实现方式（user-defined type guards vs simple type assertion functions）
- `onP2PPeerDiscoveryChanged` diff 逻辑是否需要测试

### Deferred Ideas (OUT OF SCOPE)

- PairingNotificationProvider UX 定制（toast vs dialog）— Phase 83 只做架构重构
- Setup flow 超时处理
- 多个配对 session 同时存在的 UI 处理
- `p2p.ts` 中 `classifyPairingError` 等工具函数的共享位置

---

## Phase Requirements

> No requirement IDs assigned (TBD). This phase is pure architecture refactoring with no new user-facing features.

---

## Standard Stack

### Core

| Library       | Version | Purpose                              | Why Standard                                         |
| ------------- | ------- | ------------------------------------ | ---------------------------------------------------- |
| React 18      | 18.x    | UI framework                         | Already in use                                       |
| TypeScript 5  | 5.x     | Type safety                          | Already in use                                       |
| Redux Toolkit | 2.x     | State management for discoveredPeers | Already in use — devicesSlice exists                 |
| Vitest        | 1.x     | Unit testing                         | Already in use — existing test files in `__tests__/` |

### Supporting

| Library                  | Version | Purpose                       | When to Use                                        |
| ------------------------ | ------- | ----------------------------- | -------------------------------------------------- |
| `@testing-library/react` | 13.x    | React hook testing            | Existing pattern — used in all existing hook tests |
| `sonner`                 | latest  | Toast notifications           | Already in use — PairingNotificationProvider       |
| `framer-motion`          | 11.x    | Step transitions in SetupPage | Already in use                                     |

### No New Dependencies

This phase introduces no new packages. All needed abstractions (`usePairingEvents`, `daemonWs.subscribe`, Redux slices) already exist.

---

## Architecture Patterns

### Recommended Project Structure (changes only)

```
src/
├── hooks/
│   └── useSetupFlow.ts         # NEW — extracted from SetupPage.tsx
├── api/
│   ├── p2p.ts                  # DELETE — facade removed
│   └── daemon/
│       └── events.ts           # NEW — shared event utilities (diff logic, type guards)
├── store/slices/
│   └── devicesSlice.ts         # MODIFY — add discoveredPeers to state
└── hooks/
    └── useDeviceDiscovery.ts   # MODIFY — state → Redux, subscribe via daemonWs
```

### Pattern 1: React Hook as Event Subscription Entry Point

**What:** `usePairingEvents` wraps `daemonWs.subscribe(['pairing'], handler)` with React lifecycle (mount/unmount). This is the established pattern from Phase 79.

**When to use:** Any component needing daemon WS pairing events. NOT used for setup state (which uses `setupRealtimeStore`) or clipboard (which uses `useClipboardNewContent`).

**Key decision (Claude's discretion — recommend):** Extend `usePairingEvents` to also handle `setup.spaceAccessCompleted` (from the `setup` topic). This event is needed by `PairingNotificationProvider` alongside pairing events. Keeping both in one hook avoids a second subscription.

**Implementation:** Add a second `daemonWs.subscribe(['setup'], handler)` inside the same `useEffect`, merging `setup.spaceAccessCompleted` into the existing callbacks interface:

```typescript
// Source: analysis of existing usePairingEvents pattern
// Extend callbacks interface to include space access:
export interface UsePairingEventsCallbacks {
  // ...existing pairing callbacks...
  onSpaceAccessCompleted?: (data: {
    sessionId: string
    peerId: string
    success: boolean
    reason?: string
  }) => void
}
```

### Pattern 2: Redux as Single Source of Truth for Peer State

**What:** `discoveredPeers` migrates from local `useState` in `useDeviceDiscovery` to `devicesSlice.discoveredPeers`. The hook retains lifecycle management (start/stop scan, 10s timeout) but writes to Redux.

**When to use:** Any shared state that multiple components need to read.

**Implementation:**

```typescript
// devicesSlice.ts — add discoveredPeers field
interface DevicesState {
  // ...existing fields...
  discoveredPeers: DiscoveredPeer[] // NEW
  discoveredPeersLoading: boolean // NEW
}

// useDeviceDiscovery.ts — dispatch instead of setState
const dispatch = useDispatch()
dispatch(setDiscoveredPeers(nextPeers))
```

### Pattern 3: Facade Removal with Direct Module Imports

**What:** `p2p.ts` was an indirection layer. After D-13 deletion, all callers import from `daemon/` modules directly.

**Migration map:**

| Old import (`@/api/p2p`)   | New import (`@/api/daemon`)         |
| -------------------------- | ----------------------------------- |
| `getLocalDeviceInfo`       | `@/api/daemon/pairing`              |
| `getPairedPeersWithStatus` | `@/api/daemon/pairing`              |
| `getDeviceSyncSettings`    | `@/api/daemon/device`               |
| `updateDeviceSyncSettings` | `@/api/daemon/device`               |
| `acceptP2PPairing`         | `@/api/daemon/pairing`              |
| `rejectP2PPairing`         | `@/api/daemon/pairing`              |
| `unpairP2PDevice`          | `@/api/daemon/pairing`              |
| `classifyPairingError`     | Extract to `@/api/daemon/events.ts` |

### Pattern 4: Type Guard Functions for WS Payload Validation

**What:** Replace `event.payload as any` with typed payload interfaces + type guard functions.

**Implementation approach:**

```typescript
// In useDaemonEvents.ts — typed payload interfaces
export interface PeersChangedPayload {
  peers: Array<{
    peerId: string
    deviceName?: string | null
    connected: boolean
  }>
}

export interface PeersNameUpdatedPayload {
  peerId: string
  deviceName: string
}

// Type guard functions
export function isPeersChangedPayload(payload: unknown): payload is PeersChangedPayload {
  if (typeof payload !== 'object' || payload === null) return false
  return 'peers' in payload && Array.isArray((payload as PeersChangedPayload).peers)
}
```

**Recommendation:** Use simple type guard functions (returning `boolean`) rather than TypeScript user-defined type guards (`asserts payload is T`). Simple guards are easier to test and compose.

---

## Don't Hand-Roll

| Problem                        | Don't Build                                                     | Use Instead                                              | Why                                                                                   |
| ------------------------------ | --------------------------------------------------------------- | -------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Peer discovery event diffing   | Custom Map-based diff in multiple components                    | Shared `diffPeerSnapshots` utility in `daemon/events.ts` | The `knownPeers` Map pattern is already in `p2p.ts` — extract once, reuse everywhere  |
| Pairing event subscription     | Direct `daemonWs.subscribe` calls in components                 | `usePairingEvents` hook                                  | Hook manages lifecycle (mount/unmount/reconnect), prevents leaks                      |
| Setup step index computation   | Duplicated `getStateOrdinal` / `getStepInfo` logic in SetupPage | `useSetupFlow` hook                                      | Complex state-machine mapping belongs in a testable hook, not a render component      |
| Type assertion for WS payloads | `as any` casts scattered across event handlers                  | Typed payload interfaces + type guard functions          | Centralized type definitions enable intellisense and catch mismatches at compile time |

**Key insight:** The `onP2PPeerDiscoveryChanged` `knownPeers` Map diff is the only genuinely complex piece in `p2p.ts`. Everything else is simple API forwarding that just needs redirecting.

---

## Runtime State Inventory

> Skip — this is a pure frontend refactoring phase. No backend, database, or OS-level state changes.

**Stored data:** None — no database or persistent storage changes.
**Live service config:** None — no external service configuration changes.
**OS-registered state:** None — no OS-level registrations.
**Secrets/env vars:** None — no secret key or env var changes.
**Build artifacts:** `src/api/p2p.ts` deleted — no installed package cleanup needed.

---

## Common Pitfalls

### Pitfall 1: Accidental Deletion of Still-Used Imports from p2p.ts

**What goes wrong:** `p2p.ts` deleted but some import is missed (e.g., `getP2PPeers` used by `useDeviceDiscovery`, `onSpaceAccessCompleted` used by `setupRealtimeStore`). Build fails.

**Why it happens:** `p2p.ts` re-exports from multiple daemon modules and also defines local utilities (`classifyPairingError`, `onP2PPeerDiscoveryChanged`). The refactor must identify all exported symbols and redirect each to its canonical source.

**How to avoid:**

1. List ALL exports from `p2p.ts` before deleting
2. For each export, determine the canonical source module
3. Update ALL call sites before deleting the file
4. Run `bun run build` to verify no broken imports

**Verification:** `grep -r "from '@/api/p2p'" src/` must return zero results after refactor.

### Pitfall 2: Stale State After discoveredPeers Migration to Redux

**What goes wrong:** `useDeviceDiscovery` writes to Redux but `JoinPickDeviceStep` reads from the hook's local state (not Redux). Devices disappear after migration.

**Why it happens:** `JoinPickDeviceStep` reads `peers` prop from `useDeviceDiscovery`, but the plan redirects `useDeviceDiscovery` to write to Redux instead. The prop chain needs updating.

**How to avoid:** `JoinPickDeviceStep` already receives `peers` as a prop from `SetupPage.tsx`. After migration:

- `SetupPage` reads `discoveredPeers` from Redux via `useSelector`
- Passes it to `JoinPickDeviceStep` (already done — SetupPage already has the peers from `useDeviceDiscovery`)
- This is a no-change path IF `JoinPickDeviceStep` takes peers as a prop (it does)

### Pitfall 3: Missing Type Guard for New WS Topics

**What goes wrong:** `useDeviceDiscovery` migrates from `onDaemonRealtimeEvent` (legacy bridge) to `daemonWs.subscribe(['peers'], handler)`. The legacy bridge does field normalization (`eventType` → `type`) that direct `daemonWs` does NOT do. Event types like `peers.changed` get dispatched as `peers.changed` (eventType field), not `peers.changed` (type field) — which is the same string, so this is actually fine. But the `onDaemonRealtimeEvent` bridge also normalizes `sessionId`/`session_id`. Direct `daemonWs` already uses `sessionId`. So this is safe.

**How to avoid:** Verify with existing test: `src/__tests__/lib/daemon-ws.test.ts` already tests direct `daemonWs` delivery. Confirm peers.changed pattern matches expectations.

### Pitfall 4: Circular Imports After p2p.ts Deletion

**What goes wrong:** `daemon/pairing.ts` re-exports `classifyPairingError` from `p2p.ts` (`export { classifyPairingError } from '@/api/p2p'`). When `p2p.ts` is deleted, `daemon/pairing.ts` breaks.

**Why it happens:** `daemon/pairing.ts` line 126: `export { classifyPairingError } from '@/api/p2p'`

**How to avoid:** Move `classifyPairingError` definition to `daemon/events.ts` before deleting `p2p.ts`. Update `daemon/pairing.ts` re-export to point to `daemon/events.ts`. Verify `PairingNotificationProvider` imports `classifyPairingError` from the new location.

### Pitfall 5: useSetupFlow API Breaking Existing SetupPage Contract

**What goes wrong:** `useSetupFlow` is designed to encapsulate state mapping and Tauri commands, but `SetupPage` needs to remain compatible with existing step components (all of which take `direction`, `loading`, and step-specific props).

**How to avoid:** `useSetupFlow` should NOT try to own the `direction` animation logic (which uses `useMemo` with `prevStateRef`). That can stay in `SetupPage.tsx` as a local concern. The hook's job is: state-to-step-index mapping, `runAction` with loading/error management, and step info computation. The direction logic is a rendering concern.

---

## Code Examples

### Example: Extending usePairingEvents with Space Access

```typescript
// src/hooks/useDaemonEvents.ts

// New payload interface
export interface SpaceAccessCompletedPayload {
  sessionId: string
  peerId: string
  success: boolean
  reason?: string | null
  ts: number
}

// Updated callbacks interface
export interface UsePairingEventsCallbacks {
  onRequest?: (data: { sessionId: string; peerId?: string; deviceName?: string }) => void
  onVerification?: (data: {
    sessionId: string
    peerId?: string
    deviceName?: string
    code?: string
    localFingerprint?: string
    peerFingerprint?: string
  }) => void
  onVerifying?: (data: { sessionId: string; peerId?: string; deviceName?: string }) => void
  onComplete?: (data: { sessionId: string; peerId?: string; deviceName?: string }) => void
  onFailed?: (data: { sessionId: string; error?: string }) => void
  // NEW
  onSpaceAccessCompleted?: (data: {
    sessionId: string
    peerId: string
    success: boolean
    reason?: string
  }) => void
}

// In useEffect — second subscription for setup topic
useEffect(() => {
  const pairingHandler = (event: DaemonWsEvent) => {
    /* ... existing pairing logic ... */
  }
  const setupHandler = (event: DaemonWsEvent) => {
    if (
      event.eventType === 'setup.spaceAccessCompleted' &&
      callbacksRef.current.onSpaceAccessCompleted
    ) {
      const p = event.payload as SpaceAccessCompletedPayload
      callbacksRef.current.onSpaceAccessCompleted({
        sessionId: p.sessionId,
        peerId: p.peerId,
        success: p.success,
        reason: p.reason ?? undefined,
      })
    }
  }

  const unsubPairing = daemonWs.subscribe(['pairing'], pairingHandler)
  const unsubSetup = daemonWs.subscribe(['setup'], setupHandler)
  return () => {
    unsubPairing()
    unsubSetup()
  }
}, [])
```

### Example: devicesSlice State Shape for discoveredPeers

```typescript
// src/store/slices/devicesSlice.ts

// DiscoveredPeer type shared between hook and slice
export interface DiscoveredPeer {
  id: string
  deviceName: string | null
  device_type: string
}

interface DevicesState {
  // ...existing fields...
  discoveredPeers: DiscoveredPeer[]
  discoveredPeersLoading: boolean
}

// Reducer actions
reducers: {
  setDiscoveredPeers: (state, action: { payload: DiscoveredPeer[] }) => {
    state.discoveredPeers = action.payload
    state.discoveredPeersLoading = false
  },
  clearDiscoveredPeers: state => {
    state.discoveredPeers = []
  },
}
```

### Example: diffPeerSnapshots Utility

```typescript
// src/api/daemon/events.ts

export interface PeerSnapshotPeer {
  peerId: string
  deviceName?: string | null
  connected: boolean
}

export interface PeerDiffEvent {
  peerId: string
  deviceName?: string | null
  addresses: string[]
  discovered: boolean
}

/**
 * Converts a full peers snapshot into discovered/lost events.
 * Call with the previous snapshot Map to detect which peers are new vs gone.
 *
 * @param nextPeers   Full snapshot from peers.changed payload
 * @param knownPeers  Map of previously known peers (maintained by caller)
 * @param callback    Called for each discovered (new) or lost (removed) peer
 *
 * Usage in useDeviceDiscovery:
 *   const knownPeers = useRef(new Map<string, { deviceName?: string | null }>())
 *   diffPeerSnapshots(payload.peers, knownPeers.current, (event) => { ... })
 */
export function diffPeerSnapshots(
  nextPeers: PeerSnapshotPeer[],
  knownPeers: Map<string, { deviceName?: string | null }>,
  callback: (event: PeerDiffEvent) => void
): void {
  const nextMap = new Map<string, { deviceName?: string | null }>()
  for (const peer of nextPeers) {
    nextMap.set(peer.peerId, { deviceName: peer.deviceName ?? null })
    if (!knownPeers.has(peer.peerId)) {
      callback({
        peerId: peer.peerId,
        deviceName: peer.deviceName ?? null,
        addresses: [],
        discovered: true,
      })
    }
  }
  for (const [peerId, previous] of knownPeers.entries()) {
    if (!nextMap.has(peerId)) {
      callback({
        peerId,
        deviceName: previous.deviceName ?? null,
        addresses: [],
        discovered: false,
      })
    }
  }
  knownPeers.clear()
  for (const [peerId, peer] of nextMap.entries()) {
    knownPeers.set(peerId, peer)
  }
}
```

### Example: useSetupFlow Hook Skeleton

```typescript
// src/hooks/useSetupFlow.ts

import { useCallback, useMemo, useRef, useState } from 'react'
import {
  cancelSetup,
  confirmPeerTrust,
  selectJoinPeer,
  startJoinSpace,
  startNewSpace,
  submitPassphrase,
  verifyPassphrase,
  type SetupState,
} from '@/api/setup'
import { useSetupRealtimeStore } from '@/store/setupRealtimeStore'

export interface StepInfo {
  total: number
  current: number
}

export interface UseSetupFlowReturn {
  setupState: SetupState | null
  hydrated: boolean
  stepInfo: StepInfo | null
  direction: 'forward' | 'backward'
  loading: boolean
  runAction: (action: () => Promise<SetupState>) => Promise<void>
}

export function useSetupFlow(): UseSetupFlowReturn {
  const { setupState, hydrated, syncSetupStateFromCommand } = useSetupRealtimeStore()
  const [loading, setLoading] = useState(false)
  const prevStateRef = useRef<SetupState | null>(null)

  const direction = useMemo(() => {
    return getStateOrdinal(setupState) >= getStateOrdinal(prevStateRef.current)
      ? 'forward'
      : 'backward'
  }, [setupState])

  // (update prevStateRef in useEffect)

  const runAction = useCallback(
    async (action: () => Promise<SetupState>) => {
      setLoading(true)
      try {
        const newState = await action()
        syncSetupStateFromCommand(newState)
      } catch (error) {
        console.error('Failed to dispatch event:', error)
      } finally {
        setLoading(false)
      }
    },
    [syncSetupStateFromCommand]
  )

  const stepInfo = useMemo(() => getStepInfo(setupState, prevStateRef.current), [setupState])

  return { setupState, hydrated, stepInfo, direction, loading, runAction }
}

// getStateOrdinal and getStepInfo moved verbatim from SetupPage.tsx
```

---

## State of the Art

| Old Approach                             | Current Approach                      | When Changed | Impact                                            |
| ---------------------------------------- | ------------------------------------- | ------------ | ------------------------------------------------- |
| Tauri event bridge (`listen('p2p://*')`) | `daemonWs.subscribe` direct WebSocket | Phase 79     | Lower latency, daemon-owned event lifecycle       |
| Local state in `useDeviceDiscovery`      | Redux `devicesSlice.discoveredPeers`  | Phase 83     | Shared state across components, persistence-ready |
| `p2p.ts` facade over daemon modules      | Direct daemon module imports          | Phase 83     | Eliminates indirection, clearer dependency graph  |
| `as any` WS payload assertions           | Typed interfaces + type guards        | Phase 83     | Compile-time safety, intellisense support         |
| Business logic in SetupPage render       | `useSetupFlow` hook                   | Phase 83     | Testable, reusable, separation of concerns        |

**Deprecated/outdated:**

- `src/api/p2p.ts` facade module — replaced by direct imports from `daemon/` modules
- `src/api/realtime.ts` `onDaemonRealtimeEvent` — still used by `useDeviceDiscovery` and `setupRealtimeStore`, but these migrate to direct `daemonWs.subscribe` or hooks
- `as any` WS payload casts in `usePairingEvents` — replaced by typed interfaces

---

## Open Questions

1. **Should `onDaemonRealtimeEvent` be deleted after migration?**
   - What we know: `onDaemonRealtimeEvent` is a legacy bridge from Phase 79. After migration, `useDeviceDiscovery` and `setupRealtimeStore` migrate to direct `daemonWs.subscribe` or hooks.
   - What's unclear: Whether any other consumer still needs the `onDaemonRealtimeEvent` abstraction after migration.
   - Recommendation: Check all remaining callers with `grep -r "onDaemonRealtimeEvent" src/`. If only `setupRealtimeStore` and `useDeviceDiscovery` use it, migrate both and delete the bridge. If unexpected consumers appear, deprecate with JSDoc `@deprecated` and delete in a follow-up phase.

2. **`onSpaceAccessCompleted` in PairingNotificationProvider — hook or direct subscribe?**
   - What we know: `PairingNotificationProvider` needs both pairing events (available via `usePairingEvents`) and space access completion. `D-03` says `setupRealtimeStore`'s usage keeps its deduplication, but `PairingNotificationProvider`'s usage is simpler.
   - What's unclear: Whether extending `usePairingEvents` to also handle `setup.spaceAccessCompleted` creates coupling that's too broad.
   - Recommendation: Extend `usePairingEvents` with `onSpaceAccessCompleted` callback — the notification provider legitimately needs both event types in the same session context, and having one hook reduces subscription count.

3. **Should discoveredPeers persist across page refreshes?**
   - What we know: Redux state does not persist by default. The discovered peer list is ephemeral.
   - What's unclear: Is persistence desired? If yes, add `redux-persist` or manual localStorage sync.
   - Recommendation: Keep ephemeral for now. Persistence is a UX enhancement outside Phase 83 scope.

---

## Environment Availability

Step 2.6: SKIPPED — no external dependencies identified. This phase modifies only TypeScript/React frontend code. No new CLI tools, databases, or services are required.

---

## Validation Architecture

### Test Framework

| Property           | Value                                                                                                    |
| ------------------ | -------------------------------------------------------------------------------------------------------- |
| Framework          | Vitest 1.x                                                                                               |
| Config file        | `vitest.config.ts` (project root)                                                                        |
| Quick run command  | `bun test -- src/hooks/__tests__/useDaemonEvents.test.ts src/hooks/__tests__/useDeviceDiscovery.test.ts` |
| Full suite command | `bun test`                                                                                               |

### Phase Requirements → Test Map

| Req ID | Behavior                                                                 | Test Type | Automated Command                                                                    | File Exists?     |
| ------ | ------------------------------------------------------------------------ | --------- | ------------------------------------------------------------------------------------ | ---------------- |
| TBD    | `usePairingEvents` handles space access events correctly                 | unit      | `bun test -- src/hooks/__tests__/useDaemonEvents.test.ts`                            | ✅               |
| TBD    | `useDeviceDiscovery` writes to Redux `discoveredPeers`                   | unit      | `bun test -- src/hooks/__tests__/useDeviceDiscovery.test.ts`                         | ✅               |
| TBD    | `devicesSlice` has `discoveredPeers` field                               | unit      | `bun test -- src/store/slices/__tests__/devicesSlice.test.ts`                        | ❌ Wave 0 needed |
| TBD    | `useSetupFlow` returns correct stepInfo and direction                    | unit      | `bun test -- src/hooks/__tests__/useSetupFlow.test.ts`                               | ❌ Wave 0 needed |
| TBD    | `diffPeerSnapshots` utility produces correct discovered/lost events      | unit      | `bun test -- src/api/daemon/__tests__/events.test.ts`                                | ❌ Wave 0 needed |
| TBD    | `PairingNotificationProvider` uses `usePairingEvents` (no p2p.ts import) | unit      | `bun test -- src/components/__tests__/PairingNotificationProvider.realtime.test.tsx` | ✅               |
| TBD    | No remaining imports from `src/api/p2p`                                  | smoke     | `grep -r "from '@/api/p2p'" src/` (exit code 0 = clean)                              | ✅               |

### Sampling Rate

- **Per task commit:** Quick run command for affected test files
- **Per wave merge:** Full suite (`bun test`)
- **Phase gate:** Full suite green + `grep` smoke test for zero p2p.ts imports before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src/store/slices/__tests__/devicesSlice.test.ts` — test discoveredPeers reducer actions (set, clear)
- [ ] `src/hooks/__tests__/useSetupFlow.test.ts` — test stepInfo, direction, runAction
- [ ] `src/api/daemon/__tests__/events.test.ts` — test `diffPeerSnapshots` utility
- [ ] `vitest.config.ts` already exists at project root — no framework install needed

---

## Sources

### Primary (HIGH confidence)

- `src/hooks/useDaemonEvents.ts` — existing `usePairingEvents` pattern, Phase 79 established
- `src/lib/daemon-ws.ts` — `daemonWs.subscribe` API, already has typed `DaemonWsEvent<T>`
- `src/api/daemon/pairing.ts` — canonical pairing types (P2PPairingVerificationEvent, etc.)
- `src/api/daemon/device.ts` — canonical device sync settings types
- `src/store/slices/devicesSlice.ts` — existing Redux slice pattern

### Secondary (MEDIUM confidence)

- `src/__tests__/lib/daemon-ws.test.ts` — direct daemonWs delivery pattern verified
- `src/__tests__/hooks/useDaemonEvents.test.ts` — existing hook test patterns
- `src/__tests__/hooks/useDeviceDiscovery.test.ts` — existing discovery test patterns
- `src/components/__tests__/PairingNotificationProvider.realtime.test.tsx` — notification test patterns

### Tertiary (LOW confidence)

- TypeScript user-defined type guard patterns — standard TypeScript idiom, not project-specific

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — no new dependencies, all infrastructure exists
- Architecture: HIGH — all decisions locked in CONTEXT.md, patterns established by Phase 79
- Pitfalls: MEDIUM — mechanical refactor with known risks (circular imports, p2p.ts deletion), well-understood

**Research date:** 2026-04-02
**Valid until:** ~30 days (stable TypeScript/React patterns, no fast-moving libraries in scope)

# Phase 34: Optimize JoinPickDevice Page - Research

**Researched:** 2026-03-16
**Domain:** React frontend UX — event-driven device discovery, CSS/Framer Motion animation, custom hooks
**Confidence:** HIGH

## Summary

Phase 34 is a pure frontend change that replaces the 3-second polling loop in `SetupPage.tsx` (lines 202-212) with event-driven device discovery, and replaces the spinner in `JoinPickDeviceStep.tsx` with a Bluetooth/AirDrop-like pulse/ripple animation. All backend events are already emitted; no Rust changes are needed.

The codebase uses Framer Motion v12 for step transitions and has Tailwind CSS v4 with pre-existing `@keyframes` in `App.css`. The pattern for Tauri event listeners is established: async setup returning an unlisten function, with cleanup on `useEffect` teardown. Multiple existing custom hooks (`useClipboardEventStream`, `useFileSyncNotifications`) demonstrate the exact hook structure needed for `useDeviceDiscovery`.

The existing test suite for `JoinPickDeviceStep` and `SetupPage` polling must be updated: the polling-specific test file (`setup-peer-discovery-polling.test.tsx`) will need rewriting to cover event-driven behavior, and a new test for `useDeviceDiscovery` hook itself will be needed.

**Primary recommendation:** Extract device discovery logic into `useDeviceDiscovery()` hook that replaces the polling useEffect; add Tailwind `@keyframes` ripple animation (pure CSS for the pulse circle, Framer Motion `AnimatePresence` for device entry/exit).

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

**Scanning Animation Style**

- Use **pulse/ripple animation** (concentric circles expanding outward) centered on a **Radar/Search icon** (Lucide)
- Animation conveys "actively scanning for devices" similar to Bluetooth/AirDrop discovery
- When first device is discovered, pulse animation **shrinks and moves up** to become a compact scanning indicator positioned **above the device list** (between subtitle and list)
- Compact indicator: small pulsing dot + "Scanning..." text, doesn't take much space

**Device Appearance & Disappearance**

- New devices appear with **fade-in + slide-down** animation (similar to iOS Bluetooth discovery)
- When device is lost (mDNS expired), device card **fades out and is removed** from list
- **No connection status indicator** (green/red dot) — all shown devices are "available"
- Device name updates (DeviceAnnounce) happen **silently** with no visual feedback

**Scanning Lifecycle**

- Initial pulse animation shows for **10 seconds** before transitioning to "no devices found" empty state
- After timeout, **event listeners continue running in background** — if a device appears later, UI automatically switches from empty state to device list
- "Rescan" button **resets to full pulse animation** with fresh 10-second timeout
- Empty state includes **troubleshooting tips**: confirm same network, confirm main device has pairing enabled, check firewall settings

**Event-Driven Architecture**

- **Hybrid approach**: initial `getP2PPeers()` call on mount + event listeners for incremental updates
- Listen to three existing backend events: `p2p-peer-discovery-changed`, `p2p-peer-connection-changed`, `p2p-peer-name-updated`
- **Completely remove 3-second polling** (`setInterval` in SetupPage.tsx:202-212) — no fallback
- State management extracted into a **custom `useDeviceDiscovery()` hook** encapsulating event listeners, initial load, timeout, and peer state management
- Pure frontend change — backend already emits all needed events

### Claude's Discretion

- Exact CSS animation implementation (keyframes, timing functions) for pulse ripple
- Framer Motion vs pure CSS for animations
- Exact layout spacing and typography for scanning indicator
- Hook internal implementation details

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope
</user_constraints>

---

## Standard Stack

### Core

| Library       | Version   | Purpose                                                    | Why Standard                                                                  |
| ------------- | --------- | ---------------------------------------------------------- | ----------------------------------------------------------------------------- |
| framer-motion | ^12.23.26 | Animated `motion.div`, `AnimatePresence`                   | Already used in `StepLayout.tsx`, `PairedDevicesPanel.tsx` — project standard |
| lucide-react  | ^0.577.0  | Radar icon for scanning center                             | Already imported in `JoinPickDeviceStep.tsx`                                  |
| Tailwind CSS  | ^4.1.18   | `animate-pulse`, custom `@keyframes` ripple                | App.css already has `@keyframes` patterns                                     |
| react-i18next | existing  | New i18n keys for scanning indicator, troubleshooting tips | Project i18n standard                                                         |

### Supporting

| Library                         | Version            | Purpose                          | When to Use             |
| ------------------------------- | ------------------ | -------------------------------- | ----------------------- |
| @tauri-apps/api/event           | existing (Tauri 2) | `listen()` for P2P events        | All event subscriptions |
| Vitest + @testing-library/react | ^4.0.17 / ^16.3.2  | Hook unit tests, component tests | Always                  |

### Alternatives Considered

| Instead of                                      | Could Use               | Tradeoff                                                                                                                   |
| ----------------------------------------------- | ----------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Pure CSS @keyframes for ripple                  | Framer Motion keyframes | Framer Motion adds JS overhead for a purely decorative animation; CSS keyframes are preferred for infinite loops (lighter) |
| Framer Motion `AnimatePresence` for device list | CSS transition classes  | Framer Motion is already used and handles unmount animations correctly — use it                                            |

**Installation:** No new packages needed. All libraries already in `package.json`.

---

## Architecture Patterns

### Recommended Project Structure

```
src/
├── hooks/
│   └── useDeviceDiscovery.ts        # NEW: event-driven discovery hook
├── pages/
│   └── setup/
│       ├── JoinPickDeviceStep.tsx   # MODIFY: add ScanningPulse, AnimatePresence list
│       └── types.ts                 # MODIFY: update JoinPickDeviceStepProps
└── pages/
    └── SetupPage.tsx                # MODIFY: remove polling useEffect, use hook
```

### Pattern 1: Event-Driven Tauri Listener Hook

**What:** A custom hook that sets up multiple Tauri event listeners via async `listen()`, performs an initial fetch, manages a timer, and returns derived state.
**When to use:** Whenever a component needs to react to backend events for its lifetime.

**Established project pattern** (from `useClipboardEventStream.ts`):

```typescript
// Pattern: async setup with cancellation guard, returns cleanup
useEffect(() => {
  if (!enabled) return
  let cancelled = false

  const unlistenPromise = listen<EventPayload>('event-name', event => {
    if (cancelled) return
    // handle event
  })

  return () => {
    cancelled = true
    unlistenPromise.then(fn => fn())
  }
}, [enabled])
```

**For useDeviceDiscovery, combine three listeners:**

```typescript
// Source: project pattern from useClipboardEventStream.ts + SetupPage.tsx
export function useDeviceDiscovery(active: boolean) {
  const [peers, setPeers] = useState<DiscoveredPeer[]>([])
  const [scanPhase, setScanPhase] = useState<'scanning' | 'hasDevices' | 'empty'>('scanning')
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const resetScan = useCallback(() => {
    setScanPhase('scanning')
    // restart 10s timer
  }, [])

  useEffect(() => {
    if (!active) return
    let cancelled = false

    // Initial load
    getP2PPeers().then(list => {
      if (cancelled) return
      if (list.length > 0) setScanPhase('hasDevices')
      setPeers(...)
    })

    // Start 10s timeout
    timeoutRef.current = setTimeout(() => {
      setScanPhase(prev => prev === 'scanning' ? 'empty' : prev)
    }, 10_000)

    // Three event listeners
    const discoveryPromise = onP2PPeerDiscoveryChanged(event => {
      if (cancelled) return
      if (event.discovered) {
        setPeers(prev => addOrUpdate(prev, event))
        setScanPhase('hasDevices')
      } else {
        setPeers(prev => prev.filter(p => p.id !== event.peerId))
        // if now empty and was hasDevices, switch to empty
      }
    })
    const connectionPromise = onP2PPeerConnectionChanged(...)
    const namePromise = onP2PPeerNameUpdated(...)

    return () => {
      cancelled = true
      if (timeoutRef.current) clearTimeout(timeoutRef.current)
      discoveryPromise.then(fn => fn())
      connectionPromise.then(fn => fn())
      namePromise.then(fn => fn())
    }
  }, [active])

  return { peers, scanPhase, resetScan }
}
```

### Pattern 2: Pulse Ripple Animation (Pure CSS)

**What:** Concentric expanding circles using `@keyframes` — lightweight, no JS runtime cost.
**When to use:** Infinite decorative animations where no state-driven control is needed.

The project already defines `@keyframes` in `App.css`. Add ripple there:

```css
/* Source: App.css pattern */
@keyframes ripple {
  0% {
    transform: scale(1);
    opacity: 0.6;
  }
  100% {
    transform: scale(2.5);
    opacity: 0;
  }
}
```

Tailwind class approach in JSX (two concentric rings with delay):

```tsx
// Central icon with two expanding rings
<div className="relative flex items-center justify-center">
  {/* Ring 1 */}
  <div className="absolute h-24 w-24 rounded-full bg-primary/20 animate-ripple" />
  {/* Ring 2 — delayed */}
  <div className="absolute h-24 w-24 rounded-full bg-primary/20 animate-ripple [animation-delay:0.75s]" />
  {/* Icon */}
  <div className="relative z-10 flex h-16 w-16 items-center justify-center rounded-full bg-primary/10">
    <Radar className="h-8 w-8 text-primary" />
  </div>
</div>
```

### Pattern 3: Device List with AnimatePresence (Framer Motion)

**What:** Wrap device list items in `AnimatePresence` so items animate in/out.
**When to use:** List items that can be dynamically added or removed.

**Established project pattern** (from `PairedDevicesPanel.tsx`):

```tsx
// Source: src/components/device/PairedDevicesPanel.tsx lines 248-267
<AnimatePresence>
  {isExpanded && (
    <motion.div
      initial={{ height: 0, opacity: 0 }}
      animate={{ height: 'auto', opacity: 1 }}
      exit={{ height: 0, opacity: 0 }}
      transition={{ duration: 0.2, ease: 'easeInOut' }}
    >
      ...
    </motion.div>
  )}
</AnimatePresence>
```

For device list items (fade-in + slide-down):

```tsx
<AnimatePresence>
  {peers.map(peer => (
    <motion.div
      key={peer.id}
      initial={{ opacity: 0, y: -8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 4 }}
      transition={{ duration: 0.2, ease: 'easeOut' }}
    >
      {/* device card content */}
    </motion.div>
  ))}
</AnimatePresence>
```

### Pattern 4: Scanning Phase Transition

**What:** When `scanPhase` transitions from `'scanning'` to `'hasDevices'`, the large pulse animation shrinks and moves up to a compact indicator.
**Recommendation:** Use `AnimatePresence` to swap between the large pulse block and the compact indicator — same pattern as step transitions.

```tsx
// Large scanning state (scanPhase === 'scanning')
<AnimatePresence mode="wait">
  {scanPhase === 'scanning' && (
    <motion.div key="scanning-full" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}>
      {/* Large ripple animation */}
    </motion.div>
  )}
  {scanPhase === 'hasDevices' && (
    <motion.div key="scanning-compact" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}>
      {/* Compact: small dot + "Scanning..." */}
      {/* Then device list below */}
    </motion.div>
  )}
  {scanPhase === 'empty' && (
    <motion.div key="empty-state" ...>
      {/* Troubleshooting tips + Rescan button */}
    </motion.div>
  )}
</AnimatePresence>
```

### Anti-Patterns to Avoid

- **Polling in a useEffect with setInterval:** Already in the codebase at `SetupPage.tsx:202-212` — REMOVE completely, replace with hook.
- **Keeping scanning state in SetupPage instead of the hook:** The hook owns the state; `SetupPage` just passes props down. Keeps `SetupPage` from growing further.
- **Using `if let` equivalent (`if (event.discovered)`) without handling the `else`:** Per CLAUDE.md, handle both cases explicitly — device lost case must update state.
- **Not cleaning up the timeout ref on resetScan:** Always clear existing timeout before setting a new one.

---

## Don't Hand-Roll

| Problem                                    | Don't Build                   | Use Instead                                  | Why                                                                                      |
| ------------------------------------------ | ----------------------------- | -------------------------------------------- | ---------------------------------------------------------------------------------------- |
| Fade-in/out of list items on mount/unmount | CSS class toggling with state | `AnimatePresence` + `motion.div`             | Unmount animations require keeping component in DOM briefly — Framer Motion handles this |
| Cross-browser `@keyframes` ripple          | Vendor prefix handling        | Tailwind + modern CSS                        | Tailwind handles vendor prefixes; all target platforms are Chromium-based (Tauri)        |
| Cleanup of async Tauri listeners           | Manual Promise.all tracking   | Same pattern as `useClipboardEventStream.ts` | Pattern already tested and correct                                                       |

**Key insight:** Tauri's `listen()` is async — the unlisten function must be awaited before calling. The established pattern of storing the Promise and calling `.then(fn => fn())` in cleanup is already validated in the codebase.

---

## Common Pitfalls

### Pitfall 1: Stale Closure in Timer Callback

**What goes wrong:** `setScanPhase(prev => ...)` inside a `setTimeout` may capture stale state if not using the functional updater form.
**Why it happens:** Timer callback closes over the state value at setup time.
**How to avoid:** Always use `setScanPhase(prev => prev === 'scanning' ? 'empty' : prev)` — the functional form reads current state.
**Warning signs:** Empty state appearing even when devices were discovered after the timer was set.

### Pitfall 2: Framer Motion AnimatePresence Needs Stable Keys

**What goes wrong:** List items re-animate every render if keys change.
**Why it happens:** `AnimatePresence` tracks items by `key` prop — using array index as key causes spurious animations.
**How to avoid:** Always use `peer.id` (peerId string) as the key for device cards.
**Warning signs:** All items flash on any state update.

### Pitfall 3: Multiple Listener Registrations

**What goes wrong:** If `active` prop flips rapidly or `useEffect` deps change, multiple listeners accumulate.
**Why it happens:** New listeners registered before old ones are cleaned up.
**How to avoid:** The cleanup function in `useEffect` must return before the new effect runs — this is React's guarantee. Use the `cancelled` guard pattern from `useClipboardEventStream.ts`. Do NOT call `addEventListener` outside of useEffect.
**Warning signs:** Devices appearing doubled or events firing twice.

### Pitfall 4: Test File Covers Polling — Must Be Rewritten

**What goes wrong:** `setup-peer-discovery-polling.test.tsx` explicitly tests `setInterval` behavior ("starts polling", "stops polling"). After Phase 34, this test will fail or test removed behavior.
**Why it happens:** The test mocks `getP2PPeers` being called repeatedly via timer.
**How to avoid:** Rewrite the test to cover event-driven behavior: verify that event listeners are set up, verify initial `getP2PPeers()` call, verify state transitions based on events.
**Warning signs:** CI fails on the polling test after polling is removed.

### Pitfall 5: `onP2PPeerConnectionChanged` May Not Signal "Lost"

**What goes wrong:** If `p2p-peer-connection-changed` fires with `connected: false` but the peer is still "discovered" (just not connected), the device should remain in list.
**Why it happens:** Discovery (mDNS) and connection are separate concepts. The CONTEXT.md says "No connection status indicator — all shown devices are available". Devices are removed only on `p2p-peer-discovery-changed` with `discovered: false`.
**How to avoid:** `p2p-peer-connection-changed` events should NOT remove devices from the list. Only `p2p-peer-discovery-changed` with `discovered: false` triggers removal.

---

## Code Examples

Verified patterns from project source:

### Tauri Event Listener Cleanup Pattern

```typescript
// Source: src/hooks/useClipboardEventStream.ts
useEffect(() => {
  if (!enabled) return
  let cancelled = false

  const unlistenPromise = listen<EventPayload>('event-name', event => {
    if (cancelled) return
    // handle
  })

  return () => {
    cancelled = true
    unlistenPromise.then(fn => fn())
  }
}, [enabled])
```

### P2P Discovery Event Type

```typescript
// Source: src/api/p2p.ts lines 382-400
// P2PPeerDiscoveryChangedEvent has:
//   peerId: string
//   deviceName: string
//   addresses: string[]
//   discovered: boolean   ← true = add, false = remove
```

### AnimatePresence for Conditional UI Blocks

```tsx
// Source: src/components/device/PairedDevicesPanel.tsx
<AnimatePresence>
  {condition && (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.2 }}
    >
      ...
    </motion.div>
  )}
</AnimatePresence>
```

### Custom @keyframes in App.css (Existing Pattern)

```css
/* Source: src/App.css */
@keyframes fade-in {
  0% {
    opacity: 0;
    transform: translateY(10px);
  }
  100% {
    opacity: 1;
    transform: translateY(0);
  }
}
.animate-fade-in {
  animation: fade-in 0.6s ease-out forwards;
}
```

New ripple animation to add:

```css
@keyframes ripple-out {
  0% {
    transform: scale(1);
    opacity: 0.5;
  }
  100% {
    transform: scale(2.8);
    opacity: 0;
  }
}
.animate-ripple {
  animation: ripple-out 1.8s ease-out infinite;
}
```

---

## State of the Art

| Old Approach                   | Current Approach                            | When Changed | Impact                                                   |
| ------------------------------ | ------------------------------------------- | ------------ | -------------------------------------------------------- |
| `setInterval` polling every 3s | Event-driven via `listen()`                 | Phase 34     | Eliminates unnecessary network calls, instant updates    |
| Single spinner for scanning    | Pulse/ripple + phase-aware UI               | Phase 34     | Better UX that clearly communicates "actively searching" |
| Empty state with Scan Again    | Empty state + troubleshooting tips + Rescan | Phase 34     | Actionable guidance for users who cannot find devices    |

**Deprecated/outdated after Phase 34:**

- `handleRefreshPeers` callback in `SetupPage.tsx`: polling useEffect replaced; the callback may still be used for manual "Rescan" via `resetScan()` from hook.
- `isScanningInitial` state in `SetupPage.tsx`: replaced by `scanPhase` from `useDeviceDiscovery()`.

---

## Open Questions

1. **Should `p2p-peer-connection-changed` affect the device list at all?**
   - What we know: CONTEXT.md says "No connection status indicator — all shown devices are available". Removal only on `p2p-peer-discovery-changed` with `discovered: false`.
   - What's unclear: Whether `connection-changed` with `connected: false` still implies the device should be removed (if it disconnected without a discovery-changed event).
   - Recommendation: Handle `connection-changed` as a silent update only. Only remove on `discovery-changed` with `discovered: false`. Observe actual backend behavior during implementation.

2. **i18n keys for compact scanning indicator and troubleshooting tips**
   - What we know: `en-US.json` has `setup.joinPickDevice.empty.description` but it's generic.
   - What's unclear: Whether to add new keys (`scanning.compact`, `empty.tips.*`) or extend existing ones.
   - Recommendation: Add new i18n keys; do not modify existing key content (backward compatible with any cached translations).

---

## Validation Architecture

### Test Framework

| Property           | Value                                                              |
| ------------------ | ------------------------------------------------------------------ |
| Framework          | Vitest ^4.0.17 + @testing-library/react ^16.3.2                    |
| Config file        | `vite.config.ts` (test section), setup file at `src/test/setup.ts` |
| Quick run command  | `bun test src/hooks/useDeviceDiscovery.test.ts --run`              |
| Full suite command | `bun test --run`                                                   |

### Phase Requirements → Test Map

| Req ID | Behavior                                                      | Test Type | Automated Command                                                          | File Exists?       |
| ------ | ------------------------------------------------------------- | --------- | -------------------------------------------------------------------------- | ------------------ |
| N/A    | `useDeviceDiscovery` initial load calls `getP2PPeers()`       | unit      | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`            | ❌ Wave 0          |
| N/A    | Discovery event with `discovered: true` adds peer to list     | unit      | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`            | ❌ Wave 0          |
| N/A    | Discovery event with `discovered: false` removes peer         | unit      | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`            | ❌ Wave 0          |
| N/A    | After 10s with no devices, `scanPhase` becomes `'empty'`      | unit      | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`            | ❌ Wave 0          |
| N/A    | `resetScan()` resets to `'scanning'` phase                    | unit      | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`            | ❌ Wave 0          |
| N/A    | Device appearing after empty state switches phase back        | unit      | `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`            | ❌ Wave 0          |
| N/A    | JoinPickDeviceStep renders ripple animation in scanning phase | unit      | `bun test src/pages/setup/__tests__/ --run`                                | ❌ Wave 0          |
| N/A    | Polling test file updated to cover event-driven behavior      | unit      | `bun test src/pages/__tests__/setup-peer-discovery-polling.test.tsx --run` | ✅ (needs rewrite) |

### Sampling Rate

- **Per task commit:** `bun test src/hooks/__tests__/useDeviceDiscovery.test.ts --run`
- **Per wave merge:** `bun test --run`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src/hooks/__tests__/useDeviceDiscovery.test.ts` — covers all hook behavior (new file)
- [ ] `src/pages/__tests__/setup-peer-discovery-polling.test.tsx` — REWRITE to test event-driven behavior instead of polling
- [ ] `src/pages/setup/__tests__/JoinPickDeviceStepScanning.test.tsx` — optional: covers scanning phase rendering

_(Existing test infrastructure (Vitest, @testing-library/react, framer-motion mock pattern) covers all needs — no new framework install required)_

---

## Sources

### Primary (HIGH confidence)

- Project source: `src/pages/SetupPage.tsx` — polling useEffect at lines 202-212, existing state shape
- Project source: `src/pages/setup/JoinPickDeviceStep.tsx` — existing component to be modified
- Project source: `src/hooks/useClipboardEventStream.ts` — canonical Tauri listener hook pattern
- Project source: `src/api/p2p.ts` lines 342-400 — three event listener functions and their payload types
- Project source: `src/components/device/PairedDevicesPanel.tsx` — AnimatePresence + motion.div pattern
- Project source: `src/App.css` — existing @keyframes patterns
- Project source: `src/i18n/locales/en-US.json` — existing i18n keys for `joinPickDevice`
- Project source: `src/pages/__tests__/setup-peer-discovery-polling.test.tsx` — test to be rewritten

### Secondary (MEDIUM confidence)

- `package.json`: framer-motion ^12.23.26, tailwindcss ^4.1.18, lucide-react ^0.577.0 — confirmed versions

### Tertiary (LOW confidence)

- None

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all libraries confirmed in package.json, all patterns confirmed in codebase
- Architecture: HIGH — hook pattern identical to `useClipboardEventStream`, animation patterns identical to existing usages
- Pitfalls: HIGH — polling test rewrite requirement confirmed by reading test file; cleanup patterns confirmed by code

**Research date:** 2026-03-16
**Valid until:** 2026-04-16 (stable stack, no fast-moving dependencies)

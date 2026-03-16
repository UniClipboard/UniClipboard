# Phase 34: Optimize JoinPickDevice page: event-driven discovery with scanning UX - Context

**Gathered:** 2026-03-16
**Status:** Ready for planning

<domain>
## Phase Boundary

Optimize the JoinPickDeviceStep page UX to provide a Bluetooth/AirDrop-like scanning experience with pulse animation, and switch from 3-second polling to event-driven device discovery using existing backend events. Pure frontend change — no backend modifications needed.

</domain>

<decisions>
## Implementation Decisions

### Scanning Animation Style

- Use **pulse/ripple animation** (concentric circles expanding outward) centered on a **Radar/Search icon** (Lucide)
- Animation conveys "actively scanning for devices" similar to Bluetooth/AirDrop discovery
- When first device is discovered, pulse animation **shrinks and moves up** to become a compact scanning indicator positioned **above the device list** (between subtitle and list)
- Compact indicator: small pulsing dot + "Scanning..." text, doesn't take much space

### Device Appearance & Disappearance

- New devices appear with **fade-in + slide-down** animation (similar to iOS Bluetooth discovery)
- When device is lost (mDNS expired), device card **fades out and is removed** from list
- **No connection status indicator** (green/red dot) — all shown devices are "available"
- Device name updates (DeviceAnnounce) happen **silently** with no visual feedback

### Scanning Lifecycle

- Initial pulse animation shows for **10 seconds** before transitioning to "no devices found" empty state
- After timeout, **event listeners continue running in background** — if a device appears later, UI automatically switches from empty state to device list
- "Rescan" button **resets to full pulse animation** with fresh 10-second timeout
- Empty state includes **troubleshooting tips**: confirm same network, confirm main device has pairing enabled, check firewall settings

### Event-Driven Architecture

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

</decisions>

<specifics>
## Specific Ideas

- Reference: Bluetooth device scanning UX — pulse animation indicating "searching", devices appearing one by one as discovered
- Reference: AirDrop discovery — clean, centered animation with smooth transitions
- The transition from "scanning" to "device list" should feel smooth and natural, not jarring
- Empty state should be helpful, not just "nothing found" — guide users to solve the problem

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `onP2PPeerDiscoveryChanged()` (src/api/p2p.ts:382-400): Listens for peer discovered/lost events with peerId, deviceName, addresses, discovered flag
- `onP2PPeerConnectionChanged()` (src/api/p2p.ts:342-357): Listens for connection state changes
- `onP2PPeerNameUpdated()` (src/api/p2p.ts:362-377): Listens for device name updates
- `getP2PPeers()` (src/api/p2p.ts:236-243): One-shot fetch of current discovered peers for initial load
- Framer Motion already used for step transitions in StepLayout (AnimatePresence)
- Lucide icons available (Radar, Search, RefreshCw, Monitor, Smartphone, Laptop)

### Established Patterns

- SetupPage uses `useCallback`/`useEffect` for event listener management with proper cleanup
- Tauri event listeners return unlisten functions for cleanup
- `P2PPeerDiscoveryChangedEvent` has `discovered: boolean` flag for add/remove differentiation
- `isScanningInitial` state already exists but needs better UX treatment

### Integration Points

- `SetupPage.tsx`: Remove polling useEffect (lines 202-212), replace with hook usage
- `JoinPickDeviceStep.tsx`: Add pulse animation component, update empty state with troubleshooting tips
- `types.ts`: May need to update `JoinPickDeviceStepProps` for new scanning states
- New file: `src/hooks/useDeviceDiscovery.ts` for the custom hook

</code_context>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 34-optimize-joinpickdevice-page-event-driven-discovery-with-scanning-ux_
_Context gathered: 2026-03-16_

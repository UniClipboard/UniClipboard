---
phase: 34-optimize-joinpickdevice-page-event-driven-discovery-with-scanning-ux
verified: 2026-03-16T06:30:00Z
status: human_needed
score: 16/16 must-haves verified
human_verification:
  - test: 'Visual scanning animation'
    expected: 'Pulse/ripple animation with 3 concentric rings and Radar icon centered, smooth 10-second transition to empty state, then troubleshooting tips with Rescan button that resets to full pulse'
    why_human: 'CSS animation and framer-motion transitions require visual confirmation; cannot verify animation rendering or timing feel programmatically'
  - test: 'Compact scanning indicator above device list'
    expected: "When a device is discovered, the full pulse animation is replaced by a small pulsing dot + 'Scanning...' text above the animated device list"
    why_human: "AnimatePresence mode='wait' transitions between phases require visual observation"
  - test: 'Device list fade-in/slide-down animation'
    expected: 'New devices appear with opacity 0->1 and y -8->0 animation; devices removed via mDNS expiry fade out (opacity 0, y 4)'
    why_human: 'framer-motion AnimatePresence item animations cannot be asserted programmatically'
  - test: 'Language switch updates anonymous device names immediately'
    expected: 'Switching locale from EN to ZH causes anonymous device names to immediately show the Chinese fallback without rescan'
    why_human: 'i18n reactivity requires runtime locale switching to verify'
  - test: 'No 3-second polling in network traffic'
    expected: 'Network tab shows getP2PPeers called once on mount, not repeated every 3 seconds'
    why_human: 'Network tab observation cannot be automated in this context'
---

# Phase 34: Optimize JoinPickDevice Page — Verification Report

**Phase Goal:** Replace 3-second polling with event-driven device discovery using existing backend events, and transform the JoinPickDevice step into a Bluetooth/AirDrop-like scanning experience with pulse animation, animated device list, and troubleshooting empty state.
**Verified:** 2026-03-16T06:30:00Z
**Status:** human_needed
**Re-verification:** No — initial verification

## Requirements Coverage Note

SCAN-01 through SCAN-06 are referenced in ROADMAP.md for Phase 34 but are NOT defined in `.planning/REQUIREMENTS.md`. These IDs are phase-internal requirements that exist only in the PLAN frontmatter and ROADMAP. The REQUIREMENTS.md file covers separate milestone requirements (LOG, FLOW, SEQ, CT, GSYNC, KB, FSYNC, FCLIP, LINK, LINK). This is an ORPHANED set of requirement IDs — not blocking for this phase since all behaviors are verified directly against the plan must_haves, but the traceability table in REQUIREMENTS.md does not include them.

## Goal Achievement

### Observable Truths

| #   | Truth                                                                                  | Status          | Evidence                                                                                                                                                             |
| --- | -------------------------------------------------------------------------------------- | --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | useDeviceDiscovery hook returns peers, scanPhase, and resetScan                        | VERIFIED        | `src/hooks/useDeviceDiscovery.ts` line 176: `return { peers, scanPhase, resetScan }`                                                                                 |
| 2   | Hook sets up three Tauri event listeners with proper cleanup                           | VERIFIED        | Lines 110-172: onP2PPeerDiscoveryChanged, onP2PPeerNameUpdated, onP2PPeerConnectionChanged; cleanup via `.then(fn => fn())`                                          |
| 3   | 10-second timeout transitions scanPhase from scanning to empty                         | VERIFIED        | Line 54-56: `setScanPhase(prev => (prev === 'scanning' ? 'empty' : prev))` after 10_000ms; Test 3 passes                                                             |
| 4   | Device appearing after empty state transitions back to hasDevices                      | VERIFIED        | Lines 128: `setScanPhase('hasDevices')` on discovery event; Test 5 passes                                                                                            |
| 5   | resetScan resets to scanning phase with fresh timeout                                  | VERIFIED        | Lines 82-87: clears peers, resets phase, calls startTimeout and loadPeers; Test 6 passes                                                                             |
| 6   | When active goes false, hook resets peers to [] and scanPhase to scanning              | VERIFIED        | Lines 90-95 (deactivation path) and lines 161-164 (cleanup); Test 7 passes                                                                                           |
| 7   | getP2PPeers() failure calls onError callback — does not leave hook in broken state     | VERIFIED        | Lines 72-78: catch block calls onErrorRef.current, sets scanPhase to 'scanning'; Tests 8 and 11 pass                                                                 |
| 8   | Hook stores raw deviceName from backend (string or null) — no fallback mapping in hook | VERIFIED        | Line 65: `deviceName: p.deviceName ?? null`; Test 10 passes                                                                                                          |
| 9   | Hook has NO direct dependency on sonner/toast                                          | VERIFIED        | No import of 'sonner' in useDeviceDiscovery.ts (grep confirmed 0 matches)                                                                                            |
| 10  | i18n keys exist for scanning indicator and troubleshooting tips in both locales        | VERIFIED        | en-US: `scanning.compact = "Scanning..."`, `empty.tips.heading = "Troubleshooting tips"`; zh-CN: `scanning.compact = "扫描中..."`, `empty.tips.heading = "排查建议"` |
| 11  | CSS ripple animation defined in App.css                                                | VERIFIED        | `src/App.css` lines 70-82: `@keyframes ripple-out` and `.animate-ripple` class                                                                                       |
| 12  | Hook state machine is covered by dedicated unit tests                                  | VERIFIED        | 11 tests pass in `src/hooks/__tests__/useDeviceDiscovery.test.ts`                                                                                                    |
| 13  | JoinPickDeviceStep shows pulse/ripple animation during scanning phase                  | VERIFIED (code) | Lines 85-91: 3 concentric `animate-ripple` divs + Radar icon; human needed for visual confirmation                                                                   |
| 14  | Compact scanning indicator above device list when devices found                        | VERIFIED (code) | Lines 110-116: ping dot + `t('scanning.compact')` text before device list                                                                                            |
| 15  | After 10s with no devices, empty state shows troubleshooting tips and Rescan button    | VERIFIED (code) | Lines 159-187: tips section + RescanButton with `onRescan` handler                                                                                                   |
| 16  | 3-second polling completely removed from SetupPage                                     | VERIFIED        | No `setInterval`, `handleRefreshPeers`, `peersLoading`, or `isScanningInitial` in SetupPage.tsx                                                                      |

**Score:** 16/16 truths verified (automated), 5 items flagged for human visual verification

### Required Artifacts

| Artifact                                                    | Expected                                          | Status   | Details                                                                                                                                          |
| ----------------------------------------------------------- | ------------------------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `src/hooks/useDeviceDiscovery.ts`                           | Event-driven discovery hook                       | VERIFIED | 177 lines; exports `useDeviceDiscovery`, `DiscoveredPeer`, `ScanPhase`                                                                           |
| `src/hooks/__tests__/useDeviceDiscovery.test.ts`            | Hook state machine unit tests                     | VERIFIED | 305 lines; 11 tests all pass                                                                                                                     |
| `src/pages/setup/types.ts`                                  | Updated JoinPickDeviceStepProps with scanPhase    | VERIFIED | Imports and re-exports `ScanPhase`, `DiscoveredPeer` from hook; `JoinPickDeviceStepProps` has `scanPhase`, `onRescan`, `peers: DiscoveredPeer[]` |
| `src/App.css`                                               | Ripple animation keyframes                        | VERIFIED | Contains `@keyframes ripple-out` (lines 70-79) and `.animate-ripple` class (lines 80-82)                                                         |
| `src/i18n/locales/en-US.json`                               | English scanning/troubleshooting i18n keys        | VERIFIED | `scanning.compact = "Scanning..."`, `empty.tips.heading = "Troubleshooting tips"` and all 3 tip keys                                             |
| `src/i18n/locales/zh-CN.json`                               | Chinese scanning/troubleshooting i18n keys        | VERIFIED | `scanning.compact = "扫描中..."`, `empty.tips.heading = "排查建议"` and all 3 tip keys                                                           |
| `src/pages/setup/JoinPickDeviceStep.tsx`                    | Scanning UX with pulse animation, AnimatePresence | VERIFIED | Contains `animate-ripple`, `AnimatePresence`, `Radar`, all 3 scan phases                                                                         |
| `src/pages/SetupPage.tsx`                                   | Hook-based discovery replacing polling            | VERIFIED | Contains `useDeviceDiscovery`, `onError` callback, `onRescan={resetScan}`                                                                        |
| `src/pages/__tests__/setup-peer-discovery-polling.test.tsx` | Rewritten event-driven tests                      | VERIFIED | Contains 'event-driven' in describe label; 5 tests pass                                                                                          |

### Key Link Verification

| From                                     | To                                       | Via                                                                                              | Status | Details                                                                                                    |
| ---------------------------------------- | ---------------------------------------- | ------------------------------------------------------------------------------------------------ | ------ | ---------------------------------------------------------------------------------------------------------- |
| `src/hooks/useDeviceDiscovery.ts`        | `src/api/p2p.ts`                         | imports onP2PPeerDiscoveryChanged, onP2PPeerConnectionChanged, onP2PPeerNameUpdated, getP2PPeers | WIRED  | All 4 functions imported and used (lines 2-7, 62, 110, 142, 156)                                           |
| `src/pages/SetupPage.tsx`                | `src/hooks/useDeviceDiscovery.ts`        | imports useDeviceDiscovery hook                                                                  | WIRED  | Line 19: `import { useDeviceDiscovery } from '@/hooks/useDeviceDiscovery'`; line 78: destructured and used |
| `src/pages/setup/JoinPickDeviceStep.tsx` | `framer-motion`                          | AnimatePresence + motion.div for device list items                                               | WIRED  | Line 1: `import { AnimatePresence, motion } from 'framer-motion'`; used at lines 74, 120                   |
| `src/pages/SetupPage.tsx`                | `src/pages/setup/JoinPickDeviceStep.tsx` | passes scanPhase, peers, onRescan from hook                                                      | WIRED  | Lines 253-266: `scanPhase={scanPhase}`, `peers={peers}`, `onRescan={resetScan}`                            |

### Requirements Coverage

| Requirement | Source Plan   | Description                                    | Status           | Evidence                                                                       |
| ----------- | ------------- | ---------------------------------------------- | ---------------- | ------------------------------------------------------------------------------ |
| SCAN-01     | 34-01-PLAN.md | Event-driven hook replaces polling             | SATISFIED        | useDeviceDiscovery hook with 3 Tauri listeners; polling removed from SetupPage |
| SCAN-02     | 34-01-PLAN.md | Hook state machine (scanning/hasDevices/empty) | SATISFIED        | ScanPhase type and 11 passing unit tests                                       |
| SCAN-03     | 34-01-PLAN.md | CSS ripple animation                           | SATISFIED        | @keyframes ripple-out + .animate-ripple in App.css                             |
| SCAN-04     | 34-01-PLAN.md | i18n keys for scanning and troubleshooting     | SATISFIED        | scanning.compact and empty.tips.\* in both locales                             |
| SCAN-05     | 34-02-PLAN.md | JoinPickDeviceStep scanning UX phases          | SATISFIED (code) | 3 phases in JoinPickDeviceStep.tsx; human visual confirmation needed           |
| SCAN-06     | 34-02-PLAN.md | Rewritten event-driven tests                   | SATISFIED        | 5 tests pass; describe block labeled 'setup event-driven device discovery'     |

**Note:** SCAN-01 through SCAN-06 are defined in PLAN frontmatter but NOT in REQUIREMENTS.md. They represent phase-internal requirements only. REQUIREMENTS.md traceability table has no entry for Phase 34 — this is expected for a UX improvement phase outside the formal v1 requirement set.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
| ---- | ---- | ------- | -------- | ------ |
| None | —    | —       | —        | —      |

No TODO/FIXME/placeholder comments, no return null stubs, no empty implementations found in any of the key files.

### Human Verification Required

#### 1. Pulse/Ripple Scanning Animation

**Test:** Run `bun tauri dev`, navigate to Setup, click "Join existing space" to reach JoinPickDevice step
**Expected:** Three concentric rings expand outward from a Radar icon center, pulsing with staggered 0.6s delays (1.8s animation duration, ease-out infinite)
**Why human:** CSS animation rendering and visual timing require direct observation

#### 2. Phase Transition: scanning to hasDevices

**Test:** While in JoinPickDevice step, have a device appear on the LAN (or trigger `onP2PPeerDiscoveryChanged` via mock)
**Expected:** Full pulse animation fades out, compact ping dot + "Scanning..." text appears above an animated device list; device card slides down with fade-in
**Why human:** AnimatePresence mode="wait" transition and motion.div entrance animations require visual confirmation

#### 3. Phase Transition: scanning to empty state

**Test:** Stay on JoinPickDevice with no devices for 10 seconds
**Expected:** Full pulse animation fades out, empty state appears with troubleshooting tips (same network, pairing enabled, firewall), and "Scan again" button. Clicking "Scan again" resets to full pulse animation
**Why human:** Transition timing and UX flow require visual confirmation

#### 4. Language switch updates anonymous device names immediately

**Test:** Trigger discovery of a device with null deviceName, then switch language from EN to ZH
**Expected:** The fallback label changes from "unknownDevice" to the Chinese equivalent immediately, without rescan
**Why human:** i18n reactivity requires runtime locale switching to observe

#### 5. No polling in network tab

**Test:** Open DevTools Network tab, navigate to JoinPickDevice step, observe for 10+ seconds
**Expected:** getP2PPeers (or equivalent IPC call) is called exactly once on mount, never again until Rescan is clicked
**Why human:** Network tab observation cannot be automated

### Gaps Summary

No automated gaps found. All code artifacts exist, are substantive, and are correctly wired. All 16 must-have truths pass automated verification. The phase goal is achieved at the code level.

The 5 human verification items are all visual/runtime behaviors that cannot be verified programmatically (animation rendering, transition timing, language reactivity). These do not represent code deficiencies — the implementation is complete and correct.

---

_Verified: 2026-03-16T06:30:00Z_
_Verifier: Claude (gsd-verifier)_

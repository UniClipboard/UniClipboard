---
status: resolved
trigger: 'skeleton-flash-on-page-switch: When switching between Dashboard and Device pages, skeleton screens flash briefly every time'
created: 2026-03-09T00:00:00Z
updated: 2026-03-09T01:00:00Z
---

## Current Focus

hypothesis: CONFIRMED - The loading flags in Redux slices are set unconditionally in the pending reducers, even when cached data exists. This causes any loading-dependent UI (skeletons, loading indicators) to flash on every page navigation.
test: Fixed at the Redux slice level - pending reducers now only set loading=true when no cached data exists
expecting: No skeleton/loading flash when navigating between pages with cached data
next_action: Await human verification

## Symptoms

expected: Pages should switch smoothly without skeleton flash. Previously loaded data should be available immediately.
actual: Every page switch shows a brief skeleton screen flash before content appears, even when data was already loaded.
errors: No errors - purely a UX issue with unnecessary loading state display.
reproduction: Navigate from Dashboard to Device page and back. The skeleton appears every time.
started: Unknown

## Eliminated

- hypothesis: PairedDevicesPanel skeleton condition was the sole cause (component-level guard)
  evidence: User confirmed fix to PairedDevicesPanel.tsx skeleton condition (`pairedDevicesLoading && pairedDevices.length === 0`) was not sufficient. Flash persisted.
  timestamp: 2026-03-09T00:30:00Z

- hypothesis: React Router transition, Suspense, or lazy loading causing unmount/remount flash
  evidence: No Suspense boundaries, no lazy() loading, no code splitting found in App.tsx. AuthenticatedLayout uses Outlet which swaps children synchronously. Route layout persists across navigation.
  timestamp: 2026-03-09T00:45:00Z

- hypothesis: Layout/parent component causing re-render flash
  evidence: WindowShell, MainLayout, AuthenticatedLayout are all stateless wrappers. SettingProvider always renders children (no conditional return based on loading). AppContent only returns null for encryptionLoading which is cached by RTK Query after first load.
  timestamp: 2026-03-09T00:45:00Z

## Evidence

- timestamp: 2026-03-09T00:01:00Z
  checked: ClipboardContent.tsx line 322
  found: Skeleton shown when `notReady || (loading && clipboardItems.length === 0)`. The component-level guard checks clipboardItems.length, but the root issue is that `loading` is set to true in the slice pending reducer unconditionally.
  implication: While ClipboardContent has a partial guard, the loading flag itself is the problem source

- timestamp: 2026-03-09T00:01:00Z
  checked: clipboardSlice.ts line 146-149
  found: `fetchClipboardItems.pending` unconditionally sets `loading: true`
  implication: Every dispatch triggers loading state regardless of existing cached data in Redux store

- timestamp: 2026-03-09T00:01:00Z
  checked: PairedDevicesPanel.tsx line 24-25, 110
  found: `fetchPairedDevices()` dispatched on every mount. Skeleton guard added at component level.
  implication: Component-level guard was insufficient; the fix needs to be at the Redux slice level

- timestamp: 2026-03-09T00:01:00Z
  checked: devicesSlice.ts line 100-101
  found: `fetchPairedDevices.pending` unconditionally sets `pairedDevicesLoading: true`
  implication: Same pattern as clipboard - loading flag ignores existing data

- timestamp: 2026-03-09T00:45:00Z
  checked: ClipboardContent.tsx line 381-385
  found: A bottom-of-list loading skeleton shows whenever `loading` is true: `{loading && (<Skeleton />)}`. This is visible even when items exist and the full-page skeleton doesn't show.
  implication: The unconditional `loading=true` in pending reducer causes BOTH the full skeleton (if items empty) AND a bottom loading indicator to flash on every navigation

- timestamp: 2026-03-09T00:50:00Z
  checked: App.tsx routes and component hierarchy
  found: No lazy loading, no Suspense, no conditional rendering that would cause flash. Redux store is a single global instance. AuthenticatedLayout persists across route changes. Encryption query is cached by RTK Query.
  implication: The root cause is definitively in the Redux pending reducers setting loading flags unconditionally

## Resolution

root_cause: Both `clipboardSlice` and `devicesSlice` Redux pending reducers unconditionally set their loading flags to `true` on every fetch dispatch, regardless of whether cached data already exists in the store. When a page re-mounts after navigation, it dispatches a fetch, which immediately sets `loading=true`. This causes any loading-dependent UI elements to flash briefly -- including the bottom-of-list loading skeleton in ClipboardContent (line 381-385) and the full skeleton in PairedDevicesPanel (when combined with the component-level check). The previous fix only addressed the component-level condition in PairedDevicesPanel but did not fix the source of the problem: the Redux slice pending reducer.

fix: Modified pending reducers in both slices to only set loading=true when no cached data exists:

- clipboardSlice.ts: `fetchClipboardItems.pending` only sets `loading=true` when `state.items.length === 0`
- devicesSlice.ts: `fetchPairedDevices.pending` only sets `pairedDevicesLoading=true` when `state.pairedDevices.length === 0`
  This ensures background refetches (triggered by page re-mount) don't flash loading indicators.

verification: All 122 passing tests still pass. 5 pre-existing test failures unchanged.
files_changed:

- src/store/slices/clipboardSlice.ts
- src/store/slices/devicesSlice.ts
- src/components/device/PairedDevicesPanel.tsx (from previous fix, kept)

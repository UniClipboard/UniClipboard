---
status: resolved
trigger: 'Dashboard flashes stale content after clearing all clipboard history from Settings page'
created: 2026-03-13T00:00:00Z
updated: 2026-03-13T00:00:00Z
---

## Current Focus

hypothesis: CONFIRMED - StorageSection.handleClearHistory calls storageApi.clearAllClipboardHistory() but never dispatches Redux action to clear clipboard.items
test: n/a - root cause confirmed by code reading
expecting: n/a
next_action: Apply fix - dispatch clearAllItems or reset items in Redux after clear succeeds

## Symptoms

expected: Dashboard should immediately show empty state without any flash of stale content after clearing history from Settings
actual: When navigating back to Dashboard after clearing all clipboard history from Settings, old entries flash briefly before empty state appears
errors: No error messages - visual/UX issue with stale cache rendering
reproduction: 1. Have clipboard history entries 2. Navigate to Settings 3. Clear all history 4. Navigate back to Dashboard 5. Old entries flash briefly
started: Ongoing, likely since clear history feature was implemented

## Eliminated

## Evidence

- timestamp: 2026-03-13T00:01:00Z
  checked: StorageSection.tsx handleClearHistory (line 419-430)
  found: Calls storageApi.clearAllClipboardHistory() then loadStats(), but NEVER dispatches any Redux action
  implication: Redux state.clipboard.items remains stale after backend clear

- timestamp: 2026-03-13T00:01:30Z
  checked: clipboardSlice.ts fetchClipboardItems.pending reducer (line 146-155)
  found: Loading state only shown when state.items.length === 0. When items are cached, no loading spinner appears.
  implication: Stale items render instantly on Dashboard, then get replaced when async fetch completes = flash

- timestamp: 2026-03-13T00:02:00Z
  checked: clipboardSlice.ts clearAllItems thunk and fulfilled reducer (lines 94-104, 204-206)
  found: clearAllItems.fulfilled sets state.items = []. But this thunk calls clearClipboardItems (different IPC command), not clearAllClipboardHistory.
  implication: Two separate clear pathways exist; Settings uses one that bypasses Redux

## Resolution

root_cause: StorageSection.handleClearHistory() calls storageApi.clearAllClipboardHistory() (Tauri IPC) but never dispatches a Redux action to clear state.clipboard.items. When user navigates to Dashboard, stale items render from Redux cache before the async re-fetch replaces them with empty array. The pending reducer skips loading state when items exist, so there's no spinner to mask the stale content.
fix: Added resetItems sync reducer to clipboardSlice that clears items and error. StorageSection.handleClearHistory now dispatches resetItems() immediately after clearAllClipboardHistory() succeeds, so Redux state is empty before user navigates to Dashboard.
verification: TypeScript compiles clean, existing clipboardSlice tests pass. User confirmed fix works end-to-end.
files_changed:

- src/store/slices/clipboardSlice.ts (added resetItems reducer + export)
- src/components/setting/StorageSection.tsx (import dispatch + resetItems, call after clear)

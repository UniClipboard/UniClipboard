---
phase: quick-9
plan: 1
type: execute
wave: 1
depends_on: []
files_modified:
  - src/components/clipboard/ClipboardContent.tsx
autonomous: true
requirements: [QUICK-9]

must_haves:
  truths:
    - 'Relative timestamps update automatically even when no new clipboard content arrives'
    - 'Items less than 1 hour old refresh their timestamps every 30 seconds'
    - 'Items older than 1 hour refresh their timestamps every 60 seconds'
  artifacts:
    - path: 'src/components/clipboard/ClipboardContent.tsx'
      provides: 'Periodic timestamp refresh via tick counter'
      contains: 'setInterval'
  key_links:
    - from: 'tick state'
      to: 'clipboardItems useMemo'
      via: 'tick included as dependency'
      pattern: 'tick'
---

<objective>
Fix stale relative timestamps on clipboard items that never update when no new content arrives.

Purpose: When a user leaves the clipboard page open, timestamps like "2m ago" should naturally progress to "3m ago", "4m ago", etc. Currently they freeze because `useMemo` only recalculates when `reduxItems` changes.

Output: Self-refreshing timestamps with smart interval (30s for recent items, 60s baseline).
</objective>

<execution_context>
@/home/wuy6/.claude/get-shit-done/workflows/execute-plan.md
@/home/wuy6/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@src/components/clipboard/ClipboardContent.tsx
</context>

<tasks>

<task type="auto" tdd="true">
  <name>Task 1: Add periodic tick to force timestamp recalculation</name>
  <files>src/components/clipboard/ClipboardContent.tsx</files>
  <behavior>
    - Timestamps update every 30 seconds when items less than 1 hour old exist
    - Timestamps update every 60 seconds as baseline for older items
    - Timer cleans up on unmount (no memory leaks)
    - No unnecessary re-renders when clipboard list is empty
  </behavior>
  <action>
In `ClipboardContent.tsx`, add a `tick` state variable and a `useEffect` that sets up an interval to increment it. Use a smart interval approach:

1. Add state: `const [tick, setTick] = useState(0)`

2. Add a `useEffect` that computes the appropriate interval based on item ages:
   - If any item has `activeTime` within the last hour (3600000ms from now), use 30000ms interval
   - Otherwise use 60000ms interval
   - If `reduxItems` is empty, do not start any interval (return early)
   - On each tick, increment: `setTick(t => t + 1)`
   - Clean up with `clearInterval` on unmount or when interval changes

3. Add `tick` as a dependency of the `clipboardItems` useMemo (line ~153). This is the key change - since `convertToDisplayItem` already computes relative time from `Date.now()`, adding `tick` as a dependency forces periodic recalculation of all time strings.

The dependency array of `clipboardItems` useMemo becomes:
`[reduxItems, filter, searchQuery, convertToDisplayItem, tick]`

The `useEffect` dependencies should be `[reduxItems]` so it recomputes the interval when items change.

IMPORTANT: Do NOT extract timestamp logic to a separate hook or component. The simplest approach (tick counter as useMemo dependency) is the correct one here - it keeps all logic colocated and avoids over-engineering.
</action>
<verify>
<automated>cd /home/wuy6/myprojects/UniClipboard && bun run build</automated>
</verify>
<done> - `tick` state variable exists and increments on a setInterval - Interval is 30s when recent items exist, 60s otherwise - `clipboardItems` useMemo includes `tick` in its dependency array - Interval cleans up properly (clearInterval in useEffect cleanup) - No interval runs when clipboard list is empty - Build succeeds with no TypeScript errors
</done>
</task>

</tasks>

<verification>
- `bun run build` succeeds
- Manual: Open clipboard page, wait 30-60 seconds, observe timestamps updating without new clipboard activity
</verification>

<success_criteria>
Relative timestamps on clipboard items automatically refresh at appropriate intervals (30s for recent, 60s for older) even when no new clipboard content arrives.
</success_criteria>

<output>
After completion, create `.planning/quick/9-optimize-stale-relative-timestamps-on-cl/9-SUMMARY.md`
</output>

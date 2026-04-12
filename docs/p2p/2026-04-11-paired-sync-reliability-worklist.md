# Paired Sync Reliability Worklist

## Status

- Draft
- Date: 2026-04-11
- Companion to: [Paired Sync Reliability Draft](2026-04-11-paired-sync-reliability-draft.md)

## Purpose

This document converts the product draft into a build-ready worklist.

It is intentionally focused on:

- what must be true before implementation starts
- what must ship first
- what may wait until later
- how to judge whether the work actually fixed the user problem

It does not prescribe code structure in detail.

## Core Outcome

For paired devices, clipboard sync should behave like a durable relationship instead of a fragile discovery session.

The minimum acceptable user outcome is:

1. The latest clipboard content is not silently lost.
2. Recovery happens automatically in common interruption cases.
3. The app can clearly say whether content is synced, retrying, or stuck.

## Wave 1 Decisions Locked

These rules are now fixed for Wave 1.

### 1. Pending State Model

- one pending stack per peer
- maximum 10 pending clipboard records per peer
- hard-coded for now, configurable later if needed

### 2. Ordering Model

- dispatch is last-in, first-out
- new clipboard content is pushed on top
- the next item to send is always taken from the top
- an item already being sent is not interrupted mid-flight

### 3. Overflow Model

- if the stack is full and a new record arrives, remove the oldest pending record at the bottom first
- then push the new record onto the top

### 4. Success And Failure Model

- a peer is marked synced only after the receiver finishes and returns an acknowledgement
- a peer delivery attempt fails if 60 seconds pass with no transfer progress
- a peer delivery attempt also fails if the final acknowledgement does not arrive within 60 seconds
- fanout to multiple peers is evaluated independently per peer

## Still Needs Decision

These are the remaining decisions that should be locked before detailed engineering planning.

### 1. Escalation Budget

Choose how long the product may stay in automatic retry before visible escalation.

Recommended starting point:

- short silent retry window
- visible pending state after that
- stronger warning only after a clearly longer timeout

Exact values should be set during implementation planning.

### 2. Top-Level Partial Success State

Choose how the UI summarizes multi-peer fanout when some peers succeed and others fail or remain pending.

## Must Ship In Wave 1

Wave 1 is the minimum bar for claiming user-visible improvement.

### A. Pending Outbound Clipboard

Must exist:

- every outbound clipboard attempt creates a pending record in the target peer's stack
- immediate success clears it
- failure leaves it pending

Wave 1 is incomplete without this.

### B. Bounded Stack Behavior

Must exist:

- per-peer stack cap of 10 pending items
- newest content pushed on top
- oldest pending item evicted first on overflow

Wave 1 should not ship with unbounded pending growth.

### C. LIFO Dispatch With No Mid-Flight Interruption

Must exist:

- dispatch always chooses the topmost pending item next
- an item already being sent is allowed to finish or fail before the stack picks the next item

Wave 1 should not ship with ambiguous dispatch order.

### D. Paired Peer Eligibility Must Survive Discovery Gaps

Must exist:

- paired peers remain send candidates during temporary visibility loss

Wave 1 should not require fresh rediscovery as the only path to a send attempt.

### E. Recent Reachability Reuse

Must exist:

- the system retains recent working reachability information long enough to support recovery attempts

Wave 1 should no longer behave as if a recently working peer becomes unknown immediately after a short gap.

### F. Explicit Ack-Based Success And Timeout Failure

Must exist:

- sync success only after receiver acknowledgement
- 60-second no-progress timeout
- 60-second final-ack timeout
- per-peer result tracking for multi-peer fanout

Wave 1 should not ship with guessed success.

### G. Basic Delivery States

Must exist:

- Synced
- Sending
- Recovering
- Pending, will retry automatically

Wave 1 should not ship if the product still fails silently.

### H. Automatic Recovery Triggers

Must exist at minimum for:

- app start
- daemon start
- wake from sleep
- repeated delivery failure
- new outbound activity after idle period

Wave 1 should reduce restart dependence materially.

## Should Ship In Wave 2

Wave 2 makes recovery more reliable and more understandable.

### A. Better Recovery Coverage

Add:

- network change triggers
- stronger full-session rebuild behavior after repeated failed recovery
- better handling for prolonged peer invisibility

### B. Better Device-Level Visibility

Add:

- device-specific delivery status
- clearer distinction between retrying and blocked

### C. Better Operational Validation

Add:

- telemetry and logs specifically tied to pending, recovered, delivered, and expired clipboard delivery states

## Can Wait Until Later

These are valuable but should not block the first fix.

- richer retry heuristics
- user controls for retry/discard behavior
- unifying clipboard and file transfer reliability behavior
- advanced delivery diagnostics in the UI

## Explicitly Out Of Scope

Do not let these expand the first fix:

- internet-wide sync
- relay or NAT traversal work
- replaying a long historical clipboard backlog beyond the bounded stack
- redesigning pairing trust semantics
- solving file transfer reliability in the same first wave

## Engineering Guardrails

Implementation planning should enforce these rules:

1. One clear owner for outbound delivery state.
2. One clear owner for delivery confirmation.
3. Discovery and delivery eligibility must not be treated as the same thing.
4. Recovery logic must replace restart dependency, not hide it behind more retry noise.
5. User-visible state must come from real delivery state, not guesswork.

## Release Checklist

Do not consider the work done unless all items below pass.

### Reliability Checks

1. Two paired devices sync again after a long idle period without restart.
2. Temporary visibility loss does not silently drop recent clipboard content that is still inside the 10-item stack.
3. A failed immediate send becomes pending and is retried automatically.
4. When more than 10 pending records build up for one peer, the oldest pending record is evicted first.

### Sleep/Wake Checks

1. One device sleeps and wakes without requiring app restart.
2. The first copy after wake either delivers or enters visible retry state.
3. Recovery eventually delivers the current in-flight item or the top of the remaining pending stack.

### Network Change Checks

1. Recovery works after a local IP or interface change.
2. Recovery works after brief LAN disruption when both devices become reachable again.

### User State Checks

1. The app never reports success before actual delivery.
2. The app can show pending state without implying permanent failure too early.
3. The app can escalate when recovery has clearly failed.
4. The app can represent partial fanout results when peers diverge.

### Regression Checks

1. Normal active sync remains fast.
2. Rapid repeated copies do not create an ever-growing pending backlog.
3. Restarting the app is no longer the main recovery path in ordinary interruption scenarios.
4. New clipboard content can overtake older pending content without interrupting an item already in flight.

## Suggested Execution Order

1. Lock the remaining pre-implementation decisions.
2. Build per-peer pending stack state with 10-item cap.
3. Add LIFO dispatch plus overflow eviction rules.
4. Add acknowledgement-based completion and 60-second failure rules.
5. Decouple paired-peer eligibility from momentary discovery visibility.
6. Add recent reachability reuse for recovery attempts.
7. Add user-visible delivery states.
8. Add automatic recovery triggers.
9. Run the release checklist in real sleep, idle, and network-change scenarios.

## Exit Criteria

This work should only be called complete when the product experience changes in a way the user can feel:

- content no longer disappears silently
- restart is no longer the normal recovery method
- paired devices feel persistent
- the app tells the truth about delivery state

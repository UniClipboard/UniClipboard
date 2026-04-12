# Paired Sync Reliability Draft

## Status

- Draft
- Date: 2026-04-11
- Scope: paired clipboard sync reliability on LAN
- Companion worklist: [Paired Sync Reliability Worklist](2026-04-11-paired-sync-reliability-worklist.md)

## Problem Statement

Today the product treats "can discover peer right now" and "can deliver clipboard content right now" as tightly coupled.

That behavior is not acceptable from the user's point of view.

If two devices are already paired, open, and recently working, the user expects them to remain a long-lived relationship. A short discovery gap, sleep/wake cycle, or temporary LAN instability must not silently turn into clipboard loss.

The product problem is therefore:

- delivery is too dependent on momentary peer visibility
- recovery is too dependent on rediscovery
- failed sends are too easy to lose silently
- the product does not clearly communicate whether content is delivered, pending, or blocked

## User Promise

For paired devices, the product should feel persistent.

The user should be able to assume:

1. "If both devices are basically online, sync will recover by itself."
2. "If sync cannot complete immediately, my latest clipboard content is not lost."
3. "If there is still a problem, the app will tell me clearly."

## Product Principles

1. Pairing is a durable relationship.
2. Discovery is an accelerator, not the only prerequisite for delivery.
3. Temporary transport failure must degrade to pending delivery, not silent loss.
4. Recent clipboard content should be prioritized through a bounded pending stack.
5. Recovery should be automatic first, user-visible second, user-actionable last.

## Required Product Behavior

### 1. Separate Relationship From Visibility

A paired device must remain eligible for delivery even when it is not currently rediscovered.

Temporary loss of live LAN visibility must not immediately remove the peer from send consideration.

### 2. Always Create a Pending Delivery Record

Every outbound clipboard change for paired peers must first become a pending delivery attempt.

If immediate delivery succeeds, the pending state can be cleared.

If immediate delivery fails, the content must remain pending until one of these happens:

- delivery succeeds
- the content is evicted by bounded stack overflow
- the user explicitly discards it
- the product reaches a clearly defined terminal failure policy

### 3. Use a Bounded Per-Peer Pending Stack

For normal clipboard sync, the system should maintain one pending stack per target peer.

Wave 1 locked rules are:

- each peer keeps at most 10 pending clipboard records
- the limit is hard-coded for now and may become configurable later
- new clipboard content is pushed onto the top of the stack
- delivery always chooses the topmost pending item next
- if the stack is full, the oldest pending item at the bottom is removed first
- if one item is already being sent, it is not interrupted mid-flight

This gives the product two properties the user can feel:

- recent clipboard content is prioritized
- short-term delivery history is still preserved in a bounded way

### 4. Automatic Recovery Must Be Aggressive

The product must try to heal itself without requiring restart.

Recovery should be triggered by at least:

- local app startup
- daemon startup
- device wake from sleep
- network interface or IP change
- repeated delivery failure
- prolonged idle period followed by new outbound activity
- peer visibility loss followed by pending outbound work

### 5. Delivery Must Not Depend on Fresh Discovery Alone

The system should retain and reuse recent working peer reachability information for paired peers.

Fresh discovery should improve confidence and routing quality, but lack of immediate rediscovery must not be the sole reason to skip delivery attempts.

### 6. Delivery Success And Failure Must Be Explicit

For a given peer, clipboard delivery is complete only after:

- the receiver has fully accepted the payload
- the receiver has returned an acknowledgement

For a given peer, delivery attempt failure should be declared when:

- 60 seconds pass without observable transfer progress
- or the payload is fully sent but the final acknowledgement does not arrive within 60 seconds

When multiple peers are targeted, success and failure must be tracked independently per peer.

### 7. User-Facing States Must Be Explicit

The product must expose clear, simple states for clipboard delivery:

- Synced
- Sending
- Recovering
- Pending, will retry automatically
- Attention needed

The product must avoid silent failure states where content is neither delivered nor clearly pending.

### 8. Escalation Must Match Real User Impact

Short failures should stay quiet and self-heal.

Visible warnings should appear only when:

- pending delivery lasts longer than the normal recovery budget
- the target device appears unavailable for a meaningful duration
- repeated automatic recovery attempts fail

When surfaced, the message must be understandable without transport knowledge.

## Non-Goals

This draft does not require:

- replaying historical clipboard backlog beyond the bounded pending stack
- solving global internet delivery
- changing trust or pairing semantics
- merging clipboard and file transfer reliability into one queue

File transfer reliability may reuse some ideas later, but should not block clipboard sync recovery.

## Acceptance Criteria

The draft is only successful if the following user-visible outcomes are true.

### Baseline Reliability

1. Two paired devices remain able to sync after being idle for an extended period.
2. A short discovery gap does not cause silent clipboard loss.
3. If delivery cannot happen immediately, recent clipboard content is retained in the bounded per-peer stack and retried.
4. If more than 10 items accumulate for one peer, the oldest pending item is removed first.

### Sleep/Wake Recovery

1. If one device sleeps and wakes, sync recovers without restarting the app.
2. The first clipboard action after wake either succeeds quickly or enters a visible retry state.
3. A recovered connection automatically resumes delivery from the current in-flight item or the top of the remaining pending stack.

### Network Change Recovery

1. If LAN conditions change but both devices are still reachable again shortly after, sync recovers automatically.
2. The user does not need to re-pair or restart to restore normal clipboard sync.

### Failure Visibility

1. The app can tell the difference between synced, retrying, and blocked.
2. The user can understand when content is still waiting to be delivered.
3. The app does not imply success before delivery is actually confirmed.
4. Multi-device fanout can show mixed outcomes when some peers succeeded and others did not.

## Rollout Order

### Phase 1: Prevent Silent Loss

Deliver first:

- pending delivery for clipboard outbound sync
- bounded per-peer pending stack with 10-item cap
- stack dispatch rules that prioritize newer pending content
- explicit acknowledgement-based success
- 60-second no-progress failure handling
- explicit delivery states

This phase is the minimum bar because it changes the user experience from "lost" to "pending".

### Phase 2: Strong Self-Recovery

Deliver next:

- automatic recovery triggers
- stronger reuse of recent working peer reachability
- retry behavior after wake, network change, and repeated failure

This phase reduces how often the user ever sees pending state.

### Phase 3: Polish and Confidence

Deliver last:

- better status messaging
- better device-level visibility in the UI
- operational telemetry to verify recovery quality in real environments

## Release Gate

Do not consider the problem fixed unless all of the following are true:

1. Clipboard content is no longer silently dropped during temporary peer visibility loss.
2. Restarting the app is no longer the primary recovery method.
3. Paired devices behave like a durable relationship instead of a fragile moment-to-moment discovery session.
4. User-visible status matches actual delivery reality.

## Open Questions

These questions should be resolved before implementation planning is finalized:

1. What retry window should be considered normal before the UI escalates?
2. Which wake and network-change signals are available in both desktop runtimes we care about?
3. How should mixed per-peer outcomes be summarized in the top-level UI when fanout is partial?

# Connection Stability Recovery PRD

## Document Status

- PRD
- Approved for implementation planning
- Date: 2026-04-11
- Scope: LAN paired-peer connection recovery after discovery loss

## Purpose

This document narrows the current reliability discussion to one problem only:

- when both paired devices are still open and should be reachable
- but mDNS visibility is lost
- the product must recover connection automatically instead of remaining stuck offline

This PRD is intentionally narrower than the broader clipboard reliability discussion.

## Focused Problem

Today the product is too fragile when discovery drops temporarily.

From the user's point of view, the failure looks like this:

1. both devices are still open
2. both devices should still be on the same LAN
3. discovery disappears
4. the product keeps treating the peer as unavailable
5. clipboard sync stops until the user restarts the app

That behavior is the core problem for this phase.

## Core Product Rule

Loss of mDNS visibility must not immediately become long-lived offline state.

For paired peers, mDNS should be treated as:

- a live discovery signal
- a path refresh signal
- a confidence boost

It must not be treated as:

- the only proof that the peer may still be reachable
- the only condition under which the product attempts recovery

## Phase Goal

When mDNS drops but both devices are still practically online, the product should restore connection by itself.

The target user experience is:

- the product enters a short recovery state
- the product retries and rebuilds its connection path automatically
- the peer returns to online state without restart in common cases

## Definitions

These terms are load-bearing throughout the rest of this PRD and must be read before the behavior sections.

### Local Network Session

The **local network session** is the runtime object created by `Libp2pNetworkAdapter::spawn_swarm()` (`src-tauri/crates/uc-platform/src/adapters/libp2p_network/mod.rs:252`). Concretely:

- the `Swarm<Libp2pBehaviour>` instance
- its embedded `mdns::tokio::Behaviour` and `libp2p_stream::Behaviour`
- the associated `stream::Control`
- the runtime `PeerCaches` (`discovered_peers`, `address_registry`, `active_connections`)

**Rebuilding the local network session** means tearing down and recreating all of the above via a fresh `spawn_swarm()` call.

It explicitly does **not** mean rebuilding:

- the persisted paired-device database (`src-tauri/crates/uc-infra/src/db/models/paired_device_row.rs`)
- the local peer identity (keypair)
- `PeerCaches.last_dial_observations`

`last_dial_observations` is **preserved across a rebuild**, so the first retry after a rebuild can still target the last known usable address.

### Recovery Probe

A **recovery probe** is a sender-side action defined as:

1. attempt to open a business stream to the target peer
2. do not write any payload
3. close the stream immediately after the open call returns

The existing receiver already treats "stream opened, EOF before header" as a probe (`src-tauri/crates/uc-platform/src/adapters/libp2p_network/stream_handler.rs:71-72`), so this probe requires no protocol changes and no receiver-side work.

A probe is **successful** when the business stream open call returns success. No receiver response is required. No round-trip payload exchange is required.

A probe is **failed** when open returns an error or exceeds `BUSINESS_STREAM_OPEN_TIMEOUT` (`mod.rs:62`, currently `10s`).

### Usable Path

A **usable path** is one concrete multiaddr stored in `PeerCaches.last_dial_observations[peer_id]` (`peer_cache.rs:73`). It represents the last observed working dial target for that peer. Escalation Step 1 retries **only** this single multiaddr; it does not iterate over all known addresses.

### Recovery Cycle

A **recovery cycle** starts the moment a paired peer transitions from `Online` into the recovery path. It ends when the peer transitions to either `Online` or `Offline`.

Each cycle has a fresh `recovery_cycle_id`. If mDNS briefly reappears and disappears again, the second loss starts a **new** cycle; one cycle never spans an mDNS flicker.

### Silent Phase / Visible Phase

- **Silent phase** = the first 15 seconds of a recovery cycle. The user-facing state remains `Online` in the UI during this window even though internal recovery is already in progress.
- **Visible phase** = 15s to 120s of a recovery cycle. During this window the user-facing state is `Recovering`.
- After 120 seconds, if recovery has not succeeded and the escalation sequence has been exhausted, the peer is allowed to transition to `Offline`.

## Required Behavior

### 1. Do Not Mark Paired Peers Offline Immediately On Discovery Loss

If mDNS visibility disappears, the product must not immediately move a paired peer from "online" to durable "offline".

Instead it should enter a recovery window.

Required rule:

- recovery window lasts up to 120 seconds before the peer is allowed to transition to `Offline`

### 2. Keep Recent Working Reachability Long Enough To Recover

The product should retain recent working peer reachability information for paired peers for a recovery period.

This retained information is used only to support reconnection and should not be confused with permanent truth.

### 3. Actively Rebuild Connection After Discovery Loss

The product should not wait passively for fresh discovery forever.

It must actively try to restore connectivity when recovery is needed.

### 4. Recovery Must Trigger On Real-World Interruption Events

At minimum, recovery should trigger on:

- mDNS expiry for paired peers
- repeated connection or delivery setup failure
- first outbound sync attempt after a long idle period
- wake from sleep
- local network interface or IP change

### 5. Recovery Must Escalate

If lightweight recovery does not work, the product should escalate automatically.

Example escalation levels:

1. retry recent known path
2. refresh discovery and connection attempts
3. rebuild the local network session

The exact implementation is left for planning, but passive waiting is not acceptable.

### 6. Offline Must Mean Recovery Exhausted

For this phase, a paired peer should only be considered truly offline after the product has already attempted its defined recovery steps and failed.

Required rule:

- `Offline` is only allowed after the 120-second recovery window has elapsed and defined recovery steps have been exhausted

## User-Facing State Model

This phase only needs a minimal state model:

- Online
- Recovering
- Offline

Required meaning:

- `Online`: the peer is currently reachable or has just confirmed a healthy path
- `Recovering`: discovery or connection health was lost, and automatic recovery is in progress
- `Offline`: defined recovery attempts have failed

The product should not jump directly from `Online` to `Offline` on first discovery loss.

Required timing model:

- `0-15s`: **silent phase** — internal recovery in progress, user-facing state stays `Online`
- `15-120s`: **visible phase** — user-facing state is `Recovering`
- `>120s`: `Offline` is allowed only after the escalation sequence (see "Recovery Escalation Ceiling") has been exhausted

Required probe cadence:

- during the silent phase, attempt one recovery probe every `5s`, for a maximum of 3 attempts
- this cadence aligns with the existing mDNS `query_interval` (`behaviour.rs:35`) and QUIC `keepalive` (`mod.rs:73`), and it naturally feeds the Timed Rebuild Trigger (3 consecutive probe failures evaluated at the 15s mark)
- during the visible phase, continue probing at the same `5s` cadence until recovery succeeds, escalation triggers, or the 120s window elapses

## Recovery Success Criteria

The product should treat a peer as recovered only when there is **direct transport-level evidence** that the peer is usable again.

Accepted recovery proof for Wave 1 (any one of the following is sufficient):

1. a successful **Recovery Probe** as defined in Definitions — i.e., a business stream open call that returns success
2. a fresh libp2p `ConnectionEstablished` event for the target peer arriving from the swarm event loop for any reason (e.g., an incoming connection initiated by the peer itself)

Explicit non-proof:

- **rediscovery via mDNS alone is not sufficient.** mDNS only restores a discovery record; it does not prove that the transport layer can actually open a stream. Recovery must be confirmed by an actual connection-level or stream-level success event.
- time passing without a new error is not proof either.

## Probe Mechanism

The recovery probe is defined in the Definitions section above. Key points recap:

- probe = open business stream, send nothing, close immediately
- the receiver already handles this pattern as a probe (`stream_handler.rs:71-72`)
- no new protocol, no receiver-side work, no round-trip payload required
- probe success = business stream open call returns success
- probe failure = open returns an error or exceeds `BUSINESS_STREAM_OPEN_TIMEOUT` (`mod.rs:62`, currently `10s`)

Rationale:

- lowest possible transport cost
- reuses an existing, already-tested code path
- aligns recovery proof with the same stream path used by real clipboard delivery
- keeps the receiver completely unchanged in Wave 1

## Recovery Escalation Ceiling

This phase is allowed to escalate up to rebuilding the local network session (as defined in Definitions).

Required escalation sequence:

1. **Step 1 — retry the usable path.** Send a recovery probe targeted at the single multiaddr stored in `PeerCaches.last_dial_observations[peer_id]`. Do not iterate over other known addresses in this step.
2. **Step 2 — refresh discovery and broaden dialing.** Allow dialing across all known candidate addresses in `PeerCaches.discovered_peers[peer_id].addresses` plus any fresh mDNS responses. Discovery refresh must reuse the existing rate-limited discovery API; do not force a cache flush more than once per recovery cycle.
3. **Step 3 — rebuild the local network session.** Tear down and recreate the session as defined in Definitions, subject to the Rebuild Escalation Trigger rules below. `last_dial_observations` is preserved across the rebuild, so Step 1 remains available after a rebuild completes.

The escalation ceiling stops at Step 3.

It must not require:

- app restart
- re-pairing
- user intervention as the default recovery path

## Rebuild Escalation Trigger

The product should not rebuild the local network session on the first failed retry.

The rebuild step should be reserved for situations that are likely local-session problems rather than normal short peer fluctuation.

Required rebuild triggers:

### Immediate Rebuild Trigger

Allow one immediate local network-session rebuild when either of these happens during recovery:

- the device has just resumed from sleep and the first recovery probe fails
- the local network interface or local IP has changed and the first recovery probe fails

Reason:

- these events strongly suggest local transport state may be stale

Interaction with the silent phase: if an immediate rebuild triggers during the silent phase, the user-facing state transitions to `Recovering` immediately — the silent phase ends early. This is intentional; a rebuild is a real event the user deserves to see, not background noise.

### Timed Rebuild Trigger

Allow one local network-session rebuild when both of these are true:

- the peer has been in recovery for at least 15 seconds (i.e., the silent phase has ended)
- and at least 3 consecutive recovery probes have failed

Reason:

- avoids rebuilding too early
- avoids waiting passively when the lighter path is clearly not working
- the 15s threshold deliberately coincides with the silent-to-visible phase transition, so the first rebuild (if any) is the same event that surfaces `Recovering` in the UI

### Multi-Peer Rebuild Trigger

Evaluate this trigger at the moment the first peer's silent phase ends (i.e., 15s after the first peer entered recovery). Allow one local network-session rebuild when **all** of the following are true at that evaluation point:

- 2 or more paired peers have entered recovery since the first peer entered recovery
- none of those peers has recovered during their own silent phase (0–15s)

"Early recovery window" is equivalent to the silent phase as defined in Definitions.

Reason:

- simultaneous failures across multiple peers strongly suggest a local session problem rather than a single-peer problem
- evaluating at a unified 15s mark (rather than the instant the second peer enters recovery) avoids false positives from two unrelated short fluctuations that happen to land close together in time

### Rebuild Guardrails

Required guardrails:

- rebuild at most once per recovery cycle
- after rebuild, continue lighter retries inside the same 120-second recovery window
- if rebuild already happened and recovery still fails, do not keep rebuilding repeatedly in the same cycle

### Required Rebuild Log Fields

Every rebuild decision should log:

- `rebuild_id` (unique per rebuild; shared across all peers participating in a multi-peer rebuild)
- `rebuild_reason` (one of: `immediate_sleep_wake`, `immediate_network_change`, `timed_probe_failures`, `multi_peer`)
- `rebuild_allowed`
- `rebuild_already_used`
- `recovering_peer_count`
- `consecutive_probe_failures`
- `recovery_elapsed_ms`

## Tracing And Log Requirements

This phase must add tracing and logs specifically for recovery behavior.

This is a hard requirement, not optional polish.

The recovery phase is not complete if later diagnosis still depends on guessing.

### Required Recovery Events

Required structured events:

- `peer.recovery_cycle_started`
- `peer.recovery_probe_attempt`
- `peer.recovery_probe_succeeded`
- `peer.recovery_probe_failed`
- `peer.recovery_escalated`
- `peer.recovery_cycle_succeeded`
- `peer.recovery_window_exhausted`
- `peer.state_transition`
- `network.session_rebuild_started`
- `network.session_rebuild_succeeded`
- `network.session_rebuild_failed`

### Required Recovery Fields

Recovery logs should include at least:

- `peer_id`
- `recovery_cycle_id`
- `rebuild_id` (present only when a session rebuild is in progress; multiple peers participating in the same multi-peer rebuild must share one `rebuild_id`)
- `previous_state`
- `next_state`
- `trigger`
- `elapsed_ms`
- `attempt`
- `escalation_level`
- `probe_method`
- `result`
- `error`

When relevant, also include:

- `candidate_address_count`
- `candidate_addresses` (truncated to the first 5 entries; the full set is derivable from `candidate_address_count` plus per-address dial events)
- `last_seen_age_ms`
- `discovered_age_ms`
- `connected_age_ms`
- `chosen_dial_addr`
- `chosen_dial_addr_resolution`

`recovery_cycle_id` semantics:

- a new cycle id is minted at every transition **from** `Online` **into** the recovery path
- if mDNS briefly reappears and disappears again inside the same 120s window, the second loss starts a new cycle id; one cycle never spans an mDNS flicker
- multi-peer rebuild participants each keep their own `recovery_cycle_id` and share one `rebuild_id`, enabling both per-peer and rebuild-wide queries in Seq

### Required Trace Shape

One recovery cycle should be traceable end to end.

At minimum, logs must let us answer:

1. what triggered recovery
2. how many times we retried
3. whether a probe succeeded
4. whether we escalated to network-session rebuild
5. why the peer ended in `Online`, `Recovering`, or `Offline`

### Logging Outcome Standard

After Wave 1 ships, an engineer should be able to inspect one affected peer and determine:

- discovery was lost
- recovery started
- probe was attempted
- escalation did or did not happen
- local network session was or was not rebuilt
- final state and reason

## Non-Goals

This phase does not require:

- clipboard backlog or pending stack work
- file transfer reliability redesign
- internet-wide sync
- relay or NAT traversal
- full delivery-status UX for every queued clipboard item
- any change to how new outbound clipboard content is handled **while a peer is in `Recovering` state** — that behavior continues to follow the current implementation in this wave, and is the subject of the companion `paired-sync-reliability` documents

Those may be addressed later, but must not expand this first phase.

## Acceptance Criteria

This phase is only successful if these outcomes are true.

### Idle Recovery

1. Two paired devices can remain idle for an extended period and still recover connectivity automatically on the next sync attempt.
2. A temporary mDNS gap does not leave both devices stuck offline forever.
3. During the first 120 seconds after discovery loss, the peer is treated as recovering rather than immediately offline.

### Sleep/Wake Recovery

1. If one device sleeps and wakes, the product restores paired-peer connectivity without requiring restart.
2. The first new sync attempt after wake triggers automatic recovery if the peer is not immediately visible.

### Network Change Recovery

1. If the local interface or IP changes but both devices become mutually reachable again, the product restores connectivity automatically.
2. The user does not need to re-pair or restart to recover the connection.

### Offline Accuracy

1. `Offline` is shown only after recovery has already been attempted and failed.
2. Temporary discovery loss is shown as `Recovering`, not permanent offline.
3. Recovery success requires transport-level evidence (successful probe or fresh `ConnectionEstablished`); mDNS rediscovery alone does not count as recovery proof.

### Observability

1. One recovery cycle can be followed in logs from trigger to final state.
2. Probe attempts and rebuild escalation are visible as separate structured events.
3. Recovery failure can be explained from logs without guessing.

## Suggested Rollout

### Wave 1

Deliver first:

- recovery window instead of immediate offline on discovery loss
- retention of recent working peer reachability for paired peers
- automatic recovery triggers
- escalating self-heal behavior
- 120-second recovery window with 15-second silent phase
- explicit recovery success criteria
- reuse of existing lightweight business-stream probe for active recovery proof
- recovery escalation ceiling at local network-session rebuild
- recovery-specific tracing and structured logs
- minimal `Online / Recovering / Offline` state model

### Wave 2

Deliver later if needed:

- better UI messaging
- richer diagnostics
- integration with broader clipboard delivery reliability work

## Release Gate

Do not consider this phase fixed unless:

1. restart is no longer the normal way to recover after temporary mDNS loss
2. paired peers commonly return from `Recovering` to `Online` by themselves
3. the product no longer treats one mDNS drop as a durable offline verdict
4. `Offline` is not shown before the defined 120-second recovery process is exhausted

## Implementation Notes

### Observability Wiring

1. The recovery events and fields defined in this PRD must be added to the existing Seq runbook (`docs/p2p/2026-04-06-transport-observability-runbook.md`) in the same implementation wave.
2. Recovery events must be emitted from the transport layer alongside the existing `network.*` and `peer.*` events so that a single `@TraceId` can follow one recovery cycle end to end.

### Platform Signal Integration

The codebase now includes `platform_signals.rs` which provides the following triggers via `spawn_platform_signal_listener()`:

- **Wake from sleep** — IOKit-based sleep/wake hook on macOS.
- **Local network interface or IP change** — LAN-IP polling listener on all platforms.

The returned receiver is threaded through each `run_swarm` invocation so that sleep/wake and IP-change events survive session rebuilds.

### Constants Location

All timing constants introduced by this PRD are hard-coded for Wave 1:

- `silent phase duration = 15s`
- `recovery window = 120s`
- `probe cadence = 5s`
- `silent phase max probe attempts = 3`
- `timed rebuild probe failure threshold = 3`
- `multi-peer rebuild evaluation offset = 15s from first peer`

They should live alongside the existing timeout constants in `src-tauri/crates/uc-platform/src/adapters/libp2p_network/mod.rs:55-76`, matching the style of `BUSINESS_STREAM_OPEN_TIMEOUT`, `QUIC_KEEP_ALIVE_INTERVAL`, and `QUIC_MAX_IDLE_TIMEOUT_MS`. Configuration surface is explicitly deferred.

### Current Behavior To Replace

The grace period does not exist today:

- `swarm_event_loop.rs::handle_mdns_expired` emits `PeerLost` immediately when mDNS expires (`src-tauri/crates/uc-platform/src/adapters/libp2p_network/swarm_event_loop.rs:283-342`)
- `peer_cache.rs::remove_discovered` drops the discovered record on the spot (`peer_cache.rs:170-193`)

The Wave 1 implementation must insert the recovery window between these code paths and any external "peer is offline" signal.

### Identity Is Already Persistent

Paired-peer identity is already persisted in SQLite (`src-tauri/crates/uc-infra/src/db/models/paired_device_row.rs:6-14`), so recovery logic can safely assume that a paired peer's identity outlives any mDNS cache entry. No new persistence work is needed for this PRD.

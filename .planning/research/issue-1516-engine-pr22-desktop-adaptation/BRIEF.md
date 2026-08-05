# Brief: issue-1516 Engine PR #22 desktop adaptation

**Date:** 2026-08-05
**Status:** Locked
**Implemented:** 2026-08-05
**Research question:** How should the desktop application expose Engine's
network recovery without duplicating or weakening the Engine recovery policy?

## Recommendation

Use Engine PR #22's two public operations and its status-change event as the
only source of network-recovery state. Add a daemon API boundary and render a
single, last-resort manual recovery action only for a retryable final failure.

## Key findings

1. Engine owns full recovery, automatic retry, and single-flight coordination.
   A desktop-side reconnect sequence would create a second owner for the same
   state.
2. `RefreshPeerConnections` is intentionally only a lightweight reachability
   probe. It cannot replace `RecoverNetwork`.
3. WebSocket delivery is advisory. A delayed consumer receives a generic
   refresh request, so the desktop must be able to query the current status.

## Approach

### What to use

- `RecoverNetwork`: the manual last-resort request.
- `QueryNetworkRecoveryStatus`: the daemon API's authoritative status read.
- `NetworkRecoveryChanged`: prompt frontend state refresh after a status
  transition.
- The existing Devices Redux slice: one frontend owner for this status.

### What not to use

- `RefreshPeerConnections`: it only probes and redials peers; it does not
  rebuild the network session.
- A desktop retry loop, timer, or local recovery state machine: Engine already
  owns scheduling and concurrent recovery requests.

## Constraints

- The desktop project must consume the pinned Engine commit, never an Engine
  branch.
- Wire payloads must use camelCase and expose no device address or low-level
  network error text.
- A missing or delayed WebSocket notification must converge through a status
  read.
- The manual action is hidden while recovery is in progress or a retry is
  already scheduled.

## Implementation checklist

- [x] Pin the Engine commit containing PR #22.
- [x] Add daemon status and recovery endpoints with OpenAPI coverage.
- [x] Forward recovery state changes to the daemon WebSocket.
- [x] Add typed frontend accessors and Devices-state ownership.
- [x] Render the recovery state and retryable final-failure action.
- [x] Add focused daemon and frontend tests, regenerate API bindings, and run
  the targeted checks.

## Open questions

- Before merge, replace the PR head pin with the final merge commit if GitHub
  produces a different commit.

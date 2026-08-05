# Sources: issue-1516 Engine PR #22 desktop adaptation

## Engine public contract

- Engine PR #22, commit `d0dfe18735508973f0395d2d9bba50ddfafc0970`:
  - `Operation::RecoverNetwork` requests recovery and shares the in-flight
    automatic recovery instead of starting a second recovery.
  - `Operation::QueryNetworkRecoveryStatus` returns `phase`, `retryable`, and
    optional `next_retry_in_ms`.
  - `EngineEvent::NetworkRecoveryChanged` publishes every stable status change.
  - `RefreshRequired` may replace a delayed event, so a host must query the
    status rather than treating event delivery as authoritative.
- `docs/architecture/uc-engine-interface.md` in the same PR states that
  `RefreshPeerConnections` remains a lightweight probe and must not be used as
  the network-recovery operation.

## Desktop code study

- `crates/uc-webserver/src/api/event_emitter.rs` is the daemon boundary that
  projects Engine events onto the local WebSocket.
- `crates/uc-daemon-contract/src/api/dto/` owns HTTP and WebSocket wire DTOs;
  `crates/uc-webserver/src/api/openapi.rs` owns endpoint registration.
- `src/store/slices/devicesSlice.ts` is the single frontend state owner for the
  Devices page. `src/pages/DevicesPage.tsx` already consumes device state and
  daemon WebSocket updates.

## Decision

The desktop daemon will expose one status read and one recovery request. The
Devices page will only offer the manual action when Engine reports a retryable
final failure. It will never call the old presence refresh endpoint as a
substitute for recovery.

# Phase 45: Daemon API Foundation — Research

**Researched:** 2026-03-19
**Domain:** Local daemon HTTP + WebSocket transport for read-only runtime queries
**Confidence:** HIGH

## Summary

Phase 45 should introduce a new daemon-owned `HTTP + WebSocket` API without removing the existing Unix-socket JSON-RPC path on day one. The safest approach is to add a loopback-only TCP server in `uc-daemon`, keep the current Unix socket RPC temporarily for transition, and move CLI commands covered by this phase to a shared daemon HTTP client. Tauri should start/probe the daemon and inject connection metadata into the webview, but Phase 45 must not cut the frontend over to daemon business APIs yet.

The existing codebase already has most of the read-model building blocks. `uc-bootstrap::build_non_gui_runtime()` can construct a Tauri-free `CoreRuntime`; `CoreUseCases` already exposes `get_p2p_peers_snapshot()` and `list_paired_devices()`; `uc-app` also already defines `PairingDomainEvent` and `PairingEventPort`, which is the right source for future WebSocket pairing topics. The main gap is transport shape: the current daemon is a single-request Unix socket JSON-RPC server with `status` only, so Phase 45 needs a new API module, auth/token storage, HTTP DTOs, a WebSocket subscription protocol, and a way to expose daemon connection info to clients.

**Primary recommendation:** use `axum` in `uc-daemon` for both HTTP routes and WebSocket upgrades, define API DTOs in a transport-specific `api` module inside `uc-daemon`, keep sensitive payloads out of all DTOs, and implement the new server alongside the old RPC until CLI cutover is complete.

## Planning Implications

### 1. Framework choice should optimize shared transport, not minimal diff

`uc-daemon` currently uses hand-rolled Unix socket JSON-RPC on top of `tokio`. That was correct for Phase 41, but it is a poor fit for Phase 45 because:

- HTTP routing, auth middleware, and WebSocket upgrades would otherwise be rebuilt manually.
- The CLI and webview need a stable request/response shape, not JSON-RPC framing.
- Loopback TCP plus WebSocket is a better long-term surface for daemon-first UI than extending socket RPC.

`axum` is the pragmatic choice because one framework covers:

- `GET /health`, `GET /status`, `GET /peers`, `GET /paired-devices`, `GET /pairing/sessions/{sessionId}`
- bearer-token auth via request extractors or middleware
- WebSocket upgrade handling
- shared state injection via `State<T>`
- low-friction tests using `tower::ServiceExt`

### 2. Keep Unix socket RPC during transition

Replacing the Phase 41 Unix socket RPC immediately adds avoidable migration risk. The current daemon lifecycle in `src-tauri/crates/uc-daemon/src/app.rs` is already structured around an accept loop plus workers; adding an HTTP server task beside the RPC server is lower risk than swapping transports in one shot.

Recommended transition rule:

- Phase 45 adds the new HTTP + WebSocket server.
- Existing Unix socket RPC remains available temporarily.
- CLI commands covered by this phase migrate to the HTTP client.
- RPC removal is deferred until later phases prove the daemon API boundary is stable.

This keeps Phase 45 focused on the new shared boundary instead of transport deletion churn.

### 3. Auth file should live next to other daemon-local machine state

The context already locks the model: daemon generates a bearer token on first startup, persists it locally with restrictive permissions, CLI reads it, Tauri injects it at runtime, frontend never persists it.

Implementation detail recommendation:

- Add a dedicated token file helper in `uc-daemon`, separate from socket path resolution.
- Reuse bootstrap/config resolution for app-data location where possible.
- On Unix, create with `0o600` semantics and treat wider permissions as a warning or repair path.
- Expose a small `DaemonConnectionInfo` value object:
  - `base_url`
  - `ws_url`
  - `token`

This object should be consumable by CLI and by Tauri runtime injection code, but must never be logged in full.

### 4. DTOs must be explicitly transport-shaped and lower sensitivity than domain internals

Current daemon RPC types are minimal and JSON-RPC specific. Phase 45 needs transport DTOs shaped for clients:

- health DTO
- status DTO
- peer snapshot DTO
- paired-device DTO
- pairing-session summary DTO
- WebSocket subscribe / event envelope DTOs

Important boundary rule: DTOs should be defined in `uc-daemon::api::types`, not borrowed directly from internal domain structs. This avoids leaking:

- `sharedSecret`
- raw key material
- clipboard payload content
- internal state machine fields that are meaningful only inside the host

The existing `GetP2pPeersSnapshot` use case already gives a strong starting point for `/peers` and `/paired-devices`.

### 5. `/pairing/sessions/{sessionId}` cannot depend on Tauri-owned runtime state

Phase 46 is the actual pairing-host migration. That means Phase 45 cannot assume the daemon already owns the live pairing orchestrator. Planning must acknowledge this mismatch.

Recommended contract:

- Define the endpoint now.
- Back it with a daemon-owned read store abstraction in Phase 45.
- If no session is known yet, return a normal `404` or equivalent typed API error.
- Do not attempt to proxy into Tauri-owned pairing state from the daemon.

This keeps the transport stable while deferring host migration correctly to Phase 46.

### 6. WebSocket should expose domain-facing topics, not raw internal transitions

The user explicitly chose domain-event exposure over raw state-machine internals. Existing app-layer pairing events already support that direction:

- `PairingVerificationRequired`
- `PairingSucceeded`
- `PairingFailed`

The WebSocket model should therefore be topic-driven, for example:

- `status`
- `peers`
- `paired-devices`
- `pairing`

Envelope recommendation:

```json
{
  "topic": "peers",
  "type": "peers.snapshot",
  "sessionId": null,
  "ts": 1742371200000,
  "payload": {}
}
```

Server protocol recommendation:

- client sends `{ "action": "subscribe", "topics": ["peers", "paired-devices"] }`
- server responds with one snapshot event per topic
- server then streams incremental events

This is sufficient for GUI and CLI recovery logic without overfitting to one consumer.

### 7. Tauri work in Phase 45 is bootstrap/injection only

`src-tauri/src/main.rs` is still a Tauri application host. Phase 45 should keep its changes narrow:

- ensure daemon is running or startable
- obtain daemon connection info at runtime
- inject connection info into the webview in-memory only
- keep existing command routes for current UI business flows

Do not mix frontend cutover into this phase. That belongs to Phase 47.

### 8. CLI should start sharing the new boundary now

`uc-cli` currently mixes daemon RPC (`status`) with direct-mode runtime access (`devices`). For Phase 45, commands covered by the new daemon API should start using a shared HTTP client module:

- `status` -> `GET /status`
- `devices` -> `GET /paired-devices`

Direct-mode bootstrap remains valid for commands not yet represented by daemon APIs, but the direction of travel should become daemon client first.

## Recommended Phase Breakdown

### Plan 45-01

Define daemon API contract, token persistence/auth helpers, and query/read-model service boundary inside `uc-daemon`.

### Plan 45-02

Add loopback HTTP + WebSocket server to `uc-daemon`, serve the read-only endpoints, and keep Unix socket RPC alive during transition.

### Plan 45-03

Add a shared daemon HTTP client for `uc-cli`, migrate commands covered by the new API, and add Tauri-side daemon connection bootstrap/injection without cutting over frontend business flows.

## Risks And Mitigations

### Risk: token leakage exposes metadata APIs

Mitigation:

- Phase 45 DTOs must remain metadata-only.
- Never log token values.
- Never place token in URL query strings.
- Never persist token in frontend storage.

### Risk: loopback HTTP server accidentally binds broadly

Mitigation:

- bind explicitly to `127.0.0.1`, not `0.0.0.0`
- add a test asserting the default listen address is loopback

### Risk: CLI migration blocks if HTTP server is unavailable

Mitigation:

- keep Unix socket RPC during the transition window
- centralize daemon connection probing and error mapping in one client module

### Risk: pairing session endpoint creates pressure to move Phase 46 work into Phase 45

Mitigation:

- define a read-model abstraction only
- return `404` when the daemon does not yet know the session
- explicitly defer live pairing-host migration

## Validation Architecture

Phase 45 validation should concentrate on transport correctness, auth boundary correctness, and data exposure correctness.

Automated checks should cover:

- loopback-only bind for HTTP server
- bearer token required for protected routes and WebSocket subscribe path
- `/health`, `/status`, `/peers`, `/paired-devices`, `/pairing/sessions/{id}` route behavior
- DTO serialization keys and absence of sensitive fields
- WebSocket subscribe -> snapshot -> incremental contract
- CLI commands successfully calling daemon HTTP endpoints

Manual verification should be limited to:

- Tauri daemon startup/probing and runtime injection into the webview
- browser-side inspection that token is not persisted in `localStorage`

## Recommended Commands

- Quick server tests: `cd src-tauri && cargo test -p uc-daemon api::`
- Quick CLI tests: `cd src-tauri && cargo test -p uc-cli`
- Full phase validation: `cd src-tauri && cargo test -p uc-daemon -p uc-cli`

## Final Recommendation

Plan Phase 45 as a three-step migration:

1. create the daemon API contract and auth foundation
2. stand up the new HTTP + WebSocket server beside existing RPC
3. move covered clients onto the new boundary and inject daemon connection info into Tauri

That sequence keeps the architecture aligned with the daemon-first target while keeping Phase 45 strictly transport/read-only.

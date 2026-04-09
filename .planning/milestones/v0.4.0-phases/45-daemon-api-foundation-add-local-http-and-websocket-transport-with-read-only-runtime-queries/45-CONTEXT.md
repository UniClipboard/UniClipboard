# Phase 45: Daemon API Foundation - Context

**Gathered:** 2026-03-19
**Status:** Ready for planning

<domain>
## Phase Boundary

Establish the first local daemon-facing `HTTP + WebSocket` API surface for UniClipboard. This phase is limited to transport foundation and read-only runtime queries so GUI and CLI can begin acting as daemon clients without exposing clipboard content, write operations, or pairing state transitions yet.

**In scope:**

- Local daemon HTTP server foundation
- Local daemon WebSocket server foundation
- Loopback-only access model
- Bearer-token authentication foundation for daemon clients
- Read-only query endpoints for runtime status, peers, paired devices, and future pairing session inspection
- WebSocket topic/event envelope and initial snapshot semantics
- Tauri acting as daemon client/bootstrap host without cutting over all frontend business traffic yet

**Out of scope:**

- Clipboard content read APIs
- Any write/mutation endpoints
- Pairing commands (`initiate`, `accept`, `reject`, `cancel`)
- Full frontend cutover from Tauri commands to daemon APIs
- Replacing Tauri as business host in this phase
- Remote or cross-machine daemon access

</domain>

<decisions>
## Implementation Decisions

### Local authentication model

- Daemon listens on loopback only (`127.0.0.1`)
- Daemon uses a bearer token as the first authentication layer
- On first startup, daemon generates a random token and persists it in a local app-data file with restricted permissions
- CLI reads the same local token file
- Web frontend does **not** discover or persist the token itself
- Tauri shell injects daemon connection info and token into the running webview at runtime
- Token is not allowed in URL query strings, not stored in `localStorage`, and not treated as sufficient protection for future high-sensitivity APIs

### Sensitivity boundary for Phase 45

- Phase 45 exposes only low-sensitivity, read-only metadata APIs
- No clipboard plaintext, decrypted payloads, secrets, or key material are exposed
- No pairing control actions or settings mutations are exposed
- This phase intentionally limits blast radius if the local bearer token is leaked

### First read-only HTTP API surface

- `GET /health` — daemon liveness check for shell/client startup probing
- `GET /status` — daemon version, runtime status, worker health, and high-level connection summary
- `GET /peers` — discovered/connected peer snapshot
- `GET /paired-devices` — paired device snapshot
- `GET /pairing/sessions/{sessionId}` — session summary/read model only, intended to support later CLI/GUI session recovery
- No additional HTTP endpoints are added in this phase

### Default response data boundary

- Peer/device responses should default to metadata only:
  - `peerId`
  - `deviceName`
  - `addresses`
  - `isPaired`
  - `connected`
  - `pairingState`
- Responses must not include:
  - `sharedSecret`
  - raw keying material
  - decrypted clipboard content
  - sensitive internal state not needed by GUI/CLI read models

### WebSocket subscription model

- WebSocket uses topic-based subscription, not per-session channels
- Event envelope is unified and includes:
  - `topic`
  - `type`
  - `sessionId`
  - `ts`
  - `payload`
- Clients filter by `topic` first and by `sessionId` when relevant
- This keeps daemon-side subscription management simple while allowing GUI and CLI to share the same event stream model

### WebSocket initial sync semantics

- On subscription/connection, daemon first emits a current snapshot event for the subscribed topic
- After the snapshot, daemon emits incremental events only
- For example, `peers` topic should support an initial `peers.snapshot` followed by incremental peer change events
- This snapshot-then-incremental model is preferred over “HTTP fetch first, WS only for deltas” because it simplifies reconnect and client state recovery

### Tauri role during Phase 45

- Tauri begins acting as a daemon client host in this phase
- Tauri is responsible for daemon startup/probing, connection info injection, and desktop-shell lifecycle integration
- Existing Tauri command paths may remain temporarily for business queries in Phase 45
- Full frontend business cutover is deferred to Phase 47 and must not be silently folded into this phase

### Security posture for later phases

- The persisted bearer token is only the baseline local-client credential
- Future high-sensitivity APIs must add stronger controls than a single long-lived bearer token
- Planners and researchers should preserve the split between low-sensitivity metadata APIs and future high-sensitivity content/action APIs

### Claude's Discretion

- Exact HTTP framework/library choice
- Exact token file path naming and file-permission implementation details
- Exact WebSocket subscribe message shape
- Exact `status`, `peers`, and `paired-devices` response DTO structure
- Whether `/health` returns minimal text/JSON or a slightly richer machine-readable body

</decisions>

<specifics>
## Specific Ideas

- Daemon is the only long-lived business host going forward
- CLI should participate as a daemon client, not as a second business host
- Web frontend should ultimately communicate with daemon over HTTP and WebSocket rather than Tauri runtime commands
- Tauri should become a desktop shell and daemon bootstrap/injection layer, not the primary business API surface

</specifics>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase definition and project direction

- `.planning/ROADMAP.md` — Phase 45 boundary and dependency chain into Phases 46-48
- `.planning/PROJECT.md` — v0.4.0 runtime-mode-separation goal and current architectural direction
- `.planning/REQUIREMENTS.md` — current runtime separation requirements baseline; future Phase 45 planning should extend this direction without violating existing constraints
- `.planning/STATE.md` — latest roadmap evolution and runtime-separation decisions already locked

### Prior phase context

- `.planning/phases/40-uc-bootstrap-crate/40-CONTEXT.md` — sole composition root and builder API decisions
- `.planning/phases/41-daemon-and-cli-skeletons/41-CONTEXT.md` — current daemon/CLI skeleton decisions and existing RPC boundary

### Existing daemon and client entry points

- `src-tauri/crates/uc-daemon/src/app.rs` — current daemon lifecycle host
- `src-tauri/crates/uc-daemon/src/rpc/server.rs` — existing Unix socket RPC server pattern
- `src-tauri/crates/uc-daemon/src/rpc/handler.rs` — current read-only daemon request dispatch baseline
- `src-tauri/crates/uc-daemon/src/rpc/types.rs` — existing daemon response DTO patterns
- `src-tauri/crates/uc-daemon/src/socket.rs` — existing local transport/address resolution logic
- `src-tauri/crates/uc-cli/src/commands/status.rs` — current daemon-client path from CLI

### Shared runtime/query paths

- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` — non-GUI runtime assembly path
- `src-tauri/crates/uc-cli/src/commands/devices.rs` — existing direct-mode peer/device query path
- `src-tauri/crates/uc-app/src/usecases/mod.rs` — current shared use case entrypoints available for read-only daemon APIs

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `uc-daemon` already has a local server lifecycle, request dispatch module, and state holder; these provide the starting structure for replacing Unix-socket JSON-RPC with local HTTP/WS transport
- `uc-bootstrap::build_cli_runtime()` and `CoreUseCases` already expose read-only query paths that can back `/status`, `/peers`, and `/paired-devices`
- `GetP2pPeersSnapshot` already exists as the shared aggregation path for peer/device read models

### Established Patterns

- Non-GUI runtimes use `LoggingHostEventEmitter` and Tauri-free `CoreRuntime` assembly
- Shared read-only business logic belongs in `uc-app` use cases, not in entrypoint-specific command glue
- Tauri-specific event delivery currently lives behind adapter boundaries; Phase 45 should continue that separation rather than reintroducing Tauri-coupled business APIs

### Integration Points

- `uc-daemon` is the natural home for the local HTTP and WebSocket transport layer
- `uc-cli` should switch from direct transport-specific code to daemon HTTP/WS client calls for the APIs covered by this phase
- `uc-tauri` should gain only daemon bootstrap/client responsibilities in this phase, not a second copy of the business endpoints

</code_context>

<deferred>
## Deferred Ideas

- Clipboard history/content daemon APIs
- Pairing mutation endpoints
- Full frontend cutover off Tauri commands
- Stronger authorization model for high-sensitivity APIs beyond the baseline persisted bearer token
- Remote/non-local daemon access

</deferred>

---

_Phase: 45-daemon-api-foundation-add-local-http-and-websocket-transport-with-read-only-runtime-queries_
_Context gathered: 2026-03-19_

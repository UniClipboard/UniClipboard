# Architecture

**Analysis Date:** 2026-03-11

## Pattern Overview

**Overall:** Hexagonal Architecture (Ports and Adapters) — actively migrating from legacy Clean Architecture

**Key Characteristics:**

- Domain core (`uc-core`) defines port traits; infrastructure/platform implement them
- Use cases in `uc-app` hold `Arc<dyn Port>` references, never concrete types
- `uc-tauri/bootstrap/wiring.rs` is the single composition root: the only place that sees all crates simultaneously
- Frontend communicates with Rust exclusively via Tauri commands (IPC) and Tauri events
- Async throughout: Tokio runtime, `Arc<Mutex<T>>` for shared state

## Layers

**Domain Core (`uc-core`):**

- Purpose: Pure domain models and port trait definitions — no I/O
- Location: `src-tauri/crates/uc-core/src/`
- Contains: Domain aggregates (`clipboard/`, `device/`, `network/`, `security/`, `settings/`), all port trait definitions in `ports/`
- Depends on: Nothing (no external side-effecting crates)
- Used by: All other crates

**Application Use Cases (`uc-app`):**

- Purpose: Business logic orchestration, one use case per file
- Location: `src-tauri/crates/uc-app/src/usecases/`
- Contains: `delete_clipboard_entry.rs`, `list_clipboard_entries.rs`, `initialize_encryption.rs`, `clipboard/sync_inbound.rs`, `clipboard/sync_outbound.rs`, `pairing/`, `setup/`, `space_access/`
- Depends on: `uc-core` (port traits only, no concrete implementations)
- Used by: `uc-tauri` (wired in `bootstrap/runtime.rs` and `bootstrap/wiring.rs`)

**Infrastructure (`uc-infra`):**

- Purpose: Implements domain ports using external dependencies (database, filesystem, crypto)
- Location: `src-tauri/crates/uc-infra/src/`
- Contains: `db/` (Diesel + SQLite repositories), `security/` (XChaCha20-Poly1305 encryption), `blob/`, `settings/`, `fs/` (key slot store at `fs/key_slot_store.rs`)
- Depends on: `uc-core`
- Used by: `uc-tauri/bootstrap/wiring.rs` only

**Platform Adapter (`uc-platform`):**

- Purpose: OS-level integrations — clipboard watching, libp2p networking, IPC event bus
- Location: `src-tauri/crates/uc-platform/src/`
- Contains: `adapters/libp2p_network.rs` (full P2P network over libp2p), `clipboard/` (system clipboard read/write + watcher), `runtime/runtime.rs` (`PlatformRuntime` event loop), `ipc/` (`PlatformCommand` / `PlatformEvent` message types)
- Depends on: `uc-core`
- Used by: `uc-tauri/bootstrap/wiring.rs` only

**Tauri Bootstrap / Commands (`uc-tauri`):**

- Purpose: Composition root, Tauri command handlers, event forwarding to frontend
- Location: `src-tauri/crates/uc-tauri/src/`
- Contains: `bootstrap/wiring.rs` (DI wiring), `bootstrap/runtime.rs` (`AppRuntime` + `UseCases` accessor), `commands/` (Tauri command functions), `events/` (typed frontend events), `tray.rs`, `protocol.rs` (custom `uc://` URI scheme handler)
- Depends on: ALL crates (only permitted location)
- Used by: `src-tauri/src/main.rs`

**Observability (`uc-observability`):**

- Purpose: Structured log formatting and Seq/CLEF integration
- Location: `src-tauri/crates/uc-observability/src/`
- Contains: `clef_format.rs`, `profile.rs`, `stages.rs`, `seq/`
- Depends on: tracing ecosystem
- Used by: `uc-tauri/bootstrap/tracing.rs`

**Clipboard Probe (`uc-clipboard-probe`):**

- Purpose: Standalone binary for platform clipboard capability detection
- Location: `src-tauri/crates/uc-clipboard-probe/src/main.rs`
- Contains: Single binary entry point
- Used by: Build system / runtime capability detection

**Frontend (React + TypeScript):**

- Purpose: UI layer, state management, Tauri command invocations
- Location: `src/`
- Contains: `pages/`, `components/`, `store/` (Redux Toolkit), `api/` (Tauri invoke wrappers), `hooks/`, `contexts/`
- Depends on: Tauri JS API (`@tauri-apps/api/event`, `@tauri-apps/api/core`)
- Used by: Webview rendered by Tauri

## Data Flow

**Clipboard Capture (Local):**

1. `PlatformRuntime` (`uc-platform/src/runtime/runtime.rs`) runs `ClipboardWatcher` in a background Tokio task
2. On clipboard change, calls `ClipboardChangeHandler` trait (implemented by `AppRuntime`)
3. `AppRuntime` invokes `CaptureClipboardUseCase` from `uc-app`
4. Use case persists `ClipboardEvent` → `ClipboardSnapshotRepresentation` → `ClipboardEntry` via repository port traits
5. `uc-tauri` emits `ClipboardEvent::NewContent` Tauri event to frontend
6. Frontend hook `src/hooks/useClipboardEvents.ts` listens via `@tauri-apps/api/event` `listen()`, dispatches Redux `prependItem` action

**Clipboard Sync (Inbound from Peer):**

1. `LibP2pNetworkAdapter` (`uc-platform/src/adapters/libp2p_network.rs`) receives encrypted payload over libp2p stream
2. Calls `NetworkEventPort` with `NetworkEvent::ClipboardReceived`
3. `SyncInboundClipboardUseCase` (`uc-app/src/usecases/clipboard/sync_inbound.rs`) decrypts payload, normalizes representations, persists entry
4. If this device is the paste target, writes to system clipboard via `SystemClipboardPort`

**Tauri Command Invocation (Frontend → Backend):**

1. Frontend calls `invokeWithTrace('command_name', args)` from `src/lib/tauri-command.ts`
2. Tauri routes to matching `#[tauri::command]` function in `src-tauri/crates/uc-tauri/src/commands/`
3. Command extracts `State<'_, Arc<AppRuntime>>`, calls `runtime.usecases().xxx()`
4. Use case executes business logic through port traits
5. Returns serialized result to frontend as JSON

**Custom Resource Protocol (`uc://`):**

1. Frontend requests `uc://blob/<blob_id>` or `uc://thumbnail/<rep_id>` image URLs (generated by `src/lib/protocol.ts`)
2. Tauri intercepts via custom URI handler registered in `src-tauri/src/main.rs`
3. `resolve_uc_request()` calls `ResolveBlobResource` or `ResolveThumbnailResource` use case
4. Returns binary bytes response with proper MIME type and CORS headers

**State Management:**

- Backend: `Arc<AppRuntime>` managed by Tauri state system (`.manage()`); `AppDeps` holds `Arc<dyn Port>` for each dependency grouped as `ClipboardPorts`, `DevicePorts`, `SecurityPorts`, `StoragePorts`, `SystemPorts`, `NetworkPorts`
- Frontend: Redux Toolkit slices (`clipboardSlice`, `devicesSlice`, `statsSlice`); no RTK Query — manual async thunks + Tauri event listeners

## Key Abstractions

**Port Traits (`uc-core/src/ports/`):**

- Purpose: Contracts between business logic and the external world
- Examples: `ClipboardEntryRepositoryPort`, `EncryptionPort`, `SystemClipboardPort`, `ClipboardTransportPort`, `SettingsPort`, `BlobStorePort`, `PairingTransportPort`
- Pattern: `#[async_trait]` trait; injected as `Arc<dyn Port>` into use case constructors

**UseCases Accessor (`uc-tauri/src/bootstrap/runtime.rs` — `UseCases` struct):**

- Purpose: Factory pattern providing pre-wired use case instances to Tauri commands
- Pattern: `runtime.usecases().list_clipboard_entries()` constructs use case with ports from `AppDeps`; commands never access `runtime.deps` directly

**AppDeps (`uc-app/src/lib.rs`):**

- Purpose: Dependency bundle grouping all `Arc<dyn Port>` references passed to use cases
- Sub-groups: `ClipboardPorts`, `DevicePorts`, `SecurityPorts`, `StoragePorts`, `SystemPorts`, `NetworkPorts`

**PlatformRuntime (`uc-platform/src/runtime/runtime.rs`):**

- Purpose: OS event loop handling `PlatformCommand` messages and invoking `ClipboardChangeHandler`
- Pattern: Async `select!` loop over `event_rx` and `command_rx` channels; callback pattern maintains DIP (Platform → Core trait, never Platform → App)

**SetupOrchestrator / PairingOrchestrator:**

- Purpose: Stateful multi-step orchestrators for device setup and P2P pairing flows
- Location: `uc-app/src/usecases/setup/`, `uc-app/src/usecases/pairing/`
- Pattern: Cached in `AppRuntime` (not recreated per command call) to preserve in-memory state machine

## Entry Points

**Rust Application Entry:**

- Location: `src-tauri/src/main.rs`
- Triggers: OS process start
- Responsibilities: Tauri builder setup, load config, call `wire_dependencies`, register `Arc<AppRuntime>` with `.manage()`, register all commands in `invoke_handler![]`, start `PlatformRuntime` background task, register `uc://` custom protocol handler

**Tauri Commands Entry:**

- Location: `src-tauri/crates/uc-tauri/src/commands/` (files: `clipboard.rs`, `encryption.rs`, `pairing.rs`, `settings.rs`, `setup.rs`, `lifecycle.rs`, `updater.rs`, etc.)
- Triggers: Frontend IPC invocations via `invoke()`
- Responsibilities: Thin adapter — extract `AppRuntime` state, call `runtime.usecases().xxx().execute()`, map errors to `String`

**Frontend Entry:**

- Location: `src/main.tsx` (presumed standard Vite entry) → page components in `src/pages/`
- Triggers: Webview load
- Responsibilities: React root mount, Redux store provider, React Router v7 routes to `DashboardPage`, `DevicesPage`, `SettingsPage`, `SetupPage`, `UnlockPage`

**Composition Root:**

- Location: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`
- Triggers: Called from `main.rs` during Tauri `.setup()` callback
- Responsibilities: Instantiate all infra/platform implementations, inject into `AppDeps`, wire orchestrators, return `WiredDependencies`; contains no business logic

## Error Handling

**Strategy:** `anyhow::Result<T>` in use cases and infrastructure; Tauri commands map errors to `String` for frontend serialization

**Patterns:**

- Use cases: `anyhow::Result<T>` or domain-specific error enums (e.g., `SetupError`, `ListProjectionsError`)
- Commands: `uc.execute().await.map_err(|e| e.to_string())`
- No `unwrap()` or `expect()` in production code — use `?`, pattern matching, or `unwrap_or_else`
- Event-driven code: errors logged with `warn!`/`error!` and emitted as typed events (e.g., `NetworkEvent::Error`) so UI can surface them

## Cross-Cutting Concerns

**Logging:** `tracing` crate; spans via `#[tracing::instrument]` or manual `info_span!(...).instrument(future)`; CLEF format for Seq aggregation via `uc-observability`; development logs to terminal, production to stdout + log file

**Validation:** Domain types enforce invariants (e.g., typed ID newtypes in `uc-core/src/ids/`); business rule validation in use case layer

**Encryption:** XChaCha20-Poly1305 AEAD in `uc-infra/src/security/encryption.rs`; Argon2id KDF for passphrase-to-key; key slots file system at `uc-infra/src/fs/key_slot_store.rs`; Tauri Stronghold plugin for secure master key storage

**P2P Networking:** libp2p with mDNS discovery, TCP transport, Noise protocol encryption, yamux multiplexing; all in `uc-platform/src/adapters/libp2p_network.rs`

---

_Architecture analysis: 2026-03-11_

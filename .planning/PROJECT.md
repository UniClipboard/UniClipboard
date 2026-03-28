# UniClipboard Desktop

## What This Is

A cross-platform clipboard synchronization app built with Tauri 2, React, and Rust. It provides encrypted LAN clipboard sync for text, images, links, and files, with hexagonal architecture, full observability pipeline, per-device sync control, and file transfer with eventual consistency.

## Core Value

Seamless clipboard synchronization across devices — users can copy on one device and paste on another without interrupting their workflow.

## Current Milestone: v0.4.0 Runtime Mode Separation

**Goal:** Extract non-Tauri logic from uc-tauri into shared crates, enabling GUI / CLI / daemon as independent runtime modes.

**Target features:**

- HostEventEmitterPort abstraction replacing hardcoded Tauri AppHandle::emit()
- wiring.rs decomposition separating pure assembly from Tauri event loops
- CoreRuntime extraction from AppRuntime (Tauri-free runtime core)
- Configuration resolution extracted to reusable module
- uc-bootstrap crate as the sole composition root
- uc-daemon + uc-cli minimal skeletons with end-to-end path validation

## Current State

- **Latest shipped milestone:** v0.3.0 Log Observability & Feature Expansion (2026-03-17)
- **Current capability level:** Full-featured clipboard sync with text, image, link, and file support; structured observability; per-device sync control; CLI clipboard history commands (list/get/clear); daemon auto-recovers encryption session on startup; daemon triggers outbound clipboard sync to peers after local capture; daemon receives inbound clipboard from peers via ClipboardTransportPort and applies via SyncInboundClipboardUseCase; daemon handles file transfer lifecycle (progress, completion, failure, timeout sweeps, startup reconciliation)
- **Architecture status:** Hexagonal architecture with compiler-enforced boundaries, typed command surfaces, lifecycle governance, and consolidated sync planner; CLI direct-mode bootstrap pattern established; CLI start/stop commands for daemon lifecycle management (background/foreground modes, PID-based stop with SIGTERM); daemon encryption state recovery via existing AutoUnlockEncryptionSession use case; peer discovery deduplication fixed (local_peer_id filtering + full-snapshot peers.changed events); daemon gates PeerDiscoveryWorker on encryption state (deferred start for uninitialized devices via oneshot channel); dual-product release pipeline — CLI binary builds in parallel with App, included in GitHub Release and R2
- **LOC:** ~135K Rust + ~20K TypeScript (estimated)
- **Supported content types:** Text, Image, Link, File (all with per-device sync toggles)

## Requirements

### Validated

- ✓ Clipboard text capture and history — existing
- ✓ Device pairing and LAN sync baseline — existing
- ✓ V2 unified transfer and streaming decode foundation — v0.1.0
- ✓ At-rest blob format optimization and migration — v0.1.0
- ✓ Windows image clipboard capture reliability — v0.1.0
- ✓ Dashboard image display compatibility across platforms — v0.1.0
- ✓ Setup flow UX consistency improvements — v0.1.0
- ✓ V3 binary sync protocol, compression, and zero-copy fanout — v0.1.0
- ✓ Large-image clipboard read pipeline memory/latency improvements — v0.1.0
- ✓ Cross-layer boundary violation removal and command-layer penetration closure — v0.2.0
- ✓ Typed command DTO/error contracts and traceable API surfaces — v0.2.0
- ✓ Lifecycle governance (task cancellation, graceful shutdown, runtime cleanup) — v0.2.0
- ✓ God-object decomposition (AppDeps/SetupOrchestrator/PairingOrchestrator) — v0.2.0
- ✓ Test infrastructure consolidation (shared noop ports) — v0.2.0
- ✓ Dashboard incremental update with origin-based event routing — v0.2.0
- ✓ Runtime theme preset engine with multi-dot Appearance swatches — v0.2.0
- ✓ Dual-output structured logging with configurable profiles (dev/prod/debug_clipboard) — v0.3.0
- ✓ Flow correlation with flow_id/stage spans across clipboard capture and sync pipelines — v0.3.0
- ✓ Seq local integration with CLEF format, async batching, and cross-device tracing — v0.3.0
- ✓ Per-device sync settings with content type toggles and global master toggle — v0.3.0
- ✓ File sync via libp2p with chunked transfer, Blake3 verification, and retry logic — v0.3.0
- ✓ File clipboard integration with auto-write, stale detection, and delete cascade — v0.3.0
- ✓ File sync UI (Dashboard entries, context menu, progress, notifications) — v0.3.0
- ✓ File sync settings with quota enforcement and auto-cleanup — v0.3.0
- ✓ File sync eventual consistency with durable transfer lifecycle tracking — v0.3.0
- ✓ Link content type detection and display with per-device sync toggle — v0.3.0
- ✓ Keyboard shortcuts settings UI with click-to-record and conflict detection — v0.3.0
- ✓ macOS keychain auto-unlock confirmation modal — v0.3.0
- ✓ Event-driven device discovery replacing polling — v0.3.0
- ✓ Consolidated outbound sync planner — v0.3.0

### Active

- [ ] HostEventEmitterPort trait abstraction for multi-runtime event delivery
- [ ] wiring.rs decomposition: pure assembly vs Tauri-specific event loops
- [ ] CoreRuntime extraction from AppRuntime (Tauri-free)
- [ ] Configuration resolution extracted to reusable bootstrap module
- [ ] uc-bootstrap crate as sole composition root with scene-specific builders
- [ ] uc-daemon skeleton with worker lifecycle and local RPC
- [ ] uc-cli skeleton with command routing, direct/daemon dispatch, and output rendering
- [x] CLI clipboard list/get/clear commands with direct-mode bootstrap — Validated in Phase 42

### Deferred

- [ ] Complete chunked transfer resume protocol (CT-02, CT-04 — backend only, frontend deferred)
- [ ] Wire transfer progress events to frontend UI (CT-05)
- [ ] Add favorites persistence (domain model column needed)
- [ ] Wire lifecycle events to frontend (currently polling, not event-driven)
- [ ] Expand typed error migration to port surfaces (ARCHNEXT-01)
- [ ] Domain model refinement for anemic models (ARCHNEXT-02)
- [ ] OTel trace/log layer (Phase 3 of Issue #213)
- [ ] Collector & multi-backend support (Phase 4 of Issue #213)
- [ ] WebDAV cross-internet sync
- [ ] Runtime log profile switching (OBS-01)

### Out of Scope

- Mobile app — desktop-first
- OAuth/third-party login — not required for current product model
- Full OpenTelemetry integration — deferred to dedicated observability milestone
- Remote/cloud log shipping — clipboard logs may contain sensitive content
- In-app log viewer UI — Seq provides dedicated log UI

## Context

Shipped v0.3.0 across phases 19-35 with 363 commits over 8 days.
Tech stack: Tauri 2 + React 18 + Rust + libp2p + XChaCha20-Poly1305.
Hexagonal boundaries compiler-enforced. All sync policy consolidated into OutboundSyncPlanner.
Four content types supported: Text, Image, Link, File — each with per-device toggles.
Full file sync pipeline from chunked transfer through clipboard integration with eventual consistency.
Structured observability from dual-output logging through Seq cross-device tracing.
Phase 52 complete — daemon is now the single source of truth for space access state (WS broadcast + HTTP query), GUI no longer owns SpaceAccessOrchestrator.
Phase 56.1 complete — all daemon wire-protocol string constants centralized in `uc-core::network::daemon_api_strings`, eliminating hardcoded string drift between uc-daemon and uc-daemon-client.
Phase 57 complete — daemon is now the sole clipboard observer via real ClipboardWatcherWorker (clipboard_rs + CaptureClipboardUseCase + WS event broadcast); GUI operates in Passive mode receiving clipboard updates via DaemonWsBridge → RealtimeEvent::ClipboardNewContent → clipboard://event pipeline; write-back loop prevention via ClipboardChangeOriginPort.
Phase 60 complete — FileTransferOrchestrator extracted from uc-tauri to uc-app, wired into uc-bootstrap assembly; file_transfer_wiring.rs deleted.
Phase 61 complete — daemon triggers outbound clipboard sync to peers via OutboundSyncPlanner + SyncOutboundClipboardUseCase + SyncOutboundFileUseCase after local capture; ClipboardWatcherWorker delegates to DaemonClipboardChangeHandler.
Phase 62 complete — daemon receives inbound clipboard from peers via ClipboardTransportPort::subscribe_clipboard(); InboundClipboardSyncWorker applies via SyncInboundClipboardUseCase::with_capture_dependencies(ClipboardIntegrationMode::Full); WS events emitted only for Applied { entry_id: Some } outcomes; shared clipboard_change_origin Arc prevents write-back loops.
Phase 63 complete — daemon file transfer orchestration: DaemonApiEventEmitter forwards Transfer StatusChanged as WS events on file-transfer topic; InboundClipboardSyncWorker seeds pending transfer records via FileTransferOrchestrator with early completion cache reconciliation; FileSyncOrchestratorWorker subscribes to network events for transfer lifecycle management (progress/completed/failed), startup reconciliation, timeout sweeps, and clipboard restore.
Phase 64 complete — Tauri sync retirement: removed 896 lines of daemon-duplicated sync loops from wiring.rs (clipboard_receive, pairing_events, file_transfer_reconcile, timeout_sweep); gated restore_clipboard_entry outbound sync on Full mode to prevent double-sync with daemon; removed dead sync_inbound_clipboard accessor and blake3 dependency from uc-tauri.
Phase 66 complete — Fixed daemon WS topic registration for clipboard and file-transfer subscriptions; added WS reconnection compensation with bridge_state_monitor and DaemonReconnected event; Dashboard auto-refreshes clipboard list on reconnect.
Phase 69 complete — CLI `setup` → "Create new Space" performs encryption initialization locally via `build_cli_runtime()` + `CoreUseCases::initialize_encryption()` instead of starting daemon; eliminates macOS Keychain popup during first-time setup.

## Key Decisions

| Decision                                          | Rationale                                                    | Outcome |
| ------------------------------------------------- | ------------------------------------------------------------ | ------- |
| Two-segment framing for clipboard wire format     | Reduce overhead and enable stream decode                     | ✓ Good  |
| V3 binary protocol with Arc fanout                | Improve large payload performance and memory behavior        | ✓ Good  |
| Manual uc:// URL resolution strategy              | Ensure Windows/WebView compatibility                         | ✓ Good  |
| Background TIFF conversion                        | Keep clipboard capture path responsive                       | ✓ Good  |
| Private deps + facade accessors on AppRuntime     | Compiler-enforced boundary: commands cannot access internals | ✓ Good  |
| CommandError serde tag=code content=message       | Frontend discriminated union handling                        | ✓ Good  |
| TaskRegistry with CancellationToken cascade       | Deterministic shutdown without orphaned tasks                | ✓ Good  |
| StagedPairedDeviceStore as Arc-injected struct    | Replace OnceLock global with lifecycle-owned state           | ✓ Good  |
| AppDeps domain sub-structs (5 groups)             | Reduce god-container coupling                                | ✓ Good  |
| Origin-aware clipboard events                     | Enable local-prepend vs remote-throttle routing              | ✓ Good  |
| Runtime TS theme presets (not static CSS)         | Single source of truth, dynamic switching                    | ✓ Good  |
| Chunked 256KB network writes with progress events | Support large payload transfer with UX feedback              | ✓ Good  |
| UUID v7 for FlowId (time-ordered)                 | Enables time-sorted log querying                             | ✓ Good  |
| Option<Layer> pattern for Seq layer               | Zero overhead when disabled                                  | ✓ Good  |
| SeqLayer implements Layer directly                | Full control over CLEF format without FormatEvent adapter    | ✓ Good  |
| ContentTypes::default() all-true                  | New devices sync everything by default                       | ✓ Good  |
| OutboundSyncPlanner consolidation                 | Single policy decision point, runtime as thin dispatcher     | ✓ Good  |
| Binary codec for FileTransferMessage              | Consistent with clipboard_payload_v3 pattern                 | ✓ Good  |
| Blake3 hash verification for file transfer        | Fast cryptographic integrity check                           | ✓ Good  |
| Durable transfer lifecycle (pending→completed)    | Truthful UI state survives restart                           | ✓ Good  |
| Event-driven device discovery                     | Eliminated 3s polling; immediate responsive UX               | ✓ Good  |

## Constraints

- **Tech stack:** Tauri 2 + React + Rust (fixed)
- **Sync domain:** LAN-first with libp2p
- **Security:** XChaCha20-Poly1305 remains mandatory
- **Platform support:** macOS primary; Windows/Linux supported

---

_Last updated: 2026-03-28 after Phase 71 completion_

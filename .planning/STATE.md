---
gsd_state_version: 1.0
milestone: v0.1
milestone_name: milestone
status: unknown
stopped_at: Completed 54-01-PLAN.md
last_updated: '2026-03-24T08:33:03.945Z'
progress:
  total_phases: 27
  completed_phases: 18
  total_plans: 64
  completed_plans: 57
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-17)

**Core value:** Seamless clipboard synchronization across devices — copy on one, paste on another
**Current focus:** Phase 52 — daemon-space-access-ssot

## Current Position

Phase: 53
Plan: Not started

## Performance Metrics

**Velocity:**

- Total plans completed: 9 (this milestone)
- Average duration: 18min
- Total execution time: 165min

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
| ----- | ----- | ----- | -------- |
| —     | —     | —     | —        |

_Updated after each plan completion_
| Phase 43-unify-gui-and-cli-business-flows P01 | 5 | 4 tasks | 5 files |
| Phase 43-unify-gui-and-cli-business-flows P02 | 5 | 5 tasks | 4 files |
| Phase 36-event-emitter-abstraction P01 | 525664min | 2 tasks | 4 files |
| Phase 36-event-emitter-abstraction P02 | 60 | 2 tasks | 6 files |
| Phase 37-wiring-decomposition P02 | 35 | 2 tasks | 3 files |
| Phase 37-wiring-decomposition P03 | 24 | 2 tasks | 6 files |
| Phase 37-wiring-decomposition P04 | 15 | 2 tasks | 2 files |
| Phase 37-wiring-decomposition P05 | 55 | 3 tasks | 3 files |
| Phase 38-coreruntime-extraction P01 | 18 | 2 tasks | 9 files |
| Phase 38 P02 | 11 | 2 tasks | 4 files |
| Phase 38-coreruntime-extraction P03 | 60 | 2 tasks | 6 files |
| Phase 39-config-resolution-extraction P01 | 4 | 2 tasks | 3 files |
| Phase 39-config-resolution-extraction P02 | 3 | 1 tasks | 1 files |
| Phase 40-uc-bootstrap-crate P01 | 14 | 2 tasks | 16 files |
| Phase 40-uc-bootstrap-crate P02 | 2 | 1 tasks | 2 files |
| Phase 41-daemon-and-cli-skeletons P01 | 7 | 3 tasks | 13 files |
| Phase 41-daemon-and-cli-skeletons P02 | 4 | 2 tasks | 7 files |
| Phase 41-daemon-and-cli-skeletons P03 | 3 | 3 tasks | 10 files |
| Phase 41-daemon-and-cli-skeletons P04 | 8 | 2 tasks | 4 files |
| Phase 45-daemon-api-foundation P01 | 18 | 3 tasks | 10 files |
| Phase 45 P02 | 9 | 2 tasks | 13 files |
| Phase 45 P03 | 18 | 2 tasks | 12 files |
| Phase 46-daemon-pairing-host-migration P01 | 10 | 2 tasks | 6 files |
| Phase 46-daemon-pairing-host-migration P02 | 4 | 2 tasks | 11 files |
| Phase 46 P4 | 11 | 1 tasks | 6 files |
| Phase 46 P05 | 13 | 2 tasks | 8 files |
| Phase 46 P06 | 2 min | 2 tasks | 3 files |
| Phase 46.1 P01 | 5 | 2 tasks | 4 files |
| Phase 46.1 P02 | 5 | 2 tasks | 7 files |
| Phase 46.1 P03 | 29 | 2 tasks | 7 files |
| Phase 46.1 P04 | 9 | 2 tasks | 9 files |
| Phase 46.1 P05 | 56 | 2 tasks | 7 files |
| Phase 46.2-daemon-tauri-daemon P01 | 15 | 2 tasks | 20 files |
| Phase 46.2-daemon-tauri-daemon P02 | 14 | 2 tasks | 16 files |
| Phase 46.4-daemon-setup-gui-cli-setup-cli-gui-cli-daemon-peera-full-mode-peerb-passive-mode-peerb-peera-peera-b-a-b-a-a-b-a-b-a-a-b P01 | 24min | 2 tasks | 10 files |
| Phase 46.4 P02 | 16min | 2 tasks | 8 files |
| Phase 46.4-daemon-setup-gui-cli-setup-cli-gui-cli-daemon-peera-full-mode-peerb-passive-mode-peerb-peera-peera-b-a-b-a-a-b-a-b-a-a-b P03 | 21min | 2 tasks | 13 files |
| Phase 46.3-gui-daemon P01 | 10min | 2 tasks | 8 files |
| Phase 46.3-gui-daemon P02 | 12min | 2 tasks | 6 files |
| Phase 46.5 P02 | 5 | 1 tasks | 7 files |
| Phase 46.5 P04 | 13 | 2 tasks | 4 files |
| Phase 46.5 P01 | 6 | 2 tasks | 5 files |
| Phase 46.5 P03 | 24 | 2 tasks | 8 files |
| Phase 46.6-daemon-tauri-tauri-daemon P01 | 5 min | 2 tasks | 6 files |
| Phase 46.6 P02 | 7 | 2 tasks | 4 files |
| Phase 50-daemon-encryption-state-recovery P01 | 10 | 3 tasks | 1 files |
| Phase 52-daemon-space-access-ssot P01 | 15 | 2 tasks | 9 files |
| Phase 52 P02 | 9 | 2 tasks | 5 files |
| Phase 54-extract-daemon-client-and-realtime-infrastructure-from-uc-tauri P01 | 831 | 4 tasks | 11 files |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.

Recent decisions affecting current work:

- [v0.3.0]: OutboundSyncPlanner consolidation — single policy decision point, runtime as thin dispatcher
- [v0.2.0]: Private deps + facade accessors on AppRuntime — compiler-enforced boundary
- [Phase 36-event-emitter-abstraction]: HostEventEmitterPort synchronous (not async) matching tauri::Emitter::emit() non-async signature
- [Phase 36-event-emitter-abstraction]: PeerConnectionHostEvent collapses PeerReady/PeerConnected to Connected; PeerNotReady/PeerDisconnected to Disconnected — matching frontend binary connected:bool view
- [Phase 36-event-emitter-abstraction]: event_emitter uses RwLock<Arc<dyn Port>> not bare Arc — allows bootstrap swap from LoggingEventEmitter to TauriEventEmitter after AppHandle available
- [Phase 36-event-emitter-abstraction]: app_handle KEPT alongside event_emitter for out-of-scope callers (commands/pairing.rs, commands/clipboard.rs, apply_autostart, setup orchestrator)
- [Phase 36-event-emitter-abstraction]: file_transfer_wiring.rs handle_transfer_progress/completed/failed/spawn_timeout_sweep/reconcile_on_startup deferred to Phase 37 wiring decomposition
- [Phase 37-wiring-decomposition]: app.emit() calls replaced with HostEventEmitterPort; TauriSetupEventPort replaced by HostEventSetupPort; \_app_handle params deferred to Plan 03
- [Phase 37-wiring-decomposition P03]: assembly.rs created with zero tauri imports; BackgroundRuntimeDeps stays in wiring.rs; PlatformLayer made pub(crate) for test access; invoke_handler stays in main.rs (generate_handler! macro constraint)
- [Phase 37-wiring-decomposition]: Synchronously write activeSessionIdRef.current before calling acceptP2PPairing to close verification event race window — useEffect-based ref sync is too late when backend emits immediately
- [Phase 37-wiring-decomposition]: Subscribe before initiate: pairing event subscription moved before initiate_pairing in ensure_pairing_session to eliminate race window
- [Phase 37-wiring-decomposition]: app_closed_tx flag guards StreamClosedByPeer->PairingFailed bridge from firing on explicit application-initiated session closes
- [Phase 38-coreruntime-extraction]: tokio-util added to uc-app without 'sync' feature — locked version 0.7.17 includes CancellationToken in default features
- [Phase 38-coreruntime-extraction]: resolve_pairing_device_name inlined into uc-app/adapters.rs to keep uc-app free of uc-tauri dependency
- [Phase 38-coreruntime-extraction]: uc-tauri re-export pattern used for TaskRegistry (pub use uc_app::task_registry::TaskRegistry) for backward compatibility
- [Phase 38]: CoreRuntime::new() accepts pre-built Arc<RwLock<Arc<dyn HostEventEmitterPort>>> — caller creates the cell, CoreRuntime never wraps internally
- [Phase 38]: emitter_cell created once in with_setup() and shared with both build_setup_orchestrator and CoreRuntime::new() — same Arc, no copies
- [Phase 38-coreruntime-extraction]: AppUseCases wraps CoreUseCases via Deref<Target=CoreUseCases> — all ~35 pure domain accessors transparent without duplication
- [Phase 38-coreruntime-extraction]: build_setup_orchestrator extracted to assembly.rs as standalone pub fn — satisfies RNTM-05 single composition point, eliminating secondary wiring in runtime.rs
- [Phase 38-coreruntime-extraction]: SetupAssemblyPorts contains only 5 external adapter ports; shared-cell params (emitter_cell, lifecycle_status, watcher_control, session_ready_emitter, clipboard_integration_mode) are separate build_setup_orchestrator params
- [Phase 39]: config_resolution.rs in uc-tauri/bootstrap/ (not uc-app) — DirsAppDirsAdapter (uc-platform) cannot be a prod dep of uc-app
- [Phase 39]: resolve_app_config() returns Result<AppConfig, ConfigResolutionError> with typed enum variants for InvalidConfig and PlatformDirsFailed
- [Phase 39]: main.rs imports uc_tauri::bootstrap::resolve_app_config via bootstrap/mod.rs re-export; storage_paths moved before key_slot_store construction so vault_dir is available
- [Phase 40]: Assembly helpers widened to pub for cross-crate test access; PlatformLayer made pub in uc-bootstrap
- [Phase 40]: Re-export stub pattern: uc-tauri bootstrap modules become thin pub use uc_bootstrap::module::\* stubs
- [Phase 40]: Idempotent tracing: TRACING_INITIALIZED OnceLock guard allows safe multiple init_tracing_subscriber calls
- [Phase 40]: Builders return AppDeps (not CoreRuntime) per Codex Review R1 -- callers construct CoreRuntime with appropriate emitter/lifecycle
- [Phase 40]: GUI builder uses standalone tokio::runtime::Builder (not tauri::async_runtime) to keep uc-bootstrap tauri-free
- [Phase 41]: ClipboardIntegrationMode::Passive used for non-GUI modes (Disabled variant does not exist); Passive correctly disables OS clipboard observation
- [Phase 41]: DaemonWorker trait: async start(CancellationToken), async stop(), sync health_check() -> WorkerHealth; RuntimeState is snapshot-only (no worker ownership)
- [Phase 41]: Explicit tokio runtime construction (not #[tokio::main]) for daemon to avoid conflicts with tracing init's internal Seq runtime
- [Phase 41]: DaemonApp binds RPC socket before starting workers for fail-fast on already-running daemon
- [Phase 41]: Workers stored as Vec<Arc<dyn DaemonWorker>> for tokio::spawn 'static compatibility
- [Phase 41]: CLI dual-dispatch: status via daemon RPC, devices/space-status via direct bootstrap
- [Phase 41]: Unix socket path resolution is centralized in uc-daemon so daemon and CLI cannot drift
- [Phase 41]: On Unix, overlong XDG runtime paths warn and fall back to /tmp to stay under the 103-byte sun_path payload limit
- [Phase 43]: GetP2pPeersSnapshot uses both PeerDirectoryPort AND PairedDeviceRepositoryPort - cross-port aggregation in app layer
- [Phase 43]: P2pPeerSnapshot preserves pairing_state and identity_fingerprint for CLI output compatibility
- [Phase 43]: Aggregation logic extracted from Tauri commands into shared uc-app use case - GUI and CLI now share same business logic
- [Phase 45]: Daemon auth uses a dedicated local token file with restricted permissions and runtime-injected connection info rather than URL/query-string auth
- [Phase 45]: RuntimeState now stores daemon-owned worker snapshots so JSON-RPC and HTTP/WebSocket transports map from one internal runtime source
- [Phase 45]: WebSocket auth now reuses the daemon HTTP bearer-token check during upgrade so protected transport behavior stays aligned across routes and subscriptions.
- [Phase 45]: Phase 45 websocket topics emit snapshot-first events in client subscription order and reserve stable incremental event-type strings for later runtime fanout.
- [Phase 45]: Pairing websocket payloads stay metadata-only and never serialize keyslot files or raw challenge bytes before the daemon becomes the pairing host.
- [Phase 45]: CLI daemon reads now share one reqwest client that resolves daemon URL/token via uc-daemon helpers and preserves exit code 5 when the daemon or token file is absent.
- [Phase 45]: Tauri shell stores daemon connection info only in managed in-memory state and emits `daemon://connection-info` after the main webview finishes loading; no token persistence in browser storage or query strings.
- [Phase 46]: DaemonApp starts a single daemon-owned PairingHost alongside RPC and HTTP, making pairing session lifetime independent from Tauri/webview disconnects.
- [Phase 46]: RuntimeState now stores metadata-only DaemonPairingSessionSnapshot records for daemon-owned pairing session inspection without leaking verification secrets.
- [Phase 46]: DaemonApiState exposes one shared pairing_host facade plus lease-based discoverability/readiness controls, so mutation routes and future bridge code use the same daemon-owned host surface.
- [Phase 46]: Pairing/discovery websocket incremental payloads are camelCase and use the top-level `type` field; verification secrets only travel over authenticated realtime events, never through snapshots.
- [Phase 46]: SetupPairingFacadePort lives in uc-app and PairingOrchestrator implements it for bootstrap and non-daemon call sites.
- [Phase 46]: SetupAssemblyPorts placeholder uses a no-op facade instead of constructing a concrete PairingOrchestrator.
- [Phase 46]: Phase 46 GUI startup now always constructs PairingBridge; Tauri no longer keeps pairing action/event loops as a hidden fallback host.
- [Phase 46]: Daemon pairing host now broadcasts live pairing and peer websocket events so Tauri compatibility bridges receive runtime updates, not just snapshots.
- [Phase 46]: Bridge payload translation stays locked to the existing frontend event contract, including code/localFingerprint/deviceName fields and peer discovery delta payloads.
- [Phase 46]: Use broadcast::channel::<DaemonWsEvent>(128) in daemon regression fixtures so DaemonPairingHost::new signature stays aligned without runtime behavior changes.
- [Phase 46]: Define PH46-01..PH46-06 explicitly in REQUIREMENTS.md and map each ID to phase 46 for audit traceability.
- [Phase 46.1]: RealtimeFrontendEvent preserves the frontend wire key name type via a raw identifier and accessor in uc-core.
- [Phase 46.1]: HostEvent adds Realtime(RealtimeFrontendEvent) while keeping legacy domain variants until the Phase 46.1 frontend cutover completes.
- [Phase 46.1]: `uc-app` owns pairing, peers, and setup realtime consumers; setup subscriptions now flow through a shared `SetupPairingEventHub` instead of feature-owned websocket logic.
- [Phase 46.1]: Setup realtime delivery drops regressive per-session events after terminal success/failure so frontend-visible ordering stays monotonic across shared subscriptions.
- [Phase 46.1]: DaemonWsBridge is now the single active daemon websocket owner and background startup launches it through `start_realtime_runtime`.
- [Phase 46.1]: Tauri emitters understand `HostEvent::Realtime`, so bridge/runtime verification can compile against the unified realtime envelope.
- [Phase 46.1]: Frontend listeners now consume one `daemon://realtime` helper, and setup / space-access adapter events are serialized onto the same setup topic envelope.
- [Phase 46.1]: Frontend contract verification must run through the repository’s vitest script entrypoint rather than Bun’s native test runner to preserve jsdom/mock behavior.
- [Phase 46.1]: PairingBridge and its compatibility tests are deleted; daemon realtime now has no second Tauri-owned websocket host path.
- [Phase 46.1]: Setup pairing facade subscriptions are hub-only; legacy setup websocket subscription markers are fully removed from runtime code.
- [Phase 46.2-daemon-tauri-daemon]: GUI pairing lease renewal now uses /pairing/gui/lease so discoverability and participant readiness stay daemon-owned behind one bridge call.
- [Phase 46.2-daemon-tauri-daemon]: Daemon pairing websocket payloads carry kind/stage metadata and DaemonWsBridge translates them back into the existing frontend pairing event contract.
- [Phase 46.2-daemon-tauri-daemon]: Legacy Tauri pairing host loops and direct PairingOrchestrator state were removed from uc-tauri; bridge regression coverage now lives in daemon_ws_bridge and frontend tests.
- [Phase 46.2-daemon-tauri-daemon]: Use a typed DaemonPairingRequestError in uc-tauri so daemon error code/message data survives into Tauri command mapping without brittle string parsing.
- [Phase 46.2-daemon-tauri-daemon]: Install a single frontend p2p-command-error listener in src/api/p2p.ts and classify pairing failures once so initiator and passive UX stay aligned.
- [Phase 46.4-daemon-setup-gui-cli-setup-cli-gui-cli-daemon-peera-full-mode-peerb-passive-mode-peerb-peera-peera-b-a-b-a-a-b-a-b-a-a-b]: DaemonApiState now carries a direct SetupOrchestrator handle so setup routes stay daemon-owned.
- [Phase 46.4-daemon-setup-gui-cli-setup-cli-gui-cli-daemon-peera-full-mode-peerb-passive-mode-peerb-peera-peera-b-a-b-a-a-b-a-b-a-a-b]: Setup HTTP state responses use serialized SetupState plus fixed nextStepHint and local identity metadata.
- [Phase 46.4-daemon-setup-gui-cli-setup-cli-gui-cli-daemon-peera-full-mode-peerb-passive-mode-peerb-peera-peera-b-a-b-a-a-b-a-b-a-a-b]: setup_api HTTP regressions use a controlled fake SetupPairingFacade fixture to keep transport tests deterministic.
- [Phase 46.4]: CLI setup commands remain thin adapters over daemon /setup and /peers endpoints; uc-cli does not own setup truth.
- [Phase 46.4]: setup host stays attached after local space creation and only exits after an operator-handled request resolves or the session is canceled.
- [Phase 46.4]: CLI smoke tests serialize process-level invocations to avoid shared local state races during cargo test.
- [Phase 46.4-daemon-setup-gui-cli-setup-cli-gui-cli-daemon-peera-full-mode-peerb-passive-mode-peerb-peera-peera-b-a-b-a-a-b-a-b-a-a-b]: Reset stays daemon-owned and clears setup/session/lease/paired-device/encryption residue through existing runtime ports instead of deleting the whole profile directory.
- [Phase 46.3-gui-daemon]: GUI and daemon now inherit one workspace package version, and daemon health/status expose `packageVersion` plus `apiRevision` as the compatibility identity contract.
- [Phase 46.3-gui-daemon]: Bootstrap probing now classifies the expected local daemon endpoint as `Absent`, `Compatible`, or `Incompatible`; malformed or legacy `/health` payloads are incompatible, not absent.
- [Phase 46.3-gui-daemon]: Daemon now writes profile-aware PID metadata for the expected local endpoint and removes it on shutdown via a dedicated PID guard.
- [Phase 46.3-gui-daemon]: GUI bootstrap replacement is bounded to one incompatible-daemon terminate attempt plus one spawned-daemon startup wait, with ownership facts tracked separately from connection state.
- [Phase 46.5]: Shared daemon request authorization lives in uc-tauri daemon_client/mod so setup/query/pairing clients reuse one base_url and bearer-token path.
- [Phase 46.5]: POST /pairing/unpair executes CoreUseCases::unpair_device() and keeps the existing PairingApiErrorResponse envelope instead of introducing a second pairing error contract.
- [Phase 46.5]: Tauri pairing paired-device reads and unpair now shell directly to daemon `/paired-devices` and `/pairing/unpair` routes instead of local use cases.
- [Phase 46.5]: Tauri setup commands keep the existing invoke names, but deserialize daemon `state` payloads back into `SetupState` before returning to the frontend.
- [Phase 46.5]: Local submit_passphrase(passphrase1, passphrase2) mismatch stays in-process and returns SetupError::PassphraseMismatch without any daemon request.
- [Phase 46.5]: Pairing runtime ownership is now an explicit uc-bootstrap input via PairingRuntimeOwner instead of an implicit GUI convention.
- [Phase 46.5]: GUI and CLI bind NetworkPorts.pairing to DisabledPairingTransport so any residual local pairing path fails fast instead of silently using libp2p transport.
- [Phase 46.5]: daemon://realtime is the only active frontend pairing listener surface; legacy p2p-command-error fallback is deleted from both backend and frontend active paths.
- [Phase 46.5]: daemon API regressions must make watcher/network prerequisites explicit in fixtures; host confirm is asserted via response-level `nextStepHint = host-confirm-peer`, not by assuming a join-only state enum.
- [Phase 46.6-daemon-tauri-tauri-daemon]: Keep DaemonBootstrapOwnershipState as a bootstrap facts snapshot and move live child ownership into a separate GuiOwnedDaemonState. — Bootstrap facts remain cloneable and testable, while live Child handles now persist in managed Tauri state for later exit cleanup.
- [Phase 46.6-daemon-tauri-tauri-daemon]: Bootstrap clears managed ownership on Compatible probes and records ownership only after actual spawn or replacement paths. — This prevents GUI exit cleanup from targeting independently started compatible daemons while still carrying replacement-owned children forward.
- [Phase 46.6]: Real daemon cleanup now runs only from RunEvent::ExitRequested; main-window CloseRequested remains tray-hide and never targets daemon processes.
- [Phase 46.6]: GuiOwnedDaemonState now owns bounded daemon teardown with exit idempotency guard, restoring child ownership on cleanup failure and force-killing only after graceful timeout.
- [Phase 50]: Strategy B behavioral tests: mock AutoUnlockEncryptionSession ports directly to avoid CoreRuntime construction complexity
- [Phase 50]: recover_encryption_session() placed before check_or_remove_stale_socket for clean fail-fast with no orphaned resources
- [Phase 52-daemon-space-access-ssot]: broadcast_space_access_state_changed is a free function called by DaemonPairingHost and handle_pairing_message, avoiding code duplication
- [Phase 52-daemon-space-access-ssot]: Space access broadcasts placed at four transition sites: reset_setup_state, start_completed_host_sponsor_authorization, resolve_host_space_access_proof, apply_joiner_space_access_result
- [Phase 52]: space_access.snapshot and space_access.state_changed both map to SpaceAccessStateChangedPayload for uniform frontend event type
- [Phase 52]: SpaceAccessOrchestrator creation kept in build_gui_app for SetupAssemblyPorts internal wiring, not returned in GuiBootstrapContext
- [Phase 54-extract-daemon-client-and-realtime-infrastructure-from-uc-tauri]: uc-daemon-client added to workspace members in Plan 01 (not deferred to Plan 02) — required for verification to work

### Roadmap Evolution

v0.3.0 phases (19-35) completed and archived.
v0.4.0 runs phases 36-41. Phase numbering is continuous.

- Phase 42 added: CLI Clipboard Commands — list, get, and clear clipboard entries via CLI
- Phase 43 added: Unify GUI and CLI business flows to eliminate per-entrypoint feature adaptation
- Phase 44 added: CLI Pairing and Sync Commands — add CLI commands for device pairing and manual sync
- Phase 45 added: Daemon API Foundation — add local HTTP and WebSocket transport with read-only runtime queries
- Phase 46 added: Daemon Pairing Host Migration — move pairing orchestrator, action loops, and network event handling out of Tauri
- Phase 46.1 inserted after Phase 46: Unify realtime subscriptions on single DaemonWsBridge (URGENT)
- Phase 46.2 inserted after Phase 46: 彻底打通基于 daemon 的配对流程, 完全移除原 tauri 中相关的配对流程. 期望: 在不改变用户配对流程的情况下,内部替换成基于 daemon 的配对流程实现 (URGENT)
- Phase 46.3 inserted after Phase 46: 修复 GUI 启动 daemon 的生命周期托管与版本不匹配静默替换 (URGENT)
- Phase 46.4 inserted after Phase 46: 当前基于 daemon 重构后的 setup 流程还没有跑通(GUI), 为了加快开发和调试,我考虑先实现 cli 版本的 setup 流程, 理论上 cli 和 gui 都走的同一个流程,只是不同的入口,所以我考虑先将 cli +daemon 的方式给打通. 端到端将如何进行测试,你需要在两个终端中,分别起一个 peerA (full mode), 另一个是 peerB (passive mode), 然后让 peerB 成功与peerA 进行配对,也就是说, peerA 先新建加密空间, B 需要发现 A,B 请求A,A确认, B 输入加密口令, A 验证加密口令, 最终B成功加入 A; 对于 A 和 B 查询已配对设备都应该能看到对方. (URGENT)
- Phase 46.5 inserted after Phase 46: 将配对业务逻辑从 Tauri 层彻底移除，统一收口到 daemon (URGENT)
- Phase 46.6 inserted after Phase 46: daemon 需要跟随 tauri 启动和关闭,现在 tauri 关闭后,daemon 完全变成了孤儿进程 (URGENT)
- Phase 47 added: Frontend Daemon Cutover — switch desktop UI from Tauri commands to daemon HTTP and WebSocket APIs
- Phase 48 added: Daemon-Only Application Host Cleanup — remove legacy Tauri business entrypoints and consolidate runtime ownership
- Phase 49 added: Setup 验证码链路单一化重构 — unify setup verification code path: setup orchestrator → daemon ws → Tauri bridge → setup realtime store → SetupPage, cut SetupPage/PairingNotificationProvider dual-source
- Phase 50 added: Daemon encryption state recovery on startup — daemon 重启后从磁盘恢复 master key
- Phase 51 added: Peer discovery deduplication fix — 修复 mDNS 扫描出重复设备
- Phase 52 added: Daemon as single source of truth for space access state — daemon 作为 space access 唯一状态源
- Phase 53 added: End-to-end join space flow verification — 端到端 join space 流程验证
- Phase 54 added: Extract daemon client and realtime infrastructure from uc-tauri — 将 daemon HTTP 客户端、WebSocket 桥接、realtime 运行时从 uc-tauri 提取到独立 crate
- Phase 55 added: Extract daemon lifecycle and setup pairing bridge from uc-tauri — 将 daemon 进程生命周期管理和 setup pairing facade 从 uc-tauri 提取到 uc-daemon/uc-daemon-client
- Phase 56 added: Refactor daemon host architecture — extract peer lifecycle from PairingHost, unify host lifecycle management

### Pending Todos

- `2026-03-21-fix-setup-pairing-confirmation-toast-missing.md` — 修复 setup 中选择设备后未出现配对确认提示的问题

### Blockers/Concerns

- Phase 40 (uc-bootstrap) is high risk: crate extraction touches dependency graph across uc-tauri, uc-infra, uc-platform. Verify cargo workspace configuration before planning.

### Known Bugs (deferred to future phases)

- **[Phase 38] setup_event_port holds stale LoggingEventEmitter**: `HostEventSetupPort` captures the initial `LoggingEventEmitter` Arc at `AppRuntime::with_setup` creation time (runtime.rs:420-422). When `set_event_emitter` swaps to `TauriEventEmitter` (main.rs:673-677), the swap does NOT propagate to `SetupOrchestrator`'s internal `setup_event_port`. Result: state changes emitted from spawned listener tasks (e.g. `ProcessingJoinSpace → JoinSpaceConfirmPeer` via `start_pairing_verification_listener_with_rx`) only log to console but never reach the frontend. Orchestrator dispatch-driven transitions work because the Tauri command return value carries the state directly. **Fix**: Phase 38 unifies SetupOrchestrator assembly into a single composition point, eliminating the two-phase emitter swap problem. **UAT impact**: PeerB setup UI does not advance from ProcessingJoinSpace to JoinSpaceConfirmPeer (PIN confirm screen) even though backend state is correct.

## Session Continuity

Last session: 2026-03-24T08:32:56.711Z
Stopped at: Completed 54-01-PLAN.md
Resume file: None

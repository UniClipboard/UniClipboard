# Roadmap: UniClipboard Desktop

## Milestones

- ✅ **v0.1.0 Daily Driver** — Phases 1-9 (shipped 2026-03-06)
- ✅ **v0.2.0 Architecture Remediation** — Phases 10-18 (shipped 2026-03-09)
- ✅ **v0.3.0 Log Observability & Feature Expansion** — Phases 19-35 (shipped 2026-03-17)
- 🚧 **v0.4.0 Runtime Mode Separation** — Phases 36-41 (in progress)

## Phases

<details>
<summary>✅ v0.1.0 Daily Driver (Phases 1-9) — SHIPPED 2026-03-06</summary>

See: `.planning/milestones/v0.1.0-ROADMAP.md`

</details>

<details>
<summary>✅ v0.2.0 Architecture Remediation (Phases 10-18) — SHIPPED 2026-03-09</summary>

See: `.planning/milestones/v0.2.0-ROADMAP.md`

</details>

<details>
<summary>✅ v0.3.0 Log Observability & Feature Expansion (Phases 19-35) — SHIPPED 2026-03-17</summary>

See: `.planning/milestones/v0.3.0-ROADMAP.md`

- [x] Phase 19: Dual Output Logging Foundation (2/2 plans)
- [x] Phase 20: Clipboard Capture Flow Correlation (3/3 plans)
- [x] Phase 21: Sync Flow Correlation (2/2 plans)
- [x] Phase 22: Seq Local Visualization (2/2 plans)
- [x] Phase 23: Distributed Tracing (2/2 plans)
- [x] Phase 24: Per-Device Sync Settings (3/3 plans)
- [x] Phase 25: Content Type Toggles (2/2 plans)
- [x] Phase 26: Global Sync Master Toggle (2/2 plans)
- [x] Phase 27: Keyboard Shortcuts Settings (1/2 plans — code complete, doc gap)
- [x] Phase 28: File Sync Foundation + Link Content Type (5/5 plans)
- [x] Phase 29: macOS Keychain Modal (2/2 plans)
- [x] Phase 30: File Transfer Service (4/4 plans)
- [x] Phase 31: File Sync UI (3/3 plans)
- [x] Phase 32: File Sync Settings (3/3 plans)
- [x] Phase 32.1: File Clipboard Integration (3/3 plans)
- [x] Phase 33: File Sync Eventual Consistency (6/6 plans)
- [x] Phase 34: Event-Driven Device Discovery (2/2 plans)
- [x] Phase 35: OutboundSyncPlanner Extraction (2/2 plans)

</details>

### 🚧 v0.4.0 Runtime Mode Separation (In Progress)

**Milestone Goal:** Extract non-Tauri logic from uc-tauri into shared crates, enabling GUI, CLI, and daemon as independent runtime modes with a single composition root.

#### Phases

- [x] **Phase 36: Event Emitter Abstraction** - Replace hardcoded AppHandle::emit() with HostEventEmitterPort trait and adapters (completed 2026-03-17)
- [x] **Phase 37: Wiring Decomposition** - Split wiring.rs into pure assembly module and Tauri-specific event loop module (completed 2026-03-17)
- [x] **Phase 38: CoreRuntime Extraction** - Extract Tauri-free CoreRuntime and unify SetupOrchestrator into single composition point (completed 2026-03-18)
- [x] **Phase 39: Config Resolution Extraction** - Move path/profile/keyslot resolution from main.rs into reusable bootstrap module (completed 2026-03-18)
- [ ] **Phase 40: uc-bootstrap Crate** - Create sole composition root crate with scene-specific builders and unified logging init
- [x] **Phase 41: Daemon and CLI Skeletons** - Create uc-daemon and uc-cli crates with end-to-end path validation (completed 2026-03-18)
- [x] **Phase 42: CLI Clipboard Commands** - list, get, and clear clipboard entries via CLI (completed 2026-03-19)
- [x] **Phase 43: Unify GUI and CLI Business Flows** - eliminate per-entrypoint feature adaptation by routing both surfaces through the same application flow (completed 2026-03-19)

## Phase Details

### Phase 36: Event Emitter Abstraction

**Goal**: Background tasks deliver host events through an abstract port, eliminating direct AppHandle coupling
**Depends on**: Nothing (first phase of v0.4.0)
**Requirements**: EVNT-01, EVNT-02, EVNT-03, EVNT-04
**Success Criteria** (what must be TRUE):

1. HostEventEmitterPort trait exists in uc-core/ports and compiles without any Tauri dependency
2. TauriEventEmitter adapter wraps AppHandle and implements HostEventEmitterPort; GUI app continues to emit clipboard and sync events to the frontend as before
3. LoggingEventEmitter adapter exists and implements HostEventEmitterPort by writing events to tracing output
4. Clipboard watcher, peer discovery, and sync scheduler accept HostEventEmitterPort instead of AppHandle<R>; the compiler rejects any direct AppHandle use in these components

**Plans:** 3/3 plans complete

Plans:

- [ ] 36-01-PLAN.md — Define HostEventEmitterPort trait, HostEvent type system, TauriEventEmitter and LoggingEventEmitter adapters with contract tests
- [ ] 36-02-PLAN.md — Wire emitter port into AppRuntime, wiring.rs, and file_transfer_wiring.rs; delete obsolete event types

### Phase 37: Wiring Decomposition

**Goal**: wiring.rs is split into a pure Rust assembly module (no Tauri types) and a thin Tauri-specific event loop layer
**Depends on**: Phase 36
**Requirements**: RNTM-02
**Success Criteria** (what must be TRUE):

1. A new pure-assembly module exists that constructs application dependencies without importing any tauri crate; it compiles as a library with no Tauri feature flags
2. A separate Tauri-specific module (wiring.rs) owns the Tauri event loop setup, app handle wiring, and command registration; within the wiring split pair, it is the only module that imports tauri types (assembly.rs has zero tauri imports)
3. Existing GUI behavior is unchanged: clipboard sync, pairing, and settings all continue to function after the split
4. assembly.rs contains zero tauri imports (verified by CI lint) and its public API is Tauri-type-free, preparing it for independent `cargo check` in Phase 40 when uc-bootstrap crate is created

**Plans:** 5/5 plans complete

Plans:

- [x] 37-01-PLAN.md — Define PairingHostEvent, SetupHostEvent, SpaceAccessHostEvent in uc-core; extend TauriEventEmitter + LoggingEventEmitter with contract tests
- [x] 37-02-PLAN.md — Migrate all app.emit() calls in wiring.rs and file_transfer_wiring.rs to HostEventEmitterPort
- [x] 37-03-PLAN.md — Split wiring.rs into assembly.rs + wiring.rs, remove AppHandle from start_background_tasks, update ROADMAP

### Phase 38: CoreRuntime Extraction

**Goal**: A Tauri-free CoreRuntime struct holds AppDeps and orchestrators; SetupOrchestrator assembly lives in one composition point
**Depends on**: Phase 37
**Requirements**: RNTM-01, RNTM-05
**Success Criteria** (what must be TRUE):

1. CoreRuntime struct exists in a crate with no Tauri dependency, holds AppDeps and shared orchestrators, and passes `cargo check` independently
2. AppRuntime wraps CoreRuntime and adds only Tauri-specific handles; no orchestration logic lives in AppRuntime itself
3. SetupOrchestrator is assembled exactly once in the main composition root; runtime.rs contains no secondary wiring or orchestrator construction
4. The existing GUI setup flow (first-run setup, encrypted space unlock) continues to work end-to-end

**Plans:** 4 plans (3 complete + 1 gap closure)

Plans:

- [ ] 38-01-PLAN.md — Move TaskRegistry and lifecycle adapters from uc-tauri to uc-app (prerequisites)
- [ ] 38-02-PLAN.md — Create CoreRuntime in uc-app, refactor AppRuntime to wrap it, fix stale emitter bug
- [ ] 38-03-PLAN.md — Split UseCases into CoreUseCases + AppUseCases, extract SetupOrchestrator to assembly.rs

### Phase 39: Config Resolution Extraction

**Goal**: Path resolution, profile suffix derivation, and keyslot directory logic are extracted from main.rs into a reusable, testable module
**Depends on**: Phase 38
**Requirements**: RNTM-03
**Success Criteria** (what must be TRUE):

1. A dedicated config resolution module (not main.rs) owns all path/profile/keyslot resolution functions and is accessible to non-Tauri entry points
2. main.rs delegates to the module rather than containing inline resolution logic; main.rs shrinks accordingly
3. The resolution functions are unit-testable without a running Tauri app
4. GUI app launches and resolves config paths correctly after the extraction

**Plans:** 3/3 plans complete

Plans:

- [ ] 39-01-PLAN.md — Create config_resolution.rs module with resolve_config_path, resolve_app_config, ConfigResolutionError, and migrated tests
- [ ] 39-02-PLAN.md — Wire main.rs to use config_resolution module, delete duplicate functions, consolidate key_slot_store path resolution

### Phase 40: uc-bootstrap Crate

**Goal**: uc-bootstrap exists as the sole composition root; all entry points depend on it instead of wiring uc-infra and uc-platform directly
**Depends on**: Phase 39
**Requirements**: BOOT-01, BOOT-02, BOOT-03, BOOT-04, BOOT-05, RNTM-04
**Success Criteria** (what must be TRUE):

1. uc-bootstrap crate exists with declared dependencies on uc-core, uc-app, uc-infra, and uc-platform; `cargo check` passes
2. build_cli_context() function exists in uc-bootstrap and returns a CLI-ready dependency set without starting background workers
3. build_daemon_app() function exists in uc-bootstrap and returns daemon-ready dependencies with worker handles
4. uc-tauri's Cargo.toml lists uc-bootstrap as a dependency and no longer directly depends on uc-infra or uc-platform for composition
5. UseCases accessor is instantiated inside uc-bootstrap and shared to all entry points; no entry point constructs its own UseCases
6. Logging initialization (init_tracing_subscriber) is called exactly once inside uc-bootstrap; duplicate calls in main.rs or other entry points are removed

**Plans:** 2/3 plans executed

Plans:

- [ ] 40-01-PLAN.md — Create uc-bootstrap crate, move assembly/config/tracing/init modules, make tracing idempotent, wire uc-tauri re-exports
- [ ] 40-02-PLAN.md — Create scene-specific builders (build_gui_app, build_cli_context, build_daemon_app) with shared build_core helper
- [ ] 40-03-PLAN.md — Migrate main.rs to use build_gui_app(), update root Cargo.toml, verify GUI app functions correctly

### Phase 41: Daemon and CLI Skeletons

**Goal**: uc-daemon and uc-cli crates exist with working startup paths, end-to-end RPC connectivity, and direct command execution validated
**Depends on**: Phase 40
**Requirements**: DAEM-01, DAEM-02, DAEM-03, DAEM-04, CLI-01, CLI-02, CLI-03, CLI-04, CLI-05
**Success Criteria** (what must be TRUE):

1. uc-daemon binary starts, initializes via uc-bootstrap, logs startup, and shuts down gracefully on SIGTERM/Ctrl-C without panicking
2. Running `uniclipboard-daemon` then `uniclipboard-cli status` returns a JSON response containing uptime and worker health fields
3. Running `uniclipboard-cli devices` (direct mode via uc-bootstrap) returns a device list without the daemon running
4. `uniclipboard-cli --json status` outputs valid JSON; `uniclipboard-cli status` (no flag) outputs human-readable text
5. Exit codes are stable: 0 on success, 1 on error, 5 when daemon is unreachable
6. DaemonWorker trait exists; placeholder clipboard watcher and peer discovery workers implement it and are registered with DaemonApp

**Plans:** 4/4 plans complete

Plans:

- [ ] 41-01-PLAN.md — Create LoggingHostEventEmitter + build_non_gui_runtime() in uc-bootstrap; create uc-daemon crate with DaemonWorker trait, placeholder workers, RPC types, RuntimeState
- [ ] 41-02-PLAN.md — Create RPC server, handler, DaemonApp lifecycle, and daemon main.rs entry point
- [ ] 41-03-PLAN.md — Create uc-cli crate with clap parsing, status command (RPC), devices and space-status commands (direct mode), --json flag, exit codes
- [ ] 41-04-PLAN.md — Fix socket path SUN_LEN overflow: extract shared resolve_daemon_socket_path to uc-daemon, wire daemon and CLI (gap closure)

## Progress

| Phase                             | Milestone | Plans Complete | Status     | Completed  |
| --------------------------------- | --------- | -------------- | ---------- | ---------- |
| 1-9                               | v0.1.0    | 17/17          | Complete   | 2026-03-06 |
| 10-18                             | v0.2.0    | 22/22          | Complete   | 2026-03-09 |
| 19-35                             | v0.3.0    | 51/51          | Complete   | 2026-03-17 |
| 36. Event Emitter Abstraction     | 2/2       | Complete       | 2026-03-17 | -          |
| 37. Wiring Decomposition          | 5/5       | Complete       | 2026-03-18 | 2026-03-17 |
| 38. CoreRuntime Extraction        | 3/3       | Complete       | 2026-03-18 | -          |
| 39. Config Resolution Extraction  | 2/2       | Complete       | 2026-03-18 | -          |
| 40. uc-bootstrap Crate            | 2/3       | In Progress    |            | -          |
| 41. Daemon and CLI Skeletons      | 4/4       | Complete       | 2026-03-18 | -          |
| 45. Daemon API Foundation         | 3/3       | Complete       | 2026-03-19 | -          |
| 46. Daemon Pairing Host Migration | 6/6       | Complete       | 2026-03-20 | -          |

### Phase 42: CLI Clipboard Commands — list, get, and clear clipboard entries via CLI

**Goal:** [To be planned]
**Requirements**: TBD
**Depends on:** Phase 41
**Plans:** 1/1 plans complete

Plans:

- [x] TBD (run /gsd:plan-phase 42 to break down) (completed 2026-03-19)

### Phase 43: Unify GUI and CLI business flows to eliminate per-entrypoint feature adaptation

### Phase 44: CLI Pairing and Sync Commands

**Goal:** [To be planned]
**Requirements:** TBD
**Depends on:** Phase 43
**Plans:** 0 plans

Plans:

- [ ] TBD (run /gsd:plan-phase 44 to break down)

**Goal:** Unify GUI and CLI business flows by creating shared app-layer entrypoints, eliminating duplicated bootstrap code in CLI and cross-use-case aggregation in Tauri commands.
**Requirements**: PH43-01, PH43-02, PH43-03, PH43-04
**Depends on:** Phase 42
**Plans:** 3/3 plans complete

**Success Criteria** (what must be TRUE):

1. CLI commands acquire runtime context through one shared path (`build_cli_runtime`) instead of repeating bootstrap sequence
2. GUI and CLI clipboard flows call the same app-layer entrypoint (`CoreUseCases`)
3. Pairing peer aggregation is a shared app-layer flow, not scattered in Tauri commands
4. Both Tauri commands and CLI use the same shared pairing snapshot use case

Plans:

- [x] 43-01-PLAN.md — Unify CLI bootstrap: add build_cli_runtime() helper (completed 2026-03-19)
- [ ] 43-02-PLAN.md — Unify pairing aggregation: add GetP2pPeersSnapshot use case

### Phase 45: Daemon API Foundation — add local HTTP and WebSocket transport with read-only runtime queries

**Goal:** Add the daemon-facing HTTP/WebSocket contract foundation, including local auth, read-only DTOs, and runtime query services
**Requirements**: PH45-01, PH45-02, PH45-05, PH45-06
**Depends on:** Phase 44
**Plans:** 4 plans (3 complete + 1 gap closure)

Plans:

- [x] 45-01-PLAN.md — Daemon API Contract And Auth Foundation (completed 2026-03-19)
- [x] 45-02-PLAN.md — Add loopback HTTP + WebSocket server and serve read-only daemon routes (completed 2026-03-19)
- [x] 45-03-PLAN.md — Add shared daemon client usage for CLI/Tauri bootstrap without frontend cutover (completed 2026-03-19)

### Phase 46: Daemon Pairing Host Migration — move pairing orchestrator, action loops, and network event handling out of Tauri

**Goal:** Move pairing host ownership, action/event loops, and session projection into `uc-daemon` while keeping Tauri as a compatibility bridge.
**Requirements**: PH46-01, PH46-01A, PH46-01B, PH46-02, PH46-03, PH46-03A, PH46-04, PH46-05, PH46-05A, PH46-06
**Depends on:** Phase 45
**Plans:** 6/6 plans complete

Plans:

- [x] 46-01-PLAN.md — Daemon Pairing Host Ownership And Runtime Projection (completed 2026-03-19)
- [x] 46-02-PLAN.md — Daemon Pairing Control Surface And Realtime Contract (completed 2026-03-20)
- [x] 46-03-PLAN.md — Tauri Compatibility Bridge For Existing Pairing Contract (completed 2026-03-20)
- [x] 46-04-PLAN.md — Gap Closure For Setup Pairing Facade Extraction (completed 2026-03-20)
- [x] 46-05-PLAN.md — Gap Closure For Live GUI Pairing Bridge Activation (completed 2026-03-20)

### Phase 46.6: daemon 需要跟随 tauri 启动和关闭,现在 tauri 关闭后,daemon 完全变成了孤儿进程 (INSERTED)

**Goal:** Introduce a separate GUI runtime lifecycle owner contract for live GUI-spawned daemon children, clean them up only on real application exit, preserve close-to-tray semantics, and never terminate independently started daemons.
**Requirements**: P46.6-01, P46.6-02, P46.6-03, P46.6-04, P46.6-05
**Depends on:** Phase 46.3
**Plans:** 2/2 plans complete

Plans:

- [x] 46.6-01-PLAN.md — GUI-owned daemon lifecycle contract and spawn/replacement registration (completed 2026-03-22)
- [x] 46.6-02-PLAN.md — Real-exit cleanup gate, tray-safe no-op paths, and shutdown regression coverage (completed 2026-03-22)

### Phase 46.1: Unify realtime subscriptions on single DaemonWsBridge (INSERTED)

**Goal:** Replace the duplicated pairing/setup websocket clients with one `DaemonWsBridge`, move pairing/peers/setup realtime consumption onto shared app-layer consumers, cut the frontend to a single `daemon://realtime` contract, and delete the legacy `p2p-*` bridge path in one breaking switch.
**Requirements**: PH461-01, PH461-02, PH461-03, PH461-04, PH461-05, PH461-06
**Depends on:** Phase 46
**Plans:** 5/5 plans executed

Plans:

- [x] 46.1-01-PLAN.md — Core Realtime Port And Envelope Model (completed 2026-03-20)
- [x] 46.1-02-PLAN.md — App Realtime Consumers And Setup Event Hub (completed 2026-03-20)
- [x] 46.1-03-PLAN.md — Singleton DaemonWsBridge And Unified Runtime Startup (completed 2026-03-20)
- [x] 46.1-04-PLAN.md — Frontend Contract Cutover To daemon://realtime (completed 2026-03-20)
- [x] 46.1-05-PLAN.md — Legacy Bridge Deletion And Realtime Cleanup (completed 2026-03-20)

### Phase 46.2: 彻底打通基于 daemon 的配对流程, 完全移除原 tauri 中相关的配对流程. 期望: 在不改变用户配对流程的情况下,内部替换成基于 daemon 的配对流程实现 (INSERTED)

**Goal:** Complete the daemon-only pairing hard cutover for desktop pairing flows while preserving the existing user-visible UX and stage semantics.
**Requirements**: R46.2-1, R46.2-2, R46.2-3, R46.2-4, R46.2-5, R46.2-6, R46.2-7, R46.2-8
**Depends on:** Phase 46
**Plans:** 3/3 plans complete

Plans:

- [x] 46.2-01-PLAN.md — Daemon Pairing Host Contract And Metadata Boundary (completed 2026-03-21)
- [x] 46.2-02-PLAN.md — Requirements Traceability And Tauri Shell Cutover (completed 2026-03-21)
- [x] 46.2-03-PLAN.md — Frontend Pairing Flow Parity And Regression Coverage (completed 2026-03-21)

### Phase 46.3: 修复 GUI 启动 daemon 的生命周期托管与版本不匹配静默替换 (INSERTED)

**Goal:** Gate GUI startup on a compatible local UniClipboard daemon, boundedly replace incompatible daemons on the expected local endpoint, and surface a deterministic startup failure/recovery path instead of silently entering the main app.
**Requirements**: GUI-DMN-01, GUI-DMN-02, GUI-DMN-03, GUI-DMN-04, GUI-DMN-05
**Depends on:** Phase 46
**Plans:** 1/4 plans executed

Plans:

- [x] 46.3-01-PLAN.md — Shared release version contract and daemon compatibility probe classification (completed 2026-03-22)
- [ ] 46.3-02-PLAN.md — Narrow local daemon PID metadata and bounded incompatible-daemon replacement
- [ ] 46.3-03-PLAN.md — Startup state contract, window gating, and traced startup status/retry commands
- [ ] 46.3-04-PLAN.md — Frontend startup error page, polling recovery hook, and top-level app gating

### Phase 46.4: 当前基于 daemon 重构后的 setup 流程还没有跑通(GUI), 为了加快开发和调试,我考虑先实现 cli 版本的 setup 流程, 理论上 cli 和 gui 都走的同一个流程,只是不同的入口,所以我考虑先将 cli +daemon 的方式给打通. 端到端将如何进行测试,你需要在两个终端中,分别起一个 peerA (full mode), 另一个是 peerB (passive mode), 然后让 peerB 成功与peerA 进行配对,也就是说, peerA 先新建加密空间, B 需要发现 A,B 请求A,A确认, B 输入加密口令, A 验证加密口令, 最终B成功加入 A; 对于 A 和 B 查询已配对设备都应该能看到对方. (INSERTED)

**Goal:** Expose the daemon-backed setup flow through CLI and validate a repeatable two-terminal `peerA` / `peerB` operator workflow
**Requirements**: PH46-01, PH46-03, PH46-04, PH46-06, PH46-01A, PH45-05, CLI-01, CLI-04, CLI-05
**Depends on:** Phase 46
**Plans:** 3/3 plans complete

Plans:

- [x] 46.4-01-PLAN.md — Daemon setup transport foundation (completed 2026-03-21)
- [x] 46.4-02-PLAN.md — CLI setup command family over daemon transport (completed 2026-03-21)
- [x] 46.4-03-PLAN.md — Reset/repeatability and acceptance proof (completed 2026-03-21)

### Phase 46.5: 将配对业务逻辑从 Tauri 层彻底移除，统一收口到 daemon (INSERTED)

**Goal:** Remove GUI-owned pairing runtime and keep `uc-tauri` as a thin daemon command/realtime shell for all pairing and setup pairing flows.
**Requirements**: R46.2-1, R46.2-2, R46.2-4, R46.2-7, R46.2-8
**Depends on:** Phase 46
**Plans:** 4/4 plans complete

Plans:

- [x] 46.5-01-PLAN.md — Runtime ownership cutover in `uc-bootstrap` + `uc-platform` (completed 2026-03-22)
- [x] 46.5-02-PLAN.md — Daemon pairing/setup transport foundation (completed 2026-03-22)
- [x] 46.5-04-PLAN.md — Tauri daemon command shell cutover (completed 2026-03-22)
- [x] 46.5-03-PLAN.md — Regression and contract coverage for daemon-only pairing ownership (completed 2026-03-22)

### Phase 47: Frontend Daemon Cutover — switch desktop UI from Tauri commands to daemon HTTP and WebSocket APIs

**Goal:** [To be planned]
**Requirements**: TBD
**Depends on:** Phase 46
**Plans:** 0 plans

Plans:

- [ ] TBD (run /gsd:plan-phase 47 to break down)

### Phase 48: Daemon-Only Application Host Cleanup — remove legacy Tauri business entrypoints and consolidate runtime ownership

**Goal:** [To be planned]
**Requirements**: TBD
**Depends on:** Phase 47
**Plans:** 0 plans

Plans:

- [ ] TBD (run /gsd:plan-phase 48 to break down)

### Phase 49: Setup 验证码链路单一化重构

**Goal:** 把"加入空间验证码显示"收成一条唯一链路: setup orchestrator -> daemon websocket -> Tauri bridge -> setup realtime store -> SetupPage
**Depends on:** Phase 46.5
**Plans:** 3 plans

Plans:

- [ ] 49-01-PLAN.md — Backend integration tests: selectJoinPeer -> JoinSpaceConfirmPeer end-to-end and payload field verification
- [ ] 49-02-PLAN.md — Frontend store unit tests + SetupPage JoinSpaceConfirmPeer integration test
- [ ] 49-03-PLAN.md — PairingNotificationProvider regression + App setup gate verification

### Phase 50: Daemon encryption state recovery on startup

**Goal:** Daemon 启动时从磁盘恢复 master key 到 encryption session，解决 daemon 重启后 proof verification 失败的问题
**Requirements**: PH50-01, PH50-02, PH50-03
**Depends on:** Phase 46.6
**Plans:** 1/1 plans complete

Plans:

- [x] 50-01-PLAN.md — Wire AutoUnlockEncryptionSession into DaemonApp::run() with fail-fast and regression test

### Phase 51: Peer discovery deduplication fix

**Goal:** 修复 mDNS peer 发现去重 bug: get_discovered_peers 过滤 local_peer_id、daemon peers.changed 改为全量快照语义
**Requirements**: PH51-01, PH51-02, PH51-03
**Depends on:** Phase 50
**Plans:** 1/1 plans complete

Plans:

- [ ] 51-01-PLAN.md — Filter local_peer_id from get_discovered_peers and emit full-snapshot peers.changed

### Phase 52: Daemon as single source of truth for space access state

**Goal:** Daemon 作为 space access 唯一状态源，移除 GUI 端 SpaceAccessOrchestrator，新增 daemon WS 推送和 HTTP 查询
**Requirements**: PH52-01, PH52-02, PH52-03, PH52-04, PH52-05, PH52-06
**Depends on:** Phase 51
**Plans:** 2/2 plans complete

Plans:

- [x] 52-01-PLAN.md — Daemon-side space access state broadcasting, HTTP endpoint, and WS topic registration
- [x] 52-02-PLAN.md — GUI-side orchestrator removal, DaemonWsBridge event translation, and wiring cleanup

### Phase 53: End-to-end join space flow verification

**Goal:** 端到端集成测试验证完整 join space 流程：设备发现 → 配对请求 → 确认 → 加密口令验证 → 成功加入
**Depends on:** Phase 52
**Plans:** 0 plans

Plans:

- [ ] TBD (run /gsd:plan-phase 53 to break down)

### Phase 54: Extract daemon client and realtime infrastructure from uc-tauri

**Goal:** Extract daemon HTTP client, WebSocket bridge, realtime runtime, and connection state from `uc-tauri` into new `uc-daemon-client` crate; rename `TauriDaemon*Client` to `Daemon*Client`
**Requirements**: TBD
**Depends on:** Phase 53
**Plans:** 2/2 plans complete

Plans:

- [x] 54-01-PLAN.md -- Create uc-daemon-client crate with HTTP clients, ws_bridge, realtime, connection (Wave 1)
- [x] 54-02-PLAN.md -- Update uc-tauri call sites, delete old modules, verify clean build (Wave 2, depends on 54-01)

### Phase 55: Extract daemon lifecycle and setup pairing bridge from uc-tauri

**Goal:** [To be planned]
**Requirements**: TBD
**Depends on:** Phase 54
**Plans:** 2/3 plans complete

Plans:

- [x] TBD (run /gsd:plan-phase 55 to break down) (completed 2026-03-24)

### Phase 56: Refactor daemon host architecture — extract peer lifecycle from PairingHost, unify host lifecycle management

**Goal:** Refactor daemon host architecture so peer lifecycle handling is isolated in `PeerMonitor` and all long-lived daemon components run through one unified service lifecycle without changing the external pairing/setup API contract.
**Requirements**: PH56-01, PH56-02, PH56-03, PH56-04
**Depends on:** Phase 55
**Plans:** 3/3 plans complete

Plans:

- [ ] 56-01-PLAN.md — Rename daemon worker abstractions to service vocabulary without changing transport contracts
- [ ] 56-02-PLAN.md — Extract `PeerMonitor` and remove peer lifecycle handling from `DaemonPairingHost`
- [ ] 56-03-PLAN.md — Unify `DaemonApp` service lifecycle management while preserving typed pairing-host API access

### Phase 56.1: Eliminate hardcoded strings in pairing/setup flow (INSERTED)

**Goal:** Extract scattered hardcoded string literals (WS topic/event names, session state labels, pairing busy reasons, HTTP API paths) into shared constants in uc-core, eliminating cross-crate duplication and reducing wire-protocol mismatch risk.
**Requirements**: PH561-01, PH561-02, PH561-03, PH561-04, PH561-05
**Depends on:** Phase 56
**Plans:** 3/3 plans complete

Plans:

- [x] 56.1-01-PLAN.md — Define daemon wire-protocol string constants in uc-core daemon_api_strings module
- [x] 56.1-02-PLAN.md — Replace hardcoded strings in uc-daemon (server side) with uc-core constants
- [x] 56.1-03-PLAN.md — Replace hardcoded strings in uc-daemon-client (client side) with uc-core constants

### Phase 57: Daemon Clipboard Watcher Integration

**Goal:** Migrate clipboard watching from GUI/PlatformRuntime to daemon as the sole clipboard monitor. Daemon captures OS clipboard changes, persists entries, broadcasts WS events; GUI operates in Passive mode receiving updates via DaemonWsBridge.
**Requirements**: PH57-01, PH57-02, PH57-03, PH57-04, PH57-05, PH57-06, PH57-07
**Depends on:** Phase 56
**Plans:** 3/3 plans complete

Plans:

- [x] 57-01-PLAN.md — Real ClipboardWatcherWorker with DaemonClipboardChangeHandler, CaptureClipboardUseCase, and clipboard.new_content WS event emission
- [x] 57-02-PLAN.md — DaemonWsBridge clipboard translation, clipboard realtime consumer, and GUI Passive mode switch
- [x] 57-03-PLAN.md — Write-back loop prevention via shared ClipboardChangeOriginPort in DaemonClipboardChangeHandler

### Phase 58: Extract DTO models and pairing event types from uc-tauri to uc-app and uc-core

**Goal:** Unify duplicate clipboard DTOs (add serde to uc-app, delete uc-tauri duplicates), extract pairing aggregation DTOs to uc-app, and delete stale pairing event types. After this phase, uc-tauri has zero duplicate DTO definitions.
**Requirements**: PH58-01, PH58-02, PH58-03, PH58-04, PH58-05
**Depends on:** Phase 57
**Plans:** 2/2 plans complete

**Success Criteria** (what must be TRUE):

1. `EntryProjectionDto` in uc-app has serde derives and is the single source of truth for clipboard entry projections (no duplicate in uc-tauri)
2. `ClipboardStats` in uc-app has serde derives and is the single source of truth (no duplicate in uc-tauri)
3. `P2PPeerInfo` and `PairedPeer` live in uc-app alongside `P2pPeerSnapshot` and `LocalDeviceInfo`
4. `P2PPairingVerificationEvent`/`P2PPairingVerificationKind` are deleted (stale dead code)
5. Frontend JSON wire contract is preserved (snake_case fields, `file_transfer_ids` absent, `link_domains` present)

Plans:

- [x] 58-01-PLAN.md — Unify clipboard DTOs: add serde to EntryProjectionDto and ClipboardStats in uc-app, delete duplicates from uc-tauri models
- [x] 58-02-PLAN.md — Extract pairing DTOs (P2PPeerInfo, PairedPeer) to uc-app and delete stale P2PPairingVerificationEvent types

### Phase 59: Secure daemon resource endpoints with scoped token auth

**Goal:** Migrate blob/thumbnail resource serving from Tauri's in-process `uc://` protocol handler to daemon HTTP endpoints (`/uc/blob/:id`, `/uc/thumbnail/:id`) with production-grade security designed for future `0.0.0.0` exposure. Multi-layer security: scoped resource tokens (15s TTL, lazy refresh), rate limiting (blob 20 req/s, thumbnail 100 req/s), Origin check, encryption session binding, audit logging. GUI retains `uc://` as transparent proxy via daemon-client; frontend zero changes.
**Requirements**: TBD
**Depends on:** Phase 58
**Plans:** 0 plans

Plans:

- [ ] TBD (run /gsd:plan-phase 59 to break down)

### ~~Phase 60: Extract file transfer wiring orchestration from uc-tauri to uc-app~~ ✅ (2026-03-25)

**Goal:** Extract `file_transfer_wiring.rs` (502 lines, zero Tauri deps) from `uc-tauri/bootstrap/` into a `FileTransferOrchestrator` struct in `uc-app`, making file transfer lifecycle management available to non-Tauri runtimes (daemon, CLI).
**Requirements**: PH60-01, PH60-02, PH60-03, PH60-04, PH60-05
**Depends on:** Phase 59
**Plans:** 2/2 plans complete

**Success Criteria** (what must be TRUE):

1. `FileTransferOrchestrator` struct exists in `uc-app/src/usecases/file_sync/` holding tracker + emitter + clock + early_completion_cache
2. All 9 standalone functions from `file_transfer_wiring.rs` are methods on the orchestrator
3. `uc-bootstrap/assembly.rs` provides `build_file_transfer_orchestrator()` builder function
4. `wiring.rs` calls orchestrator methods at all integration points
5. `file_transfer_wiring.rs` is deleted from uc-tauri with no re-export stubs

Plans:

- [x] 60-01-PLAN.md — Create FileTransferOrchestrator in uc-app and wire into uc-bootstrap assembly
- [x] 60-02-PLAN.md — Rewire wiring.rs to use FileTransferOrchestrator, delete file_transfer_wiring.rs

### Phase 61: Daemon outbound clipboard sync — trigger sync to peers after local capture

**Goal:** After daemon captures a local clipboard change, trigger OutboundSyncPlanner to decide sync eligibility, then dispatch SyncOutboundClipboardUseCase and SyncOutboundFileUseCase to push content to paired peers — mirroring the AppRuntime::on_clipboard_changed flow.
**Requirements**: PH61-01, PH61-02, PH61-03, PH61-04
**Depends on:** Phase 60
**Plans:** 1/1 plans complete

**Success Criteria** (what must be TRUE):

1. DaemonClipboardChangeHandler triggers OutboundSyncPlanner + SyncOutboundClipboardUseCase after successful LocalCapture
2. RemotePush origin clipboard changes do NOT trigger outbound sync (no double-sync loop)
3. File clipboard items produce FileCandidate vec with correct extracted_paths_count
4. SyncOutboundClipboardUseCase::execute() runs via spawn_blocking (not directly in async context)

Plans:

- [x] 61-01-PLAN.md — Extend DaemonClipboardChangeHandler with outbound sync dispatch (OutboundSyncPlanner + SyncOutboundClipboardUseCase + SyncOutboundFileUseCase)

### Phase 62: Daemon inbound clipboard sync — receive peer clipboard and write to local system

**Goal:** Daemon receives inbound clipboard messages from peers via ClipboardTransportPort, applies them through SyncInboundClipboardUseCase (Full mode), writes to OS clipboard, and broadcasts clipboard.new_content WS events — mirroring the wiring.rs run_clipboard_receive_loop pattern.
**Requirements**: PH62-01, PH62-02, PH62-03, PH62-04, PH62-05
**Depends on:** Phase 61
**Plans:** 1/1 plans complete

**Success Criteria** (what must be TRUE):

1. InboundClipboardSyncWorker implements DaemonService with subscribe-loop pattern
2. SyncInboundClipboardUseCase constructed in Full mode via with_capture_dependencies
3. WS event emitted only for Applied { entry*id: Some(*) } outcomes (not for None or Skipped)
4. Shared clipboard_change_origin Arc prevents write-back loops between inbound sync and ClipboardWatcher
5. Full uc-daemon test suite passes

Plans:

- [x] 62-01-PLAN.md — Create InboundClipboardSyncWorker with subscribe-loop, SyncInboundClipboardUseCase (Full mode), conditional WS event emission, and registration in daemon main.rs

### Phase 63: Daemon file transfer orchestration — handle file sync lifecycle in daemon

**Goal:** Wire FileTransferOrchestrator into daemon: extend DaemonApiEventEmitter to forward Transfer StatusChanged WS events, extend InboundClipboardSyncWorker to seed pending transfer records, and create FileSyncOrchestratorWorker that subscribes to network events for transfer lifecycle management (progress, completed, failed), startup reconciliation, timeout sweeps, and clipboard restore.
**Requirements**: PH63-01, PH63-02, PH63-03, PH63-04, PH63-05, PH63-06, PH63-07
**Depends on:** Phase 62
**Plans:** 2/2 plans complete

**Success Criteria** (what must be TRUE):

1. DaemonApiEventEmitter emits file-transfer.status_changed WS events for Transfer StatusChanged host events (not silently dropped)
2. InboundClipboardSyncWorker seeds pending transfer DB records when Applied outcome contains file transfers
3. Early completion cache reconciliation runs after pending records are seeded
4. FileSyncOrchestratorWorker subscribes to NetworkEventPort and handles TransferProgress, FileTransferCompleted, FileTransferFailed events
5. Startup reconciliation marks orphaned in-flight transfers as failed before event loop starts
6. Timeout sweep runs on 15s interval and is cancelled on daemon shutdown
7. Full uc-daemon test suite passes with all new workers registered

Plans:

- [x] 63-01-PLAN.md — Add file-transfer WS constants, extend DaemonApiEventEmitter for Transfer StatusChanged, extend InboundClipboardSyncWorker with orchestrator for pending record seeding
- [ ] 63-02-PLAN.md — Create FileSyncOrchestratorWorker with network event loop, startup reconciliation, timeout sweep, and register in daemon main.rs

### Phase 64: Tauri sync retirement — remove sync logic from Tauri, delegate to daemon

**Goal:** Remove daemon-duplicated sync loops (clipboard receive, network realtime, file transfer reconciliation/sweep) from uc-tauri wiring.rs, gate restore_clipboard_entry outbound sync on Full mode, and clean up dead code and dependencies.
**Requirements**: PH64-01, PH64-02, PH64-03, PH64-04, PH64-05, PH64-06
**Depends on:** Phase 63
**Plans:** 2/2 plans complete

**Success Criteria** (what must be TRUE):

1. wiring.rs no longer spawns clipboard_receive, pairing_events, file_transfer_reconcile, or file_transfer_timeout_sweep tasks
2. All dead helper functions, backoff constants, and backoff utilities are removed from wiring.rs
3. restore_clipboard_entry skips direct outbound sync in Passive mode (daemon handles via ClipboardWatcherWorker)
4. sync_inbound_clipboard accessor removed from AppUseCases (zero callers)
5. blake3 dependency removed from uc-tauri Cargo.toml
6. Full uc-tauri and uc-daemon test suites pass

Plans:

- [x] 64-01-PLAN.md — Remove daemon-duplicated sync loops, helpers, constants, and blake3 dep from wiring.rs
- [x] 64-02-PLAN.md — Gate restore_clipboard_entry outbound sync on Full mode, remove dead sync_inbound_clipboard accessor

### Phase 65: Remove GUI clipboard watcher — delegate clipboard monitoring exclusively to daemon

**Goal:** Remove all GUI-side clipboard monitoring infrastructure (PlatformRuntime, WatcherControlPort, StartClipboardWatcher, platform IPC/event bus) now that daemon is the sole clipboard observer. No user-visible behavior changes.
**Requirements**: D-01, D-02, D-03, D-04, D-05, D-06, D-07, D-08, D-09, D-10, D-11, D-12, D-13, D-14, D-15
**Depends on:** Phase 64
**Plans:** 2 plans

**Success Criteria** (what must be TRUE):

1. PlatformRuntime struct no longer exists in uc-platform
2. WatcherControlPort, PlatformCommandExecutorPort, ClipboardRuntimePort deleted from uc-platform
3. StartClipboardWatcherPort deleted from uc-core
4. AppLifecycleCoordinator state machine simplified to Pending -> Network -> Announce -> Ready
5. build_setup_orchestrator() no longer accepts watcher_control parameter
6. main.rs has no PlatformRuntime startup or SimplePlatformCommandExecutor
7. Full workspace cargo test passes

Plans:

- [ ] 65-01-PLAN.md — Delete uc-platform runtime/ipc/watcher infrastructure and uc-core StartClipboardWatcherPort
- [ ] 65-02-PLAN.md — Remove watcher from AppLifecycleCoordinator, bootstrap, uc-tauri runtime, and main.rs

### Phase 66: 修复 daemon 剪切板监听导致前端 dashboard 不会自动刷新剪切板历史的问题

**Goal:** Fix the broken WS topic registration that prevents clipboard events from reaching the GUI, complete a full RealtimeEvent chain audit, fix all missing topic registrations, and add WS reconnection compensation for Dashboard refresh.
**Requirements**: PH66-01, PH66-02, PH66-03, PH66-04, PH66-05
**Depends on:** Phase 65
**Plans:** 2/2 plans complete

**Success Criteria** (what must be TRUE):

1. `is_supported_topic("clipboard")` returns true in daemon WS server
2. `is_supported_topic("file-transfer")` returns true in daemon WS server
3. All 12 RealtimeEvent variants have verified end-to-end event chains (audit per D-01)
4. Dashboard clipboard list auto-refreshes when daemon captures new clipboard content
5. Dashboard clipboard list auto-refreshes after WS reconnection from degraded state

Plans:

- [x] 66-01-PLAN.md — Fix WS topic registration: add clipboard and file-transfer to is_supported_topic with unit tests
- [x] 66-02-PLAN.md — Add WS reconnect compensation: bridge state monitor, DaemonReconnected event, frontend reconnect listener

### Phase 67: 设备在 setup 完成前不应被其他设备发现，需要在业务层进行过滤

**Goal:** Prevent uninitialized devices from advertising on the network by gating PeerDiscoveryWorker startup on encryption session state, with deferred start after setup completes
**Requirements**: PH67-01, PH67-02, PH67-03, PH67-04, PH67-05, PH67-06, PH67-07
**Depends on:** Phase 66
**Plans:** 2/2 plans complete

**Success Criteria** (what must be TRUE):

1. Daemon does NOT start PeerDiscoveryWorker when encryption is Uninitialized (first run)
2. Daemon starts PeerDiscoveryWorker normally when encryption is Initialized and unlocked
3. PeerDiscoveryWorker starts dynamically after setup completes on an uninitialized device without daemon restart
4. SetupCompletionEmitter fires oneshot channel when AppLifecycleCoordinator.ensure_ready() completes
5. Joiner setup flow continues to work via existing SetupAction::EnsureDiscovery (unchanged)
6. Peer-discovery health status reflects actual state (Stopped when deferred, Healthy when running)

Plans:

- [x] 67-01-PLAN.md — Add deferred PeerDiscoveryWorker infrastructure: recover_encryption_session returns bool, SetupCompletionEmitter, DaemonApp deferred worker support, build_non_gui_runtime_with_emitter
- [x] 67-02-PLAN.md — Wire conditional PeerDiscoveryWorker registration in daemon main.rs with oneshot channel and SetupCompletionEmitter

### Phase 68: Adopt Tauri Sidecar for daemon binary management (dev build, bundling, and path resolution)

**Goal:** Migrate daemon binary building, bundling, and path resolution from manual std::process::Command management to Tauri's externalBin sidecar mechanism. GUI launches daemon via sidecar API, build.rs stages binary with target-triple naming, and shell:allow-spawn capability grants permission.
**Requirements**: PH68-01, PH68-02, PH68-03, PH68-04, PH68-05, PH68-06
**Depends on:** Phase 67
**Plans:** 2/2 plans complete

**Success Criteria** (what must be TRUE):

1. tauri.conf.json declares daemon as externalBin sidecar at `binaries/uniclipboard-daemon`
2. build.rs copies compiled daemon binary to src-tauri/binaries/ with target-triple suffix for dev and production builds
3. spawn_daemon_process uses `app.shell().sidecar("uniclipboard-daemon")` instead of `std::process::Command`
4. GuiOwnedDaemonState holds `CommandChild` (tauri-plugin-shell) instead of `std::process::Child`
5. Shell plugin registered in main.rs and shell:allow-spawn capability configured with --gui-managed arg
6. resolve_daemon_binary_path() and daemon_binary_name() functions are deleted (replaced by sidecar path resolution)

Plans:

- [x] 68-01-PLAN.md — Add sidecar infrastructure: externalBin config, build.rs daemon copy, tauri-plugin-shell dependency, shell capability, .gitignore
- [x] 68-02-PLAN.md — Replace spawn_daemon_process with sidecar API, migrate GuiOwnedDaemonState to CommandChild, wire AppHandle through bootstrap/supervision

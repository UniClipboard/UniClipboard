# UniClipboard Desktop

## What This Is

A cross-platform clipboard synchronization app built with Tauri 2, React, and Rust. It provides encrypted LAN clipboard sync for text, images, links, and files, local encrypted clipboard search, CLI and daemon runtime modes, and observability tooling for debugging and support.

## Core Value

Seamless clipboard synchronization across devices — users can copy on one device and paste on another without interrupting their workflow.

## Current Milestone: v0.7.0 LAN-only Mode

**Goal:** 给"局域网洁癖"用户一个可观察、可控的开关——禁用 iroh 公网中继回落，让流量真正只走局域网，并把"当前是直连还是中继"暴露成可见状态。

**Target features:**

P0（MVP）
- 设置页 Network 分类下加开关 "LAN-only Mode"，默认 OFF（不打扰存量用户）
- 后端 `Settings` 新增 `network` 命名空间，字段 `network.allow_relay_fallback: bool`（默认 true，反向命名）
- 启动时把该字段读入 `IrohNodeConfig.disable_relays`（已有字段，仅暴露成用户可控）
- 切换开关后弹"重启生效"提示（不做运行时热切换，留作后续）
- 设备列表显示连接通道指示器（LAN 直连 / Relay 中继 / Offline），让"局域网专用"可观察、可验证

P1（强烈建议同期）
- 配对成功后的一次性 onboarding tip：引导感兴趣的用户发现 LAN-only 开关
- 文档：解释 "LAN-only" 边界（首次配对仍需联网经 rendezvous，要透明）

**关键决策（来自 explore 阶段）:**
- 配对仍走公网 rendezvous，**接受首次配对需联网**（自托管 rendezvous 暂不做）
- 同网段 mDNS 发现已实现，不动；跨网段连接由 relay 中继兜底，给用户开关
- 命名采用反向语义：后端字段 `allow_relay_fallback`（默认 true），前端开关呈现为 "LAN-only Mode"（toggle 关闭 = 不允许 fallback）
- 当前 `NetworkSection.tsx` 是占位组件，本里程碑会把它替换成真实内容
- **不在本里程碑范围**：自托管 rendezvous、运行时热切换、跨网段静态地址簿、独立 LAN-only 二进制 flavor

## Current State

- **Latest shipped milestone:** v0.5.0 Local Encrypted Search (archived 2026-04-13)
- **Current capability level:** Full-featured clipboard sync with text, image, link, and file support; structured observability; per-device sync control; CLI clipboard history and search commands; daemon auto-recovers encryption session on startup; local encrypted search is available through daemon routes, CLI commands, QuickPanel keyword search, and Dashboard search with content-type and time-range filters
- **Architecture status:** Hexagonal architecture with compiler-enforced boundaries, typed command surfaces, lifecycle governance, daemon-first runtime ownership, and consolidated sync planning
- **LOC:** ~324K Rust + ~39K TypeScript (estimated)
- **Supported content types:** Text, Image, Link, File
- **Archive note:** v0.5.0 was archived after Phase 93 was completed manually and planning records were backfilled during archive; audit was backfilled 2026-05-04

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
- ✓ Unified CLI/GUI/Daemon auth architecture (session exchange via /auth/connect, bare bearer rejection on L2+ routes, independent token scopes) — v0.4.0
- ✓ OpenTelemetry OTLP/HTTP-protobuf pipeline replaces legacy Seq/CLEF layer and propagates trace context across device sync boundaries — v0.4.0
- ✓ Local encrypted clipboard search backend with HKDF-derived keying, HMAC-tagged index terms, exact keyword search, time-range filtering, file-type filtering, rebuild flow, and locked-session enforcement — v0.5.0
- ✓ Local encrypted clipboard search UI across QuickPanel and Dashboard, including result counts and time-range controls — v0.5.0

### Active

- [ ] LAN-only Mode 开关（前端 Settings + 后端 network namespace）— v0.7.0
- [ ] 启动时将 `network.allow_relay_fallback` 注入 `IrohNodeConfig.disable_relays` — v0.7.0
- [ ] 设备列表"连接通道"指示器（LAN / Relay / Offline）— v0.7.0
- [ ] 切换开关后"重启生效"提示（不做运行时热切换）— v0.7.0
- [ ] 配对成功后 onboarding tip — v0.7.0
- [ ] LAN-only 边界文档（首次配对仍需联网透明化）— v0.7.0

### Deferred

- [ ] Complete chunked transfer resume protocol (CT-02, CT-04 — backend only, frontend deferred)
- [ ] Wire transfer progress events to frontend UI (CT-05)
- [ ] Add favorites persistence (domain model column needed)
- [ ] Wire lifecycle events to frontend (currently polling, not event-driven)
- [ ] Expand typed error migration to port surfaces (ARCHNEXT-01)
- [ ] Domain model refinement for anemic models (ARCHNEXT-02)
- [ ] Collector & multi-backend support for observability
- [ ] WebDAV cross-internet sync
- [ ] Runtime log profile switching (OBS-01)

### Out of Scope

- Mobile app — desktop-first
- OAuth/third-party login — not required for current product model
- Remote/cloud log shipping — clipboard logs may contain sensitive content
- In-app log viewer UI — Seq provides dedicated log UI
- 自托管 rendezvous（v0.7.0）— 配对仍走 `rendezvous.uniclipboard.app`，自建服务延后评估
- 跨网段静态地址簿 / 手动 NodeId 输入（v0.7.0）— 边缘场景，不阻塞 LAN-only MVP
- 运行时热切换 LAN-only 开关（v0.7.0）— iroh RelayMode 是 bind 时确定，本里程碑用"重启生效"提示替代
- 独立 LAN-only 二进制 flavor（v0.7.0）— 先做开关，flavor 看后续需求

## Context

Archived v0.5.0 on 2026-04-13 after local encrypted search shipped across backend, CLI, QuickPanel, and Dashboard.
Tech stack: Tauri 2 + React 18 + Rust + libp2p + XChaCha20-Poly1305.
Hexagonal boundaries are compiler-enforced. Daemon is the main runtime authority for sync, search, and space state.
Clipboard search now supports exact keyword search, time-range presets, content-type filters, file-extension filters, rebuild operations, and locked-session handling.

## Key Decisions

| Decision | Rationale | Outcome |
| --- | --- | --- |
| Two-segment framing for clipboard wire format | Reduce overhead and enable stream decode | ✓ Good |
| V3 binary protocol with Arc fanout | Improve large payload performance and memory behavior | ✓ Good |
| Manual uc:// URL resolution strategy | Ensure Windows/WebView compatibility | ✓ Good |
| Background TIFF conversion | Keep clipboard capture path responsive | ✓ Good |
| Private deps + facade accessors on AppRuntime | Compiler-enforced boundary: commands cannot access internals | ✓ Good |
| CommandError serde tag=code content=message | Frontend discriminated union handling | ✓ Good |
| TaskRegistry with CancellationToken cascade | Deterministic shutdown without orphaned tasks | ✓ Good |
| OutboundSyncPlanner consolidation | Single policy decision point, runtime as thin dispatcher | ✓ Good |
| HKDF-derived search key + HMAC-tagged exact-token index | Preserve local search without storing plaintext terms | ✓ Good |
| Search rebuild via version-flag atomic swap | Avoid SQLite table rename lock contention | ✓ Good |

## Constraints

- **Tech stack:** Tauri 2 + React + Rust
- **Sync domain:** LAN-first with libp2p
- **Security:** XChaCha20-Poly1305 remains mandatory
- **Platform support:** macOS primary; Windows/Linux supported

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---

_Last updated: 2026-05-04 — started milestone v0.7.0 LAN-only Mode_

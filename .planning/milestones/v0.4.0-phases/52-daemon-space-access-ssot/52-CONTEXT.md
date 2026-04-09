# Phase 52: Daemon as Single Source of Truth for Space Access State - Context

**Gathered:** 2026-03-23
**Status:** Ready for planning

<domain>
## Phase Boundary

重设计 daemon ↔ GUI space access 状态同步机制。Daemon 作为 space access 唯一状态源，GUI 不再持有自己的 SpaceAccessOrchestrator。

本阶段交付：

- 移除 GUI 端的 SpaceAccessOrchestrator，所有 space access 状态查询和操作通过 daemon
- 新增 `space_access.state_changed` WebSocket 事件，daemon 状态变更时推送完整快照给 GUI
- GUI 初始化时通过 HTTP API 查询一次当前状态，后续由 WebSocket 事件驱动更新
- Daemon 重启后 space access state 重置为 Idle（不做持久化）

本阶段不包括：

- Space access 状态持久化（短暂交互流程，重启重置即可）
- 修改 space access 状态机逻辑本身
- 修改加密/配对核心流程

</domain>

<decisions>
## Implementation Decisions

### GUI 状态同步机制

- **D-01:** 完全移除 GUI 端的 `SpaceAccessOrchestrator`。GUI 进程不再持有任何 space access 状态副本。所有状态由 daemon 单独管理。
- **D-02:** 前端通过 WebSocket 事件推送获取 daemon space access 状态变更。初始化时通过 daemon HTTP API 查询一次当前状态作为初始快照。
- **D-03:** 复用现有 `DaemonWsBridge` 路径将 daemon WebSocket 事件翻译为 Tauri 前端事件。

### 持久化策略

- **D-04:** 不做 space access 状态持久化。Space access 是短暂交互式流程（通常几十秒），daemon 重启后对端会因超时/连接断开感知失败，用户重新发起成本很低。
- **D-05:** Daemon 重启后 space access state 直接重置为 `SpaceAccessState::Idle`（现有行为保持不变）。

### 状态变更通知

- **D-06:** 新增 `space_access.state_changed` WebSocket 事件类型，携带完整 `SpaceAccessState` 快照。与 `peers.changed` 事件模式一致——推送完整快照而非增量 diff。
- **D-07:** 通过现有 DaemonWsBridge 路径转发，前端通过 Tauri event listener 消费。

### Claude's Discretion

- daemon HTTP API 中查询当前 space access state 的具体 endpoint 设计
- `space_access.state_changed` 事件中除 state 外是否携带额外 metadata（如 session_id）
- GUI 端移除 orchestrator 后的代码清理范围和编译适配
- DaemonWsBridge 中事件翻译的具体实现

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Space Access 状态机和 Orchestrator

- `src-tauri/crates/uc-core/src/security/space_access/state.rs` — SpaceAccessState enum（Idle, WaitingOffer, WaitingUserPassphrase, WaitingDecision, WaitingJoinerProof, Granted, Denied, Cancelled）
- `src-tauri/crates/uc-core/src/security/space_access/state_machine.rs` — SpaceAccessStateMachine transition 逻辑
- `src-tauri/crates/uc-core/src/security/space_access/event.rs` — SpaceAccessEvent enum
- `src-tauri/crates/uc-core/src/security/space_access/action.rs` — SpaceAccessAction enum
- `src-tauri/crates/uc-app/src/usecases/space_access/orchestrator.rs` — SpaceAccessOrchestrator（当前被 GUI 和 daemon 各自实例化）
- `src-tauri/crates/uc-app/src/usecases/space_access/context.rs` — SpaceAccessContext, SpaceAccessOffer
- `src-tauri/crates/uc-app/src/usecases/space_access/events.rs` — SpaceAccessCompletedEvent, SpaceAccessEventPort
- `src-tauri/crates/uc-app/src/usecases/space_access/executor.rs` — SpaceAccessExecutor side-effect 执行

### GUI Bootstrap（需移除 orchestrator）

- `src-tauri/crates/uc-bootstrap/src/builders.rs` — GuiBootstrapContext 中的 `space_access_orchestrator` 字段
- `src-tauri/src/main.rs` — GUI 端 space_access_orchestrator 的使用点（~line 327, 593）
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` — GUI 依赖注入

### Daemon Pairing Host（space access 的实际运行端）

- `src-tauri/crates/uc-daemon/src/pairing/host.rs` — DaemonPairingHost，持有 space_access_orchestrator
- `src-tauri/crates/uc-daemon/src/app.rs` — DaemonApp，持有 space_access_orchestrator

### WebSocket 事件路径（参考已有模式）

- `src-tauri/crates/uc-daemon/src/api/ws.rs` — daemon WebSocket 事件广播
- `src-tauri/crates/uc-daemon/src/api/event_emitter.rs` — DaemonApiEventEmitter
- `src-tauri/crates/uc-tauri/src/bootstrap/daemon_ws_bridge.rs` — DaemonWsBridge 事件翻译
- `src-tauri/crates/uc-app/src/realtime/setup_state_consumer.rs` — setup_state.changed 事件参考模式
- `src-tauri/crates/uc-core/src/ports/realtime.rs` — RealtimePort trait

### 前端 Space Access 消费

- `src/store/setupRealtimeStore.ts` — 前端 setup state 实时订阅（参考模式）
- `src/api/setup.ts` — handleSpaceAccessCompleted, onSpaceAccessCompleted

### Bootstrap Assembly

- `src-tauri/crates/uc-bootstrap/src/assembly.rs` — wire_dependencies 中 SpaceAccessOrchestrator 创建（~line 980）

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `SpaceAccessOrchestrator::get_state()` — 已有状态查询方法，daemon 端保留复用
- `DaemonWsBridge` — 已有 WebSocket 事件到 Tauri 事件的翻译基础设施
- `DaemonApiEventEmitter` — daemon 端事件广播机制
- `DaemonQueryService` — daemon HTTP 查询服务，可扩展添加 space access state 查询
- `peers.changed` 事件模式 — 完整快照推送的参考实现

### Established Patterns

- WebSocket 事件推送：daemon → DaemonWsBridge → Tauri event → 前端 listener
- HTTP API 查询：前端 → Tauri command → daemon HTTP → 响应
- 完整快照推送模式：`peers.changed` 已验证此模式，`space_access.state_changed` 应保持一致
- `SpaceAccessState` 已 derive `Serialize/Deserialize`，可直接用于 API 传输

### Integration Points

- `DaemonApp` 持有 `space_access_orchestrator`，是状态变更的发起点
- `DaemonPairingHost` 通过 orchestrator 执行 space access 流程
- `GuiBootstrapContext.space_access_orchestrator` — 需要移除的字段
- `src-tauri/src/main.rs` — GUI 端 orchestrator 使用点需要清理

</code_context>

<specifics>
## Specific Ideas

- 参考 Phase 51 的 `peers.changed` 完整快照模式设计 `space_access.state_changed` 事件
- GUI 端移除 orchestrator 后，现有前端代码（setupRealtimeStore.ts, setup.ts）中的 space access 相关逻辑需要适配为消费 daemon 推送的事件
- Daemon 重启后 state 已经是 Idle（SpaceAccessOrchestrator::new() 默认 Idle），无需额外处理

</specifics>

<deferred>
## Deferred Ideas

- **Space access 状态持久化** — 当前不需要，如果未来有长时间异步配对场景再考虑
- **Space access 流程超时恢复** — daemon 重启后对进行中流程的主动通知/清理，当前依赖对端超时即可

</deferred>

---

_Phase: 52-daemon-space-access-ssot_
_Context gathered: 2026-03-23_

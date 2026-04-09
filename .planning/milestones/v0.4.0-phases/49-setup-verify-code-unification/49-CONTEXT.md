# Phase 49: Setup 验证码链路单一化重构 - Context

**Gathered:** 2026-03-22
**Status:** Ready for planning
**Source:** Original phase request

<domain>
## Phase Boundary

把"加入空间验证码显示"收成一条唯一链路：

`setup orchestrator -> setup realtime event -> daemon websocket -> Tauri bridge -> setup realtime store -> SetupPage`

这次同时收口前端两个入口：

- **SetupPage** 不再直接拼本地判断、轮询和 session 猜测，只读 setup store。
- **App 顶层 setup gate** 也改为读同一个 setup store，不再单独轮询 `getSetupState()`。

普通设备配对继续走 pairing 事件，但 PairingNotificationProvider 只负责普通配对，不再感知 setup session。

</domain>

<decisions>
## Implementation Decisions

### Architecture: Single Source of Truth for Setup State

- setup orchestrator 在收到配对验证码后，直接推进到 `JoinSpaceConfirmPeer`，并发出单一 setup 状态事件
- daemon websocket 对 setup 使用 setup topic，不让前端 setup UI 再依赖 pairing verification 语义
- 保留普通 pairing realtime 给普通配对 UI 使用，但不再承担 setup 页面推进职责

### Frontend: New Independent Setup Realtime Store

- 新建轻量独立的 `useSetupRealtimeStore`，负责：
  - 首次 `getSetupState()` hydration
  - 订阅 `onSetupStateChanged()`
  - 保存当前 setup state、最近 sessionId、是否已完成初始化
- `getSetupState()` 只用于首次加载或显式重置后的重新同步
- `onSetupStateChanged()` 是 setup 异步推进的唯一来源

### Frontend: SetupPage Simplification

- SetupPage 改为只从 store 读取状态并触发动作，不再维护：
  - `activeEventSessionIdRef`
  - setup/pairing ownership 判断
  - setup 专用本地补偿逻辑

### Frontend: App Top-Level Gate

- App 顶层 setup gate 改为读同一个 store，替代当前定时轮询

### PairingNotificationProvider: Decoupling

- PairingNotificationProvider 只处理：
  - inbound pairing request toast
  - 普通设备配对 PIN dialog
  - 普通 pairing 的 verifying / complete / failed 流程
- 删除所有 setup session 识别和抑制逻辑，包括：
  - `getSetupState()` 读取
  - "ignore because setup owns session" 这类防撞分流
  - setup 期间验证码只会出现在 SetupPage

### Store Interface Contract

- 前端新增一个 setup store 对外接口，至少暴露：
  - 当前 setup state
  - 当前 setup sessionId
  - 是否完成首次 hydration
  - 启动/停止 realtime 同步的方法，或等价的 provider/hook 生命周期封装
- 现有 `onSetupStateChanged()` 契约保持不变，继续作为 setup realtime 唯一异步输入
- 不新增新的前端 pairing/setup 混合事件接口；方向是删依赖，不是再加桥接层

### Assumptions (locked)

- 本次范围包含 App 顶层 setup gate 收口；不保留顶层轮询双源
- setup store 采用轻量独立实现，不并入现有 Redux
- daemon 与 Tauri 现有 setup 事件链路已具备能力，这次以收紧 ownership 和删双通路为主，不额外设计新协议
- 旧的 setup/pairing 双通路代码在新链路和测试稳定后一起删掉，不保留兼容分流逻辑

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Relevant Phase Directories

- `.planning/phases/46-daemon-pairing-host-migration-move-pairing-orchestrator-action-loops-and-network-event-handling-out-of-tauri/` — Background: daemon pairing host migration
- `.planning/phases/46.1-unify-realtime-subscriptions-on-single-daemonwsbridge/` — Realtime transport contract
- `.planning/phases/46.5-tauri-daemon/` — Daemon pairing/setup transport foundation

### Frontend State & Stores

- `src/store/setupRealtimeStore.ts` — New setup realtime store (already created in working tree)
- `src/pages/SetupPage.tsx` — Setup page to be simplified
- `src/components/__tests__/PairingNotificationProvider.realtime.test.tsx` — Tests to be rewritten
- `src/pages/__tests__/SetupFlow.test.tsx` — Setup page tests
- `src/pages/__tests__/setup-ready-flow.test.tsx` — Setup ready flow tests
- `src/App.tsx` — App top-level setup gate

### Backend (Rust)

- `src-tauri/crates/uc-app/src/usecases/pairing/protocol_handler.rs` — Protocol handler
- `src-tauri/crates/uc-app/src/realtime/setup_consumer.rs` — Setup realtime consumer
- `src-tauri/crates/uc-app/src/realtime/pairing_consumer.rs` — Pairing realtime consumer
- `src-tauri/crates/uc-app/src/realtime/peers_consumer.rs` — Peers realtime consumer
- `src-tauri/crates/uc-daemon/src/pairing/host.rs` — Daemon pairing host

### Tauri Bridge

- `src-tauri/crates/uc-tauri/tests/daemon_ws_bridge.rs` — Daemon WS bridge tests

### Requirements

- `.planning/REQUIREMENTS.md` — Project requirements

</canonical_refs>

<specifics>
## Specific Ideas

### Test Plan from Original Request

- 后端集成测试补齐完整用户流：
  - select-peer -> pairing verification arrives -> setup.stateChanged(JoinSpaceConfirmPeer) 成立
  - 验证 setup 事件里带上页面渲染所需字段
- Tauri bridge/contract 测试确认：
  - daemon setup 事件可以一路透传到前端 setup contract
  - 普通 pairing verification 仍只给 pairing 消费者
- 前端 store 测试：
  - 首次 hydration 后进入 realtime 驱动
  - 收到 JoinSpaceConfirmPeer 时 store 正确推进
  - Completed / Welcome 时状态与 session 正确复位
- SetupPage 测试：
  - 页面只靠 setup store 就能显示验证码、确认、取消、完成
  - 不再依赖 pairing verification mock
- PairingNotificationProvider 测试重写：
  - 保留普通 pairing request / verification / verifying / success / failed
  - 删除所有"setup session suppress/ignore"类防御性测试
- App 顶层测试：
  - setup gate 跟随 setup store，而不是轮询接口结果

### Files to Modify

- `src/store/setupRealtimeStore.ts` — Complete the new store implementation
- `src/pages/SetupPage.tsx` — Remove local state/refs, use store
- `src/App.tsx` — Replace polling with store reading
- `src/components/__tests__/PairingNotificationProvider.realtime.test.tsx` — Rewrite tests
- `src/pages/__tests__/SetupFlow.test.tsx` — Update tests
- `src/pages/__tests__/setup-ready-flow.test.tsx` — Update tests
- `src-tauri/crates/uc-tauri/tests/daemon_ws_bridge.rs` — Add setup event contract test

</specifics>

<deferred>
## Deferred Ideas

None — phase scope is well-bounded by the original request.

</deferred>

---

_Phase: 49-setup-verify-code-unification_
_Context gathered: 2026-03-22 via original phase request_

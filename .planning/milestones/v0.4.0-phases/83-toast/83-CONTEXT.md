# Phase 83: 分析前端 Peer 配对请求，简化事件流架构，分离关注点,创建配对状态管理,提取业务逻辑 - Context

**Gathered:** 2026-04-02
**Status:** Ready for planning

<domain>
## Phase Boundary

分析并重构前端配对相关的事件订阅、状态管理和业务逻辑。具体包括：

- 统一前端配对事件订阅到 React hook 模式
- 整合配对状态到 Redux devicesSlice
- 提取 SetupPage 流程逻辑为独立 hook
- 为 WS event payload 添加类型安全
- 移除 `src/api/p2p.ts` facade，将逻辑合并到 daemon 模块

本阶段不包括：

- 后端配对逻辑修改（backend pairing orchestrator、PairingStreamService）
- WebSocket 连接管理（已有 daemonWs）
- 新增 daemon HTTP 端点

</domain>

<decisions>
## Implementation Decisions

### 1. 事件订阅统一

- **D-01:** `src/api/p2p.ts` 中的 `onP2PPairingVerification` 等旧路径（Tauri event bridge）全部替换为 `usePairingEvents` hook（Phase 79 建立的 React hook 模式）
- **D-02:** 前端配对事件订阅的唯一入口是 `usePairingEvents` hook（`src/hooks/useDaemonEvents.ts`）
- **D-03:** `setupRealtimeStore.ts` 的 `onSpaceAccessCompleted` 有幂等去重逻辑（`activeSessionId`、`seenEventKeys`），保留在 store 中，不迁移到 hook

### 2. 状态管理统一

- **D-04:** `discoveredPeers` 状态从独立的 `useDeviceDiscovery` hook 迁移到 Redux `devicesSlice`
- **D-05:** `devicesSlice` 作为配对相关状态的唯一来源（pairedDevices、discoveredPeers、sync settings）
- **D-06:** `useDeviceDiscovery` hook 保留用于封装设备发现的副作用（启动扫描、清理），但状态写入 Redux

### 3. 业务逻辑提取

- **D-07:** `SetupPage.tsx` 中的 `getStateOrdinal`、`getStepInfo`、`runAction` 提取为 `useSetupFlow` hook
- **D-08:** `useSetupFlow` hook 封装：状态到 step index 的映射逻辑、Tauri 命令调用封装、loading state 管理
- **D-09:** SetupPage 只负责渲染和 step 分发，不包含状态映射和命令调用逻辑

### 4. 类型安全

- **D-10:** 为每个 daemon WS event type 创建 typed payload interfaces（`src/hooks/useDaemonEvents.ts`）
- **D-11:** 用类型守卫函数替代 `as any` 断言——每个 event type 有对应的 payload interface 和类型守卫
- **D-12:** 参考已有类型：`src/api/daemon/pairing.ts` 中的 `P2PPairingVerificationEvent`、`P2PPeerConnectionEvent` 等可复用

### 5. p2p.ts facade 移除

- **D-13:** `src/api/p2p.ts` 删除，不再作为配对 API facade
- **D-14:** 设备同步设置函数（`getDeviceSyncSettings`、`updateDeviceSyncSettings`）已在 `src/api/daemon/device.ts` 中，保留
- **D-15:** `onP2PPeerDiscoveryChanged` 的 diff 逻辑（维护 `knownPeers` Map 做 discovered/lost 判断）提取到 `src/api/daemon/events.ts` 作为工具函数复用
- **D-16:** 所有调用 `p2p.ts` 的地方迁移到 `daemon/` 模块

### Claude's Discretion

- `useDeviceDiscovery` hook 的具体重构方式（是否需要拆分、或保持 hook 形态但状态写入 Redux）
- `useSetupFlow` hook 的具体 API 设计（返回什么、接受什么参数）
- `devicesSlice` 中 discoveredPeers 的 state shape（是否复用现有 peer shape 或新建类型）
- 类型守卫函数的实现方式（使用 TypeScript user-defined type guards 或简单的类型断言函数）
- `onP2PPeerDiscoveryChanged` diff 逻辑是否需要测试

### Folded Todos

无折叠的 todo。

</decisions>

<canonical_refs>

## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### 事件订阅系统

- `src/hooks/useDaemonEvents.ts` — `usePairingEvents` hook（统一入口），`useClipboardNewContent` hook
- `src/lib/daemon-ws.ts` — `daemonWs.subscribe` 基础 WebSocket 订阅
- `src/api/realtime.ts` — 旧路径（Tauri event bridge，Phase 79 前使用）

### 状态管理

- `src/store/slices/devicesSlice.ts` — Redux devicesSlice，现有 pairedDevices 和 sync settings 管理
- `src/hooks/useDeviceDiscovery.ts` — 设备发现 hook，`discoveredPeers` 状态当前在这里

### Setup 流程

- `src/pages/SetupPage.tsx` — SetupPage，包含 `getStateOrdinal`、`getStepInfo`、`runAction` 逻辑（待提取）
- `src/store/setupRealtimeStore.ts` — `useSyncExternalStore` 模式，`onSpaceAccessCompleted` 有幂等去重逻辑
- `src/api/setup.ts` — setup 命令（Tauri invoke）
- `src/api/daemon/setup.ts` — daemon setup API（Phase 79 后）

### Pairing API

- `src/api/p2p.ts` — 待删除的 facade，调用关系复杂
- `src/api/daemon/pairing.ts` — daemon pairing API，类型定义参考
- `src/api/daemon/device.ts` — 设备同步设置 API
- `src/api/daemon/index.ts` — daemon 模块导出汇总

### Pairing Notification

- `src/components/PairingNotificationProvider.tsx` — 配对通知 provider（`App.tsx` 中使用）
- `src/__tests__/App.pairing-notifications.test.tsx` — 配对通知测试

### Backend Pairing（参考，不要修改）

- `src-tauri/crates/uc-daemon/src/api/pairing.rs` — daemon pairing HTTP handlers
- `src-tauri/crates/uc-daemon/src/pairing/host.rs` — daemon PairingHost
- `src-tauri/crates/uc-app/src/usecases/pairing/orchestrator.rs` — pairing orchestrator

</canonical_refs>

<code_context>

## Existing Code Insights

### Reusable Assets

- `usePairingEvents` hook: 已有完整的事件订阅封装，只需要在使用方替换 `onP2PPairingVerification` 回调即可
- `devicesSlice`: 已有 pairedDevices、sync settings 管理，添加 discoveredPeers 很自然
- `P2PPairingVerificationEvent` 等类型: 已在 `daemon/pairing.ts` 中定义，可直接复用
- `onSpaceAccessCompleted` 幂等逻辑: `activeSessionId` + `seenEventKeys` dedupe pattern

### Established Patterns

- React hook 作为事件订阅标准模式（Phase 79 建立）
- `useSyncExternalStore` 用于非 React 状态订阅（setupRealtimeStore）
- Redux slice 管理应用状态（devicesSlice、clipboardSlice）
- facade pattern 已被迁移完成（`daemon/` 模块是唯一真实实现）

### Integration Points

- SetupPage → `useSetupFlow` → setupRealtimeStore / daemon API
- JoinPickDeviceStep → `useDeviceDiscovery` → devicesSlice
- PairingNotificationProvider → `usePairingEvents` → daemon WS
- DevicesPage → devicesSlice → daemon API

</code_context>

<specifics>

## Specific Ideas

- `onP2PPeerDiscoveryChanged` 的 `knownPeers` Map diff 逻辑值得提取为工具函数，因为它实现了"首次发现"和"消失"的语义
- `useSetupFlow` hook 应该接受 `setupState` 作为参数（从 setupRealtimeStore 获取），而不是自己订阅 store
- 迁移 discoveredPeers 到 Redux 时，考虑是否需要持久化（用户刷新页面后设备列表应该还在）
- `p2p.ts` 中的 `classifyPairingError` 函数如果有其他地方使用，应该提取到 `daemon/errors.ts`

</specifics>

<deferred>

## Deferred Ideas

- **配对通知的深度定制**: PairingNotificationProvider 当前的 UX 行为是否需要调整（toast vs dialog），属于 UI 讨论范畴，Phase 83 只做架构重构
- **Setup flow 超时处理**: 当前没有超时兜底，设备选择后如果网络断开可能卡在 ProcessingJoinStep
- **多个配对 session 同时存在**: `usePairingEvents` 文档说支持多 session，但实际 UI 是否需要处理这个场景待确认
- **p2p.ts 中的类型导出**: `classifyPairingError` 等工具函数如果被其他地方引用，提取到共享位置

</deferred>

---

_Phase: 83-toast_
_Context gathered: 2026-04-02_

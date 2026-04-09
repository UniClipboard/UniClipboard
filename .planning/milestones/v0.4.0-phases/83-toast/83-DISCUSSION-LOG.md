# Phase 83: 分析前端 Peer 配对请求 - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-02
**Phase:** 83-toast
**Areas discussed:** 事件订阅统一, 配对状态管理, 业务逻辑提取, 类型安全, p2p.ts facade 移除

---

## Gray Area 1: 事件订阅统一

| Option                       | Description                                                                                                                  | Selected |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | -------- |
| 统一到 usePairingEvents hook | usePairingEvents 是更符合 React 习惯的封装。p2p.ts 中的 onP2PPairingVerification 等改为调用 hook 或直接用 daemonWs.subscribe | ✓        |
| 保留 p2p.ts 回调，只修类型   | p2p.ts 已有 facade 模式被其他地方依赖。只修 any cast 问题                                                                    |          |
| 全部重构为 React Context     | 用 React Context 提供配对事件                                                                                                |          |
| 你决定                       |                                                                                                                              |          |

**User's choice:** 统一到 usePairingEvents hook
**Notes:** 用户选择推荐选项。确认了 `onSpaceAccessCompleted` 有幂等去重逻辑，保留在 store 中。

---

## Gray Area 2: 配对状态管理位置

| Option                    | Description                                                                   | Selected |
| ------------------------- | ----------------------------------------------------------------------------- | -------- |
| 统一到 Redux devicesSlice | discoveredPeers 已有 pairedDevices 在 devicesSlice 中管理。统一后减少状态分散 | ✓        |
| 保持现状（状态分散）      | useDeviceDiscovery 和 setupRealtimeStore 各自管理自己的状态                   |          |
| 提取为独立 pairingSlice   | 创建专门的 pairingSlice 管理配对 session 状态                                 |          |
| 你决定                    |                                                                               |          |

**User's choice:** 统一到 Redux devicesSlice
**Notes:** 用户选择推荐选项。

---

## Gray Area 3: 业务逻辑提取

| Option                 | Description                                                                                         | Selected |
| ---------------------- | --------------------------------------------------------------------------------------------------- | -------- |
| 创建 useSetupFlow hook | 将 getStateOrdinal/getStepInfo/runAction 提取为 useSetupFlow hook。SetupPage 只负责渲染和 step 分发 | ✓        |
| 保持现状               | SetupPage 的逻辑虽然多，但都在一个文件内                                                            |          |
| 创建 setupFlowService  | 提取为纯 TypeScript service（非 hook）                                                              |          |

**User's choice:** 创建 useSetupFlow hook
**Notes:** 用户选择推荐选项。

---

## Gray Area 4: 类型安全

| Option                          | Description                                                                                          | Selected |
| ------------------------------- | ---------------------------------------------------------------------------------------------------- | -------- |
| 为每个 event 创建 typed payload | 在 useDaemonEvents.ts 中为每个 WS event type 创建对应的 payload interface。用类型守卫函数替代 as any | ✓        |
| 保持 as any                     | WS payload 是动态 JSON，不值得为每个 event 写 interface                                              |          |
| 用 JSON Schema 验证             | 用 zod 或 ajv 对 payload 进行运行时验证                                                              |          |

**User's choice:** 为每个 event 创建 typed payload
**Notes:** 用户选择推荐选项。参考已有类型：`src/api/daemon/pairing.ts` 中的 P2PPairingVerificationEvent 等。

---

## Gray Area 5: p2p.ts facade 移除

| Option                   | Description                                                                                                                                        | Selected |
| ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| 移除，合并到 daemon 模块 | onP2PPairingVerification 等旧路径移除，onSpaceAccessCompleted 和 onP2PPeerDiscoveryChanged（有特殊逻辑）提取到 daemon/setup.ts 或 daemon/events.ts | ✓        |
| 降级为 thin re-export    | 保留 p2p.ts 作为 re-export layer                                                                                                                   |          |
| 保留，清理死代码         | 只移除已迁移到 daemon 的函数，保留有特殊逻辑的                                                                                                     |          |

**User's choice:** 移除，合并到 daemon 模块
**Notes:** 用户选择推荐选项。确认 onSpaceAccessCompleted 的幂等去重逻辑（activeSessionId + seenEventKeys）和 onP2PPeerDiscoveryChanged 的 diff 逻辑需要保留。

---

## Claude's Discretion

以下决策由 Claude 根据代码复杂度和改动范围决定：

- `useDeviceDiscovery` hook 的具体重构方式
- `useSetupFlow` hook 的具体 API 设计
- `devicesSlice` 中 discoveredPeers 的 state shape
- 类型守卫函数的实现方式

## Deferred Ideas

- 配对通知的深度定制（UI 范畴，Phase 83 只做架构）
- Setup flow 超时处理
- 多个配对 session 同时存在的 UI 处理
- `classifyPairingError` 函数如果被其他地方引用，提取到共享位置

# Phase 56: Refactor daemon host architecture - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-23
**Phase:** 56-refactor-daemon-host-architecture
**Areas discussed:** 职责拆分策略, Host 统一生命周期, 命名规范

---

## 职责拆分策略

### Q1: peer 生命周期事件拆出来后，放在哪里？

| Option                         | Description                                                                                          | Selected |
| ------------------------------ | ---------------------------------------------------------------------------------------------------- | -------- |
| 新建 PeerMonitor struct        | 单独的 peer_monitor.rs 或新建 peers/ module，拥有自己的 run() 方法，跟 PairingHost 平级              | ✓        |
| 拆成独立函数但留在 PairingHost | 拆出 run_peer_event_loop 和 run_pairing_protocol_loop 两个函数，但仍由 PairingHost::run() 统一 spawn |          |
| Claude 决定                    | 根据代码现状和扩展性需求自行判断最佳拆分粒度                                                         |          |

**User's choice:** 新建 PeerMonitor struct
**Notes:** None

### Q2: PeerMonitor 和 PairingHost 如何共享 NetworkEvent 流？

| Option         | Description                                                                   | Selected |
| -------------- | ----------------------------------------------------------------------------- | -------- |
| 各自订阅       | NetworkEvent 用 broadcast channel，每个消费者独立 subscribe，各自过滤         | ✓        |
| 单个路由器分发 | 一个中心化的 event router 订阅流，然后按类型分发给 PeerMonitor 和 PairingHost |          |
| Claude 决定    | 根据当前 NetworkEvent 的 subscribe 实现方式自行判断                           |          |

**User's choice:** 各自订阅
**Notes:** None

---

## Host 统一生命周期

### Q3: PairingHost 和 PeerMonitor 如何纳入 DaemonApp 的统一管理？

| Option                  | Description                                    | Selected |
| ----------------------- | ---------------------------------------------- | -------- |
| 新建 DaemonHost trait   | 与 DaemonWorker 平级的新 trait                 |          |
| 复用 DaemonWorker trait | PairingHost 和 PeerMonitor 都实现 DaemonWorker |          |
| Claude 决定             | 根据实际接口需求判断是否需要新 trait           |          |

**User's choice:** Other — "那 Worker 还有用吗,如果没用,就统一成 DaemonHost 呗"
**Notes:** 用户质疑两套机制的必要性。经查，现有两个 worker 都是 placeholder/轻量级的，真正重量级的组件反而不走 worker 机制。决定统一。

### Q4: 统一后用哪个名字？

| Option            | Description                                               | Selected |
| ----------------- | --------------------------------------------------------- | -------- |
| DaemonService     | 涵盖 worker 和 host，语义中性                             | ✓        |
| 保留 DaemonWorker | 不改名，让 PairingHost 和 PeerMonitor 也实现 DaemonWorker |          |
| DaemonHost        | 改名为 DaemonHost，强调"它们都是 daemon 的主人"           |          |

**User's choice:** DaemonService
**Notes:** None

---

## 命名规范

### Q5: 拆分后的事件循环函数怎么命名？

| Option      | Description                                                                         | Selected |
| ----------- | ----------------------------------------------------------------------------------- | -------- |
| 按职责命名  | PeerMonitor 里叫 run_peer_event_loop，PairingHost 里保留 run_pairing_action_loop 等 | ✓        |
| Claude 决定 | 根据实际拆分结果决定具体命名                                                        |          |

**User's choice:** 按职责命名
**Notes:** None

---

## Claude's Discretion

- Exact module/file organization for PeerMonitor
- Whether PeerDiscoveryWorker merges into PeerMonitor
- DaemonService trait internal implementation details
- Order of service startup/shutdown

## Deferred Ideas

None

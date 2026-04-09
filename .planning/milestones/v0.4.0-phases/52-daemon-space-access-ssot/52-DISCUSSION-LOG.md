# Phase 52: Daemon as Single Source of Truth for Space Access State - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-23
**Phase:** 52-daemon-space-access-ssot
**Areas discussed:** GUI 状态同步机制, 持久化策略, Daemon 重启恢复, 状态变更通知

---

## GUI 状态同步机制

| Option                | Description                                                                                                           | Selected |
| --------------------- | --------------------------------------------------------------------------------------------------------------------- | -------- |
| 移除 GUI orchestrator | GUI 完全不再持有 SpaceAccessOrchestrator。所有状态查询通过 daemon HTTP API / WebSocket 获取。简化架构，消除双状态源。 | ✓        |
| 变成 daemon 状态镜像  | GUI 保留 orchestrator 但仅作为本地缓存，由 daemon WebSocket 事件驱动更新。                                            |          |
| 代理模式              | GUI 的 orchestrator 变成透明代理，所有操作转发给 daemon。                                                             |          |

**User's choice:** 移除 GUI orchestrator
**Notes:** 无额外说明

### GUI 查询方式

| Option             | Description                                                                                                       | Selected |
| ------------------ | ----------------------------------------------------------------------------------------------------------------- | -------- |
| WebSocket 事件推送 | Daemon 状态变更时通过现有 WebSocket 事件通道推送。前端维护本地 snapshot。初始化时通过 HTTP API 查询一次当前状态。 | ✓        |
| 纯 HTTP 轮询       | 前端定期轮询 daemon HTTP API 获取状态。                                                                           |          |
| Claude 决定        | 由 Claude 根据现有架构选择。                                                                                      |          |

**User's choice:** WebSocket 事件推送
**Notes:** 无额外说明

---

## 持久化策略

| Option      | Description                                                      | Selected |
| ----------- | ---------------------------------------------------------------- | -------- |
| JSON 文件   | 类似 keyslot.json 模式，将 SpaceAccessState 序列化到 JSON 文件。 |          |
| SQLite      | 利用现有 Diesel SQLite 数据库。                                  |          |
| Claude 决定 | 由 Claude 根据现有基础设施选择。                                 |          |

**User's choice:** 用户质疑持久化必要性
**Notes:** 用户问"为什么需要持久化"。经分析 space access 是短暂交互流程（几十秒），daemon 重启后对端超时，重新发起成本低。

### 持久化必要性

| Option                    | Description                                                                                    | Selected |
| ------------------------- | ---------------------------------------------------------------------------------------------- | -------- |
| 不持久化，重启后重置 Idle | Daemon 重启后 state 回到 Idle。对端超时，用户重新发起。Scope 缩小为 "daemon SSOT + GUI 同步"。 | ✓        |
| 持久化进行中状态          | 保留原始设计，持久化进行中的 session 状态。                                                    |          |
| Claude 决定               | 由 Claude 根据分析结果选择。                                                                   |          |

**User's choice:** 不持久化，重启后重置 Idle
**Notes:** 无额外说明

---

## Daemon 重启恢复

此区域因持久化决策（D-04/D-05）而简化——不需要恢复逻辑，重启后直接 Idle。

---

## 状态变更通知

| Option              | Description                                                                                   | Selected |
| ------------------- | --------------------------------------------------------------------------------------------- | -------- |
| 新增专属事件类型    | 新增 space_access.state_changed WebSocket 事件，携带完整状态快照。与 peers.changed 模式一致。 | ✓        |
| 复用现有 setup 事件 | 将 space access 状态嵌入现有 setup_state.changed 事件中。                                     |          |
| Claude 决定         | 由 Claude 根据现有事件架构选择。                                                              |          |

**User's choice:** 新增专属事件类型
**Notes:** 无额外说明

---

## Claude's Discretion

- daemon HTTP API 中查询当前 space access state 的具体 endpoint 设计
- 事件 payload 中除 state 外的额外 metadata
- GUI 端移除 orchestrator 后的编译适配范围
- DaemonWsBridge 事件翻译实现

## Deferred Ideas

- Space access 状态持久化（如未来有长时间异步配对场景再考虑）
- Daemon 重启后对进行中流程的主动通知/清理

# Phase 66: daemon-dashboard - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-27
**Phase:** 66-daemon-dashboard
**Areas discussed:** 事件桥接完整性, 缺失主题全面排查, 断线重连补偿

---

## 事件桥接完整性

| Option          | Description                                                                                               | Selected |
| --------------- | --------------------------------------------------------------------------------------------------------- | -------- |
| 全链路审计      | 让 researcher 完整审计从 Daemon WS → GUI Bridge → Tauri emit → Frontend hook 的每个节点，确保没有其他断点 | ✓        |
| 只修 topic 注册 | 先只修 is_supported_topic，如果还有问题再进一步排查                                                       |          |
| Claude 决定     | 由 Claude 判断需要审计的范围                                                                              |          |

**User's choice:** 全链路审计
**Notes:** 用户进一步确认审计范围为所有 RealtimeEvent 变体，不仅限于 clipboard

### 审计范围追问

| Option                    | Description                                                      | Selected |
| ------------------------- | ---------------------------------------------------------------- | -------- |
| 仅 clipboard 事件         | 只审计 clipboard.new_content 和 clipboard.deleted 的完整链路     |          |
| 所有 RealtimeEvent        | 审计所有 RealtimeEvent 变体的 WS→Tauri emit 链路，一次性彻底排查 | ✓        |
| clipboard + file-transfer | 只审计当前已知可能有问题的两个主题                               |          |

**User's choice:** 所有 RealtimeEvent

---

## 缺失主题全面排查

| Option      | Description                                                | Selected |
| ----------- | ---------------------------------------------------------- | -------- |
| 一并修复    | 在本 phase 中修复所有发现的缺失主题，而不只是 clipboard    | ✓        |
| 只记录不修  | 本 phase 只修 clipboard，其他缺失主题记录为单独的 bug/todo |          |
| Claude 决定 | 根据实际排查结果决定是否值得一并修                         |          |

**User's choice:** 一并修复
**Notes:** None

---

## 断线重连补偿

| Option         | Description                                                          | Selected |
| -------------- | -------------------------------------------------------------------- | -------- |
| 重连后刷新列表 | WS 重连成功后触发一次 Dashboard 全量刷新，确保不会漏掉断线期间的内容 | ✓        |
| 不处理         | 本 phase 只修正常链路，断线补偿作为 future work                      |          |
| Claude 决定    | 由 Claude 判断是否值得在本 phase 处理                                |          |

**User's choice:** 重连后刷新列表
**Notes:** None

---

## Claude's Discretion

- Implementation details of reconnection detection and refresh trigger
- Integration test structure for WS topic subscription
- Commit organization for audit fixes

## Deferred Ideas

None — discussion stayed within phase scope

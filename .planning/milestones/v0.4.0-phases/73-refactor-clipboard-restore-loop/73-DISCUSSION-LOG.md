# Phase 73: Refactor clipboard restore loop prevention - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-29
**Phase:** 73-refactor-clipboard-restore-loop-prevention
**Areas discussed:** Coordinator API, Use Case Relationship, Guard Key Single Source of Truth

---

## Area 1: Coordinator API Design

| Option                     | Description                                                                                                                           | Selected |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| 方案 A: 单一 write() API   | ClipboardWriteCoordinator.write(snapshot, intent) 是唯一入口。内部管理 guard 注册、TTL、错误清理。最彻底的解决。                      | ✓        |
| 方案 B: use case 内部委托  | RestoreClipboardSelectionUseCase 等保持结构不变，但内部调用 coordinator.write()。Use case 层不变，guard 逻辑集中到 coordinator 内部。 |          |
| 方案 C: 只统一 daemon 路径 | Restore 和 inbound sync 通过 coordinator。CopyFileToClipboard 保留现状。更小范围的重构。                                              |          |

**User's choice:** 方案 A: 单一 write() API

**Notes:** 用户选择了最彻底的方案，让 Coordinator.write() 成为所有 clipboard 写的唯一入口。

---

## Area 2: Use Case Relationship

| Option               | Description                                                                                                                                        | Selected |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| 留在 use case 中     | RestoreClipboardSelectionUseCase.build_snapshot() 保留完整业务逻辑。只把 write_pipeline (guard + write + cleanup) 移到 coordinator。职责分离清晰。 | ✓        |
| 全部移到 coordinator | Coordinator 同时拥有 snapshot 构建和写入逻辑。RestoreClipboardSelectionUseCase 变成纯数据访问。更大的重构范围。                                    |          |

**User's choice:** 留在 use case 中

**Notes:** 用户选择保留 snapshot 构建逻辑在 use case 中，coordinator 只负责写入管道（guard + write + cleanup）。职责分离更清晰。

---

## Area 3: Guard Key Single Source of Truth

| Option                           | Description                                                                                                                | Selected |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------- |
| 方案 A: Coordinator 是唯一调用点 | Coordinator.write() 内部计算 key 并注册 guard。Key 派生逻辑完全封装在 coordinator 中，外部无感知。                         | ✓        |
| 方案 B: 提取到共享 helper        | 保留 origin_guard_key() 在 SystemClipboardSnapshot 上，但 uc-core 添加一个 derive_guard_key() helper。所有地方通过它调用。 |          |

**User's choice:** 方案 A: Coordinator 是唯一调用点

**Notes:** 用户选择最彻底的方案——coordinator 是唯一调用 origin_guard_key() 的地方，外部 caller 完全感知不到 key 派生的存在。

---

## Scope Discussion

**Initial question:** 用户提供了 Phase 73 的完整 plan 草稿，包含 4 个阶段（Extract boundary, Migrate all writes, Lock down composition, Remove accidental API surface）。讨论确认了所有关键决策。

---

## Claude's Discretion

以下决策留给下游 planner/agent 决定：

- ClipboardWriteCoordinator 的具体 struct 字段布局
- RestoreClipboardSelectionUseCase::restore_snapshot() 是删除还是保留为委托 shim
- CopyFileToClipboardUseCase::write_files_to_clipboard() 是删除还是保留为委托 shim
- InboundClipboardSyncWorker 如何从手动 guard 调用过渡到 coordinator 调用
- 内部错误类型和日志细节
- 如何在 uc-bootstrap/assembly.rs 中集成 coordinator 构建

## Deferred Ideas

None — discussion stayed within phase scope.

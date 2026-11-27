---
name: dev-loop
description: 编排 unic 仓库的端到端开发闭环（Coordinator 调度 Lody Agent Roles：Planner → Implementer → Verifier）。在规划、跨模块开发、修复复杂 Bug 或需要严格证据验收时启用，负责 S/M/L 任务分级、标准交接单流转、4 道硬门禁与返工熔断。
---

# dev-loop（unic 开发闭环编排手册）

本手册面向 **Coordinator（总控会话）**，用于通过 **Lody Agent Roles** 将三个独立角色串成防漂移、可复核、不死循环的端到端交付闭环（各角色在 Lody 中可独立配置模型与思考等级）：

- **Planner**（Lody Role，只读）：架构规划、公共契约守门、失败清单与分层验收矩阵 → 输出《规划单》
- **Implementer**（Lody Role，读写，推荐模型如 Gemini 4 Hydrogen）：按《规划单》落地实现、守各端红线、同步活文档与基础自检 → 输出《交付单》
- **Verifier**（Lody Role，独立验证）：diff 红线审计、实跑各端 E2E、录制截图/录屏/receipt → 输出《验收回执》
- **文档收口**（仓库 Skill）：[maintain-docs](../maintain-docs/SKILL.md)

## 一、如何派活给 Lody Roles

1. 先按 Lody MCP 规则读对应帮助后，调用 `lody_catalog(action="agent_roles")` 查找工作区中的 `Planner`、`Implementer`、`Verifier` 角色 `id`。
2. 调用 `lody_session_create` 委派任务：
   - 传 `agentRoleId` 指定目标角色；
   - 传 `useCurrentSessionAsParent: true`（在同机以共享目录 Tab 运行，直接共享当前 worktree 与未提交改动；**不要**传 `workContext`）；
   - 每次 `prompt` 必须**完全自包含**（子 Tab 不继承父会话历史），原样附带上一环节交接单与必要背景；
   - 发起异步创建后直接结束本轮等待回调，**不要轮询 `lody_operation`**。
3. 同一角色需要继续返工时，优先用 `lody_session_chat` 发往该角色已有 Tab（保留其调查上下文），或按需新开 Tab。

## 二、先按任务规模选通道

| 级别 | 适用场景 | 执行路径 |
|---|---|---|
| **S（微改动）** | 文案、单文件显而易见小修、局部文档修正 | Coordinator 写 3 行目标与验收点 → `Implementer` → `Verifier` 跑最小定向验证 |
| **M（常规任务）** | 常规功能开发、跨模块 Bugfix、Desktop / Mobile 交互改动 | 完整三段：`Planner` → **Gate 1** → `Implementer` → **Gate 2** → `Verifier` → **Gate 3/4** |
| **L（复杂 / 改契约 / 跨端）** | 新 Decision、修改公共契约、多端联动特性 | `Planner` + **强制 Gate 1 用户拍板** → 按独立可验收 Slice 循环执行 `Implementer → Verifier` → 统一 `maintain-docs` 落地收口 |

## 三、标准闭环与 4 道硬门禁

1. **阶段 1 — 委派 `Planner`（只读）**
   - 传入用户需求、Bug 现象或 Issue 信息，要求产出标准**《规划单》**（含契约门禁标记、不变量、分步实施计划、失败清单与分层验收矩阵）。
   - **Gate 1（契约与方案门禁）**：
     - 若《规划单》标记 `NEED_USER_DECISION`（涉及 `docs/contracts/` 公共契约变更）或存在重大方案分歧 → **立刻暂停，向用户汇报并等待拍板**。
     - 契约不变且方案明确（或用户已批准）→ 进入阶段 2。

2. **阶段 2 — 委派 `Implementer`（读写）**
   - 原样转交**完整《规划单》**（及 Gate 1 用户决策），要求按计划实现、跑通基础自检并产出**《交付单》**。
   - **Gate 2（基础自检门禁）**：
     - 确认《交付单》中基础单测 / 编译 / `docscheck` 已绿灯且无未授权契约偏离；否则先让 `Implementer` 补齐。

3. **阶段 3 — 委派 `Verifier`（独立验证与录证）**
   - 把**《规划单》中的失败清单与验收矩阵 + 《交付单》**一并交给 `Verifier`，要求执行静态红线审计、实跑对应端 E2E 录证并产出**《验收回执》**。
   - **Gate 3（验收与熔断门禁）**：
     - `PASS` → 进入阶段 4。
     - `FAIL（打回 Implementer）` → 把失败详情与首错现场路径发给 `Implementer` 修复，修完后**必须重新走 `Verifier` 复验**。
     - **返工熔断**：围绕同一问题 `Implementer ↔ Verifier` 返工**最多 2 轮**。第 2 轮仍失败，或 `Verifier` 判定 `FAIL（升级 Planner）`（根因在架构或契约冲突），**立即熔断**，回到 `Planner` 重规划或升级给用户。

4. **阶段 4 — 视觉确认与收尾交付（Gate 4）**
   - **视觉门禁**：涉及 UI 改动且《验收回执》提供关键截图时，Coordinator 使用 `lody_upload`（`kind: "images"`）内联展示给用户；只有用户明确认可才记为视觉验收通过。
   - **诚实边界**：向用户汇报时明确区分 `PASS` 与 `NOT_RUN` 项，绝不把构建/单测包装成原生或真机通过。
   - **活文档与 PR 收口**：确保行为改动已同步 `docs/architecture/` 或 `docs/contracts/` 并通过 `go run ./tools/docscheck`（决策完成时调用 [maintain-docs](../maintain-docs/SKILL.md)）；验收通过后最多自动提交分支并建 **Draft PR**，绝不自动 Merge 或发版。

# Phase 94: 后端字段落地 - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-05-04
**Phase:** 94-后端字段落地
**Areas discussed:** A. 取反 helper 物理位置, B. Settings 读取失败容错, C. 集成测试位置 + 范围, D. restart_required 信号 phase 归属

---

## A. 取反 helper 物理位置

| Option | Description | Selected |
|--------|-------------|----------|
| A1 | 新建 `uc-bootstrap/src/network_policy.rs` 独立模块（名字明确 + 单测就近） | ✓ |
| A2 | 放 `space_setup.rs`（贴近 `IrohNodeConfig` re-export） | |
| A3 | 放 `builders.rs` `pub(crate)` helper，`non_gui_runtime` 共用 | |

**User's choice:** A1
**Notes:** 选独立模块的核心理由 —— 模块名一眼表达"网络策略翻译"用途；truth-table 单测就近；未来扩"其他网络相关 settings → infra config"翻译有现成入口。

---

## B. Settings 读取失败时的容错策略

| Option | Description | Selected |
|--------|-------------|----------|
| B1 | 硬失败 — daemon 拒绝启动，错误上报 | |
| B2 | 默认值兜底 — 用 `Settings::default()` 继续启动，只打 warn | |
| B3 | 按错误类型分流 — `NotFound` → default；`Parse`/`IO` → 硬失败 | ✓ |

**User's choice:** B3
**Notes:** 保护 LAN-only 信任锚点（脏 settings 不能让 LAN-only 静默回滚到允许 relay）+ 首次启动友好（settings.json 不存在是常态，不能因此让新装设备起不来）。**实施前提：** planner 需要确认 `SettingsPort::load` 的当前错误返回类型是否支持区分 NotFound vs Parse/IO；若不支持需要在 Phase 94 范围内补区分。

---

## C. 集成测试位置 + 范围

| Option | Description | Selected |
|--------|-------------|----------|
| C1 | uc-infra 测 bind 行为 + uc-bootstrap 内部单测测 helper truth-table | ✓ |
| C2 | 双测 — uc-infra + uc-bootstrap 端到端 settings→bind injection | |
| C3 | 仅 uc-bootstrap 端到端覆盖（偏离 ROADMAP 锁定文件位置） | |

**User's choice:** C1
**Notes:** 边界清晰 —— infra 测 bind invariant，bootstrap 测翻译 invariant；端到端链路由 roadmap 验收标准 #1 的手工流程兜底。**显式不做：** Phase 94 不新增 uc-bootstrap 端到端集成测试。

---

## D. restart_required 信号的 phase 归属

| Option | Description | Selected |
|--------|-------------|----------|
| D1 | Phase 94 PUT 响应就加 `restart_required: bool`（API 契约一次稳定） | ✓ |
| D2 | Phase 94 不动 PUT 响应，Phase 95 时再加 | |
| D3 | Phase 94 application 层 use case 返回 restart_required 但 HTTP 不暴露（中间态） | |

**User's choice:** D1
**Notes:** API 契约一次稳定，避免 Phase 95 双线（wire + UI）改动。`UpdateSettingsResponse` DTO 加 `restart_required: bool` 字段；OpenAPI schema 同步更新；当 patch 含 `network` 字段且至少有一个变更时返回 `true`。

---

## Claude's Discretion

- DTO 字段名 rust ↔ JSON 转换（按既有 `#[serde(rename_all = "camelCase")]` 模式）
- `UpdateNetworkSettings` 是否拆独立 use case，还是 `UpdateSettings` 内分支判断 — 由 planner 决定
- iroh integration test fixture 复用策略 — 由 planner 决定
- `tracing` target / level / 字段格式 — 由 planner 决定

---

## Deferred Ideas

- **PR 模板 checkbox "[ ] 我没有尝试在运行时重建 iroh endpoint"** — 留给 Phase 97（reviewer checklist 一起加）。
- **`UpdateNetworkSettings` 独立 use case 拆分** — 由 planner 决定；当前 `SettingsFacade::update` 泛 patch 也满足。
- **`IrohNode::endpoint()` 访问器方式** — 留给 Phase 96 连接通道指示器决策。
- **runtime 热切换 LAN-only Mode** — 整里程碑显式排除，需独立 phase。
- **OTLP `connection_path` 标签** — Future Requirements D4，v0.7.x。

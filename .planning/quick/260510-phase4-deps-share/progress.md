# progress — 会话日志

## 2026-05-10 会话初始化

### 上下文交接

接续 `.planning/quick/260510-daemon-reload-arch/` (上一会话):

- 上半场已落地 (commits c819098d / 940aa83f / 9f627afc):
  - AppFacade 5 个 daemon-lifecycle 字段改 ArcSwapOption
  - daemon 不再装第二份 AppFacade，改为 build_daemon_lifecycle_facades + swap
  - SearchFacade.coordinator 走 ArcSwapOption,set_coordinator/clear_coordinator API
  - 进程内只有一份 AppFacade ✅

- 下半场遗留 (本会话目标):
  - WireOverrides 仍透传 5 层 —— 因为 daemon 端仍跑 wire_dependencies
  - daemon reload 仍重建 sqlite pool / repos —— 因为 build_daemon_app 内调
    build_core → wire_dependencies_with_overrides

### 调研记录

- 已 grep `WireOverrides` —— 散落在 9 个文件、约 22 处引用
- 已读 `uc-bootstrap/src/builders.rs::build_core` / `build_daemon_app`,
  确认拆分点
- 已读 `uc-desktop/src/bootstrap.rs::build_gui_app`,确认进程级装配的当前
  做法
- 已读 `uc-desktop/src/daemon/host.rs::start_in_process`,确认 daemon-lifecycle
  装配的当前做法 (它已经接受 `Arc<AppFacade>` 入参，再加 deps 入参就能消除
  WireOverrides 透传)
- 已读 `uc-desktop/src/daemon/bootstrap.rs::build_daemon_bootstrap_assembly`,
  当前签名 `(WireOverrides) -> ...`,目标签名换为接受已有 deps

### 计划文件创建

- `.planning/quick/260510-phase4-deps-share/task_plan.md` — Phases A-E
- `.planning/quick/260510-phase4-deps-share/findings.md` — 当前散落点表 +
  装配链对比 + 待回答问题
- `.planning/quick/260510-phase4-deps-share/progress.md` — 本文件

### 实施前查证 (2026-05-10 完成)

1. [x] 读 `WiredDependencies` 字段定义 —— 关键状态全 Arc 化，`AppDeps`
   内部全 `Arc<dyn Port>` (见 findings.md §5 Q1)
2. [x] 审计 `BackgroundRuntimeDeps` 各字段归属 —— **重大发现**: 含两个
   一次性 `mpsc::Receiver` (`spool_rx` / `worker_rx`), spool/blob worker
   语义上是进程级而非 daemon-lifecycle, 但当前装在 daemon 启动里。
   这是 task_plan 原方案没覆盖的隐藏错位 (见 findings.md §5 Q2 / §6)
3. [x] `init::reconcile_*` 时机平移 OK —— 当前在 `build_daemon_app` 里
   (每次 daemon 启动跑一次), 目标 `build_daemon_lifecycle` 时机等价，
   port 入参从已有 `WiredDependencies` 读 (见 findings.md §5 Q4)
4. [x] 与用户对齐命名：rename `build_gui_app` → `build_process_runtime`,
   `GuiBootstrapContext` → `ProcessRuntimeContext`

### Phase A 范围扩大 (2026-05-10)

基于 Q2 的发现，重新评估 Phase A 的拆分：

- 原方案：只拆 `build_daemon_app` 的"wire vs daemon-lifecycle"
- 修正后：还要把 `spawn_blob_processing_tasks` 从 daemon 启动移到 GUI
  shell `run()` setup 阶段 (与 standalone `daemon::run` 入口一致)
- 影响：`daemon::start_in_process` 不再消费 `BackgroundRuntimeDeps`,
  `daemon::bootstrap::DaemonBootstrapAssembly` 移除 background / blob_ports
  字段，task_registry 归属切到进程级

**待用户确认 (Phase A 启动前)**:

`spawn_blob_processing_tasks` 从 daemon 启动移到进程级 —— 这超出了原
task_plan 的"删 WireOverrides + 不重建 sqlite pool"目标范围。两条路线：

- **路线 X (推荐，范围合理)**: 本次 PR 一并处理。理由：不处理就拆不
  彻底 —— `BackgroundRuntimeDeps` 含 receiver, 如果 daemon 仍持有它，
  reload 时 receivers 已被消费; 必须把它从 daemon 装配里抠出来才能
  让 daemon reload 不重建 deps。
- **路线 Y (保守，多一个 PR)**: 本次 PR 只处理"AppDeps 进程级 + Wire
  Overrides 删除", `BackgroundRuntimeDeps` 仍然由 daemon 装配持有，
  但拆出"daemon reload 跳过 spawn_blob_processing_tasks"的 if 分支
  (检测已 spawn 过)。下一 PR 再把 background workers 上提。

倾向路线 X —— Y 引入"已 spawn 标志"是新的状态泄漏，与 Phase 4 上半场
"消除 WireOverrides 这种代偿"的精神冲突。

## 错误记录

| 错误 | 第几次尝试 | 解决 |
|---|---|---|
| (尚未发生) | — | — |

## 测试结果

### Phase A + B + C 完成后 (2026-05-10)

| 测试套 | 结果 |
|---|---|
| `cargo check --workspace` | 干净 |
| `cargo test -p uc-application --lib` | 413 passed |
| `cargo test -p uc-bootstrap --lib` | 12 passed |
| `cargo test -p uc-bootstrap --tests` | 5 passed (整合测试 4 binaries) |
| `cargo test -p uc-desktop --lib` | 48 passed |
| `cargo test -p uc-tauri --lib` | 21 passed |
| `cargo test -p uc-webserver --lib` | 45 passed |
| **合计** | **544 passed** |

### Commit 序列

| commit | 内容 |
|---|---|
| 181a504e | rename build_gui_app → build_process_runtime |
| 9fa945b7 | split BackgroundRuntimeDeps out of WiredDependencies |
| 655d187a | add build_daemon_lifecycle, derive Clone on AppDeps |
| 283cbc5a | daemon 复用进程级 deps,blob workers 上提到进程启动 |
| b53c7492 | 删除 WireOverrides 整套机制 |

## 剩余工作 (Phase D / E)

### Phase D — 验证 daemon reload 不重建 deps

钉死 "daemon reload 复用进程级 deps" 这条契约。倾向写集成测试
(uc-desktop tests),断言 daemon spawn 前后 sqlite pool 内部 Arc 地址
保持。次选：加探针 log。

### Phase E — 测试与回归

- 全栈 cargo test (含 long-running integration)
- pnpm exec vitest run (前端，本次重构理论上无前端改动，只验证)
- 手动复现 mobile_sync 重启路径
- 手动验证 standalone daemon binary 独立启动
- 手动验证 lan_listener_error 端到端可见

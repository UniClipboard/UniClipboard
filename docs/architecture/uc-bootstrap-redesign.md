# uc-bootstrap 重新设计方案

> 状态：执行中 —— **Phase 0 已完成（2026-06-25）**，Phase 1–4 待执行
> 范围：`crates/uc-bootstrap`（组合根）的内部结构、公开表面、跨 crate 职责归属
> 不改变任何对外可观察行为；所有阶段均为 `refactor:` / `arch:` 级别

## 1. 背景与问题

`uc-bootstrap` 是六边形架构里 **唯一** 被允许同时依赖 `uc-core + uc-application + uc-infra + uc-platform` 的 crate（sole composition root，见 `lib.rs` 模块 doc 与 `VISION.md`「架构原则」）。它天然是所有层的汇聚点，体量大有其 **本质复杂度**。但当前 14 个平铺模块、7109 行里混入了大量 **意外复杂度**：

| 乱源 | 具体表现 | 违反的项目规则 |
|---|---|---|
| **ADR-008 迁移残留死代码** | GUI 装配早已 fork 到 `uc-desktop/src/gui_wiring.rs`（文件头注释自述 "replaces the `uc_bootstrap::build_gui_client_context()` call"），但 bootstrap 里的旧版从未删除 | 「不保留无移除计划的并行新旧逻辑」「单一真相源」 |
| **god module** | `assembly.rs` 1607 行混 6 类职责（infra 构造 / platform 构造 / bundle 结构 / GUI-client 路径 / 路径解析 / 杂项 helper） | `crates/AGENTS.md` 已标 `smallest safe edits only` 的 HOTSPOT |
| **职责错置** | `correlation.rs` 是完整的 `tracing_subscriber::Layer` 实现（observability 逻辑，非装配）；`init.rs` / `pending_import.rs` 是启动期业务编排 | 组合根纯粹性 |
| **公开表面过宽** | `lib.rs` re-export ~42 符号，真实外部契约仅 21 个 | 最小 API 表面 |
| **命名债** | `space_setup.rs` / `space_setup` 模块实际装配整个 iroh sync engine，与 `uc_application::facade::space_setup` 撞名 | 标准术语、名实一致 |

## 2. 设计目标与约束

### 2.1 重新定位（单一职责）

> **uc-bootstrap = daemon 进程的组合根**：把 port 接到 adapter，产出 daemon（及 CLI dev-tools 的 in-process facade）所需的依赖图。

不属于本 crate：
- GUI 装配 → 已属 `uc-desktop`
- observability 的 **实现逻辑** → 应属 `uc-observability`
- 业务 **启动编排** → 介于 wiring 与 use case 之间，**经决策留在本 crate** 的 `startup/` 子模块（务实：它在 wiring 之后用 wired ports 跑一次性协调，是组合根的合理延伸）
- 所有迁移残留死代码 → 删

### 2.2 硬约束

- **不破坏真实外部契约**（§3 的 21 符号）。
- **先稳定契约边界，再重组内部**：daemon 当前有 6 处 reach into 子模块路径（见 §3 标 ⚠️），内部重组前必须先让所有外部消费方只依赖顶层 re-export，否则模块搬迁会破坏 daemon 编译。
- **原子提交**（`docs/agent/architecture-rules.md` §Atomic Commit）：`arch:`（跨 crate 边界移动）与 `refactor:`（crate 内结构）永不同 commit；每个 commit 独立可编译、测试通过。
- **零行为变化**：除死代码删除外，不改任何装配逻辑；`cargo test -p uc-bootstrap -p uc-daemon` 全程保持绿。

## 3. 真实外部契约（权威清单 — 重构不可破坏）

来源：全仓 `use uc_bootstrap::` 实际 import 语句（非同名 fork、非注释）。**uc-desktop / uc-tauri 确认零引用**（它们的 GUI 装配自带于 `uc-desktop`）。

### apps/daemon（19 符号）

| 符号 | 当前路径 | 备注 |
|---|---|---|
| `wire_dependencies` | ⚠️ `assembly::` | 子模块路径 |
| `WiredDependencies` | ⚠️ `assembly::` | 子模块路径 |
| `get_storage_paths` | ⚠️ `assembly::` | 子模块路径 |
| `install_panic_logging_hook` | ⚠️ `tracing::` | 子模块路径 |
| `FileTransferLifecycle` | ⚠️ `file_transfer_lifecycle::` | 子模块路径 |
| `build_daemon_lifecycle` | ⚠️ `builders::` | 子模块路径 |
| `compose_event_context` | 顶层 | |
| `init_tracing_subscriber` | 顶层 | |
| `BackgroundRuntimeDeps` | 顶层 | |
| `SyncEngineAssembly` | 顶层 | |
| `SystemClipboardWiring` | 顶层 | |
| `build_mobile_sync_facade` | 顶层 | |
| `build_app_facade_from_deps` | 顶层 | |
| `AppFacadeAssemblyOptions` | 顶层 | |
| `TaskRegistry` | 顶层 | |
| `ClipboardRestoreAssembly` | 顶层 | |
| `resolve_clipboard_integration_mode` | 顶层 | |
| `BlobProcessingPorts` | 顶层 | |
| `spawn_blob_processing_tasks` | 顶层 | |

### apps/cli（dev-tools feature，2 符号）

| 符号 | 当前路径 |
|---|---|
| `CliAppRuntime` | 顶层 |
| `build_cli_app_runtime` | 顶层 |

## 4. 符号三分类台账

每个 `lib.rs` re-export 的去向。判定依据：§3 权威 import + crate 内引用。标 †的需在 Phase 0 执行时用 `cargo check` 终判（防遗漏的间接引用）。

### 4.1 契约（保持 public 顶层 re-export）
§3 全部 21 符号。

### 4.2 仅内部自用（降 `pub(crate)`，不删）
`build_cli_wiring_context`、`build_clipboard_write_coordinator`、`LoggingHostEventEmitter`、`build_sync_engine_assembly`、`SyncEngineAssemblyError`、`reconcile_peer_addresses`、`reconcile_trusted_peers`、`build_analytics_sink`。

> **执行修正（Phase 0d）**：原列入本类的 `build_cli_context`、`build_cli_context_with_profile`、`build_slice1_cli_context`、`CliBootstrapContext`、`build_cli_app_facade`、`build_non_gui_bundle`（连带 `NonGuiBundle`）、`load_config` 经 `cargo check` 级核实为 **零调用方死代码**（全仓仅在 `lib.rs` 自身 re-export 出现，被 `build_cli_app_runtime` 取代），已改为 **删除**（见 §4.4），而非降级。`build_cli_context_with_profile` / `CliBootstrapContext` 是删除前两个死入口的唯一调用方，连带死亡。

### 4.3 多余 re-export（外部从 canonical crate 直接拿，本 crate 不再转出）
`IrohNodeConfig` — canonical 定义在 `uc-infra/src/network/iroh/node.rs`；daemon 契约里不含它，从 public 表面移除（space_setup 内降 `pub(crate) use`）。

> **执行修正（Phase 0d）**：原计划遗漏了 `crates/uc-bootstrap/tests/` 这一外部消费方——4 个集成测试 import `uc_bootstrap::IrohNodeConfig`。它们本就 dev-depend `uc-infra` 并从中取其它 iroh 类型，故改为直接 `use uc_infra::network::iroh::IrohNodeConfig`（正合本节"外部从 canonical crate 直接拿"）。

### 4.4 死代码（删 — ADR-008 迁移残留）
| 符号 | 位置 | 取代者（活的） |
|---|---|---|
| `build_gui_client_context` | `assembly.rs:1202` | `uc-desktop/gui_wiring.rs:79` |
| `wire_gui_client_deps` | `assembly.rs:1160` | uc-desktop 自带 |
| `GuiClientDeps` | `assembly.rs:1144` | `uc-desktop/gui_wiring.rs:62` |
| `is_setup_complete` | `init.rs:37` | `apps/cli/setup_check.rs:37` |
| `ensure_default_device_name` | `init.rs:86` | `uc-desktop/gui_wiring.rs:31` |
| `resolve_pairing_device_name`† | `assembly.rs:1574` | uc-desktop runtime 自带同名 |
| `build_cli_context`‡ | `builders.rs` | `build_cli_app_runtime` |
| `build_cli_context_with_profile`‡ | `builders.rs` | `build_cli_app_runtime` |
| `build_slice1_cli_context`‡ | `builders.rs` | `build_cli_app_runtime` |
| `CliBootstrapContext`‡ | `builders.rs` | （随上面三个入口一起退役） |
| `build_cli_app_facade`‡ | `non_gui_runtime.rs` | `build_cli_app_runtime` |
| `build_non_gui_bundle` + `NonGuiBundle`‡ | `non_gui_runtime.rs` | 无消费方 |
| `load_config`‡ + 整个 `config.rs` | `config.rs` | 无（孤儿，config 实际由别处加载） |

‡ = 执行期（Phase 0d）`cargo check` 核实新增的死代码，原稿 §4.2 误判为 internal-only。

## 5. 现有模块 → 目标结构映射

| 现模块（行数） | 去向 | 动作 |
|---|---|---|
| `assembly.rs` (1607) | 拆分 → `layer/{infra,platform,paths}.rs` + `wiring/{deps,wire}.rs` | 拆 god module；删 GUI 路径死代码 |
| `space_setup.rs` (735) | `subsystem/sync_engine.rs` | 改名（接续既有重命名） |
| `file_transfer_lifecycle.rs` (333) | `subsystem/file_transfer.rs` | 移动 |
| `background_tasks.rs` (181) | `subsystem/blob_tasks.rs` | 移动 |
| `analytics.rs` (427) | `subsystem/analytics.rs` | 移动 |
| `network_policy.rs` (362) | `wiring/network_policy.rs` | 移动 |
| `builders.rs` (268) | `entrypoint/{daemon,cli}.rs` | 按入口拆 |
| `non_gui_runtime.rs` (751) | `entrypoint/non_gui.rs` | 移动 |
| `init.rs` (518) | `startup/reconcile.rs` | 删 2 个死函数；reconcile_* 进 startup/ |
| `pending_import.rs` (665) | `startup/pending_import.rs` | 移动 |
| `tracing.rs` (818) | `observability/tracing.rs` | 移动（correlation 引用改跨 crate） |
| `correlation.rs` (334) | **`uc-observability`** | 跨 crate 搬迁（仅 tracing.rs 引用，8 处） |
| `config.rs` (47) | **删** | `load_config` 无调用方（全文件死代码）；整文件删除 |
| `task_registry.rs` (7) | 删 | 消费方改用 `uc_core::TaskRegistry`（daemon 直依赖 uc-core，Option B） |
| `lib.rs` (56) | `lib.rs` | 收窄 re-export 到 §4.1 |

## 6. 目标模块树

```text
uc-bootstrap/src/
  lib.rs              # 门面：只 re-export §4.1 的契约符号（TaskRegistry 改由 uc_core 出，故为 20）
  # config.rs 已删（load_config 无调用方，死代码）
  layer/              # ① 单层 adapter 装配（拆自 assembly.rs）
    infra.rs          #   DB pool / repos / encryption / timers / fs
    platform.rs       #   clipboard / secure storage
    paths.rs          #   路径解析（厘清与 uc-app-paths 真相源边界）
  wiring/             # ② 组合根核心（拆自 assembly.rs）
    deps.rs           #   WiredDependencies + 4 bundle 结构
    wire.rs           #   wire_dependencies() + 各 sub-assembly builder
    network_policy.rs #   relay 策略翻译
  subsystem/          # ③ 子系统装配片段
    sync_engine.rs    #   ← space_setup.rs
    file_transfer.rs  #   ← file_transfer_lifecycle.rs
    blob_tasks.rs     #   ← background_tasks.rs
    analytics.rs      #   ← analytics.rs
  entrypoint/         # ④ 场景入口构造器
    daemon.rs         #   ← builders.rs (daemon-lifecycle)
    cli.rs            #   ← builders.rs (CLI dev-tools)
    non_gui.rs        #   ← non_gui_runtime.rs
  startup/            # ⑤ 启动编排（决策：留 bootstrap）
    reconcile.rs      #   ← init.rs 的 reconcile_*
    pending_import.rs #   ← pending_import.rs
  observability/
    tracing.rs        #   subscriber 装配（correlation 搬走后 use uc_observability::correlation）
```

## 7. 分阶段执行 plan

每个 Phase 内部按原子提交规则细分。验证基线：`cargo check --workspace` + `cargo test -p uc-bootstrap -p uc-daemon`（含 slice2 e2e）+ openapi/sdk drift check（若触及生成 doc 注释）。

### Phase 0 — 删死代码 + 收窄表面（`refactor:`，零行为变化，ROI 最高）✅ 已完成 2026-06-25
- **0a** ✅ 删 GUI-client 路径（`build_gui_client_context` / `wire_gui_client_deps` / `GuiClientDeps`）+ `resolve_pairing_device_name` + 顺手清 unused `use tracing::info;`。
- **0b** ✅ 删 `init.rs` 的 `is_setup_complete` / `ensure_default_device_name` 两个死函数 + unused import。
- **0c** ✅ **决定 = Option B**：删 `task_registry.rs` shim，daemon `process_runtime.rs` 改 `use uc_core::TaskRegistry`，bootstrap 内部改 `uc_core::task_registry::TaskRegistry`，`uc_bootstrap::TaskRegistry` re-export 移除。理由：daemon 已直接依赖 uc-core（`Cargo.toml`）且广泛 `use uc_core::`，删 shim 即单一真相源。
- **0d** ✅ 真正 internal-only 的 §4.2 符号降 `pub(crate)`；§4.3 `IrohNodeConfig` 移出 public（测试改取 uc-infra canonical）。执行期发现原 §4.2 把 7 个符号误判为 internal-only（实为死代码），改为删除——见 §4.4 ‡ 行。
- 提交：3 个原子 `refactor:` commit（drop TaskRegistry shim / remove GUI-client dead code / narrow public surface）。
- DoD：✅ `cargo check --workspace` 绿；✅ 死代码符号全仓 `rg` 清零；✅ `cargo test -p uc-bootstrap -p uc-daemon` 绿（含 46 unit + 5 e2e target）。

### Phase 1 — 契约边界稳定化（`refactor:`）
- 把 daemon 的 6 处 ⚠️ 子模块路径 import 改走顶层 `uc_bootstrap::X`（前提：lib.rs 顶层 re-export 这 6 个符号）。
- 目的：让 `lib.rs` 成为 **唯一** 对外契约面，后续内部重组对 daemon/cli 完全透明。
- DoD：外部对 uc_bootstrap 的引用全部经由顶层；`rg 'uc_bootstrap::(assembly|tracing|builders|file_transfer_lifecycle)::'` 在 apps/ + src-tauri/ 清零。

### Phase 2 — `space_setup` → `sync_engine` 改名（`refactor:`）
- 文件/模块 `space_setup.rs` → `subsystem/sync_engine.rs`（先不建子目录，或与 Phase 4 合并）；模块声明 + crate 内 6 处 `crate::space_setup::` 引用更新。
- 不碰 `uc_application::facade::space_setup`、字段名 `space_setup`。

### Phase 3 — 跨 crate 职责归位（`arch:`）
- **3a** `correlation.rs` 搬 `uc-observability`；`observability/tracing.rs` 改 `use uc_observability::correlation::...`（8 处引用，全在 tracing.rs）。
- **3b** `init.rs` 剩余 reconcile_* + `pending_import.rs` 收进 `startup/` 子模块。
- 注意：3a 跨 crate 边界移动 = `arch:`，与 3b 的 crate 内整理分开 commit。

### Phase 4 — 拆 god module + 子目录归位（`refactor:`，小步切）
- 逐子模块从 `assembly.rs` 抽出 → `layer/{infra,platform,paths}.rs`、`wiring/{deps,wire,network_policy}.rs`；其余模块移入 `subsystem/` `entrypoint/` `observability/`。
- 每抽一个子模块一个 commit，独立可编译可测。
- 因 Phase 1 已稳定契约，本阶段对 daemon/cli **零影响**（只动 crate 内 path + lib.rs 内部 mod 声明，顶层 re-export 不变）。

## 8. 不做什么（本质复杂度 / 反过度工程）

- **不拆新 crate**：bootstrap 必然链接所有层，拆 crate 不会减依赖、只增协调成本。仅把 **确实不属于装配** 的 `correlation` 搬到已有的 `uc-observability`。
- **不动 `wire_dependencies` 内的 bundle 字面量**：5 个 bundle 的字段装配是本质复杂度，Phase B（已完成）已收过。
- **不强行把 startup 编排做成 use case**：经决策保留在 `startup/`（§2.1）。
- **不重命名字段/参数 `space_setup`**：它是指向装配路径的角色标签，类型才表达内容。
- `config.rs` / `network_policy.rs` / `background_tasks.rs` 已洁净，仅移动不重写。

## 9. 待执行时核实项（codex / 评审重点）

1. ✅ †标记符号终判（Phase 0）：`resolve_pairing_device_name` = 死（删）；`build_analytics_sink` = internal-only（降 `pub(crate)`）。另发现 7 个原 §4.2 符号实为死代码（§4.4 ‡）。
2. ✅ Phase 0c `TaskRegistry` = Option B（daemon 直依赖 uc-core，删 shim + re-export）。
3. ⏳ `layer/paths.rs` 与 `uc-app-paths`(directory-layout authority) 的真相源边界——是否存在重复逻辑，可否直接委托。（Phase 4 落地时核实）
4. ⏳ 目标子目录划分（6 组）是否过度——可在 Phase 4 落地时按实际收敛。
5. ⚠️ **新增教训**：契约核实必须含 `crates/uc-bootstrap/tests/`（集成测试是外部消费方），且"零外部 import"≠"非死代码"——需区分"有 crate 内调用方"(internal-only) 与"全仓零调用方"(dead)。

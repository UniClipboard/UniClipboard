# ADR-008 P1 执行计划：抽出 `uc-daemon` 库

- **承接**：[ADR-008](./adr-008-uniclipd-split-gui-as-client.md) §4 P1（抽库）+ D1 + OQ-desktop-residue
- **日期**：2026-05-30
- **性质**：**纯结构迁移、零行为变化、revert-safe**。`GuiInProcess` 与 `Standalone` 两条路径全程行为不变；`DaemonRunMode::GuiInProcess` 迁入但 **不删**（D2 删除留到 P3）。
- **方法**：9-agent 规划 workflow 逐文件分类 + consumer 引用面 + 反依赖风险分析。

## 0. 净效果（边界）

```
uc-daemon（新建，lib only）── GUI-agnostic runtime 构件库
        ▲ forward dep
        │
uc-desktop ── host 胶水：host.rs(run/start_in_process/ProcessRuntimeHandles)、daemon_probe、DesktopRuntime、bootstrap
        ▲                      ▲
   uc-tauri                 uc-cli
   （零改动，               （零改动，仍经
    经 uc_desktop::daemon::* 再导出）  uc_desktop::daemon::run_standalone_from_env）
```

- **uc-daemon 依赖任何 uc-desktop 内容 = 0**（铁律，本计划的核心约束）。
- **uc-cli 零改动、uc-tauri 零改动**：靠 uc-desktop `daemon/mod.rs` 的 re-export shim 保 `uc_desktop::daemon::*` 接口面逐字不变。
- uc-cli 仍依赖 uc-desktop（CLI 解耦是 **P2** 的事，非 P1，见 OQ-1）。
- `uniclipd` 二进制 **不在 P1 范围**（P1 只抽 lib，bin 是 P2）。

## 1. uc-daemon Cargo.toml 依赖集

`version.workspace = true`（与 uc-desktop 同 `0.13.0-alpha.1`，保 `DAEMON_VERSION` 运行值不变）。

workspace crate：`uc-application`、`uc-core`、`uc-bootstrap`、`uc-infra`、`uc-platform`(features=["test-helpers"])、`uc-observability`、`uc-daemon-contract`、`uc-daemon-local`、`uc-webserver`。
第三方：`tokio`(full)、`tokio-util`、`async-trait`、`anyhow`、`serde`(derive)、`serde_json`、`chrono`(serde)、`tracing`。
dev-deps：`tempfile`、`mockall`（迁移 mobile_lan_lifecycle / run_mode / service_plan 的单测要）。

**显式不含**：任何 GUI 框架（tauri/iced/objc2/...）、`uc-desktop`、`reqwest`、`uc-daemon-client`（probe 留 uc-desktop）。

## 2. 迁 / 留 / 拆

### 迁入 uc-daemon（GUI-agnostic）
`run_mode.rs`（保 GuiInProcess variant，不删）、`tokio_runtime.rs`、`service.rs`、`state.rs`、`handle.rs`、`ownership.rs`、`bootstrap.rs`、`run_loop.rs`(改 `crate::DAEMON_VERSION`→`env!("CARGO_PKG_VERSION")`)、`startup_recovery.rs`、`runtime_controls.rs`、`runtime_assembly.rs`、`search_assembly.rs`、`app_facade_assembly.rs`、`service_assembly.rs`、`service_plan.rs`、`app.rs`、`app_assembly.rs`、`mobile_lan_lifecycle.rs`、`workers/*`、`peers/*`、`search/*`。

> **3 处关键重分类**（侦察初判 stay/split → 改为 move，否则造成 uc-daemon→uc-desktop 反依赖）：
> - `mobile_lan_lifecycle.rs`：零 GUI 耦合，`app.rs`(迁) 硬引用 `MobileLanLifecycleController` → **整体迁**。
> - `app_assembly.rs`：GuiInProcess 牵连仅是 **参数值**(`listens_to_os_signals`/`process_mode` 由 host.rs 传入)，无 enum 分支/无 ProcessRuntimeHandles → **迁**。
> - `workers/clipboard_watcher.rs`：`capture_gate` 是普通 `Arc<AtomicBool>`，只有 docstring 提 GuiInProcess → **整体迁**(doc 措辞 P3 再改)。

### 留 uc-desktop（到 P3）
- `host.rs` 的 `start_in_process()`（GuiInProcess 装配编排器）+ `ProcessRuntimeHandles` struct（捆 `WiredDependencies`/storage_paths/clipboard_write_coordinator/file_transfer_*，进程级 GUI 持有资源）。
- `host.rs` 的 `run()`/`run_standalone_from_env()`/`RUN_MODE_ENV`/`RUN_MODE_SERVER`：因碰 `crate::bootstrap::build_process_runtime` + `crate::DesktopRuntime`（迁走会反依赖）→ **P1 留**（见 OQ-1）。
- `daemon_probe.rs`、`bootstrap.rs`(ProcessRuntimeContext/build_process_runtime)、`runtime.rs`(DesktopRuntime)。

### 拆
- `host.rs`：runtime 主体调用链全在 uc-daemon；`start_in_process` 与 `run*` 留 uc-desktop，改为 `use uc_daemon::daemon::{...}` **前向调用** uc-daemon 的 ~9 个 `build_daemon_*` 装配函数 + 构造 `MobileLanLifecycleController`/`AppFacadeListenerSpawner`。
- `mod.rs`：迁走的模块声明进 uc-daemon `lib.rs`；uc-desktop 留瘦身 shim（见 §4）。

## 3. 有序执行步骤（每步带 compile gate）

| # | 步骤 | gate | revert-safe |
|---|---|---|---|
| 1 | 建空 uc-daemon crate 骨架（workspace member + Cargo.toml + 空 lib.rs/daemon/mod.rs），暂不接 uc-desktop | `cargo check -p uc-daemon` | ✓ |
| 2 | 迁叶子纯数据/工具：`run_mode/tokio_runtime/service/state/handle`；uc-desktop 加 uc-daemon 依赖 + 改 mod.rs 为 `pub use uc_daemon::run_mode` 等 | `cargo check -p uc-daemon && -p uc-desktop` | ✓ |
| 3 | 迁 `ownership.rs`（只依赖 handle）；uc-desktop re-export | `cargo check -p uc-daemon && -p uc-tauri` | ✓ |
| 4 | 迁叶子 worker/feature：`workers/*`、`peers/*`、`search/*` | `cargo check -p uc-daemon` | ✓ |
| 5 | 迁 `mobile_lan_lifecycle.rs` 整体（带测试，需 uc-webserver::mobile_lan + mockall）→ 解锁 app.rs | `cargo check -p uc-daemon && cargo test -p uc-daemon mobile_lan --no-run` | ✓ |
| 6 | 迁中层装配：`bootstrap/run_loop(改 DAEMON_VERSION)/startup_recovery/runtime_controls/runtime_assembly/search_assembly/app_facade_assembly/service_plan/service_assembly` | `cargo check -p uc-daemon` | ✓ |
| 7 | 迁 `app.rs` + `app_assembly.rs`；`build_daemon_app_instance`/`DaemonAppAssemblyInput` 设 pub 供 host.rs 用 | `cargo check -p uc-daemon` | ✓ |
| 8 | 拆 `host.rs`：start_in_process/ProcessRuntimeHandles/run* 留 uc-desktop，内部 `use crate::daemon::{...}`→`use uc_daemon::daemon::{...}`；补齐 uc-daemon 侧 pub | `cargo check -p uc-desktop` | ✓ |
| 9 | 定稿 uc-desktop `daemon/mod.rs` shim（见 §4）；核 daemon_probe 导入 | `cargo check -p uc-desktop && cargo test -p uc-desktop daemon_probe --no-run` | ✓ |
| 10 | 全工作区验证 + 行为不变确认（GuiInProcess 经 uc-tauri、Standalone 经 uc-cli `uniclip daemon` 均编译；GuiInProcess variant 仍在；迁走的单测在新家跑） | `cargo check --workspace && cargo test -p uc-daemon && -p uc-desktop && cargo clippy --workspace -- -D warnings` | ✓ |

## 4. API shim（uc-desktop `daemon/mod.rs`，保接口面逐字不变）

```
pub use uc_daemon::run_mode;            // → uc_desktop::daemon::run_mode / DaemonRunMode
pub use uc_daemon::DaemonHandle;        // → uc_desktop::daemon::DaemonHandle（uc-tauri restart 路径）
pub use uc_daemon::DaemonOwnership;     // → uc_desktop::daemon::DaemonOwnership（uc-tauri run.rs/restart.rs）
pub use host::ProcessRuntimeHandles;    // 本地留（uc-tauri run.rs / daemon_probe）
pub(crate) use host::start_in_process;  // 本地留（daemon_probe）
// run / run_standalone_from_env / RUN_MODE_* 本地留（uc-cli 经 uc_desktop::daemon::* 零改动）
```

**consumer 净影响**：uc-cli 0、uc-tauri 0；uc-desktop 多一条 forward dep；uc-daemon 反依赖 uc-desktop = 0。

## 5. 留到 P3 的 GuiInProcess 残余

`DaemonRunMode::GuiInProcess` variant（迁入但不删）、`start_in_process()`、`ProcessRuntimeHandles`、`daemon_probe` 的 `start_owned_in_process`、`DaemonOwnership::Owned` 的 GUI-持有语义、clipboard_watcher/app_assembly 的 GuiInProcess docstring、`build_process_runtime`/`DesktopRuntime`（P3 可经 `ProcessRuntimeBootstrap` trait 反转让 uc-daemon 拥有 run()）。

## 6. 风险

- **反依赖陷阱（最高）**：app.rs/app_assembly 硬引用 mobile_lan_lifecycle——已靠"整体迁 mobile_lan_lifecycle"化解（最重要的一处重分类）。
- **DAEMON_VERSION 语义**：run_loop 的 `crate::DAEMON_VERSION` 迁后变 uc-daemon 的 `CARGO_PKG_VERSION`，两者同 workspace 版本值相同——必须 `version.workspace = true` 保运行值不变（否则 daemon health/upgrade 检测行为变）。
- **可见性扩面**：host.rs(留) 要用的符号须从 pub(crate) 提到 pub（build_daemon_* / 各 Input / DaemonHandle::new 等），P1 接受扩面、P3 收窄。
- **测试搬迁**：迁走文件的 `#[cfg(test)]` 随文件走；mockall 加进 uc-daemon dev-deps；daemon_probe 的 wiremock 测试留 uc-desktop。
- **uc-platform feature 统一**：uc-daemon 须同样 `features=["test-helpers"]` 镜像 uc-desktop，避免平台适配器选择漂移。
- **循环依赖**：只要迁走的文件不出现 `crate::`(uc-desktop) 路径即无环——已 grep 确认仅 host.rs(留) + run_loop.rs(DAEMON_VERSION，已改) 有 crate:: 引用。
- **Cargo.lock**：加 member 改 lock 图，步骤 10 全量 `cargo check --workspace` 兜底 feature 统一意外。

## 7. 待确认（OQ，附推荐）

| OQ | 推荐 |
|---|---|
| **OQ-1**：P1 后 uc-cli 仍依赖 uc-desktop（`run_standalone_from_env`+`build_process_runtime` 留 uc-desktop），CLI 解耦推到 **P2**——可接受吗？ | **接受**。这与 ADR §4 phasing 一致（P2 才 CLI 解耦）；P1 保持纯结构、uc-cli 零改动。把 run* 迁 uc-daemon 需连 build_process_runtime + 一个非 DesktopRuntime 的轻量 process bootstrap 一起迁，超出"纯结构"。 |
| **OQ-2**：`RUN_MODE_ENV`/`RUN_MODE_SERVER` 归属 | 留 uc-desktop（随 run*）。 |
| **OQ-3**：uc-daemon API 形态 | 保留现有 ~9 个 `build_daemon_*` free function 调用形态（P1 不引入 `DaemonRuntimeBuilder`，降风险）。 |
| **OQ-4**：crate 名 `uc-daemon`（lib `uc_daemon`）、`uniclipd` bin 出 P1 范围 | 确认。P1 只抽 lib，bin 是 P2。 |
| **OQ-5**：mobile_lan_lifecycle 在 P1 整体迁（覆盖侦察初判的"stay-P3"） | 确认整体迁（无 GUI 耦合、app.rs 需要；部分拆买不到收益还多一道缝）。 |

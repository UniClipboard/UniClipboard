# Phase 4 下半场 — daemon 复用进程级 deps，删除 WireOverrides

> 接续 `.planning/quick/260510-daemon-reload-arch/`。上一会话落地了 AppFacade
> 单例化 + daemon-lifecycle 子 facade swap (commits 9f627afc / 940aa83f /
> c819098d)。本会话要解决遗留的两条目标：
>
> 1. **删除 WireOverrides 整套机制** —— 当前 daemon 端仍跑第二次
>    `wire_dependencies_with_overrides`,所以 5 层透传的 `mobile_sync_endpoint_info`
>    Optional Arc 仍然必要。
> 2. **daemon reload 不重建 sqlite pool / repos / settings repo** —— 当前
>    `build_daemon_app` → `build_core` → `wire_dependencies_with_overrides`
>    每次 reload 重做整套 deps，这是不必要的浪费。

## Goal

让 daemon-lifecycle 装配脱离 `wire_dependencies`,直接接受 GUI shell 已装好的
`AppDeps` / `BackgroundRuntimeDeps` 作为输入。具体目标：

- **进程内只有一份 `AppDeps`** —— sqlite pool / repos / settings repo /
  secure storage / blob store / clipboard write coordinator /
  mobile_sync_endpoint_info adapter 全是进程级一次性资源。
- **daemon reload 不重建 sqlite pool** —— reload 前后 `Pool` 实例地址稳定，
  插探针验证。
- **`WireOverrides` 整体从代码库中消失** —— `grep -r WireOverrides src-tauri`
  零命中。
- **standalone daemon binary 仍可独立运行** —— `uc_desktop::daemon::run` 入口
  自己装一份进程级 deps + facade，然后跑 daemon-lifecycle。

## 核心思路

当前 daemon 装配链 (问题):

```
build_daemon_bootstrap_assembly(WireOverrides)
  └─ build_daemon_app(WireOverrides)              ← daemon 端 wire 第二次
       └─ build_core(_, WireOverrides)
            └─ wire_dependencies_with_overrides   ← 创建第二份 sqlite pool/repos
```

目标装配链 (修复后):

```
build_daemon_bootstrap_assembly(WiredDependencies, BackgroundRuntimeDeps, ...)
  └─ build_daemon_lifecycle(已有 deps + 配置)     ← 只装 daemon-lifecycle 资源
       ├─ space_setup_assembly (绑 iroh)
       ├─ blob_transfer_facade
       └─ ...
```

`build_daemon_app` 拆成两半：进程级一次性的部分由 GUI shell 在 `build_gui_app`
中跑掉，daemon 只装 lifecycle 资源 (iroh node / space_setup / blob 等)。

## 已完成的阶段

(本会话从零起步，无已完成阶段)

## 待办阶段

### Phase A — 拆 build_daemon_app + 上提 background workers

**Status**: 🔲 todo

**问题**: `uc-bootstrap/src/builders.rs::build_daemon_app` 当前耦合三件事：
1. 进程级 deps 装配 (`build_core` → `wire_dependencies_with_overrides`) ——
   sqlite pool / repos / settings / secure storage 等
2. daemon-lifecycle 装配 —— iroh node bind / `build_space_setup_assembly` /
   `init::reconcile_*`
3. (隐式) `BackgroundRuntimeDeps` 装配 —— 含 spool_rx / worker_rx receiver,
   被 `spawn_daemon_background_tasks` (在 `start_in_process` 里) 消费。
   语义上是进程级 long-lived，代码物理上挂在 daemon 启动里。

**目标**: 拆成三块，职责清晰：

- `build_process_runtime() -> ProcessRuntimeContext` —— 装 1+3 输入素材，
  返回 `WiredDependencies` (含 `BackgroundRuntimeDeps` receivers) +
  `storage_paths`。GUI shell 与 standalone daemon 都用。
  (即把当前 `build_gui_app` / `GuiBootstrapContext` 一并 rename —— 见决策记录)
- shell 自己跑 `spawn_blob_processing_tasks(ctx.background, blob_ports,
  ctx.task_registry)` —— 一次性，挂在进程级 task_registry。
- `build_daemon_lifecycle(deps, storage_paths, config) -> DaemonLifecycle`
  —— 跑 2，接受已有 deps 作输入，只装 daemon-lifecycle 资源。

**实施要点**:

- `WiredDependencies` 已是 public type，可以直接作输入。
- `init::reconcile_peer_addresses` / `reconcile_trusted_peers` 当前在
  `build_daemon_app` 里跑，要搬到 `build_daemon_lifecycle`(daemon 启动时
  reconcile，不是进程启动时)。
- `spawn_blob_processing_tasks` 从 `daemon::start_in_process` 移到 GUI shell
  setup 阶段 (`uc-tauri/src/run.rs::run`) 与 standalone binary
  (`uc-desktop/src/daemon/host.rs::run`) 各自跑一次。daemon
  `start_in_process` 不再持 `BackgroundRuntimeDeps` / `BlobProcessingPorts`。
- task_registry 归属：`runtime.task_registry()` 是进程级，blob/spool worker 挂
  在这上面。daemon 内的 worker (clipboard sync / presence / keepalive) 仍挂
  在 daemon 自己的 cancel scope 上 (`DaemonHandle.cancel`),不影响。
- 验证：standalone daemon binary (`uc_desktop::daemon::run`) 走"先
  `build_process_runtime` → spawn blob workers → 后 `build_daemon_lifecycle`"
  三步;in-process 路径 GUI shell `run()` 同样三步。

### Phase B — daemon_probe / daemon::host 接受已有 deps

**Status**: 🔲 todo

**改动**:

- `uc-desktop/src/daemon/bootstrap.rs::build_daemon_bootstrap_assembly` 签名
  从 `(WireOverrides) -> ...` 改为 `(WiredDependencies, BackgroundRuntimeDeps,
  AppPaths, ...) -> ...`。
- `uc-desktop/src/daemon/host.rs::start_in_process` 在 `app_facade` 之外再加
  `wired_deps: WiredDependencies` 入参 (或者从 `app_facade` 反查 deps —
  待决)。
- `uc-desktop/src/daemon/host.rs::run` (standalone 入口) 自己 `build_process_runtime`
  得到 deps，装 facade，然后跑 daemon-lifecycle。
- `uc-desktop/src/daemon_probe.rs::bootstrap_daemon_in_process` /
  `start_owned_in_process` / `reload_in_process_daemon` 全部改签名：
  从透传 `WireOverrides` 改为透传"已有 deps 句柄"。
- `uc-tauri/src/run.rs` daemon spawn 路径 / `commands/restart.rs` reload 路径
  跟着改。

**关键决策点**: `start_in_process` 入参是直接收 `WiredDependencies` 还是
让 `AppFacade` / `DesktopRuntime` 持有一个 deps handle 反查？倾向方案 A (显
式传 deps),让数据流清晰、不引入隐式依赖。

### Phase C — 删除 WireOverrides 机制

**Status**: 🔲 todo

**前提**: Phase A + B 完成，daemon 不再调 `wire_dependencies` —— 此时
mobile_sync_endpoint_info Arc 自然只有一份 (从 `WiredDependencies` 出来),
不需要 caller 注入。

**改动**:

- `uc-bootstrap/src/assembly.rs`:
  - 删除 `WireOverrides` struct
  - `wire_dependencies_with_overrides` 函数体合并回 `wire_dependencies`
  - `create_infra_layer` 删除 `mobile_sync_endpoint_info_override` 参数
- `uc-bootstrap/src/builders.rs`:
  - `build_core` 删除 `wire_overrides` 参数
  - `build_daemon_app`(若 Phase A 后还存在) 删除 `wire_overrides` 参数
- `uc-bootstrap/src/lib.rs` 删除 `WireOverrides` /
  `wire_dependencies_with_overrides` re-export
- `uc-desktop/src/bootstrap.rs::build_gui_app`:
  - 不再创建 `WireOverrides`,`wire_dependencies` 内部 new endpoint_info Arc 即可
  - `GuiBootstrapContext.mobile_sync_endpoint_info` 字段从 deps 反查 (它
    已经在 `wired.deps` 里),或者直接删除——deps 自身就是 SoT
- `uc-desktop/src/daemon/bootstrap.rs` / `host.rs` / `daemon_probe.rs` 删除
  全部 `WireOverrides` 引用与透传
- `uc-tauri/src/run.rs` 删除 `WireOverrides` import / 创建 / `.manage`
  注册 / daemon spawn 透传
- `uc-tauri/src/commands/restart.rs` 删除 `WireOverrides` import / 创建

**验证**:

```bash
grep -r WireOverrides src-tauri  # 必须零命中
grep -r wire_dependencies_with_overrides src-tauri  # 必须零命中
```

### Phase D — 验证 daemon reload 不重建 deps

**Status**: 🔲 todo

**目标**: 钉死"daemon reload 复用进程级 deps"这条契约。

**手段** (二选一):

- **方案 A (轻量)**: 在 `build_gui_app` 里把 `Arc<sqlx::SqlitePool>` 地址
  log 出来 (debug! target = "deps_lifecycle"),`reload_in_process_daemon`
  内部再 log 一次，manual 验证地址相同。
- **方案 B (强契约)**: 写集成测试 `daemon_reload_reuses_pool.rs`,断言
  `start_in_process` → `reload_in_process_daemon` 前后 `pool.size()` /
  pool 内部地址保持。

倾向方案 B (有回归保护)。

### Phase E — 测试与回归

**Status**: 🔲 todo

- `cargo test --workspace`
- `pnpm exec vitest run` (前端)
- 手动复现 mobile_sync 改设置 → 重启 daemon → UI 自动恢复
- 手动验证 standalone `uniclipboard-daemon` 二进制可独立启动
- 手动验证 lan_listener_error 在 LAN 端口被占用时端到端可见

## 验收标准

- [ ] `grep -r WireOverrides src-tauri` 零命中
- [ ] `grep -r wire_dependencies_with_overrides src-tauri` 零命中
- [ ] `build_daemon_app` 不再调用 `build_core`(被拆为 `build_process_runtime`
  + `build_daemon_lifecycle`)
- [ ] `daemon::start_in_process` 不再持有 `BackgroundRuntimeDeps` /
  `BlobProcessingPorts` —— blob/spool worker 在 GUI shell `run()` 与
  standalone `daemon::run` 各自跑一次
- [ ] daemon reload 前后 sqlite pool 地址稳定 (探针 / 测试钉死)
- [ ] standalone `uniclipboard-daemon` 二进制能独立启动并响应健康检查
- [ ] `cargo test --workspace` 干净通过
- [ ] `pnpm exec vitest run` 干净通过
- [ ] mobile_sync 重启路径手动复现成功 (零回归)
- [ ] lan_listener_error 端到端可见

## 决策记录

| 时间 | 决策 | 理由 |
|------|------|------|
| 2026-05-10 | Phase 4 下半场独立会话/PR 处理 | 上次会话已落地 AppFacade 单例化 (commit 940aa83f),范围已大;deps 共享是独立目标 |
| 2026-05-10 | 选择"拆 build_daemon_app"而非"daemon 自己 wire 但接受 endpoint_info override" | 后者只是把 WireOverrides 改成更复杂的结构体，治标不治本;前者直接消除"两份 deps"这个根因 |
| 2026-05-10 | `build_gui_app` → `build_process_runtime`,`GuiBootstrapContext` → `ProcessRuntimeContext` | 当前名字带 "GUI" 但 standalone binary (`daemon::run`) 也在调它，`host.rs:51-52` 自己加了注释解释这个不一致——需要注释解释的命名是命名错误的信号;重命名后与 Phase A 拆出的 `build_daemon_lifecycle` 对仗工整 (进程级 vs daemon 启停级);Phase A/B/C 反正要改调用链每个 caller 签名，顺手 rename 零额外成本 |
| 2026-05-10 | 路线 X: `spawn_blob_processing_tasks` 一并从 daemon 上提到进程级 | 实施前查证发现 `BackgroundRuntimeDeps` 含 `mpsc::Receiver` 不能 clone，如果只拆 AppDeps 而 daemon 仍持 `BackgroundRuntimeDeps`,daemon reload 拿不到 receiver(已被消费)。路线 Y(加"已 spawn flag" 让 reload 跳过) 是新代偿，与"消除 WireOverrides 代偿"精神冲突;路线 X 一刀到位，见 findings.md §6 Phase A 修正后的拆分范围 |

## 风险与未决问题

- **standalone binary 路径**: `daemon::run` 当前调 `build_gui_app`(即便没
  GUI 也用这个名字装进程级 runtime)。Phase A 已决定 rename 为
  `build_process_runtime` + `ProcessRuntimeContext`(见决策记录),并同步
  改 `uc-desktop/src/bootstrap.rs` module doc 措辞从"GUI shell 启动"换成
  "进程级运行时装配，GUI shell 与 standalone binary 共用"。
- **`init::reconcile_*` 时机**: 当前在 `build_daemon_app` 内调用，意味"每次
  daemon 启动都跑一次 reconcile"。拆分后要保持这个语义 —— reconcile 应该跟
  daemon-lifecycle 走，不是跟进程启动走。
- **WiredDependencies 移交所有权**: 当前 `WiredDependencies.deps` 已经被
  `build_gui_app` 消费成 `AppDeps`,想再传给 daemon 装配需要 deps 是 `Arc`
  而不是 owned。检查 `AppDeps` 字段是否都已经 Arc-wrapped(预期 yes，但要确认)。

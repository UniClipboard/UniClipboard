# findings — Phase 4 下半场调查

## 0. 2026-05-11 重大发现:BIND_LOCK 撞墙 → Phase 4 上半场架构根本性妥协

### 0.1 现场

Phase A/B/C 落地后，手动复现 mobile_sync 重启路径：
- 启动 dev → 点击 "Restart" → panic
- 位置：`crates/uc-infra/src/network/iroh/node.rs:489`
- 信息:`IrohNodeBuilder::bind called more than once in the same process —
  runtime hot-swap of LAN-only Mode is explicitly out of scope (Phase 94 / Pitfall 3)`
- 调用栈：`restart_daemon` → `reload_in_process_daemon` → `start_owned_in_process`
  → `start_in_process` → `build_daemon_bootstrap_assembly` → `build_daemon_lifecycle`
  → `build_space_setup_assembly` → `IrohNodeBuilder::bind` → BANG

### 0.2 根因 (超出 Phase 4 下半场原定范围)

iroh 在 `node.rs:452-489` 用 `static BIND_LOCK: OnceLock<()>` 强制**同进程
只能 bind 一次**。这是 Pitfall 3 的结构性防御 —— `.planning/research/PITFALLS.md`
明确写：

> "`IrohNode` 的 lifecycle 仍由 `uc-bootstrap` 单点拥有，进程内重启路径 **不存在**"
> "`UpdateNetworkSettings` 入口必须返回 `restart_required: bool`"
> "在 `uc-bootstrap` 增加 `assert!()`:进程启动后只能 `bind` 一次 (用 `OnceCell` 强制)"

### 0.3 Phase 4 上半场的隐藏冲突

上半场 task_plan (`260510-daemon-reload-arch/task_plan.md`) "目标分层" 一段：

```
daemon-lifecycle(每次 daemon start/stop 重建):
├─ iroh node + space_setup_assembly(绑 iroh_config)  ← 这里
```

把 iroh 划入 daemon-lifecycle。但 BIND_LOCK 实际禁止"daemon start/stop 时重建
iroh"。这两者从根上不兼容。

上半场没暴露这个冲突，原因：
- 上半场实际测试主要走 P0 panic 修复 + restart_app(进程级)→ restart_daemon
  (in-process) 切换路径，**没有真的复现一次完整 daemon reload 流程**
- restart_daemon 命令引入 (commit 3fa73b8e) 后，只要触发，必撞 BIND_LOCK
- dev 测试可能因为 `test-util` feature 跳过 BIND_LOCK 而看不到 panic,prod
  build 才暴露

### 0.4 三个心智模型的两两冲突

| 心智模型 | 物理约束 |
|---|---|
| daemon = 网络服务，包含 iroh | iroh 进程级单次 bind (Pitfall 3) |
| 重启 daemon = 重启网络栈 | 同进程不允许重 bind |
| in-process daemon = daemon 跑在 GUI 进程里 | GUI 进程不退出，iroh 就不能换 |

三者两两不冲突，凑齐就冲突。Phase 4 上半场选了"in-process daemon",于是与
另外两条冲突，**只能割裂处理**。

### 0.5 用户判断 (本次)

> "但是 daemon 不就包含了 iroh 本身吗，那这样不就非常割裂吗，
>  重启 daemon 不应该包含 iroh 的重启吗"

直觉对。命名"daemon"暗示了网络服务，iroh 不在里面反直觉。

### 0.6 三条方案对比

| 选项 | 概念一致性 | 工作量 | 代价 |
|---|---|---|---|
| **A. 接受割裂** (OnceCell 缓存 iroh，前端区分 restart_app / restart_daemon) | 差 — "daemon 不含 iroh" 反直觉 | 小 | 长期文档/前端要解释这个分裂 |
| **B. 放弃 in-process，回到独立 daemon binary** | 好 — "重启 daemon = 重启 binary",iroh 自然跟着重启 | 大 — 要解决新旧 daemon 端口冲突 (graceful wait)+ IPC 通信 + daemon 进程协调 | 重做 Phase 4 上半场的核心架构 |
| **C. 取消 `restart_daemon`,所有重启走 `restart_app`(进程级)** | 好 — "重启 = 进程重启，语义干净" | 中 — 需要修好 `app.restart()` 路径下的端口冲突 + graceful wait | 用户体验有 GUI 重启那一瞬间的视觉跳跃 |

**用户决策**: 选 C。理由：
- restart_daemon 是 in-process 模型下的妥协代偿，本次 BIND_LOCK panic 就是
  这个妥协的代价
- daemon 概念应该完整 (含 iroh),重启 daemon = 重启整个网络栈
- 进程重启的视觉跳跃是可以接受的代价，换来概念清晰



## 1. 当前 WireOverrides 散落点 (源代码 grep, 截至 2026-05-10)

| 文件 | 行号 | 用途 |
|---|---|---|
| `uc-bootstrap/src/assembly.rs` | 339 | struct 定义 |
| `uc-bootstrap/src/assembly.rs` | 465 | 引用文档 |
| `uc-bootstrap/src/assembly.rs` | 739 | `wire_dependencies` 调 `wire_dependencies_with_overrides(_, default)` |
| `uc-bootstrap/src/assembly.rs` | 743-748 | `wire_dependencies_with_overrides` 入口 |
| `uc-bootstrap/src/builders.rs` | 25 | use import |
| `uc-bootstrap/src/builders.rs` | 75 | `build_core` 入参 |
| `uc-bootstrap/src/builders.rs` | 114 | `build_cli_context_with_profile` 走 default |
| `uc-bootstrap/src/builders.rs` | 133 | `build_slice1_cli_context` 走 default |
| `uc-bootstrap/src/builders.rs` | 146 | `build_daemon_app` 入参 |
| `uc-bootstrap/src/lib.rs` | 25 | re-export |
| `uc-desktop/src/bootstrap.rs` | 18, 32, 59, 61 | `build_gui_app` 创建并注入 endpoint_info |
| `uc-desktop/src/daemon/bootstrap.rs` | 12, 41-46 | `build_daemon_bootstrap_assembly` 入参 |
| `uc-desktop/src/daemon/host.rs` | 28, 71-72, 107 | `start_in_process` 入参 / `run` 走 default |
| `uc-desktop/src/daemon_probe.rs` | 27, 187, 283, 324 | bootstrap / start_owned / reload 三个入口 |
| `uc-tauri/src/run.rs` | 22, 247 | use + daemon spawn 透传 |
| `uc-tauri/src/commands/restart.rs` | 21, 181 | use + reload 透传 |

**总计**: 9 个文件、约 22 处引用。删除全部需要同步改 5 层调用栈的签名。

## 2. 当前装配链 (问题图谱)

```
进程启动:
  uc-tauri::run::run
    ├─ build_gui_app()
    │   └─ wire_dependencies_with_overrides(_, WireOverrides{ms_eis: Arc#A})
    │        ├─ create_infra_layer(_, _, _, _, _, Some(Arc#A))
    │        └─ → WiredDependencies{ deps_A, background_A, ... }
    ├─ DesktopRuntime::with_setup(deps_A, ...)  → AppFacade_A (进程级单例 ✅)
    └─ .manage(Arc#A)    ← Phase 4 后会删除

daemon 启动 (in-process):
  uc-tauri::run 取 .state::<Arc#A>()  ← 这层取出来再透传 5 层
    └─ bootstrap_daemon_in_process(WireOverrides{ms_eis: Arc#A})
         └─ start_owned_in_process(WireOverrides{ms_eis: Arc#A})
              └─ start_in_process(_, app_facade, WireOverrides{ms_eis: Arc#A})
                   └─ build_daemon_bootstrap_assembly(WireOverrides{ms_eis: Arc#A})
                        └─ build_daemon_app(WireOverrides{ms_eis: Arc#A})  ← 问题！
                             └─ build_core(_, WireOverrides{ms_eis: Arc#A})
                                  └─ wire_dependencies_with_overrides(_, ...)
                                       └─ → WiredDependencies{
                                                deps_B,           ← 重建 sqlite pool!
                                                background_B,     ← 重建 daemon-lifecycle (期望)
                                                ...
                                            }
                                       └─ create_infra_layer 用 Arc#A 而非 new()  ✅ 共享生效
```

**症状**: `deps_A` 与 `deps_B` 共用 sqlite 文件 (WAL 多 pool 兼容，运行没问题),
但内存中是两份独立 pool / repos。daemon reload 重建 `deps_B` —— 不必要的浪费，
也是 `WireOverrides` 透传 5 层的根源 (Arc#A 必须穿过这条链才能避免被 new 一份)。

## 3. 目标装配链

```
进程启动:
  uc-tauri::run::run
    ├─ build_process_runtime()  (新)
    │   └─ wire_dependencies(&config)
    │        └─ → WiredDependencies{ deps, background, ... }   (唯一一份)
    ├─ DesktopRuntime::with_setup(deps.clone(), ...)  → AppFacade (进程级单例)
    └─ (deps + background 留在 GuiBootstrapContext / DesktopRuntime 里)

daemon 启动 (in-process):
  uc-tauri::run 直接调 (无 WireOverrides 透传):
    bootstrap_daemon_in_process(_, app_facade, deps_handle)
      └─ start_owned_in_process(_, app_facade, deps_handle)
           └─ start_in_process(run_mode, app_facade, deps_handle)
                └─ build_daemon_bootstrap_assembly(deps, background, storage_paths, config)
                     └─ build_daemon_lifecycle(deps, ...)   (新, 不调 wire)
                          ├─ build_iroh_node_with_config
                          ├─ build_space_setup_assembly
                          ├─ init::reconcile_peer_addresses (move 进来)
                          └─ → DaemonLifecycle { space_setup, blob, ... }
```

`WiredDependencies` / `AppDeps` 的字段都已 `Arc`-wrapped —— 把 deps 当 handle
传入 `start_in_process` 让 daemon-lifecycle 装配读它即可，不需要再 wire。

## 4. 关键源代码锚点

### 必须改的核心函数

```
uc-bootstrap/src/builders.rs:73    build_core (拆: 进程级 vs daemon-lifecycle)
uc-bootstrap/src/builders.rs:145   build_daemon_app (拆出 build_daemon_lifecycle)
uc-bootstrap/src/assembly.rs:339   WireOverrides struct (删)
uc-bootstrap/src/assembly.rs:743   wire_dependencies_with_overrides (合并回 wire_dependencies)
uc-desktop/src/bootstrap.rs:48     build_gui_app (走纯 wire_dependencies)
uc-desktop/src/daemon/bootstrap.rs:45  build_daemon_bootstrap_assembly (签名换)
uc-desktop/src/daemon/host.rs:104  start_in_process (签名加 deps,删 WireOverrides)
uc-desktop/src/daemon/host.rs:53   run (standalone) (调用面跟着改)
uc-desktop/src/daemon_probe.rs:187 bootstrap_daemon_in_process (签名换)
uc-desktop/src/daemon_probe.rs:283 start_owned_in_process (签名换)
uc-desktop/src/daemon_probe.rs:324 reload_in_process_daemon (签名换)
uc-tauri/src/run.rs:124-242        daemon spawn 路径 (删 .manage / 改透传)
uc-tauri/src/commands/restart.rs:181  reload 调用面
```

### 不能动的契约 (要确认仍守住)

- `wire_dependencies` 内部 new 一份 `mobile_sync_endpoint_info` Arc —— 删除
  WireOverrides 后，这是 SoT。daemon LAN listener 与 GUI facade 都通过
  `AppDeps.mobile_sync.endpoint_info` 读它 (已经是这样)。
- `init::reconcile_*` 必须在 daemon 启动时跑一次 (而非进程启动时跑一次) —— 当前
  `build_daemon_app` 里的位置语义正确，搬到 `build_daemon_lifecycle` 即可。
- standalone daemon binary `uc_desktop::daemon::run` 必须仍能独立启动 ——
  没有 GUI shell 时，自己装一份进程级 runtime + facade。
- `WAL` 模式下多 pool 当前没问题，但拆完之后只剩一份 pool，这个隐式契约
  自然消失 (好事)。

## 5. 字段归属审计 (2026-05-10 实施前查证)

### Q1: `WiredDependencies` / `AppDeps` 字段是否全部 Arc 化？

**结论**: ✅ 关键状态字段全部 `Arc<dyn Port>` 或 `Arc<具体类型>`。
- `AppDeps` (`uc-application/src/deps.rs:138`) — 全是 `Arc<dyn Port>` 或
  sub-port struct (内部 Arc),**整个 struct 可以通过 `Arc<AppDeps>` 在
  GUI / daemon 之间共享，或者 `AppDeps` 本身就因内部全 Arc 而 cheap-clone**
- `WiredDependencies` (`uc-bootstrap/src/assembly.rs:139`):
  - `deps: AppDeps` ✅
  - `emitter_cell: Arc<RwLock<...>>` ✅
  - `trusted_peer_repo` / `peer_addr_repo` / `blob_reference_repo` /
    `migration_state` / `key_migration` / `blob_migration_repo` /
    `mobile_sync_endpoint_info` 全 `Arc<dyn ...>` ✅
  - `iroh_blob_store_dir` / `iroh_identity_dir`: `PathBuf` (Clone, ok)
  - `background: BackgroundRuntimeDeps` ⚠️ 见 Q2

### Q2: `BackgroundRuntimeDeps` —— 关键约束 ⚠️

**结论**: 这个 struct **不是** Clone-able 的，含两个一次性 `mpsc::Receiver`:
- `spool_rx: mpsc::Receiver<SpoolRequest>` ❌
- `worker_rx: mpsc::Receiver<RepresentationId>` ❌

其余字段都是 Arc / Sender / 数值常量，可以共享或 clone。

**消费路径**: `spawn_blob_processing_tasks`
(`uc-bootstrap/src/background_tasks.rs:70-160`) **解构整个
`BackgroundRuntimeDeps`**, 把 `spool_rx` / `worker_rx` move 进
`SpoolerTask` / `BackgroundBlobWorker` long-lived 任务。一旦 spawn,
这两个 receiver 就消失了。

**当前装配链 (问题)**:
```
build_daemon_app
  └─ wire_dependencies → BackgroundRuntimeDeps (含两个 receiver)
       └─ start_in_process
            └─ spawn_daemon_background_tasks(background, ...)
                 └─ spawn_blob_processing_tasks  (消费 receivers)
```

意味着 **daemon 启动消费 receivers**。所以 daemon reload 现在能跑 (因为
每次 reload 都重新 wire 一份 fresh receivers 出来),但拆掉 daemon wire
之后 reload 路径就没 receiver 可用。

**这是一个超出"删 WireOverrides"原意的隐藏错位**: spool / blob worker
在语义上是 **进程级 long-lived background task** (跟 sqlite pool 同级),
不是 daemon-lifecycle —— 它们消费的是磁盘 spool 文件 / 内存 cache，跟
daemon iroh / HTTP 没关系。但代码把它们装在 daemon 启动里跑。

### Q3: 各组件归属重新分类

| 组件 | 当前位置 | 正确归属 | 备注 |
|---|---|---|---|
| `AppDeps` (sqlite pool / repos / settings / secure storage) | daemon 启动重做 | 进程级 | 本次 PR 移上去 |
| `representation_cache` / `spool_manager` | daemon 启动重做 | 进程级 | 跟 spool worker 一起 |
| `spool_rx` / `worker_rx` 受体 | daemon 启动消费 | 进程级 | 配套 spool/blob worker |
| `file_transfer_lifecycle` | daemon 启动 | 进程级 (Arc 字段是) | 注:lifecycle 内部 sweep/reconcile 任务可能是 daemon-lifecycle，要细看 |
| `clipboard_write_coordinator` | daemon 启动 | 进程级 (GUI command 也用) | run.rs:143 GUI 已经在用 |
| `mobile_sync_endpoint_info` | daemon wire 时 override 共享 | 进程级 (wire 内部 new 一份即可) | 删 WireOverrides 后由 wire 内部产生 |
| `space_setup_assembly` (iroh node) | daemon 启动 | daemon-lifecycle ✅ | abort iroh + drop assembly |
| `clipboard_sync_facade` / `blob_transfer_facade` | daemon 启动 | daemon-lifecycle ✅ | 已 swap 进 AppFacade |
| `mobile_sync_apply_inbound` (lifecycle facade) | daemon 启动 | daemon-lifecycle ✅ | 已 swap 进 AppFacade |
| HTTP server / LAN listener / iroh node bind | daemon 启动 | daemon-lifecycle ✅ | 当前正确 |
| PID 文件 / auth token / DaemonHandle | daemon 启动 | daemon-lifecycle ✅ | 当前正确 |
| `init::reconcile_*` | daemon 启动 (目前在 build_daemon_app 里) | daemon-lifecycle ✅ | 时机不变，物理上从 builders.rs 搬到 build_daemon_lifecycle |

### Q4: `init::reconcile_*` 时机平移

**结论**: ✅ 平移没问题。当前在 `build_daemon_app` 里 (每次 daemon 启动跑
一次),目标位置 `build_daemon_lifecycle` 也是"daemon 启动时"语义，等价。
但 **调用所需的 ports** (`member_repo`, `peer_addr_repo`, `trusted_peer_repo`)
都来自 `WiredDependencies` —— 拆分后这些 Arc 要从已有的 `WiredDependencies`
入参里读，不能从一个新建的 wired 里读。

## 6. Phase A 修正后的拆分范围

基于 Q2 的发现，原方案"`build_daemon_app` 拆成 `build_process_runtime` +
`build_daemon_lifecycle`" 仍然成立，但要把"装 background workers"明确
归到进程级：

### 修正后的目标分层

```
build_process_runtime() -> ProcessRuntimeContext
  ├─ tracing init / panic hook (idempotent)
  ├─ wire_dependencies → WiredDependencies
  │    ├─ AppDeps (sqlite pool / repos / settings / secure / mobile_sync_endpoint_info)
  │    └─ BackgroundRuntimeDeps (含两个 receiver)
  ├─ get_storage_paths
  └─ → ProcessRuntimeContext { deps, background, storage_paths, config, emitter_cell, ... }

GUI shell run() / standalone daemon run():
  ├─ ctx = build_process_runtime()
  ├─ runtime = TauriAppRuntime/DesktopRuntime::with_setup(ctx.deps, ...)
  ├─ spawn_blob_processing_tasks(ctx.background, ...) ← receivers 在这里被消费 (一次性)
  │    spawn 进进程级 task_registry (来自 runtime), 不绑 daemon-lifecycle
  └─ start_in_process(run_mode, app_facade, deps_handle, ...) ← daemon-lifecycle

build_daemon_lifecycle(deps, storage_paths, config) -> DaemonLifecycle
  ├─ 读 settings (network policy)
  ├─ 用现有 deps 内的 member_repo/peer_addr/trusted_peer 跑 reconcile
  ├─ build_iroh_node + build_space_setup_assembly
  ├─ build_daemon_lifecycle_facades (5 个子 facade)
  └─ → DaemonLifecycle { space_setup, blob_transfer_facade, mobile_sync_facade, ... }
```

### 关键边界变化

- **spool / blob worker spawn** 从 `daemon::start_in_process` 移到
  GUI shell `run()` 的 setup 阶段 (与 standalone binary `daemon::run`
  一致，标记进程级)
- daemon `start_in_process` 不再持有 `BackgroundRuntimeDeps` ——
  `BlobProcessingPorts` 也不需要在这里组装
- `daemon::bootstrap::DaemonBootstrapAssembly` 字段瘦身：移除
  `non_gui_bundle.task_registry` (改用进程级 task_registry) /
  `clipboard_write_coordinator` (改从 ctx 入参) 等

### 风险

1. **task_registry 归属切换**: 当前 daemon 用 `non_gui_bundle.task_registry`,
   GUI 用 `runtime.task_registry()` —— 两个 registry 各自独立。改完后
   blob/spool worker 必须挂在进程级 registry。要确认 daemon 内的其他
   worker (clipboard sync / presence / keepalive) **不依赖** 这两条
   blob/spool worker 的 cancel token 顺序。
2. **standalone binary 路径**: `uniclipboard-daemon` 二进制无 GUI shell,
   `build_process_runtime` 后还要自己装 task_registry + spawn workers,
   再跑 daemon-lifecycle。`daemon::run` 入口要重写。

## 7. 上次会话遗留参考

详见 `.planning/quick/260510-daemon-reload-arch/` 三件套。重点：
- `findings.md §3` 真正的架构错位
- `findings.md §4` WireOverrides 是症状而非病因
- `task_plan.md Phase 4` 验收标准

## 8. WireOverrides 删除后端到端的数据流 (设计验证)

## 6. 上次会话遗留参考

详见 `.planning/quick/260510-daemon-reload-arch/` 三件套。重点：
- `findings.md §3` 真正的架构错位
- `findings.md §4` WireOverrides 是症状而非病因
- `task_plan.md Phase 4` 验收标准

## 9. WireOverrides 删除后端到端的数据流 (设计验证)

mobile_sync_endpoint_info 写读路径，删除前 vs 删除后：

**删除前**:
```
daemon LAN listener.bind() OK
  → write to deps_B.mobile_sync.endpoint_info  ← Arc#A (wire 时被 override 注入)
  ← read from deps_A.mobile_sync.endpoint_info ← Arc#A (build_gui_app 创建)
GUI facade.mobile_sync.get_settings()
```

**删除后**:
```
daemon LAN listener.bind() OK
  → write to deps.mobile_sync.endpoint_info     ← Arc#X (wire 内部 new,唯一一份)
  ← read from deps.mobile_sync.endpoint_info    ← Arc#X (同一份)
GUI facade.mobile_sync.get_settings()
```

`deps` 在删除后只有一份，Arc 共享是结构性事实，不需要约定。

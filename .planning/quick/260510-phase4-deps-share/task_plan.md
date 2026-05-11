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

### Phase A — 拆 build_daemon_app + 上提 background workers

**Status**: ✅ complete (commits 181a504e / 9fa945b7 / 655d187a / 283cbc5a)

**落地内容**:

- `build_gui_app` → `build_process_runtime`,`GuiBootstrapContext` →
  `ProcessRuntimeContext` 重命名 (commit 181a504e)
- `BackgroundRuntimeDeps` 从 `WiredDependencies` 拆出来:wire 函数返回
  tuple `(WiredDependencies, BackgroundRuntimeDeps)`,持久 vs 一次性
  消费两类资源分离 (commit 9fa945b7)
- `AppDeps` 与 7 个 sub-port struct + `WiredDependencies` 全部
  `#[derive(Clone)]`,所有字段都是 Arc<dyn>/PathBuf,clone 廉价。新增
  `build_daemon_lifecycle(wired)` 函数 (commit 655d187a)
- daemon 路径主体改造 (commit 283cbc5a):
  - 新增 `ProcessRuntimeHandles { wired, storage_paths,
    clipboard_write_coordinator, file_transfer_lifecycle }`
  - `start_in_process` 入参 WireOverrides → ProcessRuntimeHandles
  - `build_daemon_bootstrap_assembly` 改用 build_daemon_lifecycle 而非
    build_daemon_app(daemon 不再 wire 自己的 deps)
  - `daemon::start_in_process` 不再 spawn_blob_processing_tasks ——
    blob/spool worker 由 caller 在进程启动期一次性 spawn
  - GUI shell `uc-tauri/src/run.rs::run` 编排:build_process_runtime →
    spawn blob workers (挂在 runtime task_registry) → bootstrap
    daemon (透传 ProcessRuntimeHandles)
  - standalone daemon binary `uc-desktop/src/daemon/host.rs::run` 同样
    编排
  - 删除 `daemon/background_tasks.rs` (无 caller)

### Phase B — daemon_probe / daemon::host 接受已有 deps

**Status**: ✅ complete (随 Phase A 一并落地，commit 283cbc5a)

`daemon_probe::bootstrap_daemon_in_process` /
`start_owned_in_process` / `reload_in_process_daemon` 三个入口的
`wire_overrides` 参数全部换成 `process_handles: ProcessRuntimeHandles`,
`uc-tauri/src/commands/restart.rs::restart_daemon` 也跟着改。

### Phase C — 删除 WireOverrides 机制

**Status**: ✅ complete (commit b53c7492 + 后续 doc cleanup)

- 删除 `WireOverrides` struct
- 删除 `wire_dependencies_with_overrides` 函数 (合并回 `wire_dependencies`)
- 删除 `create_infra_layer` 的 `mobile_sync_endpoint_info_override` 参数
- 删除 `build_daemon_app` 函数 + `DaemonBootstrapContext` struct
- 更新 lib.rs re-export
- 验证：`grep -r "WireOverrides\|wire_dependencies_with_overrides\|
  DaemonBootstrapContext"` 零命中

## 待办阶段

### Phase D / E — 已 retire

原 Phase D (daemon reload 不重建 deps 集成测试) 与 Phase E (相关回归)
基于"daemon reload 是合法路径"的假设。2026-05-11 实际复现踩到 BIND_LOCK,
确认 in-process daemon reload 与 iroh Pitfall 3 不兼容 (见 findings.md §0)。
用户决策采用方案 C(取消 in-process reload，所有重启走进程级),Phase D/E
不再适用。

### Phase F — 选项 C 实施：取消 in-process daemon reload，回到进程级重启

**Status**: 🔲 in_progress

**Goal**: 消除"daemon reload 但 iroh 不重启"的概念割裂，所有"需要重启"
设置都走 `app.restart()`(整进程重启)。daemon 概念恢复完整 —— "重启
daemon = 重启含 iroh 的整套网络栈 = 重启进程"。

**Phase A/B/C 保留**: 进程级 deps 共享 (WiredDependencies 跨 daemon 启停
复用) 的代码改造保留 —— 即便没有 in-process reload，这套架构也让 daemon
内部装配链路清晰，sqlite pool 不再被两份装配重复构造。

**待删除/退役**:

- **前端**: 把 restart_daemon 调用切回 restart_app
  - `src/components/setting/NetworkSection.tsx::handleRestart` (LAN-only Mode)
  - `src/components/device/MobileSyncSettingsSheet.tsx::handleRestart`
    (mobile_sync 设置)
  - `src/lib/daemon-ws-bootstrap.ts::registerDaemonRestartListener` 删除
    (没有 `app://daemon-restarting` / `app://daemon-ready` 事件了)
  - `src/main.tsx` 移除 listener 注册
  - `src/components/setting/__tests__/NetworkSection.test.tsx` 测试期望
    字符串改回 `restart_app`
- **后端**: 删除 in-process daemon reload 路径
  - `uc-tauri/src/commands/restart.rs::restart_daemon` 命令删除
  - `uc-tauri/src/commands/restart.rs::RestartDaemonError` 删除
  - `uc-tauri/src/run.rs` invoke_handler 移除 `restart_daemon` 注册
  - `uc-desktop/src/daemon_probe.rs::reload_in_process_daemon` 删除
  - `uc-desktop/src/daemon_probe.rs::ReloadInProcessDaemonError` 删除
  - `uc-desktop/src/daemon_probe.rs::start_owned_in_process` 改名为
    `start_in_process_at_bootstrap`(只在 GUI 启动期调一次),不再支持
    reload 复用

**保留**:

- Phase 1 P0 panic 修 (`http_handle_consumed` flag),它防御的是任意 HTTP
  server 早退场景，不局限于 reload
- `uc-webserver/tests/graceful_shutdown_port_reuse.rs` 契约测试保留 ——
  即便不走 in-process reload，这条契约对"daemon 优雅退出"仍有价值
- Phase A/B/C 的 deps 共享重构全部保留

**新增工作：app.restart() 路径的端口冲突修复**

`app.restart()` 触发 Tauri 启动新进程 + exit 当前进程。旧进程退出过程中
HTTP listener / LAN listener 还在持端口，新进程 bind 会撞
`WSAEADDRINUSE`。Phase 1 已修了"bind 失败 → panic" 的连带 panic，但 daemon
仍然起不来。需要：

1. **GUI exit 阶段触发 daemon graceful shutdown**: Tauri `RunEvent::Exit`
   或 `ExitRequested` 时，主动 `DaemonOwnership::take_owned()` + handle.shutdown
   等到端口释放再 exit。
2. **新进程启动期的 bind retry 兜底**: 即便 GUI exit 做了 shutdown，边界
   情况下新进程仍可能撞上端口未释放窗口。`run_http_server` /
   LAN listener bind 改为短 retry 循环 (几百 ms 重试 N 次) 而非一次性失败。

**验证**:

- 启动 dev → 点 mobile_sync restart → GUI 重启 → daemon 起来 → 不 panic
- 启动 dev → 切 LAN-only Mode → GUI 重启 → iroh 用新配置 bind → 不 panic
- `cargo test --workspace` 全绿
- `pnpm exec vitest run` 全绿

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
| 2026-05-11 | 方案 C: 取消 in-process daemon reload，所有重启走 `app.restart()` (进程级) | 实测 mobile_sync 重启触发 BIND_LOCK panic(iroh 同进程单次 bind,Pitfall 3 硬约束)。Phase 4 上半场把 iroh 划入 daemon-lifecycle 与 BIND_LOCK 根本性冲突。三选项对比 (findings.md §0.6):A(OnceCell 缓存接受割裂) 概念分裂、B(回独立 binary) 工作量大，C(进程级重启) 概念干净，代价只是 GUI 重启视觉跳跃。用户选 C —— daemon 概念应该完整 (含 iroh),"重启 daemon = 重启网络栈 = 重启进程"语义自洽 |

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

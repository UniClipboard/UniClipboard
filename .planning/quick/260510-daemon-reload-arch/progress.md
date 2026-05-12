# progress — 会话日志

## 2026-05-10

### 调查阶段

- 拉 Sentry issue UNICLIPBOARD-RUST-4 → 拿到完整 panic stack + 用户日志
  片段；定位到 `uc-desktop/src/daemon/app.rs` 同一个 `JoinHandle` 被
  poll 两次。
- 用户提供复现路径："改 mobile_sync 配置 → 点重启 → 崩溃"。沿因果链定位
  到根因：`app.restart()` 进程级重启 + in-process daemon 持有端口的
  组合 → 新进程 bind 撞 `WSAEADDRINUSE`。

### Phase 1 (P0): 修 panic

- 引入 `http_handle_consumed` flag，select! 命中 http 分支后置位；
  shutdown 阶段用 flag 守卫二次 `timeout(http_handle).await`。
- `cargo test -p uc-desktop --lib` 48 测试通过。

### Phase 2: 闸门评估

- **R1**（bootstrap 二次跑资源冲突）：tracing/panic hook 幂等、sqlite WAL
  支持多 pool、iroh 有显式 shutdown 序列、PID 文件有 Drop guard、HTTP
  listener graceful shutdown 干净 → 通过。
- **R5**（GUI AppFacade 是否依赖 daemon 内部 Arc）：完全独立 → 通过。
  副发现 `lan_listener_error` 字段在 GUI 路径下永远 None。
- **R2**（同进程 listener drop 后端口立即可复用）：写回归测试
  `uc-webserver/tests/graceful_shutdown_port_reuse.rs` → 通过。

### Phase 3 (P1): restart_daemon 命令 + 前端协调

- `uc-desktop/src/daemon_probe.rs` 新增 `reload_in_process_daemon()` +
  `ReloadInProcessDaemonError`。
- `uc-tauri/src/commands/restart.rs` 新增 `restart_daemon` Tauri 命令；
  保留 `restart_app` 作兜底。
- `uc-tauri/src/run.rs` invoke_handler 注册新命令。
- 前端 `daemon-ws-bootstrap.ts` 新增 `registerDaemonRestartListener`
  监听 `app://daemon-restarting` / `app://daemon-ready` 协调 WS 断重连。
- `main.tsx` 注册 listener；`NetworkSection.tsx` /
  `MobileSyncSettingsSheet.tsx` 切到 `restart_daemon` 调用。
- 测试更新：`NetworkSection.test.tsx` Test 6/7 改用 `restart_daemon`
  字符串。
- `cargo test -p uc-tauri --lib commands::restart` 4 测试通过；
  前端 `pnpm exec vitest run` 410 测试通过。

### Phase 4 (R5 副发现 follow-up): endpoint_info Arc 上提

- 引入 `WireOverrides` struct 让 caller 在 wire 之前注入预先建好的
  `Arc<InMemoryMobileSyncEndpointInfoAdapter>`。
- 沿 `wire_dependencies → build_daemon_app → start_in_process →
  build_daemon_bootstrap_assembly → bootstrap_daemon_in_process →
  start_owned_in_process → reload_in_process_daemon` 全链路加 overrides
  参数。
- `uc-tauri/src/run.rs` 创建 Arc 一次，`.manage()` 注册，daemon spawn
  与 restart_daemon 命令都从 state 取出 clone 透传。
- 全栈 `cargo build -p uc-tauri` 干净；`cargo test` uc-bootstrap +
  uc-desktop + uc-tauri + uc-webserver lib 共 126 测试通过。

### 架构反思（用户洞察介入）

- 用户提出："只要设计到比较繁琐的参数传输和复杂的状态管理，那一定是
  当前的架构不适用当前的业务，可以进行重构。"
- 重新审视：5 层透传 Optional Arc 是症状不是病因。真正的错位是
  in-process daemon 模型下进程内有两份 AppDeps + 两份 AppFacade，daemon
  "重启" 重做整套 wire 是不必要的浪费。
- 正确分层：进程级一次性资源（GUI 启动建一次）+ daemon-lifecycle 资源
  （每次 daemon start/stop 重建）。详见 `findings.md` §3-§4。
- 决策：本次会话不动手 Phase 4 重构，独立 PR / 独立 review。

### 文档归档（本步骤）

- 创建 `.planning/quick/260510-daemon-reload-arch/{task_plan,findings,progress}.md`
- 项目用 `.planning/quick/<date>-<short-name>/` 子目录约定，与 GSD 风格一致。

---

## 已修改文件清单

### Rust 后端

- `src-tauri/Cargo.lock`（依赖更新）
- `src-tauri/crates/uc-desktop/Cargo.toml` (新增 thiserror)
- `src-tauri/crates/uc-bootstrap/src/assembly.rs` (WireOverrides struct +
  create_infra_layer 加 override 参数 + wire_dependencies_with_overrides 入口)
- `src-tauri/crates/uc-bootstrap/src/builders.rs` (build_core /
  build_cli_context_with_profile / build_slice1_cli_context /
  build_daemon_app 接受 overrides)
- `src-tauri/crates/uc-bootstrap/src/lib.rs` (re-export)
- `src-tauri/crates/uc-desktop/src/bootstrap.rs` (build_gui_app 创建 Arc +
  GuiBootstrapContext 暴露字段)
- `src-tauri/crates/uc-desktop/src/daemon/app.rs` (P0 flag 守卫)
- `src-tauri/crates/uc-desktop/src/daemon/bootstrap.rs` (透传 overrides)
- `src-tauri/crates/uc-desktop/src/daemon/host.rs` (start_in_process 加
  overrides + run() 走 default)
- `src-tauri/crates/uc-desktop/src/daemon_probe.rs` (新增
  reload_in_process_daemon + ReloadInProcessDaemonError + 透传 overrides)
- `src-tauri/crates/uc-tauri/src/commands/restart.rs` (新增
  restart_daemon + RestartDaemonError + 4 测试)
- `src-tauri/crates/uc-tauri/src/run.rs` (解构 endpoint_info / .manage /
  daemon spawn 透传 / invoke_handler 注册)

### 前端

- `src/lib/daemon-ws-bootstrap.ts` (registerDaemonRestartListener +
  resetConnectionState)
- `src/main.tsx` (注册 daemon restart listener)
- `src/components/setting/NetworkSection.tsx` (handleRestart 切到
  restart_daemon + 撤销 pending/loading)
- `src/components/device/MobileSyncSettingsSheet.tsx` (切到 restart_daemon)
- `src/components/setting/__tests__/NetworkSection.test.tsx` (Test 6/7
  期望字符串改为 restart_daemon)

### 新增测试

- `src-tauri/crates/uc-webserver/tests/graceful_shutdown_port_reuse.rs`
  (R2 契约测试)

---

## 测试结果汇总（截至 Phase 3 完成）

| 测试套 | 结果 |
|---|---|
| `cargo test -p uc-desktop --lib` | 48 passed |
| `cargo test -p uc-tauri --lib commands::restart` | 4 passed |
| `cargo test -p uc-bootstrap -p uc-desktop -p uc-tauri -p uc-webserver --lib` | 126 passed |
| `cargo test -p uc-bootstrap --tests` (integration) | 17 passed across 4 binaries |
| `cargo test -p uc-webserver --test graceful_shutdown_port_reuse` | 1 passed |
| `pnpm exec vitest run` (前端全量) | 410 passed |

预先存在的失败（与本次改动无关，pristine 也挂）：

- `cargo test -p uc-tauri --doc` — `crate::run` 引用在 doctest 上下文不
  resolve；pristine main 上 2 failed，本次改动后 1 failed（实际减少了
  1 个）。是文档示例代码用 `crate::` 导致的环境配置问题。

---

## 错误记录

| 错误 | 第几次尝试 | 解决 |
|---|---|---|
| TaskCreate InputValidationError "tool's schema was not sent" | 1 | 使用 ToolSearch 加载 TaskCreate/TaskUpdate/TaskList 的 schema 后正常 |
| `cargo: could not find Cargo.toml in spot-capricorn` | 1 | 项目是 Tauri，Rust 工程在 `src-tauri/`；`cd src-tauri` 后正常 |
| rust-analyzer diagnostic stale | 多次 | 编辑后 rust-analyzer 跟不上；用 `cargo check` 验证真实状态 |
| `pnpm exec vitest run --reporter=basic` failed to load | 1 | basic 不是 vitest 内置 reporter；去掉 --reporter 参数走默认 |

---

---

## 2026-05-10 二轮会话：Phase 4 上半场

### 决策

- 用户选定 **激进方案**：把 AppFacade 中的 5 个 daemon-lifecycle 字段
  从 `Option<Arc<X>>` 改为 `Arc<arc_swap::ArcSwapOption<X>>`，daemon 启停 swap。
- 进程内只装一份 AppFacade（GUI 启动期装 / standalone CLI 入口装），
  daemon 启动 swap 子 facade，daemon 退出 clear。

### Phase 4 上半场（本次会话内落地）

- commit 9f627afc: AppFacade 字段重构。`space_setup` / `member_roster` /
  `clipboard_sync` / `blob_transfer` / `mobile_sync` 五字段改 ArcSwapOption,
  加 `swap_daemon_lifecycle` / `clear_daemon_lifecycle` API。调用面适配
  (`.as_ref()` / `.clone()` → `.load_full()` / `.load()`):
  uc-application app_facade.rs ~20 处 wrapper, uc-webserver routes 3 处，
  uc-cli mobile_sync 1 处，uc-tauri 1 处，uc-desktop daemon/app.rs 3 处。

- commit 940aa83f: daemon 不再装第二份完整 AppFacade。改造：
  - `daemon/app_facade_assembly.rs`: 重写为 `build_daemon_lifecycle_facades`
    返回 `DaemonLifecycleFacades`(5 个子 facade) + local_device_id。
    删除 `build_daemon_app_facade` / `DaemonAppFacadeAssembly` /
    `DaemonAppFacadeAssemblyInput`。
  - `daemon/host.rs::start_in_process`: 新增 `app_facade: Arc<AppFacade>`
    入参，装出 lifecycle facades 后调 `app_facade.swap_daemon_lifecycle(...)`,
    daemon main loop 退出后 `app_facade.clear_daemon_lifecycle()` 清空。
  - `daemon/host.rs::run` (standalone binary 入口): 自己 build_gui_app +
    DesktopRuntime::with_setup 装一份进程级 deps + facade，然后调
    start_in_process。
  - `daemon_probe.rs`: bootstrap_daemon_in_process / reload_in_process_daemon /
    start_owned_in_process 全部加 `app_facade` 参数。
  - `uc-tauri/src/run.rs` / `commands/restart.rs`: daemon spawn / reload
    透传 `runtime.app_facade()`。
  - `uc-application/src/facade/search/mod.rs`: SearchFacade.coordinator
    也走 ArcSwapOption,`set_coordinator` / `clear_coordinator` API。
  - `uc-bootstrap/src/non_gui_runtime.rs`: 抽出 `build_mobile_sync_facade`
    helper 供 daemon-lifecycle 装配复用，build_app_facade_from_deps 也改用。

### 验证

- `cargo check --workspace` 干净
- `cargo test -p uc-application -p uc-bootstrap -p uc-desktop -p uc-tauri
  -p uc-webserver --lib`: 539 passed (413+12+48+21+45)

### Phase 4 下半场（**未在本次会话落地**）

剩下两个目标涉及 `uc-bootstrap::build_daemon_app` 拆解 + WiredDependencies
跨 crate 流动，独立 PR 处理：

1. **删除 WireOverrides 整套机制**（含 5 处签名透传 + uc-tauri .manage 注册 +
   restart / bootstrap_daemon 路径透传）
2. **daemon reload 不重建 sqlite pool / repos / settings repo**
   —— 需要让 daemon-lifecycle 装配脱离 `wire_dependencies`，接受已有 deps 作输入

下次接手该读什么：
- `uc-bootstrap/src/builders.rs::build_daemon_app`（要拆解）
- `uc-bootstrap/src/assembly.rs::WiredDependencies`（要让 daemon 复用进程级实例）
- `uc-desktop/src/bootstrap.rs::build_gui_app`（要扩为持有 daemon-lifecycle
  装配所需的 wired 字段）
- `uc-desktop/src/daemon/bootstrap.rs::build_daemon_bootstrap_assembly`
  （要改为接受已有 deps + emitter_cell）

---

## 后续 follow-ups（下次接手该看什么）

1. **优先级最高**: 实施 task_plan.md 的 Phase 4 — 拆 daemon-lifecycle 与
   进程级资源分层，删除 WireOverrides 整套机制。这是对本次"症状治标"的
   根治方案。

2. **本次未处理**: standalone daemon binary (`run_mode = Standalone`)
   路径在 Phase 4 重构时要单独走通 —— 它没有 GUI 注入 facade，必须自己
   wire。

3. **本次未处理**: Tauri doctest 失败（pristine 已存在）。优先级低。

4. **观察项**: `restart_daemon` 命令在生产环境的实际成功率。Sentry 应当
   增加 `command.restart.restart_daemon` span 的 metrics（持续时间分布、
   ShutdownFailed/BootstrapFailed/ExternalDaemon 错误占比），用于评估
   reload 路径的健康度。

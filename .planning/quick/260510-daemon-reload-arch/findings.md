# findings — 调查与架构发现

## 1. Sentry panic 根因

**Issue**: `UNICLIPBOARD-RUST-4` — `panic: JoinHandle polled after completion`，
Windows 上 3 次重现，release `uc-bootstrap@0.7.0`。

**直接原因**: 同一个 `JoinHandle` 被 poll 了两次。

`uc-desktop/src/daemon/app.rs`：

- `app.rs:303` — `let mut http_handle = tokio::spawn(run_http_server(...))`
- `app.rs:417` — select! 分支 `result = &mut http_handle =>` 命中后，tokio
  内部 `try_read_output → Core::take_output` 把 `Stage::Finished(...)` 取走
  并替换为 `Stage::Consumed`
- `app.rs:461` — shutdown 阶段 `tokio::time::timeout(_, http_handle).await`
  再 poll → 看到 `Stage::Consumed` → `panic!("JoinHandle polled after completion")`

stack trace 完整对得上：`timeout::poll → JoinHandle::poll → try_read_output
→ Core::take_output → panic`。

**外因（用户日志证实）**:

```
11:24:30.951  HTTP server exited unexpectedly: Ok(Err(... os error 10048))
11:24:30.961  panic: JoinHandle polled after completion
```

`Ok(Err(...))` 嵌套语义：外层 Ok = JoinHandle 自身没 panic；内层 Err =
`run_http_server` 业务返回 Err（bind 失败）。Windows os error 10048 =
`WSAEADDRINUSE`，端口被占用。

**触发场景（用户复现确认）**: 用户改 mobile_sync 配置 → UI 提示需要重启
→ 点重启 → `restart_app` Tauri 命令调 `app.restart()` —— Tauri 这个 API
spawn 新的可执行文件 + exit 当前进程，**新旧进程会有短暂重叠期**：

- 旧 GUI 进程 = 旧 daemon = 旧 HTTP listener 仍持端口
- 新 GUI 进程已被 spawn，新 daemon 跑到 `run_http_server` 的
  `tokio::net::TcpListener::bind(addr).await?` 时端口仍被占着
- 普通 `bind`，没开 `SO_REUSEADDR` / `SO_EXCLUSIVEADDRUSE` → bind 立即失败
- task 主动 return Err → JoinHandle Ready → select! 命中 → break → shutdown
  阶段二次 await → panic

`Defensive unregister before registering global shortcut failed`（28.891）
是同一个根因（新旧进程重叠）的副产物 —— 全局快捷键是 OS 进程级资源，
老进程还没退出新进程注册前的 defensive unregister 当然失败。

## 2. 闸门评估（已完成的可行性确认）

### R1: bootstrap 二次跑的资源冲突

| 资源 | 二次启动行为 | 评估 |
|---|---|---|
| tracing subscriber | OnceLock 幂等 | ✅ |
| panic hook | OnceLock 幂等 | ✅ |
| sqlite pool | 每次 `init_db_pool()` 新建；WAL 模式天然支持多 pool 共存 | ✅ |
| iroh node | `SpaceSetupAssembly::shutdown()` (`space_setup.rs:113-123`) 显式做 abort ingest → abort progress translator → facade on_shutdown → `iroh_node.shutdown()`；run_loop.rs:62 退出前最后一步 | ✅ |
| HTTP listener (127.0.0.1:port) | `axum::serve.with_graceful_shutdown(cancel)` cancel 后 listener drop → 端口立即对同进程释放（R2 契约测试钉死） | ✅ |
| PID 文件 | `DaemonPidFileGuard::Drop`（`app.rs:489`）shutdown 时删除 | ✅ |
| mobile_sync endpoint_info Arc | 每次 wire 都 new；旧的随旧 facade drop | ✅ |

### R5: GUI AppFacade 是否依赖 daemon 内部 Arc

**结论**: ✅ GUI AppFacade 完全独立。证据链：

- `bootstrap.rs:48` GUI 启动 `wire_dependencies(&config)` → 一份独立 `AppDeps`
- `runtime.rs:88` `DesktopRuntime::with_setup` 用这份 deps 通过
  `build_app_facade_from_deps` 拼出 GUI AppFacade
- `assembly.rs:444` 每次 wire 都 `Arc::new(InMemoryMobileSyncEndpointInfoAdapter::new())`
- daemon `start_in_process` 内部又跑一次 wire，得到第二份独立 endpoint_info Arc

→ daemon reload 不会让 GUI commands 拿到的 facade 失效。但同时也暴露了
**lan_listener_error 副发现**：daemon 写 daemon 那份 Arc，GUI facade 读
GUI 那份 Arc，永远不通气。

### R2: 同进程 graceful shutdown 后端口立即可复用

**契约测试**: `uc-webserver/tests/graceful_shutdown_port_reuse.rs`

```
spawn axum + bind 127.0.0.1:0 → cancel → await serve task return →
立即在同一 SocketAddr rebind → 必须成功
```

测试通过。这是 P1 reload 路径核心假设的回归保护——任何未来给 server bind
加 `SO_EXCLUSIVEADDRUSE` 之类的改动会让本测试 panic。

## 3. 真正的架构错位（重大发现）

P1 + R5 落地后，用户提出关键反馈：**WireOverrides 透传链繁琐 → 这是
当前架构不适合业务的信号**。

重新审视：当前 P1 reload 做了 7 步：

```
1. 关 HTTP server
2. 关 services
3. 关 iroh node
4. 【重做 wire_dependencies】  ← 不必要
5. 重做 build_space_setup_assembly
6. 重启 services
7. 重启 HTTP server
```

第 4 步重做了 sqlite pool、所有 repos、settings repo、secure storage、blob
store、clipboard write coordinator、mobile_sync_endpoint_info adapter ……
这些都是 **进程级一次性资源**，跟 daemon 重启无关。

为什么会这样？因为 `wire_dependencies` 是个"自给自足"的 composition root，
它打包了"所有依赖" —— 要 daemon-lifecycle 资源就得连带把 deps 一起重做。

### 深层错位汇总

**错位 1**: in-process daemon 模型下进程内有两份 AppFacade，本来不该有

- GUI 端：`DesktopRuntime::with_setup` → `build_app_facade_from_deps`
- daemon 端：`build_daemon_app_facade` 在 `start_in_process` 内部
- 它们底下指向同一份磁盘资源，但持有两份独立内存 Arc → endpoint_info 不通
  气、`lan_listener_error` 永远 None

**错位 2**: AGENTS.md 设计意图 vs 代码现状不符

`uc-desktop/AGENTS.md` 与 memory `project_gui_uses_inprocess_facade.md`：
"GUI 走 in-process facade — uc-tauri 直调 AppFacade"。设计意图是 in-process
模型下只有一份 AppFacade，GUI commands 直调它，daemon services 也用它。
但代码现状把 daemon 装配做成了"完整重 wire 一次"。

**错位 3**: `start_in_process` 职责过宽

它现在做了：
- 装 deps（应该归进程级一次性）
- 装 AppFacade（应该归进程级一次性）
- 装 background tasks（部分进程级、部分 daemon-lifecycle）
- 装 daemon services（daemon-lifecycle）
- 起 HTTP server（daemon-lifecycle）
- 起 LAN listener（daemon-lifecycle）
- 起 iroh node（daemon-lifecycle）
- 写 PID 文件（daemon-lifecycle）

应该被拆成两个职责：进程级装配 + daemon-lifecycle 装配。

## 4. WireOverrides 是症状而非病因

### 当前实现回顾

`WireOverrides` struct 让 caller 注入预先建好的 `mobile_sync_endpoint_info`
Arc，这样 GUI wire 和 daemon wire 共享同一份。语法上这个 Arc 沿着 5 层
调用栈往下传：

```
uc-tauri::run.rs (创建 + .manage())
  ↓
bootstrap_daemon_in_process(_, _, _, _, _, WireOverrides{...})
  ↓
start_owned_in_process(_, _, _, _, _, WireOverrides{...})
  ↓
start_in_process(run_mode, WireOverrides{...})
  ↓
build_daemon_bootstrap_assembly(WireOverrides{...})
  ↓
build_daemon_app(WireOverrides{...})
  ↓
build_core(_, WireOverrides{...})
  ↓
wire_dependencies_with_overrides(_, WireOverrides{...})
  ↓
create_infra_layer(_, _, _, _, _, mobile_sync_endpoint_info_override)
```

5 层透传一个 Optional Arc，`reload_in_process_daemon` 也带这个参数。

### 设计选择对比（事后回顾）

| 方案 | 当下评价 |
|---|---|
| A. 单参数 `Option<Arc<...>>` 加在 `wire_dependencies` 末尾 | 5 个 caller 都得显式写 `None`，噪声 |
| **B. `WireOverrides` struct + 双入口** ← **当前选择** | 字段 named、扩展性、CLI 路径不感知 ……但仍然是补丁 |
| C. builder 模式 | 与同步函数风格冲突，价值 < 成本 |
| D. 全局 OnceCell | 违反 uc-infra "禁止全局状态泄漏" 硬约束 |

### 病因诊断

**根因不是"参数怎么传"，而是"为什么需要传"**。

正确分层下：

- 一份 deps 一份 facade（进程级，建一次）
- daemon-lifecycle 装配只接受"已有 AppFacade + 配置"作输入
- mobile_sync_endpoint_info 永远只有一份（归 AppFacade，daemon worker 写、
  GUI command 读，永远一致）
- **不需要 `WireOverrides` 任何形式**

这与方案 A/B/C/D 的对比无关 —— 它们都是在错的前提下选最不痛的写法。

### 用户判断的洞察

> "只要设计到比较繁琐的参数传输和复杂的状态管理，那一定是当前的架构
> 不适用当前的业务，可以进行重构。"

这条原则在本场景完全命中。`WireOverrides` 本身工程质量没问题，但它的
存在本身就是架构信号 —— 5 层透传一个 Optional Arc 不是"扩展性预留"，
是"两份 deps 不该存在"的代偿。

## 5. 副发现（独立的次生 bug，本次未修）

### MobileSyncSettingsViewDto.lan_listener_error 字段路径死循环

- daemon LAN listener bind 失败 → daemon 端写 `endpoint_info_B.set_bind_failure(...)`
- GUI commands 调 `runtime.app_facade().mobile_sync.get_settings()` →
  读 `endpoint_info_A`（永远空）
- 用户在 UI 上永远看不到 daemon 真实 bind 失败原因

**当前会话部分修复**: Phase 3 的 `WireOverrides` 让 GUI 与 daemon 共享同
一份 Arc，理论上 daemon 写、GUI 读已经通气。**但这条路径的最终修复**
应当通过 Phase 4 的架构整治达成，让"只有一份 endpoint_info"成为结构
性事实而不是依赖共享 Arc 的精巧约定。

### 独立 daemon binary（standalone）路径 vs in-process daemon 路径

`run_mode = Standalone` 与 `run_mode = GuiInProcess` 共用同一套
`start_in_process` 装配。Phase 4 拆 lifecycle 时要保证 standalone 路径
不破坏 —— standalone 没有 GUI shell 的 AppFacade 来注入，必须自己 wire
一次 deps + facade，然后跑 daemon-lifecycle。这条路径独立。

## 6. 关键文件索引

修复涉及的代码 / 文档锚点（按 crate 分组）：

### uc-desktop

- `src/daemon/app.rs:303` — http_handle spawn
- `src/daemon/app.rs:417-426` — select! http 分支 + flag 置位
- `src/daemon/app.rs:461-470` — shutdown 阶段 flag 守卫
- `src/daemon/host.rs::start_in_process` — daemon 装配入口
- `src/daemon/bootstrap.rs::build_daemon_bootstrap_assembly` — daemon 一次性装配
- `src/daemon/app_facade_assembly.rs` — daemon 端独立装配 AppFacade（Phase 4 删除）
- `src/daemon_probe.rs::reload_in_process_daemon` — P1 reload 高层 API
- `src/daemon_probe.rs::ReloadInProcessDaemonError` — reload 错误分类
- `src/runtime.rs::DesktopRuntime::with_setup` — GUI 端 facade 装配（Phase 4 唯一 facade）
- `src/bootstrap.rs::build_gui_app` — GUI 启动入口

### uc-bootstrap

- `src/assembly.rs:443-451` — endpoint_info Arc new + override unwrap
- `src/assembly.rs::WireOverrides` — Phase 3 引入，Phase 4 删除
- `src/builders.rs::build_daemon_app` — 内部跑 build_core → wire
- `src/space_setup.rs::SpaceSetupAssembly::shutdown` — iroh 资源 cleanup

### uc-tauri

- `src/run.rs:124-129` — `GuiBootstrapContext` 解构 + endpoint_info_arc
- `src/run.rs:152-160` — `.manage()` 注册（Phase 4 后只剩 daemon_ownership / connection_state）
- `src/run.rs:227-242` — daemon spawn 路径（Phase 4 后传"已有 AppFacade"）
- `src/commands/restart.rs::restart_daemon` — Tauri 命令入口
- `src/commands/restart.rs::RestartDaemonError` — 前端 typed 错误

### 前端

- `src/lib/daemon-ws-bootstrap.ts::registerDaemonRestartListener` — WS 重连协议
- `src/components/setting/NetworkSection.tsx::handleRestart` — LAN-only 切换重启
- `src/components/device/MobileSyncSettingsSheet.tsx::handleRestart` — mobile sync 重启
- `src/main.tsx` — listener 注册位

### 测试

- `uc-webserver/tests/graceful_shutdown_port_reuse.rs` — R2 契约测试
- `uc-tauri/src/commands/restart.rs::tests` — RestartDaemonError 序列化 / 映射
- `src/components/setting/__tests__/NetworkSection.test.tsx::Test 6/7` —
  前端切到 restart_daemon

# Daemon reload & 架构分层重构 — task_plan

> 入会上下文：用户在 mobile_sync 配置变更后点"重启" → daemon panic
> (`JoinHandle polled after completion`)。Sentry issue UNICLIPBOARD-RUST-4，
> Windows 上 3 次重现，release `uc-bootstrap@0.7.0`。
>
> 本次会话从修 panic 开始，沿因果链一路上溯到一个更深的架构错位：
> in-process daemon 模型下进程内有两份 AppDeps + 两份 AppFacade，
> 导致状态不一致、reload 路径需要繁琐的 Arc 透传。

## Goal

修复 mobile_sync 重启崩溃这条用户路径，**并** 把"daemon 重启 = 重做整套
依赖装配"这个根本错位修正成"daemon 重启 = 重做 daemon-lifecycle 资源"。

最终用户感知：

- mobile_sync / LAN-only 配置改完点"重启"，UI 短暂 loading → 自动恢复，
  不闪窗、不重启进程、不崩溃。
- 端口被占用 / bind 失败这种 daemon 内部错误能反映到 UI（lan_listener_error）。

最终代码感知：

- 进程内只有一份 AppDeps、一份 AppFacade。
- daemon 重启不重建 sqlite pool / repos / settings repo。
- `WireOverrides` 这种"caller 给 wire 注入共享 Arc"的特殊机制不再需要。

---

## 已完成的阶段（本次会话内落地）

### Phase 1 — P0 修 daemon http_handle double-poll panic

**Status**: ✅ complete

**改动**: `src-tauri/crates/uc-desktop/src/daemon/app.rs`

select! 命中 `result = &mut http_handle =>` 分支后，shutdown 阶段不能再
`timeout(http_handle).await` —— 第二次 poll 已经 take_output 过的 JoinHandle
触发 `panic!("JoinHandle polled after completion")`。引入 `http_handle_consumed`
flag 守卫 shutdown 阶段。

**验证**: `cargo test -p uc-desktop --lib` 48 测试通过。

### Phase 2 — restart_daemon 命令（in-process daemon reload）

**Status**: ✅ complete

**目的**: 替代 `app.restart()` 进程级重启 —— 后者 spawn 新进程时旧进程还
持着端口，bind 必然撞 `WSAEADDRINUSE` (Windows os error 10048)，进而触发
Phase 1 的 panic（修了 panic，但用户体验仍然是"重启崩溃"）。

**改动**:

- `uc-desktop/src/daemon_probe.rs`：新增 `reload_in_process_daemon()` 高层
  API（take_owned → shutdown → start_in_process → wait_for_health → load_info）
  与 `ReloadInProcessDaemonError` 错误分类（NotOwned / Shutdown / Bootstrap）
- `uc-desktop/Cargo.toml`：加 `thiserror = "1"`
- `uc-tauri/src/commands/restart.rs`：新增 `restart_daemon` Tauri 命令；
  保留 `restart_app` 作为兜底通道
- `uc-tauri/src/run.rs`：注册 `restart_daemon` 到 `invoke_handler!`
- `src/lib/daemon-ws-bootstrap.ts`：新增 `registerDaemonRestartListener`，
  监听 `app://daemon-restarting` / `app://daemon-ready` 事件做 WS 断开 / 重连
- `src/main.tsx`：注册 listener
- `src/components/setting/NetworkSection.tsx` & `MobileSyncSettingsSheet.tsx`：
  调用 `restart_daemon` 取代 `restart_app`
- 测试：`uc-webserver/tests/graceful_shutdown_port_reuse.rs` 钉死 axum
  graceful_shutdown 后同进程 rebind 同端口的契约

**验证**:

- 后端：`cargo test -p uc-tauri --lib commands::restart` 4 测试通过
- 前端：`pnpm exec vitest run` 410 测试通过（含 NetworkSection.test.tsx
  Test 6 / Test 7 切换至 `restart_daemon`）

### Phase 3 — endpoint_info Arc 上提共享（R5 副发现）

**Status**: ✅ complete（**但实现方式有架构疑虑**，见下方 Phase 4）

**目的**: 当前 GUI 启动调一次 `wire_dependencies` 装出 GUI deps，
in-process daemon 启动时再调一次 `wire_dependencies` 装出 daemon deps。
两次 wire 各 `Arc::new(InMemoryMobileSyncEndpointInfoAdapter::new())` 一份，
互不通气。daemon LAN listener 写 daemon 那份，GUI facade 读 GUI 那份 ——
GUI 永远看不到 daemon 的 bind 失败原因。

**实现**: 引入 `WireOverrides` struct，让 caller 在 wire 之前注入已有 Arc，
GUI 端创建一份 Arc → 同时传给 GUI wire 和 daemon wire。

**改动**:

- `uc-bootstrap/src/assembly.rs`：新增 `WireOverrides` struct +
  `wire_dependencies_with_overrides()` 入口；`create_infra_layer` 加
  `mobile_sync_endpoint_info_override` 参数
- `uc-bootstrap/src/builders.rs`：`build_core` / `build_daemon_app` 加 overrides 参数
- `uc-bootstrap/src/lib.rs`：re-export `WireOverrides` /
  `wire_dependencies_with_overrides`
- `uc-desktop/src/bootstrap.rs::build_gui_app`：内部 new 一份 Arc，
  通过 `WireOverrides` 传给 wire；新字段 `GuiBootstrapContext.mobile_sync_endpoint_info`
- `uc-desktop/src/daemon/bootstrap.rs::build_daemon_bootstrap_assembly`：加 overrides 参数
- `uc-desktop/src/daemon/host.rs::start_in_process`：加 overrides 参数
- `uc-desktop/src/daemon_probe.rs`：`bootstrap_daemon_in_process` /
  `start_owned_in_process` / `reload_in_process_daemon` 全部加 overrides 参数
- `uc-tauri/src/run.rs`：把 GUI 端 Arc `.manage()` 注册，daemon spawn 时透传
- `uc-tauri/src/commands/restart.rs`：reload 调用透传 Arc

**验证**: 全栈编译干净，`cargo test -p uc-bootstrap -p uc-desktop -p uc-tauri
-p uc-webserver --lib` 126 测试通过；前端 410 测试通过。

---

## 待办阶段

### Phase 4 — daemon-lifecycle 与进程级资源分层（架构整治）

**Status**: 🔶 partial — 2026-05-10 二轮会话上半场落地;下半场 (WireOverrides
删除 + daemon 不重建 deps) 留作下一个独立 PR / 会话

**问题陈述**: 见 `findings.md` §3「真正的架构错位」与 §4「WireOverrides 是
症状而非病因」。简单说：

- in-process 模型下进程内 **不应当有两份 AppDeps + 两份 AppFacade**
- daemon "重启" 当前重做了整套 wire（sqlite pool、所有 repos、secure
  storage），这是不必要的浪费 + 错位
- `WireOverrides` 透传链是这个错位的副产物，不是病根

**实施方案选定（用户决策）**: 激进方案 —— 把 AppFacade 中的 6 个
daemon-lifecycle 字段从 `Option<Arc<X>>` 改为 `Arc<arc_swap::ArcSwapOption<X>>`，
daemon 启停时 swap。进程内 **唯一一份** AppFacade，GUI 与 daemon 共用。

涉及字段：
- `space_setup` / `member_roster` —— iroh 网络栈相关
- `clipboard_sync` / `blob_transfer` —— iroh 上的同步业务
- `mobile_sync` —— 因绑 enhanced apply_inbound（带 blob_materializer +
  host_event_emitter）也是 daemon-lifecycle
- `clipboard_restore` 暂保持 `Option<Arc<>>`（进程级，启动设一次不变）

`search` 字段是 `Arc<SearchFacade>` 必填，内部 coordinator 已经是 Option，
SearchFacade 内部 swap，不动 AppFacade 层。

**目标分层**:

```
进程级一次性（GUI 启动建一次，进程退出销毁）：
├─ AppDeps（sqlite pool + 所有 repos + settings + secure storage）
├─ AppFacade（含 mobile_sync facade + endpoint_info Arc + 其他全部）
├─ task_registry / emitter_cell / clipboard_write_coordinator

daemon-lifecycle（每次 daemon start/stop 重建）：
├─ HTTP server（绑 settings 决定的 port）
├─ LAN listener（绑 mobile_sync settings）
├─ iroh node + space_setup_assembly（绑 iroh_config）
├─ daemon worker tasks（clipboard sync / blob / presence / keepalive）
├─ PID 文件 / auth token
└─ DaemonHandle（cancel + join）
```

**预期改动范围**（约 300-500 行实质代码 + 测试）:

1. 拆 `start_in_process` 职责：把"装 deps + 装 AppFacade"剥离出去，
   只留"daemon-lifecycle 装配"
2. 删 `daemon/app_facade_assembly.rs::build_daemon_app_facade`：daemon
   复用 GUI 端 AppFacade
3. `start_in_process` 入参从"自给自足"改为"接受已有 AppFacade + lifecycle
   配置"
4. standalone daemon binary 入口（`run()`）独立处理：自己 wire 一次 deps
   + facade（无 GUI），然后跑 daemon-lifecycle
5. 删除 `WireOverrides` 整套机制（含 5 处签名透传 + 1 处 .manage 注册）
6. `reload_in_process_daemon` 简化为 stop_daemon_lifecycle +
   start_daemon_lifecycle
7. 同步重梳 `daemon/app_assembly.rs` / `daemon/run_loop.rs` 持有的
   facade 引用方式

**为什么本次不顺手做**: 本次会话主线是修复 panic 与用户路径，工作量已经
不小；架构整治是独立目标，应当独立 PR 与独立 review，避免与 hotfix 耦合。

---

## 验收标准

### Phase 1-3（已完成）

- [x] `cargo build` / `cargo test` 在工作区干净通过
- [x] `pnpm exec vitest run` 在前端干净通过
- [x] R2 契约测试 `graceful_shutdown_port_reuse` 通过
- [x] 手动复现路径：mobile_sync 改设置 → 点重启 → daemon 不再 panic
- [x] 端口 reload 后 GUI WS 自动重连（前端事件协议落地）

### Phase 4（待办）

- [ ] **零 `WireOverrides` 引用**：`grep -r WireOverrides src-tauri` 无命中
- [ ] **进程内只有一份 AppFacade**：`build_daemon_app_facade` 删除；
  `cargo expand` 检查 daemon path 不再装配 facade
- [ ] **daemon reload 不重建 sqlite pool**：在 reload 前后插探针，
  `Pool::state` 的 `&self` 地址保持稳定
- [ ] **standalone daemon binary 仍可独立运行**：`uniclip daemon` 走
  原 wire 一次 → 跑 daemon-lifecycle 路径
- [ ] **现有功能 0 回归**：reload 后 mobile_sync 注册 / sync /
  clipboard sync 全部正常；剪贴板 history 列表跨 reload 不丢
- [ ] **lan_listener_error 端到端可见**：故意把 LAN port 占住 → daemon
  reload → UI 在 NetworkSection / MobileSyncSettingsSheet 看到具体错误
  原因（端口占用 / IP 不存在 / 权限）
- [ ] **测试**：daemon-lifecycle 单元测试覆盖 stop/start cycle 至少 3 次
  无资源泄漏；`tokio_metrics` 或类似探针 reload 前后 task 数量稳定

---

## 最终期望（一句话）

**进程内只有一份业务装配；daemon 是这份装配上"可重启的运行时表达"，
而不是另一份独立装配。** 用户改配置点重启，UI 短暂 loading 后无感恢复，
没有进程闪退，没有端口冲突崩溃，没有装配冗余浪费。

---

## 决策记录

| 时间 | 决策 | 理由 |
|------|------|------|
| 2026-05-10 | 先修 P0 panic 再做 P1 reload | 用户痛点优先；panic 是任意 HTTP server 早退都触发的 contract 违反 |
| 2026-05-10 | P1 走 in-process reload 而非 process restart + bind retry | 同进程 listener drop 端口立即可复用（R2 契约测试钉死）；进程级 restart 在 in-process daemon 模型下天然冲突 |
| 2026-05-10 | restart_app 命令保留作兜底而非直接删除 | 未来可能仍有"GUI 状态损坏需要彻底刷新"场景，保留语义清晰的进程级重启入口 |
| 2026-05-10 | endpoint_info Arc 共享走 `WireOverrides` 而非全局 OnceCell / builder 模式 | 见 `findings.md` §4 决策对比；但事后认定整个机制是症状，应通过 Phase 4 拆 lifecycle 移除 |
| 2026-05-10 | Phase 4 不在本会话内动手 | 范围控制；架构整治应独立 PR 独立 review |

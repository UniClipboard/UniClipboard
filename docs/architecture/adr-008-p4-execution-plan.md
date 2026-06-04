# ADR-008 P4 执行计划：轻量模式 + 保活 + 可观测性

- **承接**：[ADR-008](./adr-008-uniclipd-split-gui-as-client.md) §4 P4 + D3 / D9 / D10 / D17 / D19 / D20，及评审新增 D21 / D22 的 P4 残余。
- **日期**：2026-06-03
- **性质**：**功能交付阶段**（ADR §4：行为变化在 P3 集中、功能交付在 P4）。本阶段正式落地动机 4「轻量模式」。与 P1/P2 的「纯结构 / 等价替换」不同，P4 引入用户可见的新行为，故每切片须独立可发布、revert-safe、带行为门禁。
- **前提**：P1（抽库）/ P2（`uniclipd` 二进制 + CLI 解耦）/ P3（GUI 转纯 client，D2+D5 切换）均已落地（HEAD `c6b2cf68`）。`cargo check --workspace` 干净。
- **方法**：3-agent 逐子系统勘探（进程生命周期 / autostart / 多 profile+ 可观测性）+ 枢纽点人工逐行核实（D9/D21/D22 真实完成度）。

## 0. 净效果（P4 终态边界）

```text
关窗  (CloseRequested)      → 隐藏到托盘，Tauri 进程留，daemon 留           [P3 已成立]
轻量  (托盘 / 显式)         → GUI 进程整体退出（无托盘无快捷键），daemon 留   [P4 新建：一次性通知 + 自愈标志]
彻底退出 (托盘 / 显式)      → GUI 退 + 停「本 GUI spawn 的」daemon            [P4 新建：ownership 分类 + 复用 graceful stop]
重开 app                   → 冷启动 → probe attach 既有 uniclipd → resync   [P3 框架已就绪，P4 收尾]
```

- ~~登录自启目标从 GUI 切为 `uniclipd`~~ **修订（2026-06-04）：自启目标保持 GUI，daemon 由 GUI 拉起**（见 ADR D10/D17 修订）；自启 = settings 派生投影到 `tauri-plugin-autostart` login item，不自建 OS 原生 daemon 载体。轻量模式仅在当前会话有效，下次登录 GUI 照常自启。
- 多 profile = N 个独立 `uniclipd`（数据/端口/keychain/iroh 已隔离）；per-profile 自启单元，默认仅主 profile。
- 每进程独立日志文件 + daemon 为 product analytics 唯一权威发送方。
- 崩溃可见性靠「启动写 start marker、graceful 清除、残留 = 上次异常退出」反向模式。

## 1. 进入 P4 的真实起点（已核实）

> 实施路径把 D9/D16/D21/D22 排在 P2。逐行核实后，**D21/D22 已实质落地**，**D9 存在缺口**——这改变了 P4 的切片前置关系，下表为准。

| 决策 | 排期 | 核实结论 | file:line |
|---|---|---|---|
| **D21** graceful 终止 | P2 | ✅ 已落地。`wait_for_shutdown_signal`（SIGTERM+ctrl_c）→ cancel cascade；webserver `with_graceful_shutdown`；shutdown 前 flush delivery 记录 | `uc-daemon/src/daemon/app.rs:480,600`、`uc-daemon/src/daemon/app.rs:417`、`uc-webserver/src/api/server.rs:439` |
| **D22** 单实例 + PID identity | P2 | ✅ 已落地。绑端口前 per-profile `try_acquire`；terminate 调用方均先过 `verify_pid_identity`。**残余**：`UC_DISABLE_DAEMON_SINGLE_INSTANCE` 逃生阀缺失 | `uc-daemon/src/daemon/host.rs:73`、`uc-daemon-local/src/instance_lock.rs`、`uc-desktop/src/daemon_probe.rs:279`、`uc-cli/src/commands/stop.rs:62` |
| **D9** 解锁契约 | P2 | ⚠️ **缺口**。无 `--unattended` flag、无互斥校验纯函数；`uses_auto_unlock_setting()` 恒 `false` → daemon **无条件 force keyring 解锁、无视 `auto_unlock_enabled`**。attended（GUI-spawned 尊重 auto_unlock）路径已不存在 | `uc-daemon/src/daemon/run_mode.rs:61`、`uc-core/src/settings/model.rs:206` |
| **D3** 三态 / ownership | P4 | ⬜ orphan-on-quit interim。`DaemonOwnership` 恒 `External`，`take_owned()` 恒 `None`，退出不停 daemon | `uc-daemon/src/daemon/ownership.rs:25`、`uc-tauri/src/run.rs:663`、`uc-tauri/src/tray.rs:168` |
| **D3** PID metadata 归属 | P4 | ⬜ `DaemonPidMetadata{pid,mode,started_at_ms}` 无 `spawned_by` 字段，无法区分「GUI spawn」vs「cli start」 | `uc-daemon-local/src/process_metadata.rs:35`、`uc-daemon-local/src/spawn.rs:50` |
| **D10** autostart 投影 | P4 | 🟢 **决策反转后近乎就绪**（2026-06-04：自启=GUI，见 ADR D10 修订）。现有 `tauri-plugin-autostart` 包 GUI 正是新方向；P4-4 只需确认"冷启动 attach 失败→spawn daemon"闭环 + per-profile 标识 + 删 daemon-side reconcile 计划。已砍 `StartupIntegrationProvider` / 三平台原生投影 | `uc-platform/src/ports/autostart.rs`、`uc-tauri/src/adapters/autostart.rs`、`uc-tauri/src/commands/autostart.rs:34`、`uc-tauri/src/run.rs:247,377` |
| **D17** 崩溃可见性 | P4 | ⬜ 未做（**已缩水**，2026-06-04）。仅需 start marker / clean-shutdown sentinel + 下次 GUI 红条；已砍 systemd OnFailure / launchd 节流 / 常驻通知 | — |
| **D19** 多 profile | P4 | 🟡 隔离 85% 就绪（端口/数据目录/keychain/iroh identity 全 per-profile）。缺：跨进程 BIND（D22 锁已补）、GUI 运行期切 profile 语义、per-profile 自启默认 | `uc-daemon-local/src/socket.rs:77`、`uc-platform/src/app_dirs.rs:24`、`uc-platform/src/system_secure_storage.rs:72` |
| **D20** 日志/analytics 单源 | P4 | 🟡 日志名固定 `uniclipboard.json`（无角色前缀），但 `ScopeContext.device_role` 已有；`POST /analytics/capture` 已落地，但 GUI 仍残留进程内 sink（update_telemetry/updater 直发） | `uc-observability/src/init.rs:90`、`uc-observability/src/scope.rs:38`、`uc-webserver/src/api/analytics.rs`、`uc-tauri/src/run.rs:617`、`uc-tauri/src/commands/update_telemetry.rs:258` |

**已收敛 OQ（落地照办）**：~~OQ-windows（Task Scheduler `schtasks` + `StartupIntegrationProvider`）~~ **2026-06-04 取消，自启=GUI 回归 `tauri-plugin-autostart`（见 §3）**、OQ-lightweight-discoverability（`tauri-plugin-notification` 一次性 + `app_data_root` 自愈 JSON 标志 + per-profile + 中英双文案）、OQ-migration（`DaemonProcessMode::InProcess` 保留 legacy-read-only）。

## 2. 切片（每片独立可发布、revert-safe、带门禁）

> 依赖序：P4-0 独立先行；功能核心链 **P4-1 → P4-2 → P4-3**；保活链 **P4-2 → P4-4 → P4-5**；P4-6 可任意时点并行；P4-7 收尾。

### P4-0 · 可观测性地基（无用户可见行为变化） `refactor:`
- D20 日志按角色前缀：`uc-observability/src/init.rs:90` 的 `rolling::daily(logs_dir, "uniclipboard.json")` 改为从 `ScopeContext.device_role` 取前缀 → `uniclipboard-{gui|daemon|cli}.json.<date>`。消除两进程 append 同一文件的竞争（轻量模式让 daemon 长期 detached，此为后续调试前提）。
- D22 残余：补 `UC_DISABLE_DAEMON_SINGLE_INSTANCE` 逃生阀（对齐 GUI 侧 `UC_DISABLE_SINGLE_INSTANCE`）。
- 确认 D21 shutdown 序列对在途 transfer/sync 的排空覆盖面（`app.rs:417` 已 flush delivery 记录，核实 iroh endpoint / `BIND_LOCK` 释放在 cancel cascade 内）。
- **gate**：`cargo check --workspace` + 起两进程（GUI+daemon）确认产出两份角色日志文件；clippy clean。

### P4-1 · ownership 分类数据面（无 UX 变化） `feat:` ✅ 已落地
- `DaemonPidMetadata` 加 `spawned_by: DaemonSpawnOrigin`（`Gui` / `Cli` / `Unknown`），随 `write_current_pid_with_mode` 写入；旧 PID 文件缺字段 → `Unknown`（serde default，向后兼容 OQ-migration）。
- `spawn_detached_daemon(origin)` 经 env `UC_DAEMON_SPAWN_ORIGIN` 透传给被拉起的 `uniclipd`，daemon 写 PID 时 `DaemonSpawnOrigin::from_env()` 自检回填（与 `UC_HOST_ROLE` 同款，无 app.rs 改动）；GUI spawn → `Gui`、`cli start` → `Cli`。
- 分类 predicate `DaemonPidMetadata::is_gui_spawned()`。
- **`DaemonOwnership` 枚举重设计移至 P4-3**（与其消费者「彻底退出→停」同切片落地，避免一个用不上的中间态枚举）；P4-1 只做持久数据面与分类原语。
- **gate（已过）**：`cargo check --workspace` clean；clippy clean（changed crate）；`uc-daemon-local` process_metadata 6/6（含 spawned_by round-trip / serde default / env 解析）、daemon_probe 16/16、stop 5/5。行为不变。

### P4-2 · D9 解锁契约（autostart 硬前置） `feat:` ✅ 已落地
> **范围决策（人确认）**：① 仅 GUI-spawned 转 attended；`cli start` / headless / 手跑保持现状 force-unlock（最小/安全，复用 P4-1 spawn origin）。② P4-2 只 fail-fast，机器可读状态文件 + GUI 红条并入 **P4-5**。
- **attended 判定**（`startup_recovery::is_attended`，纯函数）：`spawn_origin == Gui && run_mode != ServerHeadless && !strict_unattended`。attended → 尊重 `auto_unlock_enabled`（`false` → 保持 locked，GUI 解锁后经 `/lifecycle/ready` 释放 deferred 服务，通路已核实：`App.tsx` `shouldSignalDaemonLifecycleReady` 在 `session_ready` 时触发）；其余 → force-unlock（历史行为）。修掉 P3-3 把 GUI-spawned daemon 也 force-unlock 的回归。
- **互斥校验纯函数**（`uc-daemon-local::spawn_contract::validate_unattended_unlock`，单一事实源）：禁「strict-unattended」+「`auto_unlock_enabled = false`」。
- **strict-unattended 自检**（`host.rs::start_in_process` 最前）：`UC_DAEMON_UNATTENDED=1`（autostart/service-manager 设，P4-4 接上）+ `auto_unlock=false` → `tracing::error!` + 返回 `Err` → 进程非零退出。`cli start`/headless 不设该 env，故不触发（保持现状）。
- 移除 run-mode 维度的死方法 `uses_auto_unlock_setting()`（解锁决策已迁至 D9 契约）。
- **gate（已过）**：`cargo check --workspace` clean；clippy clean（changed files）；spawn_contract 2/2（互斥矩阵 + env 真值）、run_mode 4/4、startup_recovery 3/3（attended 矩阵）。
- **遗留至 P4-4**：把 `validate_unattended_unlock` 接进 GUI 设置页 / CLI 的前置友好报错 + autostart 单元的 `UC_DAEMON_UNATTENDED=1`（互斥左操作数「unattended 自启开关」随 P4-4 per-profile 单元投影新增）；D16 setup→operational 重启透传 flag。

### P4-3 · D3 三态 UX（功能交付核心） `feat:` ✅ 自动门禁已过（三态真机 UAT 待用户）
> **决策（人确认）**：① 彻底退出的优雅关停 = **GUI 发 SIGTERM、daemon 自排空**（复用 cli stop + D21 handler，无新 detach-RPC）。② **修订（2026-06-03）**：明确点「退出」**停连接的 daemon、不论谁拉起**（推翻原 D3"只停本 GUI spawn 的"；三态里关窗/轻量已是"保留 daemon"出口）。
- **退出决策读 PID 文件 + 两个安全闸**：`uc_desktop::daemon_probe::stop_local_daemon_on_full_quit()` 读 PID metadata，`verify_pid_identity==Active` 且 **非** legacy `InProcess`（旧 GUI 进程内 daemon，杀它会带挂旧 GUI）才 SIGTERM（复用 `terminate_local_daemon_pid`）。不再看 `spawned_by`——`cli start` 的常驻 daemon 也停。stale / 复用 PID 绝不发信号（D22 铁律#11）。`spawned_by` 仍服务 D9 attended 判定。
- `DaemonOwnership` 收敛为 None/External 轻量标记：删死的 in-process `Owned(DaemonHandle)` 变体 + `set_owned/is_owned/take_owned`（无生产 caller），不再耦合 `DaemonHandle`。
- **三态 UX**（`uc-tauri`）：`QuitIntent`（managed AtomicBool，默认 = 不停 daemon）→ 只有托盘「退出」`request_full_quit` 翻它；`ExitRequested` 读它，true 才 `stop_local_daemon_on_full_quit()`。故关窗（hide）/ 轻量 / Cmd-Q / restart 都不停 daemon，只有显式「彻底退出」停。
- 托盘新增「轻量模式 / Lightweight Mode」项（i18n 进 `MenuLabels`）：`enter_lightweight_mode` 发一次性系统通知（`tauri-plugin-notification`）后 `app.exit(0)`，daemon 留守。去重用 `app_data_root/lightweight-notified.json` 自愈标志（temp+rename，per-profile，不塞 settings.json），中英双版文案。
- **gate（已过）**：`cargo check --workspace` clean；clippy clean（changed files）；uc-daemon --lib 45/45（ownership 3 + startup_recovery 3 …）、uc-desktop daemon_probe 20/20（含 4 个 full_quit ownership/identity 门禁测试）、uc-tauri lightweight 2/2（QuitIntent 默认不停 + flag 原子写）。无 TS/command 变更，故无需 binding 重生 / tsc。
- **待用户三态真机 UAT**：关窗留托盘 / 轻量退进程留 daemon + 通知只弹一次 / 彻底退 **停连接的 daemon（含 `cli start` 起的）** / 重开 attach 后剪贴板面板 resync 非空（D8 框架 `DaemonWsBridge` 自动重连已在）。

### P4-4 · D10 autostart = GUI 自启（**已按 2026-06-04 决策大幅简化**） `feat:` ✅ 核心已就绪（确认 + 注释收尾已落地，per-profile 留 P4-7）
> **决策（人确认 2026-06-04）**：自启目标 = **GUI**，daemon 是被 GUI 拉起的内核。**砍掉** 原"自建 launchd/systemd-user/Task Scheduler 三平台原生 daemon 投影 + `StartupIntegrationProvider`"整坨高风险工作（见 ADR D10/D17 修订）。
> **核实结论（2026-06-04）**：P3-3 已建好全部核心链路，本切片无新功能代码——① 自启目标=GUI（`tauri-plugin-autostart` 注册当前 exe，`run.rs:248`/`adapters/autostart.rs:28`）；② settings 派生 + GUI 侧投影（`commands/autostart.rs:63-70` 先写 daemon settings(HTTP) 再 `reconcile_autostart`）；③ 启动期 reconcile 自愈陈旧项（`run.rs:378-386`）；④ GUI 冷启动必拉 daemon（`bootstrap_daemon_in_process` 三分支无绕过，`daemon_probe.rs:201-238`）；⑤ daemon 侧零 autostart 代码，无需 reconcile。
> **本切片实际改动**：清理 stale "留待 P4" 注释（`run.rs` setup + `daemon_probe.rs` spawn doc）+ 在 plugin init 处显式标注 per-profile 自启缺口（空 launch args，留 P4-7）。`cargo check -p uc-tauri -p uc-desktop` 绿。
> **唯一真实 gap → 留 P4-7**：`tauri_plugin_autostart::init(..., Some(vec![]))` 空 args + autolaunch 固定 bundle id → 非主 profile 自启不带 `UC_PROFILE`、会污染主 profile login item。D19 默认仅主 profile 注册自启，对主 profile 无害。
- **保留 `tauri-plugin-autostart` 自启 GUI**：目标二进制 = GUI（非 `uniclipd`），无 `--unattended`。确认 GUI 冷启动路径在 attach 不到本 profile 活跃 daemon 时一定 detached 拉起一个（复用 `daemon_probe` spawn，P3 已就绪）——这是"自启 GUI 即等于后台同步起来"的闭环，需补一条启动期断言/测试。
- settings 派生投影仍成立，载体回到 `tauri-plugin-autostart`：改 `general.auto_start` → 同步注册/删除 login item；关 → 删（杜绝幽灵自启）。投影由 **GUI 侧** 执行（autostart 是 `gui-side` 副作用，ADR D12/§D10 line149；插件 API 只能在 Tauri 进程调用）——无需 daemon-side reconcile（原计划那条是为"daemon 独立自启"准备的，本决策下取消）。
- **取消项**：原"autostart 单元 ExecStart 固定带 `UC_DAEMON_UNATTENDED=1`"（P4-2 遗留）随之取消——自启 GUI=attended，daemon 经 GUI spawn 走 attended 路径尊重 `auto_unlock`（P4-2 已落地）。strict-unattended 仅 headless server + `cli start` 触发，无自启单元消费者。
- per-profile：自启项标识带 profile（D19），默认仅主 profile 注册。
- **gate**：`cargo check --workspace`；GUI 冷启动 attach 失败→spawn daemon 的启动期断言测试；开/关自启后 OS login item 真实出现/消失（手工 UAT，至少 macOS+Linux）；clippy clean。

### P4-5 · D17 崩溃可见性（**已按 2026-06-04 决策缩水**） `feat:`
> **决策（承 D10/D17 修订）**：GUI 必自启，故 daemon 崩溃后下次登录 GUI 起来即可见——**砍掉** systemd `OnFailure` 系统通知、launchd-vs-systemd 节流语义、"长期不开 GUI 也要主动通知"那套常驻路径。
- 反向 marker：daemon 启动写 start marker（含 pid + started_at），graceful shutdown（D21 handler）才清除；下次启动检测到「PID 文件残留 + 无 clean-shutdown sentinel」= 上次异常退出。
- 下次 GUI 起来读到 → 红条提示（GUI 必自启，下次登录即覆盖；无常驻通知组件）。持久重启计数器 + 清零策略 **降级为可选**（"仅提示近期异常、不报次数"即可，按需再补）。
- **gate**：`cargo check --workspace`；模拟 SIGKILL 后下次启动检出残留 marker；graceful 退出后无残留；红条在重开 GUI 显示。

### P4-6 · D20 analytics 单源收口 `refactor:` / `feat:` ✅ 已落地（2026-06-04）
> **核实结论（2026-06-04）**：核心链路 P3-c 已就绪——daemon `POST /analytics/capture` 端点 + 契约 + 前端 webview 直发（`src/api/daemon/analytics.ts`）。本切片补完 Rust 侧残余 + 收口两决策（人确认）：① **完整路由**——GUI 自身 Rust 后台任务（updater / scheduler / notify_context）发的 update 动作事件也走 daemon；② **各 profile 独立 distinct_id**。
> **本切片实际改动**：
> - 设备级信号（`active_device_count` / `is_first_run` / `app_opened`）核实 **只由 daemon 发**——GUI 纯客户端走 `build_gui_client_context`，不 compose `EventContext`、不 emit 设备级事件（`wire_gui_client_deps` 原先仍建真实 PostHog sink，本切片改为 `NoopAnalyticsSink`，杜绝进程内发送端）。oneshot/`cli start` 同理不 compose EventContext，天然只发动作级。
> - 扩展契约 `CaptureUiEventRequest` +`CheckPerformed`/`NotificationShown` 两变体（+4 mirror 枚举），webserver `into_event` 映射 + `mirror_enums_share_wire_form` 锁 wire 等价。
> - 新增 `DaemonForwardingAnalyticsSink`（`uc-tauri/src/analytics_forward.rs`，实现 `AnalyticsPort`）+ `DaemonAnalyticsClient`（`uc-daemon-client`）：把 update `Event` 映射成契约 POST 到 daemon（session JWT，`client_type=gui`），call-site 零改动。`run.rs` 在拿到 `DaemonConnectionState` 后用它覆盖 GUI 的 analytics 端口。
> - 删死代码：前端已迁 HTTP 后，`capture_update_ui_event` Tauri command（含 mirror 枚举）整文件删除 + specta 注册移除；openapi/sdk/specta 三套产物重生成（操作数仍 60）。
> **多 profile person 语义收口**：**各 profile 独立 distinct_id**——`resolved_app_dir_name()` 已给整个 `app_data_root` 套 `-<profile>` 后缀（`uc-platform/src/app_dirs.rs`），`analytics/` 子目录天然 per-profile，各 profile 独立 `anonymous_user_id`/`space_person_id`。**无需代码改动**；后果：同一台机器多 profile 在 PostHog 计为多设备（已确认接受）。原 Explore「各 profile 共享同一 analytics 目录」判断有误，已核实纠正。
> **gate（已过）**：`cargo check --workspace` 绿；clippy 新增文件无 warning；webserver analytics 单测 10/10、forwarder 单测 3/3、specta_export、前端 `update-telemetry.test.ts` 5/5、`tsc --noEmit` 全过；GUI 不再持进程内 PostHog sink。
> **遗留**：动作级事件 daemon 转发的真机端到端 UAT（两进程不双计设备级 / PostHog DAU 不翻倍）待用户验证。

### P4-7 · D19 收尾 + OQ 收口 `feat:`
- per-profile 自启：默认仅主/默认 profile 注册 **GUI 自启项**（`tauri-plugin-autostart`，标识带 profile）；非主 profile 默认前台、显式开启才注册。
- GUI 运行期切 profile 语义（评审遗留）：**采纳冷启动**（见 §3 OQ-gui-profile-switch），不做热切换（与 ADR 否决「运行中热迁移活跃 iroh node」一致）。
- 卸载清理（OQ-uninstall-cleanup）+ 降级回滚收敛（OQ-downgrade-rollback）：落地 §3 收口结论。**简化（承 D10 修订）**：无自建 service unit，卸载残留收敛为"login item + crash marker"，由 `tauri-plugin-autostart` 卸载即清 + daemon 启动自愈，原"`uniclipd --uninstall-cleanup` 删 service unit"子命令降级为可选（仅清 marker）。
- **gate**：`cargo check --workspace`；卸载后无残留 login item + marker；降级方向不误杀高版本活进程。

## 3. Open Question 收口（落地决策）

| OQ | 状态 | 落地结论（推荐） |
|---|---|---|
| **OQ-uninstall-cleanup** | 开放 → 收口（**2026-06-04 简化**） | 自启回归 GUI 后无自建 service unit，残留收敛为「`tauri-plugin-autostart` login item + crash marker」：① 卸载 GUI 即清 login item（插件标准行为）；② crash marker 由 daemon 启动自愈/卸载脚本清；③ `uniclipd --uninstall-cleanup` 降级为可选（仅清 marker，无 service unit 可删）。原"删 service unit + 三平台卸载器 hook"整段不再需要。 |
| **OQ-downgrade-rollback** | 开放 → 收口 | ① 收敛方向：**incumbent 运行中 daemon 默认胜**；磁盘低版本 client **不得杀** 更高版本运行 daemon——拒启 + 红条「运行中 daemon 更新，重启收敛或重新升级」。唯一 sanctioned takeover 仍是 incompatible-version 替换（graceful-first）。② `schema_version` 前向不兼容降级：daemon 读到更高 schema 直接拒启 + 写机器可读状态 + GUI 红条，不静默 corrupt。本期交付「安全拒绝 + 可见」，不保证自动数据降级。 |
| **OQ-gui-profile-switch** | 开放 → 收口 | 采纳 **强制冷启动**：GUI 内切 profile = 重启 GUI 进程并以新 `UC_PROFILE` 起来（必要时拉起目标 profile 的 `uniclipd`）。理由：热切换需断当前 WS + 重走端口/token 发现/session/resync + 可能热迁移 iroh，复杂度高且与 ADR 反对的「运行中热迁移」同源。 |
| OQ-windows | 已收敛 → **2026-06-04 取消** | 原 Task Scheduler `schtasks` + `StartupIntegrationProvider` 方案随"自启=GUI"决策取消；Windows 自启回归 `tauri-plugin-autostart`（注册表 Run），无 daemon 原生载体、无保活降级问题。 |
| OQ-lightweight-discoverability | 已收敛 | `tauri-plugin-notification` 一次性 + 自愈 JSON 标志（per-profile）。落地于 P4-3。 |
| OQ-multiprofile-person | 开放 → 收口（**2026-06-04**） | **各 profile 独立 distinct_id**（人确认）。核实：`resolved_app_dir_name()` 给整个 `app_data_root` 套 `-<profile>` 后缀，`analytics/` 子目录天然 per-profile，各 profile 已有独立 `anonymous_user_id`/`space_person_id`。**无需代码改动**。后果：同一台机器多 profile 在 PostHog 计为多设备（接受）。收口于 P4-6。 |

## 4. 风险

- ~~**D9 缺口是 autostart 的硬前置**~~（已落地，且 2026-06-04 决策后自启=GUI=attended，硬前置关系解除）：P4-2 已修复 attended 路径。strict-unattended 仅 headless server + `cli start` 触发。
- ~~**自建 OS 投影替换 `tauri-plugin-autostart`**~~（**2026-06-04 决策已取消该工作**）：自启回归 GUI 复用现有插件，三平台原生 daemon 载体细节风险整体消除。残余风险仅"GUI 冷启动 attach 失败→spawn daemon"闭环须在 P4-4 加断言。
- **「彻底退出→停」误杀**：必须严格 `spawned_by==Gui` + `verify_pid_identity`（D22 铁律#11）双闸，否则误杀用户 `cli start` 常驻 daemon。P4-1 的 ownership 分类是此闸前提。
- **轻量模式完全隐形 UX**（D3 已知风险）：一次性通知是最低缓解；崩溃中途死亡另靠 P4-5 systemd `OnFailure`。通知去重标志文件损坏 → 宁可多发一次也不漏发。
- **多 profile × analytics person 聚合**：聚合语义定错会污染 PostHog 设备计数；P4-6 须显式定义并测两进程不双计。
- **per-profile 自启 ×N**：默认仅主 profile 注册，防 Windows 服务注册爆炸；非主 profile 显式开启才注册。

## 5. 待人最终确认

- D21 graceful handler 超时具体值、前端 WS 优雅关闭由谁触发（daemon 自等排空 vs GUI 彻底退出前先发 detach RPC）——ADR §3.3 遗留，P4-3 落地前定。
- §3 三条 OQ 收口结论（uninstall-cleanup 三层策略 / downgrade 拒绝方向 / profile 切换冷启动）是否采纳。
- OQ-packaging CI 产物（sidecar 公证 / musl `aws-lc-sys` / snap spawn AppArmor）——随 P4 打包实测落定，可单列收尾子任务。

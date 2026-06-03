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

- 登录自启目标从 GUI 切为 `uniclipd`（D10/D17）；自启 = settings 派生投影到 OS 原生载体（launchd / systemd-user / Task Scheduler）。
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
| **D10/D17** autostart 投影 | P4 | ⬜ 仍 `tauri-plugin-autostart` 包 GUI；无 KeepAlive/Restart、无 `uniclipd` 目标、无 per-profile unit。已有 `AutostartPort` trait + 启动期 reconcile + 回滚命令可复用骨架 | `uc-platform/src/ports/autostart.rs`、`uc-tauri/src/adapters/autostart.rs`、`uc-tauri/src/commands/autostart.rs:34`、`uc-tauri/src/run.rs:247,377` |
| **D17** 崩溃可见性 | P4 | ⬜ 完全未做。无 start marker / clean-shutdown sentinel / 重启计数器 | — |
| **D19** 多 profile | P4 | 🟡 隔离 85% 就绪（端口/数据目录/keychain/iroh identity 全 per-profile）。缺：跨进程 BIND（D22 锁已补）、GUI 运行期切 profile 语义、per-profile 自启默认 | `uc-daemon-local/src/socket.rs:77`、`uc-platform/src/app_dirs.rs:24`、`uc-platform/src/system_secure_storage.rs:72` |
| **D20** 日志/analytics 单源 | P4 | 🟡 日志名固定 `uniclipboard.json`（无角色前缀），但 `ScopeContext.device_role` 已有；`POST /analytics/capture` 已落地，但 GUI 仍残留进程内 sink（update_telemetry/updater 直发） | `uc-observability/src/init.rs:90`、`uc-observability/src/scope.rs:38`、`uc-webserver/src/api/analytics.rs`、`uc-tauri/src/run.rs:617`、`uc-tauri/src/commands/update_telemetry.rs:258` |

**已收敛 OQ（落地照办）**：OQ-windows（Task Scheduler `schtasks` AtLogOn 免管理员 + `StartupIntegrationProvider`）、OQ-lightweight-discoverability（`tauri-plugin-notification` 一次性 + `app_data_root` 自愈 JSON 标志 + per-profile + 中英双文案）、OQ-migration（`DaemonProcessMode::InProcess` 保留 legacy-read-only）。

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

### P4-4 · D10/D17 autostart 投影 `feat:`
- `uc-platform` 新增 `StartupIntegrationProvider`（`src/ports/startup.rs`），覆盖 autostart + keepalive + 目标二进制选择；平台差异由 adapter 吸收（参考 `AutostartPort` / `AppDirsPort` 体例，**不在 uc-platform 内 `cfg(target_os)` 分支**）。
- 自建 OS 原生投影（替换 `tauri-plugin-autostart`），目标二进制 = `uniclipd`、固定带 `--unattended`（D9）：
  - macOS：launchd LaunchAgent（`KeepAlive`，用户级免 root）。
  - Linux：`systemd --user`（`Restart=on-failure`）；非 systemd fallback 见 §3。
  - Windows：每用户 Task Scheduler（`schtasks` AtLogOn 免管理员），保活显式降级。
- settings 派生投影：改 `general.auto_start` → 同步重写/删除投影；关 → 删投影（杜绝幽灵自启）。**daemon-side reconcile**：`PUT /settings` 改 autostart 字段后由 daemon 侧订阅者执行投影（参考 `telemetry_enabled` 的 dedicated setter 模式 `uc-webserver/src/api/settings.rs:121`），补 GUI 命令路径外的 reconcile 缺口。per-profile unit 名带 profile（D19）。
- **gate**：`cargo check --workspace`；三平台单元生成/删除单测（生成内容含 uniclipd 路径 + `--unattended` + KeepAlive/Restart）；开/关自启后 OS 载体真实出现/消失（手工 UAT，至少 macOS+Linux）。

### P4-5 · D17 崩溃可见性 `feat:`
- 反向 marker：daemon 启动写 start marker（含 pid + started_at），graceful shutdown（D21 handler）才清除；下次启动检测到「PID 文件残留 + 无 clean-shutdown sentinel」= 上次异常退出。
- 持久重启计数器 + 清零策略（稳定运行 ≥T 秒归零 / 用户显式彻底退出归零），或降级为「仅提示近期异常、不报次数」。
- 下次 GUI/CLI 起来读到 → 红条提示；轻量模式中途死亡的主动通知路径（systemd `OnFailure` 单元发系统通知）覆盖「长期不开 GUI」场景，与 P4-3 的「进入轻量」一次性通知区分。区分 launchd（~10s throttle）vs systemd 语义，绝不静默。
- **gate**：`cargo check --workspace`；模拟 SIGKILL 后下次启动检出残留 marker；graceful 退出后无残留；红条在重开 GUI 显示。

### P4-6 · D20 analytics 单源收口 `refactor:` / `feat:`
- 设备级信号（`active_device_count` / `is_first_run` / heartbeat）**只由 daemon 发**；oneshot 抑制设备级、只发动作级（否则每次 `uniclip send` 算一次设备活跃）。
- 清 GUI 进程内 sink 残余：`uc-tauri` 的 update_telemetry / updater 动作事件（`run.rs:617`、`commands/update_telemetry.rs:258`、`commands/updater.rs`）改走 daemon `POST /analytics/capture`（session JWT），daemon 成唯一权威发送方。
- 定义多 profile × 同设备的 PostHog person 聚合语义（各 profile 独立 distinct_id vs 合并）——收口 §3 OQ。
- **gate**：`cargo check --workspace`；核实启两进程不双计设备级事件（PostHog DAU 不翻倍）；GUI 不再持进程内 PostHog sink。

### P4-7 · D19 收尾 + OQ 收口 `feat:`
- per-profile 自启：默认仅主/默认 profile 注册轻量自启单元；非主 profile 默认前台、显式开启才注册（避免 Windows 服务注册 ×N）。
- GUI 运行期切 profile 语义（评审遗留）：**采纳冷启动**（见 §3 OQ-gui-profile-switch），不做热切换（与 ADR 否决「运行中热迁移活跃 iroh node」一致）。
- 卸载清理（OQ-uninstall-cleanup）+ 降级回滚收敛（OQ-downgrade-rollback）：落地 §3 收口结论。
- **gate**：`cargo check --workspace`；卸载清理子命令删净投影 + marker；降级方向不误杀高版本活进程。

## 3. Open Question 收口（落地决策）

| OQ | 状态 | 落地结论（推荐） |
|---|---|---|
| **OQ-uninstall-cleanup** | 开放 → 收口 | ① 防御性自愈：daemon 每次启动 reconcile 投影——单元里的二进制路径不存在则自删（防幽灵自启）。② 主动清理：`uniclipd --uninstall-cleanup` 子命令删除 service unit + autostart 投影 + crash marker，由各平台卸载器调用（Windows MSI custom action / Linux 包 postrm）。③ macOS pkg 无标准 hook → 文档化「GUI 内提供『移除所有系统集成』按钮」+ 卸载前提示，并由 ①自愈兜底。 |
| **OQ-downgrade-rollback** | 开放 → 收口 | ① 收敛方向：**incumbent 运行中 daemon 默认胜**；磁盘低版本 client **不得杀** 更高版本运行 daemon——拒启 + 红条「运行中 daemon 更新，重启收敛或重新升级」。唯一 sanctioned takeover 仍是 incompatible-version 替换（graceful-first）。② `schema_version` 前向不兼容降级：daemon 读到更高 schema 直接拒启 + 写机器可读状态 + GUI 红条，不静默 corrupt。本期交付「安全拒绝 + 可见」，不保证自动数据降级。 |
| **OQ-gui-profile-switch** | 开放 → 收口 | 采纳 **强制冷启动**：GUI 内切 profile = 重启 GUI 进程并以新 `UC_PROFILE` 起来（必要时拉起目标 profile 的 `uniclipd`）。理由：热切换需断当前 WS + 重走端口/token 发现/session/resync + 可能热迁移 iroh，复杂度高且与 ADR 反对的「运行中热迁移」同源。 |
| OQ-windows | 已收敛 | Task Scheduler `schtasks` AtLogOn（免管理员）+ `StartupIntegrationProvider`；保活显式降级。落地于 P4-4。 |
| OQ-lightweight-discoverability | 已收敛 | `tauri-plugin-notification` 一次性 + 自愈 JSON 标志（per-profile）。落地于 P4-3。 |

## 4. 风险

- **D9 缺口是 autostart 的硬前置**（最高）：当前 daemon 无条件 force-unlock，attended 路径缺失；P4-4 autostart 必须在 P4-2 之后，否则「无人值守自启 + auto_unlock=false」组合无人拦截（违反 §5.3 铁律#7）。
- **自建 OS 投影替换 `tauri-plugin-autostart`**：三平台原生载体细节（plist KeepAlive / systemd user 单元 enable-linger / schtasks XML）易碎，须每平台真机 UAT；保留旧插件路径直到三平台验证通过再删（revert-safe）。
- **「彻底退出→停」误杀**：必须严格 `spawned_by==Gui` + `verify_pid_identity`（D22 铁律#11）双闸，否则误杀用户 `cli start` 常驻 daemon。P4-1 的 ownership 分类是此闸前提。
- **轻量模式完全隐形 UX**（D3 已知风险）：一次性通知是最低缓解；崩溃中途死亡另靠 P4-5 systemd `OnFailure`。通知去重标志文件损坏 → 宁可多发一次也不漏发。
- **多 profile × analytics person 聚合**：聚合语义定错会污染 PostHog 设备计数；P4-6 须显式定义并测两进程不双计。
- **per-profile 自启 ×N**：默认仅主 profile 注册，防 Windows 服务注册爆炸；非主 profile 显式开启才注册。

## 5. 待人最终确认

- D21 graceful handler 超时具体值、前端 WS 优雅关闭由谁触发（daemon 自等排空 vs GUI 彻底退出前先发 detach RPC）——ADR §3.3 遗留，P4-3 落地前定。
- §3 三条 OQ 收口结论（uninstall-cleanup 三层策略 / downgrade 拒绝方向 / profile 切换冷启动）是否采纳。
- OQ-packaging CI 产物（sidecar 公证 / musl `aws-lc-sys` / snap spawn AppArmor）——随 P4 打包实测落定，可单列收尾子任务。

# A3 Findings — Bootstrap / GUI Shell

> 范围：`uc-bootstrap` / `uc-desktop` / `uc-tauri` / `uc-cli`, 共 ~10K 行
> 重点：方案 C 取消 in-process daemon reload 后，Phase A/B/C 重构是否变冗余

## 0. 速答 5 个核心问题

| # | 问题 | 结论 |
|---|---|---|
| 1 | `Clone` derive 还需要吗？| 🟡 **部分需要** —— 因为 `ProcessRuntimeHandles.wired` 在 GUI shell 启动期被 `.manage(_.clone())` + `.spawn(_.clone())` clone 至少 2 次。但每次都是"启动期一次性 fan-out 给静态消费者", 不是 reload。完全可以由"提前各拿一份 Arc"替代 |
| 2 | `ArcSwapOption<XxxFacade>` 还需要吗？| 🔴 **不需要** —— swap_in 全进程只调一次，swap_out 实际触发时进程已经在退出。`SearchFacade::clear_coordinator` 物理零调用点。回到 `OnceCell<Arc<X>>` 或 `Option<Arc<X>>` (boot 时填入) 即可 |
| 3 | `build_daemon_lifecycle` vs `build_process_runtime` 拆分还有价值？| 🟢 **保留** —— 不是因为 reload, 而是因为 `build_daemon_lifecycle` 是 async (iroh bind 必须在 tokio runtime), `build_process_runtime` 是同步。物理上必须拆 |
| 4 | `ProcessRuntimeHandles` 结构体 | 🟡 **可简化** —— 它的真正功能是"把 GUI 已装好的进程级 deps 透传给 daemon-lifecycle 装配，避免 daemon 自己再 wire 一份"。问题在字段数偏多，且 `clipboard_write_coordinator` / `file_transfer_lifecycle` 已经在 `wired.deps` 链路里能找到，重复存放 |
| 5 | standalone daemon binary 是否还在生产路径？| 🟢 **是** —— `uniclip start` 子命令 detached-spawn `uniclip daemon` (`uc-cli/src/main.rs:244`), 走 `uc_desktop::daemon::run(Standalone)`。"GUI shell 与 standalone binary 共用进程级装配"的理由仍站得住 |

## 🔴 必删 / 必改 (确定为死代码或死路径)

### R1. `SearchFacade::clear_coordinator` 零调用点

- 位置：`uc-application/src/facade/search/mod.rs:120`
- 现状：定义存在，文档说 "daemon 退出时 caller 调", 但 grep 整个 `src-tauri` 找不到任何 caller (`set_coordinator` 也只 `daemon/host.rs:224` 一处，进程内仅装入一次)
- 根因：设计意图是 daemon stop 时回收，但方案 C 后 daemon 一辈子只装入一次，进程退出时 Arc drop 自动清，不需要显式 clear
- 处理：
  - 删 `clear_coordinator`
  - `set_coordinator` 改成构造期注入，因为是 async 装配，用 `OnceCell::set()` 更对路
  - `coordinator: ArcSwapOption<SearchCoordinator>` → `coordinator: OnceCell<Arc<SearchCoordinator>>`

### R2. `AppFacade::clear_daemon_lifecycle` 在 daemon main loop 退出 callback 里调，但那时进程也在 dying

- 位置：`uc-application/src/facade/app_facade.rs:162` + `uc-desktop/src/daemon/host.rs:255-258`
- 现状：daemon main loop 退出后调 `clear_daemon_lifecycle()`, 把 5 个 swap 字段全置 None
- 根因：daemon main loop 触发退出的两个路径 —— `RunEvent::ExitRequested` graceful shutdown / `restart_app` —— 之后进程都会 `app.exit(0)` 或 `app.restart()`。"残留 GUI command 拿到 None" 的窗口期实际是几毫秒
- 处理：
  - 弱命题：保留，进程退出前的 clean-up 没有坏处
  - 强命题：删，ArcSwap 全删，启动期 `OnceCell::set` 注入，daemon 退出 = 进程退出，Arc drop 自动清

### R3. `tauri::Builder::manage(process_handles.clone())` 死注册

- 位置：`uc-tauri/src/run.rs:181`
- 现状：`process_handles.clone()` 被 `.manage()` 进 Tauri State, 但 `grep -rn "State<.*ProcessRuntimeHandles"` 全工程 **零命中**
- 根因：历史上某个 Tauri command 可能需要从 State 拿 deps, 现在所有 Tauri command 都从 `Arc<TauriAppRuntime>` 拿 `app_facade()` / `task_registry()`
- 处理：
  - 删 line 181 `.manage(process_handles.clone())`
  - line 269 的 `process_handles_for_daemon = process_handles.clone()` 可以改成 `let process_handles_for_daemon = process_handles;` (move 而不是 clone)

### R4. `restart.rs` 模块说明里 9 行历史叙事可精简

- 位置：`uc-tauri/src/commands/restart.rs:4-13` 注释
- 现状：描述决策 C 历史很完整，但 grep 验证 `restart_daemon` / `reload_in_process_daemon` / `RestartDaemonError` / `ReloadInProcessDaemonError` 在代码中 **零命中** (commit `0f4fa652` 删干净了)
- 评估：这条算文档冗余而非代码冗余，但建议精简到 2 行 —— 当前 9 行历史叙事在 review 时容易让人误以为还有相关代码

## 🟡 可削减 (机制本身合理但当前规模过大 / 抽象过早)

### Y1. `WiredDependencies` / `AppDeps` 的 `#[derive(Clone)]` 派生

- 位置：`uc-bootstrap/src/assembly.rs:151` + `uc-application/src/deps.rs`
- 现状：派生 Clone 的理由 (注释 line 146-150): "in-process daemon 路径在每次 daemon spawn 时 clone 一份给 daemon-lifecycle 装配用"
- 实际 clone 调用点：
  - `uc-tauri/src/run.rs:144` `wired.deps.clone()` (TauriAppRuntime 构造)
  - `uc-tauri/src/run.rs:181` `process_handles.clone()` (.manage, **R3 可删**)
  - `uc-tauri/src/run.rs:269` `process_handles.clone()` (daemon spawn)
  - `uc-desktop/src/daemon_probe.rs:209/231` `process_handles.clone()` (Absent / Incompatible 二选一)
  - `uc-desktop/src/daemon/host.rs:90` `wired.deps.clone()` (standalone binary 内部)
- 评估：clone 调用点都集中在"启动期一次性 fan-out", 没有 reload 多次 clone。Clone 派生没有删除的强理由 (clone 廉价), 但 **逻辑论据已经不成立**, 应改注释：
  ```
  原："daemon reload 时 clone 一份给新 daemon-lifecycle"
  现："启动期 GUI shell 把同一份 deps fan-out 给 TauriAppRuntime / daemon spawn"
  ```

### Y2. `BackgroundRuntimeDeps` 拆出 `WiredDependencies` 的价值减弱

- 位置：`uc-bootstrap/src/assembly.rs:112`
- 现状注释 (line 141-148): "WiredDependencies 可以在 daemon reload 时被多次借用，而 BackgroundRuntimeDeps 只在进程启动时 spawn 一次"
- 实际：没有 reload, "多次借用"理由不再成立。但 `BackgroundRuntimeDeps` 含 2 个 `mpsc::Receiver` (不可 Clone), 仍然有"消费一次后丢弃"的物理边界
- 评估：拆分本身物理上还合理 (mpsc Receiver 不能在 reload-friendly struct 里), 但 **注释里的理由要改** —— 改成 "Receiver 不可 Clone, 而 WiredDependencies 需要被 standalone daemon binary 与 GUI shell **两种入口共用**"

### Y3. `ProcessRuntimeHandles` 字段冗余

- 位置：`uc-desktop/src/daemon/host.rs:56-62`
- 现状：4 字段 (`wired`, `storage_paths`, `clipboard_write_coordinator`, `file_transfer_lifecycle`)。其中后两个已经存在于 `BackgroundRuntimeDeps` 里，装入 `ProcessRuntimeHandles` 是因为 `BackgroundRuntimeDeps` 本身被 spawn 消费掉了
- 处理：
  - 现状 OK, 但应该意识到这是 "BackgroundRuntimeDeps 一次性消费" 的副作用
  - 应该挪 `clipboard_write_coordinator` / `file_transfer_lifecycle` 进 `WiredDependencies` (它们本身跨 daemon reload 复用), 然后 `ProcessRuntimeHandles` 就剩 `wired` + `storage_paths`, 或者直接合并成 `(WiredDependencies, AppPaths)` tuple
  - 不阻塞，性质是 "还能再精简一层"

## 🟢 待定

### G1. `build_daemon_lifecycle` 单独成函数的价值

- 位置：`uc-bootstrap/src/builders.rs:135`
- 评估：即便没有 reload, 仍是 "async 装配链的边界点": 同步装配在 `build_process_runtime`, 异步装配 (iroh) 在 `build_daemon_lifecycle`。物理上必须拆 (tokio runtime 上下文边界)。**结论：保留，但要在 `builders.rs` 顶部注释删掉"跨 daemon reload 复用"那层论据，换成"async/sync 边界 + standalone binary 与 GUI shell 共享同一套装配"**

### G2. `app_facade.rs` doc 注释整体重写

- 位置：`uc-application/src/facade/app_facade.rs:61-104`
- 现状大量提及 "daemon 启动后由 swap_daemon_lifecycle 一次性塞入，daemon 停止/重启时由 clear_daemon_lifecycle 卸下"
- 评估：实际只 "塞入一次，进程退出时清空"。如果接受 R1+R2 (改 OnceCell), 这一整段注释要重写

### G3. graceful shutdown 序列在 `restart_app` 与 `RunEvent::ExitRequested` 里重复

- 位置：`uc-tauri/src/commands/restart.rs:67-92` vs `uc-tauri/src/run.rs:520-566`
- 评估：共享常量已经提到 `run.rs::pub(crate)`, 但 emit + sleep + take_owned + shutdown 这条序列本身是 copy-paste。可以抽 `pub(crate) async fn graceful_daemon_shutdown(...)`。不阻塞 review, 因为 control flow 差异 (一个 prevent_exit + spawn, 一个 sleep + restart) 让抽函数收益有限

## 结论

### 用户最关心问题的直接回答

> 既然方案 C 取消 in-process daemon reload, 那 Phase A/B/C 重构是不是变成冗余？

**部分变冗余，但物理拆分本身仍然有价值**:

1. **ArcSwapOption 是真冗余** (R1+R2+G2): swap_out 实际无意义路径，应回退到 OnceCell 风格。这部分是 Phase A/B/C 为了 reload 准备的"半成品", 决策 C 之后没用上。约 100-150 行可删 / 简化
2. **`build_process_runtime` / `build_daemon_lifecycle` 拆分仍然合理** (G1): 但理由变了 —— 不是 "为了 reload 复用进程级 deps", 而是 "async/sync 边界 + standalone binary 与 GUI shell 共享同一套同步装配链路"。standalone binary 走 `uniclip daemon` 仍在生产路径，"共用"论据成立
3. **`Clone` derive 不必删，但注释要改** (Y1): clone 调用点都是"启动期一次性 fan-out", 没有 reload 调用方。Clone 没有功能性损失，但 doc string 里说的 "daemon reload 时 clone" 是过期叙事
4. **`ProcessRuntimeHandles` 是合理透传容器**, 但有 2 个字段重复 (Y3), 可以再精简一层
5. **死代码 R3** (`.manage(process_handles.clone())`) 不属于 Phase A/B/C 冗余，是独立的清理项

### 必改清单

| ID | 项 | 估计代码量 |
|---|---|---|
| R1 | `SearchFacade.coordinator` ArcSwap → OnceCell + 删 `clear_coordinator` | -10 +10 |
| R2 | `AppFacade` 5 个 swap 字段 → OnceCell, 删 `clear_daemon_lifecycle` + daemon-loop-exit callback 里 `app_facade_for_cleanup` | -40 +30 |
| R3 | 删 `tauri::Builder::manage(process_handles.clone())` line 181 | -1 |
| Y1+Y2+G1+G2 注释更新 | 把"为 reload 准备"的注释批量改为"startup-once / async-sync boundary" | 文档为主 |
| R4 | 精简 `restart.rs` 模块说明历史叙事 | -10 |

**总收益**: ~110 行实际代码删除 + ~120 行文档精简，概念上从 "为 reload 预留多次装配能力" 回收到 "启动期一次装入 + 进程级常驻"。配合 "daemon 进程内只活一辈子" 的新事实，心智模型更对齐方案 C。

### standalone binary 路径确认

`uniclip` CLI 的 `daemon` subcommand 在 `uc-cli/src/main.rs:244` 调 `uc_desktop::daemon::run(DaemonRunMode::Standalone)`, 由 `uniclip start` 子命令 detached-spawn (`uc-cli/src/local_daemon.rs:212-226`)。standalone binary 内部自己跑一次 `build_process_runtime` (`uc-desktop/src/daemon/host.rs:82`) + `start_in_process`, **复用 GUI shell 同一套进程级装配代码**。Phase A rename `build_gui_app` → `build_process_runtime` 的最大产出就是这个共用 —— 没死，仍在生产。

### 决策 C 收尾度验证

- `restart_daemon` / `reload_in_process_daemon` / `RestartDaemonError` / `ReloadInProcessDaemonError`: grep 整工程 **零命中** ✓
- `app.restart()` 路径里的 graceful shutdown (commit `ea09cdd3`): 落地在 `uc-tauri/src/commands/restart.rs:80-92`, 配合 `RunEvent::ExitRequested` 路径 (`uc-tauri/src/run.rs:520-566`) 双覆盖 ✓
- in-process daemon 单装一次 (没有 stop-and-start cycle): 验证 `swap_daemon_lifecycle` / `start_in_process` 调用点均唯一 ✓

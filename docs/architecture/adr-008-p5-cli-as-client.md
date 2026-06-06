# ADR-008 P5 执行计划：`uc-cli` 转纯 client + 依赖瘦身（兑现 Scope B）

- **承接**：[ADR-008](./adr-008-uniclipd-split-gui-as-client.md) §4（P2.5 端点 + D7）、§5.1 正向第一条「`uc-cli` 不再背全家桶、依赖图更干净（Scope B 目标）」，并收口 §3.2 `OQ-cli-oneshot-lifecycle`。
- **日期**：2026-06-06
- **性质**：**深化 + 瘦身阶段**。P3 已把 GUI 转成纯 client；本阶段把 **对称的另一半——`uc-cli`** 也转成纯 daemon client，并兑现 ADR 反复承诺却尚未落地的「CLI 二进制瘦身」。每切片须独立可发布、revert-safe。
- **前提**：P1（抽库）/ P2（`uniclipd` 二进制 + CLI 进程解耦）/ P3（GUI 转纯 client）/ P4（轻量模式）均已落地（HEAD `b2202dee1`）。`cargo check --workspace` 干净。
- **方法**：逐 crate 依赖核实（`cargo tree` 子树 + `-i` 反查）+ 命令级 in-process / daemon-client 双路径逐文件核实。
- **评审状态（2026-06-06）**：经 Codex 跨模型对抗评审 **8 轮（rev1→rev9）** 定稿，每条 finding 均经代码 / 父 ADR 核实后处置（完整轨迹见 `codex-review-workspace/review-log.md`、`findings.md`）。收敛后唯一剩余开放面 = **受控重启机制的实现设计细节**（P5-L 实现时落地，非阻塞 P5-0）；一处可推翻工程定调 = R7-F3「提升重启窗口接受短暂事件丢失、不建事件重放基建」。其余决策已全部钉死。
- **修订记录**：
  - **rev2（2026-06-06，经 Codex 跨模型评审 + 代码核实）**：① 补全子命令清单与切片归属（原计划漏 `upgrade`/`dev`(含 dump/seed)/`mobile_sync`(8 文件)/`app_session`，致 P5-4 删不掉 app 栈）；② P5-0 改为 **非零行为**——`process_metadata`/`socket` 实依赖 `uc_application::AppPaths` + `uc_platform`，须先下沉/注入路径再抽 thin crate；③ oneshot 生命周期改为 **daemon 侧自管**（租约/连接存活，非 CLI spawn+SIGTERM；受 D22「一 profile 一 daemon」约束，并发 CLI 必共享 oneshot）；④ 置备明确走 `/v2n/*`（新建 v2n Rust 客户端）并 **删除 setup-mode 要求**（弃 `DaemonSetupClient` /setup/*，疑 legacy）；⑤ probe/dev 同步 `cfg(debug)` 出 release CLI；⑥ 锁步同版发布收口版本错配；⑦ recv 规格定稿。基线实测刷新。
  - **rev3（2026-06-06，Codex round-2 + 读父 ADR D16）**：⚠️ **反转 rev2 的「删 setup-mode」**——读 D16（`adr-008-uniclipd-split-gui-as-client.md` line 179-190）确认 **setup-mode 是 D16 强制的 daemon 两阶段生命周期**（未置备→setup-mode 轻装，置备完 **重启** 进 operational），rev2 基于未读 D16 的错误假设。修正：**setup-mode 保留**；CLI 像 GUI 一样在 setup-mode 期经 **`/v2/setup/*`**（前端生成 SDK 实证在用，非别名）驱动置备，置备完触发 daemon 重启（透传 `--unattended`，D9）；`/setup/*`(DaemonSetupClient) 确为 legacy（前端+Rust 均无调用点），弃用判断不变。其余 rev3 修正：⑧ oneshot 补 **启动保留 + Oneshot→Standalone 提升**（防冷启 0 租约自终 / 支持升常驻，R2-F1）；⑨ 版本校验分配到 P5-1 + gate（镜像 GUI 的 package_version+api_revision，R2-F3）；⑩ 解耦 watch（无限）与 60s idle（仅 recv 等待 + oneshot 无客户端回收，R2-F4）；⑪ P5-2b 映射全 mobile-sync 子命令、debug 归 dev（R2-F5）。
  - **rev4（2026-06-06，Codex round-3 + 读父 ADR D18/D20）**：⑫ oneshot 租约改 **会话级**（非按请求，health probe 不计，R3-F1）；⑬ `send` 遵 **D18**——离线即报、不补投/落 pending、不重试、CLI 输出投递分布（R3-F3）；⑭ oneshot **抑制设备级 analytics**（D20，R3-F2）。
  - **rev5（2026-06-06，Codex round-4）**：⑮ 租约改 **连接绑定**（token-only 检测不了 kill -9，R4-F1）+ 修 §3 正文一致；⑯ 提升改 **受控重启进目标模式**（翻标志位留半 oneshot 态、无法升 ServerHeadless，R4-F2）；⑰ 抽 **P5-L daemon 生命周期前置切片**（原塞 P5-1 致 P5-2 非独立可发布，R4-F3）；⑱ `Failed{Offline}` 记录（R4-F4，**rev6 已定：写记录**，2026-06-06 人确认）；⑲ sweep 残留 `/v2n/*`→`/v2/setup/*`（R4-F5）。
  - **rev6（2026-06-06，Codex round-5）**：⑳ 租约协议显式化＝**每命令专用控制 WebSocket**（HTTP keep-alive≠命令会话，R5-F2）；㉑ 提升/重启加 **优雅交接**（req-resp drain + 长连接通知 + 自动重连，不在传输/`watch` 中硬重启，R5-F1）；㉒ 明确 **setup/operational 与 Oneshot/Standalone 两正交维度** + 按 spawn 来源定重启目标模式（R5-F3）；㉓ sweep 过期归属（提升/版本校验归 P5-L、Failed{Offline} 已决、§3 OQ 精简，R5-F4）。
  - **rev7（2026-06-06，Codex round-6）**：㉔ `switch-space` **拆出 setup-mode**——它是已置备 (`setup_complete=true`) 设备的 operational 动作，对 operational daemon 打 `/v2/setup/switch-space` + 受控重启重载身份（R6-F1）；㉕ 重启加 **串行化协调器 + quiescing 态**（停 admit 新租约防饿死、并发冲突仲裁单一目标模式，R6-F2）；㉖ 长连接交接加 **事件游标重放**（从 last-acked 续订去重，或显式声明可接受丢失；D8 非事件重放，R6-F3）；㉗ 启动 grace **到期=硬回收**（防 spawn 后未连即死的孤儿，R6-F4）；㉘ sweep §4 风险段生命周期归属→P5-L（R6-F5）。
  - **rev8（2026-06-06，Codex round-7，受控重启机制定稿）**：㉙ 受控重启改 **请求方编排锁交接**（D22 单实例锁下旧退→释锁→新 spawn，R7-F1）；㉚ 重启请求＝**独立事务、请求方先脱离 drain** 防自死锁（R7-F2）；㉛ 目标模式冲突＝**首锁定 + 冲突显式报错重试**、不静默覆盖（R7-F4）；㉜ 长连接事件连续性 **定调＝接受提升窗口短暂丢失 + CLI 明确提示**（P5 不建事件重放基建；留可推翻点，R7-F3）。
  - **rev9（2026-06-06，Codex round-8）**：㉝ 目标模式 + handover 代次 **跨进程间隙落盘持久化**（旧退新起间隙防他者起冲突模式，R8-F1）；㉞ 握手 **暴露驻留模式**、持久客户端见 Oneshot 必请求 takeover（防 daemon 在 GUI 脚下退，R8-F2）；㉟ drain **超时即中止重启、保旧 daemon**（不 force-kill 传输，R8-F3）；㊱ `mobile-sync debug` release-exclusion gate 移 P5-3、P5-2b 可独立满足（R8-F4）；㊲ sweep §1.1 switch-space=operational、§4 重连=接受丢失（R8-F5）。

## 0. 净效果（P5 终态边界）

```text
uniclip <任意业务命令>  → 仅经 uc-daemon-client 走 HTTP/WS；daemon 不在跑 → ensure 拉起（常驻或 oneshot）再连
uniclip init/join/...   → 经 v2 setup / pairing 端点完成置备（与 GUI 同路径），不再进程内起第二套 facade
uniclip probe ...       → 发 HTTP 到 uniclipd 的 dev 端点；CLI 不再链接 uc-platform
依赖子树                → uc-cli 从 ~540（实测 HEAD ad4365337）→ 目标 <200；iroh / diesel / libsqlite3-sys 从 `uniclip` 彻底消失
```

**当前与终态的差距 = 三条都通向 `iroh + diesel/sqlite` 的依赖边，须全断**（D7 只断了 `uc-desktop` 一条，app 栈三条未动）：

```text
uniclip ──① uc-bootstrap ──────→ build_cli_app_facade / build_cli_app_runtime ──→ uc-application + uc-infra → iroh/sqlite
        ──② uc-daemon-client ──→ uc-daemon-local ────────────────────────────────→ uc-application + uc-platform → iroh/sqlite
        ──③ uc-daemon-local ───→ （CLI 仅借进程管理工具）─────────────────────────→ uc-application + uc-platform → iroh/sqlite
```

- 第 ② 条是 **D7 未察觉的依赖污染**：`uc-daemon-client/Cargo.toml:3` 直接 `path` 依赖 daemon 实现库 `uc-daemon-local`，使这个本该几十依赖的 HTTP 客户端子树膨胀到 **465**——即便 CLI「走 client」也照样拉 iroh。
- 第 ①③ 条是 D7 明确 **保留** 的 in-process 置备 + 进程工具复用，本期收口。

## 1. 进入 P5 的真实起点（已核实）

> D7 评审期（2026-05-30）的两条关键判断在 P3/P4 期间已被 GUI 工作反超，下表为准。

| D7 评审期判断 | 当前核实结论 | file:line |
|---|---|---|
| 「`send`/`recv`/`watch` 派发/resend/订阅/cancel **webserver 零暴露**，须先新建端点（P2.5）」 | ❌ **已过时**。端点在 P3 GUI 转 client 时全部建好且 GUI 在用：dispatch / resend / entry-resource / blob / thumbnail / search / WS inbound 均已上线 | `uc-webserver/src/api/clipboard.rs:95`（DISPATCH）、`:96`（RESEND）、`:90`（resource）、`uc-webserver/src/api/blob.rs:18-19`、`uc-webserver/src/api/search.rs:127-129`、`uc-webserver/src/api/ws.rs` |
| 「`init`/`join`（置备）**保留 in-process**」 | 🟡 **可反转**。v2 setup/pairing 端点全套已上线（GUI 纯 client 在用），CLI 置备亦可改走端点，从而消除最后的 in-process app 栈 | `uc-webserver/src/api/v2/setup.rs:38-48`（INITIALIZE/REDEEM/SWITCH_SPACE/STATE…）、`uc-webserver/src/api/pairing.rs:20` |
| 「保留 `uc-bootstrap` + `uc-daemon-client`/`uc-daemon-local`/`uc-daemon-contract`」 | ⚠️ **与 §5.1「依赖更干净」自相矛盾**。这四个依赖把 iroh/sqlite 全量留在 `uniclip` 里，瘦身未兑现 | `uc-cli/Cargo.toml:13-18,44` |
| 「`local_daemon.rs` detached-spawn + probe 上移共享」 | ✅ 已落地，但 **寄居在重 crate `uc-daemon-local`**。CLI 只用其轻量进程工具（`process_metadata`/`socket`/`spawn`/`spawn_contract`），却被迫编译整个 app 栈 | `uc-cli/src/local_daemon.rs:8-10`、`uc-cli/src/commands/start.rs:41`、`uc-cli/src/commands/stop.rs:7` |
| `OQ-cli-oneshot-lifecycle`（`recv`/`watch` oneshot 退出条件与回收方）| ⬜ **未收口**。`DaemonRunMode` 无 oneshot 变体；长连接类无天然完成信号 | `uc-daemon/src/daemon/run_mode.rs:14`（无 `Oneshot`） |
| CLI 命令当前执行模式 | 🟡 混合：`send`/`recv`/`watch`/`blob` 双路径（daemon 优先、落回 in-process）；`search`/`status` **纯 in-process**；`init`/`join`/`invite`/`switch-space`/`members`/`devices` **强制 in-process**（`refuse_if_daemon_running`）；`probe` **纯本地平台栈** | `uc-cli/src/commands/search.rs:170`、`status.rs:38`（`build_cli_app_facade`）、`app_session.rs:32`（refuse 定义）、`init.rs:23`/`join.rs:61`/`invite.rs:44`/`members.rs:31`/`recv.rs:72`（refuse 调用） |

**净结论（rev2 修订）**：D7 的「端点建设」前置条件 **大部分已被 GUI 工作消化**——业务派发/检索/blob/WS、v2 setup `/v2/setup/*`、pairing 端点均已上线。但 **「P5 不需建新业务端点」过于乐观**：除 probe dev 端点外，`mobile_sync` 子命令组（8 文件，控制移动-LAN 同步配置/状态/开关）当前是 in-process，daemon 侧虽有 `uc-webserver/src/mobile_lan/` 同步 server，但 **未必有对应的 CLI 控制端点**——P5 是否需为 mobile-sync 新建控制端点须在 P5-2b 核实（见 §2）。其余剩余工作是 **CLI 侧的依赖切割 + in-process 路径退役 + oneshot 生命周期收口（daemon 侧自管）**。

### 1.1 子命令全集与切片归属（rev2 新增，收口 F-1）

`uc-cli/src/commands/` 实测 22 个入口，逐一标明终态与归属切片。**原计划仅覆盖加粗项的一部分，漏掉 `upgrade`/`dev`(含 dump/seed)/`mobile_sync`/`app_session`，致 P5-4 无法删除 app 栈依赖（瘦身门禁必失败）**。

| 命令 | 当前 | 终态 | 切片 |
|---|---|---|---|
| `send` | 双路径 (refuse+facade) | daemon client + oneshot | P5-1 |
| `recv` | refuse+facade | daemon client + oneshot(长连接) | P5-1 |
| `watch` | facade | daemon client + oneshot(长连接) | P5-1 |
| `blob` | refuse+facade | daemon client(+oneshot) | P5-1 |
| `search` | 纯 in-process | daemon client + oneshot | P5-1 |
| `status` | 纯 in-process | daemon client + oneshot | P5-1 |
| `init` / `join` | refuse+facade | daemon client 走 `/v2/setup/*`（**setup-mode 期**，置备完重启进 operational，D16） | P5-2 |
| `switch-space` | refuse+facade | daemon client 走 `/v2/setup/switch-space`（**operational 期**，非 setup-mode；改身份后受控重启，R6-F1） | P5-2 |
| `invite` / `members` / `devices` | refuse+facade | daemon client（query / pairing 端点） | P5-2 |
| `mobile-sync`(setup/network/devices/status/disable/debug) | in-process facade(8 文件) | daemon client；**可能需新控制端点** | **P5-2b（新增）** |
| `upgrade` | facade(无 iroh) 读升级状态 | 轻量本地读 或 小型 daemon 端点（不拉 app 栈） | **P5-4 前置（新增）** |
| `probe`(dump/restore) | 纯 uc-platform | daemon dev 端点 + `cfg(debug)` 出 release | P5-3 |
| `dev`(dump-clipboard/seed-clipboard/pairing…) | facade/platform，dev/E2E 专用 | daemon dev 端点 + `cfg(debug)` 出 release | P5-3 |
| `start` / `stop` | 进程管理（`uc_daemon_process` 后） | 不变（仅改 import） | P5-0 |
| `app_session` | refuse helper + facade builder（基础设施，非用户命令） | callers 清空后移除 | P5-4 |

**refuse_if_daemon_running 实测 15 处调用点**（join/send/invite/blob/app_session/devices/dump_clipboard/seed_clipboard/dev/members/init/recv/mobile_sync(mod+shared)/switch_space），比 D7 评审期文字列举的广——P5-1/P5-2/P5-2b/P5-3 退役 refuse 的范围须覆盖全部，P5-4 才能确认零残留 in-process 入口。

## 2. 切片（每片独立可发布、revert-safe、带门禁）

> 依赖序（rev5）：**P5-0**（含 P5-0a 路径解耦，**非零行为**，P5-0a→P5-0b）→ **P5-L**（daemon 生命周期机制，下游共享前置，R4-F3）→ 其后 **P5-1 / P5-2 / P5-2b / P5-3 可并行**（均消费 P5-L）→ **P5-4 必须最后**（P5-1+P5-2+P5-2b+P5-3 + `upgrade` 去 facade 全清空后才能删 app 栈依赖并验证瘦身）。

### P5-0 · 抽 `uc-daemon-process` thin crate + 净化 `uc-daemon-client`（**含路径解耦前置，非零行为**）`arch:` / `refactor:`

切断第 ② 条污染边——**最高杠杆、GUI 同步受益**。

> **rev2 关键修正（F-2）**：原计划称待迁模块「依赖面仅 libc/fs2/...，零 app 栈」**为假**。实测 `uc-daemon-local/src/process_metadata.rs:9-11` `use uc_application::facade::AppPaths` + `uc_platform::app_dirs::DirsAppDirsAdapter` + `uc_platform::ports::AppDirsPort`；`socket.rs:7,153` 同样依赖 `AppPaths` 与 `uc_platform::app_dirs`。因此 P5-0 **不是纯结构等价替换**——必须先做路径解耦（P5-0a），否则 thin crate 抽出来照样拉 app 栈。

**P5-0a · 路径解耦前置**（先于抽 crate）：
- 把 `process_metadata`/`socket` 用到的路径解析（pid 文件目录、socket/连接信息文件路径）与 app 栈解耦。二选一（实施时定，倾向后者）：
  - **(A) 下沉类型**：把 `AppPaths`（一组已解析 `PathBuf`）+ app-dirs 解析（`AppDirsPort`/`DirsAppDirsAdapter`，仅依赖 `directories`/`dirs` crate，不含 iroh/sqlite）下沉到轻量层（`uc-core` 或新 `uc-daemon-process`）。
  - **(B) 注入路径（推荐）**：thin crate 的进程编排函数改为 **接收已解析的 `PathBuf` 参数**，路径解析留在调用方。但注意：P5-3 后 CLI 会删 `uc-platform`，故 CLI 侧也需一个不依赖 `uc-platform` 的轻量 app-dirs 解析入口——所以 (B) 仍需把 app-dirs 解析能力放到轻量 crate 供 CLI/daemon 共用。
- **gate（P5-0a）**：`process_metadata`/`socket` 不再 `use uc_application` / `use uc_platform`；`cargo check --workspace` clean；行为等价（pid/socket 文件路径解析结果不变，加针对性单测固定路径）。

**P5-0b · 抽 thin crate + 净化 client**（P5-0a 后）：
- 新建 crate `uc-daemon-process`，从 `uc-daemon-local` 迁入进程编排模块：`process_metadata`（pid 读写 / `verify_pid_identity` / `DaemonSpawnOrigin`）、`socket`（`try_resolve_daemon_http_addr`）、`spawn`（`spawn_detached_daemon` / `resolve_daemon_exe_path`，已较干净：仅 std + `crate::process_metadata`）、`spawn_contract`（`RUN_MODE_ENV` / `RUN_MODE_SERVER` 等常量）。**P5-0a 后** 依赖面收敛到 `libc`/`fs2`/`which`/`rand`/`serde`/`thiserror`/`tracing`(+`directories` 若取 (A)/(B)-轻量解析)——零 app 栈。
- `uc-daemon-local` 反向依赖 `uc-daemon-process`；为不惊动既有 `uc_daemon_local::spawn::*` 调用点，先用 `pub use uc_daemon_process::{...}` re-export 保 path 兼容（后续切片再逐个改 import）。
- **`uc-daemon-client/Cargo.toml` 删 `uc-daemon-local` 依赖**（line 16），改依赖 `uc-daemon-process`（已有 `uc-daemon-contract` line 15）。核实 client 实际从 daemon-local 借了什么（`DaemonConnectionInfo` 读取 / socket 解析），对应改指向 process/contract。
- `uc-cli` 的 `local_daemon.rs:8-10` / `start.rs:41` / `stop.rs:7` 改 import 到 `uc_daemon_process::*`（此时 CLI 仍保留其它重依赖，瘦身在 P5-4 兑现）。
- **gate（P5-0b 终态）**：`cargo check --workspace` clean；`cargo tree -p uc-daemon-process -i iroh`/`-i diesel` **空**（thin crate 自证零 app 栈）；`cargo tree -p uc-daemon-client -i iroh` **空**、`-i diesel` **空**；`uc-daemon-client` 子树数从 **实测 499** → 预期 ~110；`uc-daemon-local` / `uc-cli` / `uc-tauri` 既有单测全过；clippy clean（changed crates）。

### P5-L · daemon 生命周期前置切片（oneshot 模式 + 连接租约 + 提升 + 版本校验 + analytics gating）`feat:`

> **rev5 新增（收口 R4-F3）**：oneshot 模式、连接绑定会话租约、启动保留、提升=重启、版本校验、analytics gating 是 **P5-1 / P5-2 / P5-2b / P5-3 共享的 daemon 侧机制**——原 rev4 塞在 P5-1 里却让 P5-2「并行」，二者实际有依赖、非独立可发布。抽成 **前置切片**，下游消费。详见 §3 OQ 收口。

> **为何 daemon 侧自管而非 CLI spawn+SIGTERM**：D22 规定一个 profile 只能有一个 daemon（独占 iroh endpoint + sqlite 写锁），故并发 CLI **必然共享** 同一 oneshot；「谁拉起谁 SIGTERM」会让 CLI-A 杀掉 CLI-B 正用的 daemon（F-3），`kill -9` 后靠空闲超时在持续事件下永不回收（F-4）。

- **`DaemonRunMode::Oneshot` 新变体**（`run_mode.rs:14`，当前无）：语义＝「临时 daemon：会话租约归零 + 无 pending 任务时自终」，常驻（Standalone/ServerHeadless）不受影响。
- **会话租约 = 每命令专用控制 WebSocket（rev6，收口 R3-F1 / R4-F1 / R5-F2）**：自终计数单位是 **命令会话租约**——非单个 HTTP 请求（否则 health probe 成「首个客户端」、请求一结束即归零、daemon 在真命令连上前就退）；**非 token-only**（acquire 后 `kill -9` 未 release 会卡死）；**也不能拿普通 HTTP keep-alive 当隐式协议**（keep-alive 连接会被客户端池复用/提前回收/跨请求共享，daemon 无法把它 1:1 绑到某条命令的生命周期）。落地：**租约协议显式化——每条命令开一条专用控制 WebSocket**，**WS open = acquire 租约、WS close/断（正常 / `kill -9` → TCP reset）= release 租约**。`watch`/`recv` 的业务 WS 本身即该租约；请求 - 响应类命令开一条轻量控制 WS 持租约至命令结束。**health probe 是普通 HTTP、不开控制 WS、不持租约**。
- **启动保留 + grace 硬回收（rev7，收口 R2-F1 / R6-F4）**：Oneshot 在 acquire 首个会话租约前不应用「0 租约→自终」（防 spawner 连上前自杀）；但 **grace 窗口到期仍无任何首租约 = 硬回收（daemon 必退）**——否则 CLI spawn 后、开控制 WS 前就死会留永久孤儿。即「首租约前不自终」**仅在 grace 窗口内成立**，过期即退。
- **两个正交维度（rev6，收口 R5-F3）**：daemon 状态 = **维度①生命阶段（D16：setup-mode ↔ operational）× 维度②驻留模式（P5：Oneshot 自终 ↔ Standalone/ServerHeadless 常驻）**，二者独立。**重启目标模式按 spawn 来源定**：`uniclip init`/`join`（CLI 置备）→ setup-mode →（重启）→ **operational + Oneshot**（置备完 CLI 会话结束即自终，符合「CLI 纯 client」，不留常驻）；GUI setup →（重启）→ **operational + Standalone**；`uniclip start` →（提升重启）→ **operational + Standalone**（`--server` → ServerHeadless）。**重启后 CLI 须重连并重取租约**，daemon 在「重启→CLI 重连」窗口内不得因 0 租约先退（沿用启动 grace + 重连握手）。
- **提升 / 重启 = 请求方编排的串行化交接（rev8，收口 R4-F2 / R5-F1 / R6-F2 / R7-F1 / R7-F2 / R7-F4，非翻标志位、非硬重启）**：run-mode 启动时已复制进多组件（服务装配、analytics gate），翻 flag 会留「半 oneshot 半常驻」（clipboard capture / sync dispatch / mobile_lan 不补起、analytics 仍抑制），且无法升到服务集不同的 `ServerHeadless`——故 `uniclip start`/GUI 接管 = **daemon 受控重启进目标模式**（与 D16「模式切换=重启」一脉相承，透传启动契约 flag）。**重启请求是独立事务、由请求方编排锁交接**，步骤：
  - ① **目标模式仲裁 + 跨进程持久化（R7-F4 / R8-F1）**：**首个重启请求锁定目标模式**（Standalone / ServerHeadless）；并发冲突请求（如 GUI 要 Standalone 撞 `start --server` 要 ServerHeadless）**显式报错 + 提示重启后重试**，**不静默覆盖、不后到覆盖**（否则可能给 GUI 一个无系统剪贴板能力的进程）。**关键：目标模式 + handover 代次必须落盘持久化（lock 目录下的 handover 文件），跨「旧退→新起」进程间隙存活**——否则间隙期另一请求者看「无 daemon」会起冲突模式；所有请求者起 daemon 前先读该文件遵守，**新 daemon 启动时校验并清除** handover 文件。
  - ② **请求方先脱离 drain（R7-F2，防自死锁）**：`start`/GUI 这个重启请求 **不持可被 drain 计数的会话租约**——协调器接受请求后返回 **交接凭证**，请求方 **释放自己的租约**，drain 才开始；否则请求方会等自己的租约排空而死锁。gate 必测「单独 `uniclip start` 能完成」。
  - ③ **进 quiescing 态（R6-F2）**：原子 **停止 admit 新会话租约**（新命令收「正在重启、请重试/重连」而非排队，防 drain 期新命令不断取租约导致饿死）。
  - ④ **drain 已 admitted 工作 + 超时即中止（R8-F3）**：请求 - 响应类在途传输有界 drain 完（不在传输中途硬重启，护 D18/F-5）；**drain 超时 → 中止本次重启、保留旧 daemon 继续 operational、向请求方明确报错（稍后重试）**，**绝不 force-kill 在途传输**（「按策略推进」会打断传输、与不打断保证矛盾，故定为中止而非强推）。
  - ⑤ **进程 + 锁交接（R7-F1）**：D22 单实例锁下「先启新会抢锁失败、先退旧又没人启新」——故 **请求方编排**：令旧 daemon 优雅退出 → **等其释放单实例锁** → 请求方以目标模式 spawn 新 daemon（新进程正常抢锁成功）。三平台交接测试。
  - ⑥ **长连接事件连续性（R7-F3，rev8 定调）**：入站 WS **当前无稳定事件游标**，建事件编号/持久化/ack/过期＝新基建、非实现细节。**P5 不建该基建**；提升重启是 **用户主动、罕见** 窗口，**接受其间短暂事件丢失 + CLI 明确提示**：`watch` 打印「daemon 已升为常驻、重连期间可能漏少量事件」、`recv` 重连后继续等待（漏掉的入站靠用户重发）。**不默认静默丢**。（⚠️ 工程定调；若产品要求 watch 严格不丢，则须单列「事件游标重放」子项建基建——留作可推翻点。）
  - **绝不在无仲裁 / 无脱离 drain / 无锁交接 / 无重连保障下重启**。
- **pending-work 计数 + graceful drain（F-5 通用机制）**：daemon 跟踪在途后台任务，关闭前带超时 drain，供请求 - 响应类命令定义「完成」用。
- **版本校验（R2-F3 / F-11）**：当前 CLI health probe 仅查 readiness（`local_daemon.rs` `HealthResponse`），**忽略 `package_version`/`api_revision`**，GUI 两者都查。在 `ensure_local_daemon_running()`/握手处 **镜像 GUI 的 package_version + api_revision 检查**：CLI 拉起的新进程直接用新版；已在跑的旧 daemon（锁步同版＝升级瞬间残留）提示重启（或 CLI 触发重启），API 不兼容明确报错。
- **握手暴露驻留模式 + 持久客户端探知（rev9，收口 R8-F2）**：health/握手响应须 **暴露当前驻留模式**（Oneshot / Standalone / ServerHeadless）。**持久客户端（`uniclip start` / GUI）发现现有健康 daemon 是 Oneshot 时，必须发起 takeover（提升重启），不可只做普通 attach**——否则 Oneshot 会在持久客户端脚下因租约归零自终。普通 health probe（非持久客户端）只读不 takeover。gate：短命令拉起 oneshot 后 GUI 打开 → 正确探知 Oneshot 并 takeover、daemon 不在 GUI 脚下退出。
- **Oneshot 抑制设备级 analytics（R3-F2 / D20）**：`Oneshot` 模式只发动作级事件、抑制设备级信号（`active_device_count`/`is_first_run`/heartbeat/startup/first-run），否则每次 `uniclip send` 虚增设备活跃、污染 PostHog DAU（D20 line 221，line 318 反模式）。按 run-mode 分支。
- **CLI 不再 SIGTERM 共享 daemon**；显式 `uniclip stop` 仍走跨平台 SIGTERM/`taskkill` + `verify_pid_identity`（D22 铁律#11）。
- **gate（P5-L）**：`cargo check`；**租约协议**——控制 WS open=acquire / close=release，`kill -9` CLI 经 WS TCP reset 自动 release（不卡死、不依赖空闲超时）、零孤儿；health-probe（普通 HTTP）不持租约不致自终；**生命周期/并发**——冷启（grace 内连上不提前自终）/ health-probe 与真命令间隙 / 多请求流期间租约保活 / 双 CLI 共享 oneshot（先完成者不杀对方在用 daemon）；**grace 硬回收（R6-F4）**——spawn 后、开控制 WS 前杀 CLI，grace 到期 daemon 必退、零孤儿；**串行化重启 + quiescing（R6-F2）**——传输中途执行 `uniclip start`（等 drain 完再重启、不断传输）、重启期新命令收「重试/重连」不饿死；**重启交接（R7-F1/F2/F4）**——**单独 `uniclip start` 能完成不自死锁**（请求方先脱离 drain）、**锁交接三平台**（旧退→释锁→新 spawn 抢锁成功、无双进程无孤儿）、**并发冲突目标模式首锁定 + 冲突显式报错重试**（不静默覆盖）；**长连接（R5-F1/R6-F3/R7-F3）**——`watch` 运行时 GUI 接管：通知 + 自动重连续订，**CLI 明确提示重连期间可能漏事件**（接受丢失定调，非静默）；**重启目标模式按来源（R5-F3）**——CLI init/join 重启进 operational+Oneshot（会话结束自终、不残留）、GUI 进 Standalone、`start --server` 进 ServerHeadless，且重启→重连窗口内不提前退；**版本校验** 旧/新/不兼容三组合清晰结果；**analytics** oneshot 不发设备级事件；clippy clean。

### P5-1 · 业务命令去 in-process 落回（send / recv / watch / blob / search / status）`feat:` / `refactor:`

> **依赖 P5-L**（oneshot 模式 / 连接租约 / 提升 / 版本校验 / analytics gating 均由 P5-L 提供）。本切片只做命令侧落回 + 命令级语义。

- 这些命令删掉 `InProcess` 分支与 `build_cli_app_facade`，统一经 `uc-daemon-client` 走 HTTP/WS；daemon 不在跑 → `ensure_local_daemon_running()`（P5-L 提供版本校验 + Oneshot spawn）拉起后再连。端点已就绪（见 §1）。
- **请求 - 响应类完成语义（F-5，用 P5-L 的 pending-work 计数）**：`search`/`status` = 响应返回即完成；`blob`/restore = 响应代表已持久化/写入。
- **`send` 遵 D18 投递语义（R3-F3）**：oneshot `send` = 即时尽力投当下在线者；
  - **仅「已被在线 peer 接受的在途传输」计为 pending 任务**（drain 完带超时才自终），不因等离线目标永不退出。
  - **离线目标 = 立即显式报告**「设备 X 离线、本次未投递」，**绝不补投/落 pending/静默/暗示重试**（D18 line 208）。
  - CLI 输出 **必须讲清投递分布**（D18 line 209）。
  - **离线目标仍写 `Failed{Offline}` 记录（2026-06-06 人确认）**：与常驻 daemon 路径一致，事后用户开常驻 daemon 可对这些离线目标手动 resend（D18「手动 resend 兜底」对 oneshot 亦成立）。注意这与「不补投/不重试」不矛盾——只落记录、不自动投。
- **recv / watch 规格定稿（F-9 / R2-F4，2026-06-06 人确认）**：
  - `recv` = **任意入站即退** + `--filter`；**等待超时** 默认 60s 可 `--timeout` 覆盖、超时（未等到）退出码非 0。
  - `watch` = **无限订阅**，仅 SIGINT / WS 断开退出（退出码 0），**无命令级 idle 超时**。
  - 输出走现有格式（json / 人读）。
- `refuse_if_daemon_running` 在这些命令上退役（拒绝 → ensure，与 D11 降级一致）；**覆盖 §1.1 全部相关调用点**（含 search/status 当前强制 in-process 路径）。
- **gate**：`cargo check`；每命令 daemon-on/off 双路径；`send` oneshot 在传输 drain 完成前不自终（F-5）；**D18 send** 混合在线/离线——在线投递、离线即时报告不排队不重试、输出含投递分布；recv 任意入站退/超时码、watch 无限订阅；clippy clean。

### P5-2 · 置备命令去 in-process（init / join / invite / switch-space / members / devices）`feat:`

反转 D7「置备保留 in-process」——端点已为 GUI 建好，CLI 复用。

> **rev3 关键修正（R2-F2，反转 rev2 的「删 setup-mode」）**：读父 ADR **D16**（`adr-008-uniclipd-split-gui-as-client.md` line 179-190）确认 **setup-mode 是强制的 daemon 两阶段生命周期**，rev2「删 setup-mode」基于未读 D16 的错误假设。须区分两件正交的事：
> - **置备协议**（用哪套 HTTP）：✅ rev2 判断正确——走 **`/v2/setup/*`**（`SETUP_INITIALIZE`/`REDEEM`/`SWITCH_SPACE`/`STATE`/`CANCEL`/`ISSUE_INVITATION`/`MIGRATION_PROGRESS`，包 `SpaceSetupFacade`）。前端生成 SDK（`src/api/generated/sdk.gen.ts`/`types.gen.ts`、`src/store/setupRealtimeStore.ts`）实证在用；`/setup/*`(DaemonSetupClient) 前端+Rust 均无调用点，确为 legacy，弃用。
> - **setup-mode**（daemon 生命周期阶段）：❌ rev2 删错，**必须保留**——未 setup-complete 的 daemon 起在 setup-mode（轻装：HTTP + `/setup/*`·`/v2/setup/*` 路由 + setup WS；clipboard capture / sync dispatch / mobile_lan **不构造**），置备完 **重启** 进 operational。

- **首次置备 `init` / `join` = 「确保目标 profile daemon 在 setup-mode + 经 `/v2/setup/*` 驱动 + 置备完重启进 operational」**，与 GUI 同协议同状态机（仅 `setup_complete=false` 的设备走此路径）：
  - daemon 未 setup-complete 时按 D16 `check_setup_complete`（拦 operational、不拦 setup-mode）会起在 setup-mode；CLI 确保其在跑即可经端点驱动。为 CLI 新建 **`/v2/setup/*` Rust 客户端**（前端在 JS 侧调，Rust 侧无现成客户端）。
  - **setup-mode → operational 重启（D16 line 188）**：置备状态机完成后 daemon 重启进 operational，重启须透传 `--unattended` 等启动契约 flag（D9 / D10）。CLI 置备流程须显式处理并等待这次重启 + 重连（与 GUI D8 reconnect 一次同理）。
- **iroh setup-bind（OQ-setup-iroh，父 ADR 开放问题，line 264）**：setup-mode 下 `/v2/setup/*` 由 `SpaceSetupFacade` 装配时无条件 `IrohNodeBuilder::bind`；**join/redeem 本质依赖 iroh**（joiner setup-mode 无 iroh → 配对端点 503）。CLI join 路径继承此上游约束；P5-2 须确认 setup-mode iroh-bind 时机/身份与 GUI 一致（不引入新机制）。
- **`switch-space` 是 operational 动作、不进 setup-mode（rev7，收口 R6-F1）**：切换空间的设备 **已 `setup_complete=true`**，强制进 setup-mode 与该状态矛盾（D16 `check_setup_complete` 拦的就是「已完成→不该回 setup-mode」）。故 `switch-space` **对正常运行的 operational daemon** 打 `/v2/setup/switch-space`（该端点在 operational 即可用），随后 **受控重启** 重载新空间身份、必要时经 `/v2/setup/migration-progress` 续 migration——走的是 P5-L 的「受控重启 + 交接」，与 `init`/`join` 的 setup-mode 路径 **分开**。
- **解锁契约（D9）**：CLI 置备/headless 沿用 force-unlock（attended 仅 GUI-spawned，P4-2 已落地）；确认 `cli`-origin 经 `/v2/setup/*` 置备 + 重启进 operational 时解锁路径正确。
- `refuse_if_daemon_running` 在置备命令上彻底删除——置备改为「确保 daemon 在跑（setup-mode）+ 经端点操作」，不再进程内起第二套 facade（消除 D7 line 113 的独占冲突根因）。
- **gate**：`cargo check --workspace`；`init`(new-Space) / `join`(redeem，**须 iroh**) 端到端经 daemon **setup-mode** 走 `/v2/setup/*` 完成 + 重启进 operational + CLI 重连成功；**`switch-space` 经 operational daemon 打 `/v2/setup/switch-space` + 受控重启重载身份（不进 setup-mode）**；`members` / `devices` 经 query 端点列出；删 refuse 后无双 facade 抢占同 profile 端口（D22 锁兜底）；端到端 UAT（新建/加入/改身份三路径全过，待用户）。

### P5-2b · `mobile-sync` 子命令组去 in-process（rev2 新增，收口 F-1）`feat:`

原计划完全漏掉 `mobile_sync/`（8 文件：setup/network/devices/status/disable/debug/mod/shared），它们当前全 in-process 起 facade——不迁移则 P5-4 删不掉 app 栈。

- **逐子命令映射端点（收口 R2-F5）**：daemon 侧已有 `uc-webserver/src/mobile_lan/`（移动设备连的 LAN 同步 server + file routes，**多数控制端点已存在**）。须把 **6 个用户子命令逐一映射到具体端点**，不留悬空：

  | 子命令 | 文件 | 终态端点 | 备注 |
  |---|---|---|---|
  | `mobile-sync setup` | `setup.rs` | 现有/新建控制端点 | 启用 + 配对 |
  | `mobile-sync network` | `network.rs` | 现有/新建控制端点 | 配网/地址 |
  | `mobile-sync devices` | `devices.rs` | query/控制端点 | 列设备 + add/revoke |
  | `mobile-sync status` | `status.rs` | query 端点 | 查状态 |
  | `mobile-sync disable` | `disable.rs` | 控制端点 | 关闭 |
  | `mobile-sync debug` | `debug.rs` | **归 dev 处置（P5-3）** | dev/E2E 性质，`cfg(debug)` 出 release，或专用 debug-only 端点 |

  - 某子命令若现有端点不覆盖 → **本切片新建对应控制端点**（这推翻 §1「P5 不需建新业务端点」的乐观论断，须显式承认）。`debug` 子命令按 dev 处置（与 §P5-3 一致），**不进 release**。
- CLI `mobile_sync/`（mod/shared + 各子命令）删 facade、改走端点；`refuse_if_daemon_running` 退役。
- **gate**：`cargo check --workspace`；mobile-sync **setup / network / devices(add/revoke) / status / disable 全部** 经 daemon 端点工作；端到端 UAT（待用户，含真实移动端配对回归）；clippy clean。**注（R8-F4）**：`mobile-sync debug` 的「仅 debug build / release 不含」gate **归 P5-3**（dev/cfg 处置切片），不在 P5-2b——使 P5-2b 能独立满足自己的 gate（其余子命令不依赖 P5-3）。

### P5-3 · `probe` + `dev` 命令组移进 `uniclipd`（人确认：移进 daemon）`feat:` / `refactor:`

> **rev2 扩围（F-1 / F-8）**：除 `probe` 外，`dev` 命令组（含 `dev dump-clipboard` / `dev seed-clipboard` / `dev pairing`，均 dev/E2E 专用、走 facade/`uc-platform`/SQLite）同属本切片；且须解决 **release CLI 残留 broken 命令** 问题。

- `probe` 子命令组 + `dev` 命令组（直接读写系统剪贴板 / 落 SQLite 的诊断，dev/E2E 专用）改造为 `uniclipd` 的 **dev-only 隐藏端点 / 路由**（参考 `uc-webserver/src/api/dev.rs` 的 debug-build-only router 模式）；CLI 端改成发 HTTP 到该端点。
- **release CLI 同步编出（F-8）**：`probe` / `dev` 子命令在 CLI 侧也 `#[cfg(debug_assertions)]` / feature-gate，使 **release `uniclip` 根本不暴露这些命令**——避免「release daemon 无 dev 路由 → release `uniclip probe` 打到不存在端点必失败」的矛盾（与 P5-4「每子命令 smoke」一致）。debug build 两侧都有、可用。
- **CLI 删 `uc-platform` 依赖**（`uc-cli/Cargo.toml:24`）——这是 CLI 唯一还需要本地平台栈的入口，移走后 CLI 与 `uc-platform`（及其 clipboard 后端）彻底脱钩。
- E2E 调试链路（依赖 `probe restore` / `dev seed-clipboard` 写系统剪贴板·落库）同步改为打 daemon dev 端点；更新相关 E2E 脚本与 `uc-cli/AGENTS.md` 里 probe/dev 例外条款。
- **gate**：`cargo check --workspace`；debug build `probe dump`/`probe restore`/`dev seed-clipboard` 经 daemon dev 端点工作；**release build：`uniclipd` 不含 dev 路由、`uniclip` 不含 `probe`/`dev` 命令且不含 `uc-platform`**（`cargo tree -p uc-cli -i uc-platform` 空；`uniclip --help` release 不列 probe/dev）；**`mobile-sync debug` 同 dev 处置——仅 debug build 可用、release 不含（R8-F4，gate 归此切片）**；E2E smoke 过。

### P5-4 · 依赖收尾 + 瘦身验证（终态门禁）`refactor:`

> **前置**：P5-1 + P5-2 + **P5-2b（mobile-sync）** + P5-3 全部落地，且 `upgrade` 已去 facade（见下），**否则任一残留 caller 都会把 app 栈拉回，终态门禁必失败**（F-1 教训）。

- **`upgrade` 去 facade（P5-4 前置子项，收口 F-1）**：`upgrade` 当前用 `uc_bootstrap` 起 facade 读升级状态（无 iroh/network）。改为不拉 app 栈的轻量路径——优先小型 daemon 端点（升级状态查询/确认），或确属本地的轻量读（仅碰升级状态文件，不经 facade）。
- `uc-cli/Cargo.toml` 删 `uc-bootstrap`、`uc-application`、`uc-platform`、`uc-daemon-local`（line 13/18/24/44）；保留 `uc-daemon-client`（P5-0 已瘦）、`uc-daemon-contract`（line 17）、`uc-daemon-process`（新）、`uc-core`（line 15，共享类型，不拉 iroh）、`uc-observability`（按实需）。
- `tokio` 从 `features=["full"]` 降到精简集（`rt-multi-thread`/`macros`/`net`/`time`/`signal` 按实需），`reqwest` + `rustls` 保留（client 用）。
- 核实 `uc_bootstrap::build_cli_app_facade` / `build_cli_app_runtime`（`non_gui_runtime.rs:502`）在 CLI 清空后是否还有其它 caller——无则评估删除或仅留测试用。
- **代码级零残留核实**：`rg "build_cli_app_facade|build_cli_app_runtime|uc_platform|uc_application" uc-cli/src` 应 **仅剩 `#[cfg(debug_assertions)]` 的 probe/dev 路径或为空**（其余 in-process 入口全退役）。
- **gate（终态）**：`cargo tree -p uc-cli -i iroh` / `-i diesel` / `-i libsqlite3-sys` **三者全空**（硬门禁，进 CI）；`uc-cli` 唯一依赖子树从 **实测 540** → **目标 <200** 并记录实测值；`cargo test -p uc-cli` 全过；release `uniclip --help` + 每个子命令 `--help` smoke（不含 probe/dev）；冷机（无 daemon）跑 `send` / `init` / `search` 端到端正常。

## 3. Open Question 收口（落地决策）

| OQ | 状态 | 落地结论（推荐） |
|---|---|---|
| **OQ-cli-oneshot-lifecycle** | 开放 → **收口（2026-06-06 人确认，经 Codex 5 轮评审定稿 rev6）** | **oneshot = daemon 侧自管生命周期**（推翻 rev1「CLI spawn+SIGTERM」：D22「一 profile 一 daemon」下并发 CLI 必共享 oneshot，CLI 杀法误杀他人正用 daemon=F-3、kill-9 后空闲超时在持续事件下失效=F-4）。定稿要点：① 新增 `DaemonRunMode::Oneshot`（语义＝会话租约归零 + 无 pending 任务时自终）。② **租约 = 每命令专用控制 WebSocket**（WS open=acquire、close/断=release；非单 HTTP 请求、非 token-only、非 HTTP keep-alive——R5-F2）；`watch`/`recv` 业务 WS 即租约；health probe 不持租约。kill-9 经 WS TCP reset 自动释放。③ 启动保留（acquire 首租约前不自终 + grace 窗口防冷启）。④ **两正交维度**：生命阶段 (setup/operational)×驻留模式 (Oneshot/Standalone)，重启目标按 spawn 来源（CLI init→operational+Oneshot 自终 / GUI→Standalone / `start`→Standalone）（R5-F3）。⑤ **提升/重启=受控优雅交接**（非翻 flag：run-mode 已复制进多组件；req-resp 等 drain、长连接通知 + 自动重连复用 D8）（R4-F2/R5-F1）。⑥ 「完成」语义按命令定义（`send` 遵 D18：在线计 pending+drain、离线即报不补投不重试但写 `Failed{Offline}`=F-5/R3-F3/R4-F4）。⑦ oneshot 抑制设备级 analytics（D20/R3-F2）。⑧ CLI 不 SIGTERM 共享 daemon；`uniclip stop` 仍跨平台 SIGTERM/`taskkill`+`verify_pid_identity`。recv：任意入站即退+`--filter`、等待超时默认 60s 可覆盖、超时码非 0；watch：无限、仅 SIGINT/断开退、无命令级 idle 超时。**机制落地于 P5-L，命令侧落地于 P5-1**。 |

## 4. 风险

- **置备改端点是行为变化集中点**：解锁契约（D9）+ iroh setup-bind（OQ-setup-iroh）+ **协议切换（弃 legacy `/setup/*`、新建客户端走 `/v2/setup/*`，F-6）** 交织，易回归。P5-2 须端到端 UAT（新建/加入/改身份三条置备路径全过）。**setup-mode + setup→operational 重启（D16，rev3 确认保留）**：CLI 须正确处理「daemon 在 setup-mode 驱动置备 → 重启进 operational → 重连」的全链路，重启透传 flag（D9/D10）漏传会破坏 unattended 自检；join 路径在 setup-mode 须 iroh 已 bind（OQ-setup-iroh，否则配对 503）。
- **oneshot 孤儿（daemon 侧自管模型，归 P5-L）**：若 daemon 的控制-WS 租约/pending 跟踪有 bug（连接断未释放、计数错、grace 到期未硬回收），oneshot 不自终 → 留绑 iroh endpoint / sqlite 写锁的孤儿、撞 D22 独占。**P5-L** 须把租约（控制 WS open/close）+ drain + grace 硬回收 + 空闲兜底做成 **带超时状态机**，gate 用「`kill -9` 后经连接断自终」「spawn 后未连即死、grace 到期硬回收」故障注入覆盖（持续事件下也须回收，不靠空闲）。
- **oneshot 启动竞态 / 提升交接失灵（R2-F1/R5-F1/R6-F2，归 P5-L）**：冷启「0 租约→自终」过早生效→自杀重试风暴；**提升经受控重启** 时若无 quiescing 会饿死、无事件重放会丢 watch/recv 订阅、并发冲突目标模式无仲裁。**P5-L** 须把「grace + 首租约后才自终 + 串行化重启协调器（跨进程持久化目标模式 + quiescing + drain 超时即中止 + 长连接重连·接受丢失+CLI 提示）」做进状态机，gate 覆盖冷启/双 CLI/传输中 start/watch 接管/竞态。
- **「完成」语义误判 → 传输被打断（F-5）**：`send`/`blob` oneshot 若把「HTTP 响应返回」当完成而忽略在途 P2P 传输，会在传输中途自终丢数据。**P5-L** 提供在途任务 pending 计数 + drain，**P5-1** 的 `send` 按 D18 把在线在途传输纳入 pending；回归测试覆盖「大文件 send 经 oneshot 不中断」。
- **命令覆盖不全 → 瘦身门禁失败（F-1）**：原计划漏 `upgrade`/`dev`/`mobile_sync`/`app_session`，任一残留 in-process caller 都会把 app 栈拉回。P5-4 前须按 §1.1 全集逐一退役，并用代码级 `rg` 零残留核实兜底。
- **mobile-sync 可能需新建端点（F-1）**：P5-2b 若发现现有 `mobile_lan` 端点不覆盖 CLI 控制需求，则须新建控制端点——这是计划外工作量，须在 P5-2b 首步核实后决定，别假设「零新端点」。
- **thin crate 路径解耦 / re-export 不全 → 编译断或仍拉 app 栈（F-2）**：`process_metadata`/`socket` 实依赖 `uc_application::AppPaths` + `uc_platform`，P5-0a 路径解耦不彻底则 thin crate 照样拉 app 栈、P5-0b gate 的 `cargo tree -i iroh` 空集断言会失败。P5-0b 先用 `uc-daemon-local` re-export 保 path 兼容，逐切片改 import。
- **release CLI 残留 broken 命令（F-8）**：`probe`/`dev` 若只在 daemon 侧 cfg 出 release、CLI 侧没 cfg 出，则 release `uniclip probe` 打到不存在端点必失败。P5-3 须两侧同步 `cfg(debug_assertions)`，gate 核实 release `--help` 不列 probe/dev。
- **CLI/daemon 版本错配（F-11，已定锁步同版）**：锁步同版发布下风险主要剩 **升级瞬间**——installer 换了新 `uniclip`，但旧 `uniclipd` 仍在跑。须加 startup/连接时版本校验 + 清晰提示「请重启 daemon」（或 CLI 触发 daemon 重启）。非锁步混装不在支持范围。
- **probe/dev 移走影响 E2E**：`probe restore` / `dev seed-clipboard` 是 E2E 写系统剪贴板·落库的手段，P5-3 须同步迁移 E2E 链路，否则 E2E 静默失效。
- **瘦身不彻底的隐性回流**：任何残留的 `uc-bootstrap` / `uc-application` 依赖都会把整棵 app 栈拉回。P5-4 的 `cargo tree -i iroh` 空集断言是硬门禁，必须进 CI（参考 CI 已有 drift-check 模式）。

## 5. 已确认决策（2026-06-06 人确认）

- **oneshot 生命周期 = daemon 侧自管**：✅ **租约/活跃客户端计数 + 连接存活主信号 + drain + 空闲兜底；CLI 不 SIGTERM 共享 daemon**（rev2 经 Codex 评审推翻 rev1「CLI spawn+SIGTERM+ 空闲超时」，因 D22 共享约束 → F-3/F-4/F-5）。落地见 §3 / P5-1。
- **CLI 置备走 `/v2/setup/*` + 保留 setup-mode（rev3 修正）**：✅ 新建 `/v2/setup/*` Rust 客户端，与 GUI 前端同协议；弃用 `DaemonSetupClient` `/setup/*`（前端+Rust 均无调用点，确为 legacy）。⚠️ **setup-mode 保留**（D16 强制两阶段生命周期，rev2「删 setup-mode」基于未读 D16 的错误假设已纠正）：CLI 在 setup-mode 期经 `/v2/setup/*` 驱动，置备完触发 daemon 重启进 operational（透传 `--unattended`，D9）。收口 F-6/F-7 + R2-F2。
- **CLI/daemon 版本校验（rev3 新增，owning P5-L）**：✅ 在 `ensure_local_daemon_running()`/握手处镜像 GUI 的 `package_version` + `api_revision` 检查；已在跑的旧 daemon 提示重启、API 不兼容明确报错。收口 R2-F3。
- **CLI 置备（`init`/`join`）解锁语义**：✅ **沿用 force-unlock**（headless 友好，与 P4-2 attended-仅-GUI 契约一致）。
- **发布模型 = 锁步同版**：✅ `uniclip` + `uniclipd` 同 workspace/同版/同 installer 发布；版本错配退化为「升级后重启运行中 daemon」+ startup 版本校验提示，不做能力协商。收口 F-11。
- **recv / watch 规格**：✅ `recv` 任意入站即退 + `--filter`，等待超时默认 60s 可 `--timeout` 覆盖、超时退出码非 0；`watch` 无限订阅、仅 SIGINT/断开退出、无命令级 idle 超时。收口 F-9 + R2-F4。
- **oneshot 会话租约 + 提升=受控重启（rev6）**：✅ 自终按 **每命令专用控制 WS 租约**（WS open/close 驱动，非单请求、非 token-only、非 HTTP keep-alive，health probe 不持）；`start`/GUI 接管经 **受控重启进目标模式 + 优雅交接**（req-resp drain、长连接通知 + 自动重连 D8）。两正交维度（生命阶段×驻留模式），重启目标按 spawn 来源。收口 R3-F1 / R4-F1 / R4-F2 / R5-F1 / R5-F2 / R5-F3。
- **`send` 遵 D18 投递语义（rev4/rev5）**：✅ 仅在线已接受传输计 pending；离线 **即时报告、不补投、不落 pending、不重试**，但 **仍写 `Failed{Offline}` 记录**（2026-06-06 人确认，事后常驻 daemon 可手动 resend）；CLI 输出讲清投递分布。收口 R3-F3 + R4-F4 / D18。
- **oneshot 抑制设备级 analytics（rev4）**：✅ `Oneshot` 模式只发动作级事件，抑制 `active_device_count`/`is_first_run`/heartbeat（D20）。收口 R3-F2。
- **`uc-core` 依赖**：✅ **保留**（子树 94、不含 iroh，避免 CLI/daemon 类型重复）。
- **瘦身红线**：✅ **终态子树 `<200` 作参考值 + `cargo tree -i iroh/diesel/libsqlite3-sys` 空集断言进 CI**（空集为硬门禁，数值为参考）。

### 5.1 P5-1 落地前须定的数值 —— ✅ 已全部收口（rev3 解耦 watch/idle）

- `recv`「首个满足条件入站」：✅ **任意入站即退**（提供 `--filter` 缩小）。
- **`recv` 等待超时**：✅ **默认 60s，`--timeout` 可覆盖**；超时（未等到入站）退出码非 0。
- **`watch` 不设命令级 idle 超时（收口 R2-F4）**：✅ `watch` 是 **无限订阅**，仅在用户 **SIGINT / WS 断开** 时退出（clean 退出码 0）；**不** 因 60s 无事件而退出（那会违背 watch 语义）。
- **60s idle 的两处正确归属**（避免与 watch 矛盾）：① `recv` 等待首个入站的超时；② **oneshot daemon「无任何客户端连接」时的自终兜底**（属 daemon 回收，非命令级）。`watch` 运行期间持 WS = 持租约 → daemon 不会因 idle 自终。
- 回收时序：✅ daemon 侧自管——**连接存活为主信号**（CLI 正常退出或 `kill -9` 均经连接断开释放租约触发回收），空闲超时仅兜底；CLI 不 SIGTERM 共享 daemon。

### 5.2 P5-2 落地前须核实（rev3 更新）

- **D16 setup-mode**：✅ 已核实——D16 强制 setup-mode 两阶段生命周期（保留，见上）。剩余落地细节：CLI 如何确保/感知 daemon 处 setup-mode、setup→operational 重启的 CLI 侧等待 + 重连实现。
- **OQ-setup-iroh（父 ADR 开放问题）**：setup-mode 下 iroh 何时 bind / 用什么身份 bind、new-Space vs join-Space 差异——P5-2（尤其 join）继承此上游依赖，须与 GUI 现行行为对齐，**不在 P5 内新解**。
- **mobile-sync 控制端点缺口**（P5-2b）：现有 `uc-webserver/src/mobile_lan/` 端点逐子命令是否覆盖（见 P5-2b 映射表）？不覆盖则新建；`debug` 归 dev。
- **`/setup/*` legacy 确认**：✅ 已核实——前端 SDK 用 `/v2/setup/*`，`/setup/*`(DaemonSetupClient) 前端+Rust 均无调用点，弃用安全。
- **oneshot `send` 离线 `Failed{Offline}` 记录**：✅ 已定（2026-06-06 人确认）——**写记录**，事后常驻 daemon 可手动 resend，与常驻路径一致。收口 R4-F4 / D18 自留点。

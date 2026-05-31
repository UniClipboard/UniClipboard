# ADR-008：拆出独立 `uniclipd` 二进制 + GUI 转 client + 轻量模式（Scope B）

- **状态**：Proposed（2026-05-30 草案；经一轮设计 grilling 推导 + 一轮多智能体对抗式评审订正；总评 needs-revision，已订正现状断言 / 新增 D21–D22 / 对齐 D18，待人最终确认 §3.3 措辞后定稿）
- **日期**：2026-05-30
- **相关文档**：[`adr-008-review-2026-05-30.md`](./adr-008-review-2026-05-30.md)（本 ADR 的对抗式评审报告）、[`adr-008-perf-spike-results.md`](./adr-008-perf-spike-results.md)（OQ-perf-gate P0 spike 实测结果）、[`adr-005-uc-engine-extraction.md`](./adr-005-uc-engine-extraction.md)、[`adr-007-headless-server-node-deployment.md`](./adr-007-headless-server-node-deployment.md)、[`module-boundaries.md`](./module-boundaries.md)、[`logging-architecture.md`](./logging-architecture.md)、`docs/uat/direct-daemon-ws.md`
- **承接**：[ADR-005](./adr-005-uc-engine-extraction.md) §7 Open Question #3、[ADR-007](./adr-007-headless-server-node-deployment.md) §2.2 与 §6 Open Question #3（被显式推迟的 **Scope B**，本 ADR 正式立项）

## 1. 背景

### 1.1 需求

把 daemon 从桌面进程里彻底拆出来，成为独立后台进程 `uniclipd`，并让 GUI 退化为它的客户端。四个动机：

1. **`uc-cli` 瘦身 / 解耦**：`uniclip` 二进制不应再背着整套桌面宿主层（desktop host + iroh + axum）。
2. **无头 / 服务器部署**：为 VPS / 容器提供干净的独立 daemon 二进制（承接 [ADR-007](./adr-007-headless-server-node-deployment.md)）。
3. **架构整洁 / 为多 shell 铺路**：daemon runtime 被多个 shell（`uc-tauri`、未来 `uc-macos-native`）与 CLI 共享，而非寄生在某个 shell 里。
4. **轻量模式（核心定性动机）**：对标 clash-verge / mihomo——进入轻量模式后 GUI 进程整体退出，后台只留 `uniclipd` 常驻；重新打开 GUI 又能重新连上后台 daemon。

动机 4 是 **定性动机**：它要求 GUI 进程可以来去而 daemon 长驻，直接否决了"GUI 进程内托管 daemon"的现状模型。动机 1–3 单独看用"独立二进制 + 保留 GUI in-process"即可满足；动机 4 把目标推到"GUI 永久 client 化"。

### 1.2 现状（评审期已核实）

> **纪律声明（评审补）**：本节断言经一轮代码核实，已知存在描述漂移（连接信息为命令轮询非 `daemon://` 事件、token 经 `daemon.token` 文件且 bearer 当前进 webview、`start_in_process` 为 `pub(crate)` 内部函数）。落地前须对本节每条断言按当前 `src-tauri/crates` 代码逐条复核，依赖错误前提的决策措辞同步订正。下文已对 C1/C7 两处订正。

- daemon runtime 物理上住在 `uc-desktop/src/daemon/`：对外 **公开** `uc_desktop::daemon::run`（`pub`，跨 crate 阻塞入口）；`start_in_process` 实为 **`pub(crate)` 进程内共享装配函数**，仅 `uc-desktop` 内部使用（唯一 caller 为 `daemon_probe.rs`），**无外部调用方**。该装配函数是 **run_mode 无关的 runtime 起点**（`run` 自身也调它），迁入 `uc-daemon` 后应保留为 daemon 的 start 主体；D2"删 `start_in_process`"精确指 **删其 GuiInProcess 调用方与 GUI-handle-shutdown 语义**，非删装配主体。
- GUI 走 `DaemonRunMode::GuiInProcess`：daemon 与 Tauri **同进程**、共享同一份 `AppFacade`（`uc-tauri/src/commands/space_setup.rs`、`uc-desktop/src/daemon/run_mode.rs`）。
- **关窗 ≠ 退出**：`uc-tauri/src/run.rs` 的 `CloseRequested` 处理是 `api.prevent_close()` + `window.hide()`——关窗即隐藏到托盘、进程不退；托盘菜单 "Quit" 才 `app.exit(0)`。
- **无独立 daemon 二进制**：`Standalone` 模式靠 `uniclip start` detached-spawn `uniclip daemon`（同一 `uniclip` 二进制 + 隐藏子命令）。
- **`uc-daemon` crate 当前不存在**（`uc-desktop/AGENTS.md` 中"`uc-daemon` 是兼容壳"的描述已过时，落地时同步订正）。
- **通信地基已就绪**：daemon 已在 `127.0.0.1` 上跑 HTTP + WebSocket（`uc-daemon-local/src/socket.rs` 按 `UC_PROFILE` 解析端口，默认 `42715`；`uc-webserver/src/api/server.rs` 用 `TcpListener::bind` 绑 loopback），鉴权为 `daemon.token`（bearer，`0o600`）→ `POST /auth/connect` 换 JWT session。
- **前端已直连 daemon**：`docs/uat/direct-daemon-ws.md` 记录 M003 已把前端从 Tauri `invoke()` 迁到"浏览器直连 daemon HTTP + WS"（bearer→session、WS `?auth=Session%20TOKEN`、snapshot 走 WS、断线重连 max 30s / 10 次）。**当前在 `GuiInProcess` 前提下连**：连接信息（端口/token）经前端 **~500ms 轮询 Tauri 命令 `get_daemon_connection_info`** 获取（`src/lib/daemon-connection-info.ts`；命令内由原生侧按 `socket.rs` profile 规则解析端口 + 读 `daemon.token` 文件），**不存在 `daemon://connection-info` 事件**；该 **RAW BEARER 经命令进入 webview**、由前端自行 `POST /auth/connect` 换 session（contract 注释明写 "Raw bearer token"）。**故 D5/§5.3#6"bearer 不进 webview"是对此现状的*反转*，非保持。**
- **进程协调件已存在**：`DaemonOwnership`（Owned/External/None 三态）、`daemon_probe.rs`（probe → Compatible/Absent/Incompatible 三态）、`DaemonPidMetadata`（pid + mode + started_at_ms）、版本握手（`DAEMON_API_REVISION` + package_version，`daemon_probe.rs` 严格比对，不匹配可 `terminate_incompatible_daemon`）。
- **解锁语义**：`run_mode.rs` 已区分 `GuiInProcess`（尊重 `auto_unlock` 设置、靠 GUI 弹框兜底）vs `Standalone`/`ServerHeadless`（强制 keyring 解锁，因无 GUI 兜底）。
- **可观测性**：日志走 `tracing_appender::rolling::daily(logs_dir, "uniclipboard.json")`（固定文件名、不带进程区分）；Sentry 在 `uc-bootstrap/src/tracing.rs` 进程内 init 一次；product analytics 走 PostHog HTTP，`compose_event_context` 有 **进程内** 幂等去重（`scope.rs` 已有 `gui-host`/daemon 角色维度但仅用于 Sentry tag）。

### 1.3 与 ADR-005 / ADR-007 的关系

- 本 ADR 是 [ADR-007](./adr-007-headless-server-node-deployment.md) §2.2 中 **被否决并要求"单开 ADR"的 Scope B** 的正式立项；不推翻 ADR-007 的 headless 决策，而是把 ADR-007 仍依赖的"单二进制自启"约定替换为"独立 `uniclipd` 二进制"。
- 与 [ADR-005](./adr-005-uc-engine-extraction.md) 自洽：本 ADR 只动 **host / shell / 进程模型** 层，不触碰引擎抽取。`uniclipd` 对引擎而言只是又一个"被注入特定适配器的一次 start"。
- 对 ADR-005 §7 OQ#3 的"是否统一走 daemon-client"给出 **肯定回答（GUI 侧）**：GUI 永久 client 化；CLI 的一次性置备命令（`init`/`join`）仍保留 in-process。
- 对 ADR-005 §2.5 投递语义做 **措辞精化**（见 D18，与 §2.5"数据落发送方本地 + 用户主动 resend、不自动补投"一致）：常驻 daemon 仅多一个长驻 **手动 resend** 入口 + 暖连接，**两条路径都不做自动最终一致**——本 ADR 不引入 ADR-005 已否决的 presence 上线自动补投。

## 2. 决策

### D1：恢复 `uc-daemon` crate，承载 daemon runtime + `uniclipd` 二进制

- 新建 `uc-daemon` crate，把 `uc-desktop/src/daemon/` 的整块 runtime 迁入，产出二进制 `uniclipd`。
- 形态：**单 crate 带 `[[bin]] name = "uniclipd"`**（lib 部分供测试 / 其它 host 复用）；OQ-crate-shape 评审收敛为 **单 crate**（不拆 lib+bin），main.rs 极薄。
- `uc-daemon` **不依赖任何 GUI 框架**（延续 `uc-desktop` 的 GUI-agnostic 铁律），**常开 `uc-webserver`**（HTTP/WS 是 GUI 与 CLI 唯一通道）。
- 能把 runtime 干净迁出 `uc-desktop` 的前提正是 D2（删 `GuiInProcess`）。

拆分后各 crate 角色：

| crate | 拆后职责 | 依赖 GUI? |
|---|---|---|
| `uc-daemon`（新建） | daemon runtime + `uniclipd` 二进制；自有 runtime/信号/解锁；常开 webserver | ❌ |
| `uc-desktop` | GUI 侧宿主胶水：daemon 生命周期协调（`DaemonOwnership`/`daemon_probe`/spawn-attach-detach）、shortcuts、client 侧 `DesktopRuntime`、桌面事件源 | ❌ |
| `uc-tauri` | Tauri 壳：窗口/托盘/全局快捷键/生命周期；经 HTTP/WS 连 `uniclipd` | ✅ |
| `uc-cli` | 去 `uc-desktop` 依赖；spawn `uniclipd`；业务命令走 daemon API（置备除外） | ❌ |

> `build_process_runtime` / `ProcessRuntimeHandles` 装配归属：OQ-desktop-residue 评审收敛为 **随 D1 整组迁入 `uc-daemon`**，`ProcessRuntimeHandles` 在 `uc-desktop` 删除；**迁移须与 D2/D5 同切片**（详见 §3）。

### D2：GUI 永远是独立 `uniclipd` 的 client，删除 `GuiInProcess`

- 删除 `DaemonRunMode::GuiInProcess` 及其 in-process 装配路径（GuiInProcess 调用方、GUI 持有 `DaemonHandle` 直接 shutdown 等）；`start_in_process` 装配主体作为 daemon start 保留并迁入 `uc-daemon`（见 §1.2 C1 订正）。
- GUI **不保留** in-process 快路径。评审已否决双路径方案：双数据路径维护成本翻倍，且"运行中从 in-process 热迁移到独立进程"≈ 热迁移一个活跃 iroh node，难以做对。
- normal 模式与轻量模式的唯一区别 **仅在于退出 GUI 时是否一并停止 daemon**，而非数据路径不同。

### D3：关窗 / 退出三态；轻量模式 = GUI 进程整体退出、`uniclipd` detach 留守

三态明确区分：

| 动作 | 行为 | 托盘 | daemon |
|---|---|---|---|
| **关窗**（CloseRequested） | 隐藏到托盘，Tauri 进程仍在 | 有 | 留 |
| **轻量模式**（托盘菜单 / 显式） | GUI 进程整体退出 | **无** | 留（detach） |
| **彻底退出**（托盘菜单 / 显式） | GUI 进程退出 | 无 | **停** |

- 轻量模式下 **无托盘、无全局快捷键**（GUI 进程全退）。唤回 GUI 的 **唯一入口 = 重新启动 app**（点 Dock/开始菜单图标 → 冷启动 → probe attach 既有 `uniclipd` → reconnect/resync）。`uniclipd` 完全不碰 GUI/快捷键，保持 GUI-agnostic。
- 这点比 clash-verge 更激进（clash-verge 轻量保留托盘）——本产品选择"轻量 = 完全隐形"。**已知 UX 风险**：用户进轻量后屏幕零痕迹，可能忘了它在跑 / 不知如何唤回；缓解见 §5.2（首次进入轻量给一次性系统通知，不留守组件）。
- **ownership 模型须重做（评审订正）**：原稿"映射既有 `DaemonOwnership(Owned/External)`，补全 detach 路径"方向错误——`Owned` 唯一生产者是 `daemon_probe set_owned → start_in_process(GuiInProcess)`，被 D2 整条删除；拆分后 GUI 与 daemon 永远两进程、结构上 **恒为 External**。后果：①"关窗→留"与"轻量→留 (detach)"在 daemon 层是 **相同的 no-op**（daemon 从未 attach 到 GUI 生命周期，"detach"动词误导），差异纯在 GUI 进程层；②"彻底退出→停"若照搬 `take_owned()` 则 External 永返 None、根本不成立；③两态无法区分"**本 GUI spawn 的可停 daemon**"vs"用户 `cli start` 的常驻 daemon"，无条件停会误杀后者。故须 **新增"由本 GUI spawn 的 standalone daemon（彻底退出时可优雅停止）"状态**（spawn 归属持久化到 `DaemonPidMetadata` 或 ownership 记录），"彻底退出→停"只停 **本 GUI 自己 spawn 的** daemon，对用户显式 `cli start` 的常驻 daemon 退化为"只退 GUI、不停 daemon"。与 D22 协同。
- 登录自启目标改为 `uniclipd`（见 D10/D17），GUI 降级为可选前端。

### D4：复用现有 `127.0.0.1` HTTP+WS，不新开 IPC

- GUI↔daemon 不引入 UDS / Windows named pipe / 自定义 IPC：daemon 本就必须对 CLI 与 mobile 暴露 HTTP/WS，且前端已在用（M003）。再加一条 IPC = API 面 fork 成两套，违反单一来源。
- 安全诉求已由现有通道解决（仅 loopback + `daemon.token` → JWT session）；这正是 clash-verge / mihomo 的形态（localhost TCP + secret）。
- 统一收口在 `uc-daemon-client` 之后（前端 WS 也视为"client 契约"的一部分），使将来若需 UDS/pipe 加固是"换传输不换 API"。**本期不做传输替换。**

### D5：GUI 延续"前端直连 daemon HTTP+WS"，Tauri shell 退为纯外壳

- 数据流：`前端 ──HTTP/WS（127.0.0.1）──▶ uniclipd（uc-webserver）──▶ AppFacade ──▶ 引擎`。
- `uc-tauri` 只负责：窗口 / 托盘 / 全局快捷键 / 生命周期 + daemon 的 spawn / probe / attach / detach；**不再持有进程内 `AppFacade`**，所有业务（含 setup/unlock，见 D15）经 HTTP/WS。
- **端口与 token 发现**：Tauri **原生进程** 从共享数据目录自解析（端口走 `socket.rs` profile 规则、token 读 `daemon.token`，原生侧有文件权限）；**前端 webview** 经原生侧注入连接信息后直连。这是"跨 GUI 重启重连"的关键。
- **token 边界（注意：本项是对现状的*反转*，非保持）**：bearer `daemon.token` ** 永不进 webview**——Tauri 原生侧用 bearer 调 `/auth/connect` 换取短命 session JWT（TTL 5min），只把 session 注入前端。XSS / 恶意前端依赖至多偷到 5min session，偷不到长期 bearer。** 当前态（已核实，见 §1.2 C7）：webview 拿到的是 RAW BEARER、自行 `POST /auth/connect` 换 session、连接信息靠 ~500ms 轮询 `get_daemon_connection_info`**；把 `/auth/connect` 从前端搬到原生侧、只下发 session，是 **P3 净新工作**，须计入工作量。
- **session 续期通道（评审补，blocker——bearer 抽走后的硬前提）**：session TTL=300s，现状靠前端持 bearer 自行 pre-emptive refresh + 401 重换。bearer 止于原生后，**原生侧须周期性（约 240s，即 TTL 80%）用 bearer 换新 session 并经 Tauri event 或 `get_daemon_session` 命令推给 webview**；前端改为不持 bearer、401/到期时向原生请求新 session；WS 重连前先刷新 session（与 D8 串联）。续期失败的 GUI 降级行为（红条提示 vs 静默重连）见 D8。传输形态收口见 OQ-session-refresh。

### D6：大负载按需拉取，不全量推过 socket

- GUI 默认拿 **元数据 + 缩略图**；原图 / 文件等大负载 **按需向 daemon 拉**（复用既有 blob / resource 的 **寻址 / 鉴权语义**），不在 WS 上全量推送。
- **已知约束（评审补）**：现有 blob/thumbnail 端点把整块字节读成 `Vec<u8>` 再整体返回（`resource/mod.rs blob()` → `BinaryResourceView{bytes:Vec<u8>}`、整块塞 axum response），无 streaming/Range，为 MB 级图片设计；D6 外推到原图/文件（`max_file_size` 默认 5GB 量级）后，拆进程下每次预览在 daemon 端 buffer + 走 loopback，64MiB×并发线性吃 RSS。故"复用既有接口"仅限 **寻址/鉴权语义**，**传输形态（full-buffer vs streaming / 是否支持 Range）由 spike 定**。
- 阈值与接口形态由 P0 spike **已实测定稿**（见 [`adr-008-perf-spike-results.md`](./adr-008-perf-spike-results.md)）：loopback 传输非瓶颈（full-buffer TTFB 256MiB 仍 <10ms、吞吐 8–12GiB/s），瓶颈是 **full-buffer 内存×并发**（64MiB×4=328MiB、256MiB×4=1.29GiB + p99 飙到 162ms）。钦定 **自动内联预览阈值 = 8MiB**（≤8MiB 走现有 full-buffer 端点内联，>8MiB 转显式下载、不自动预览）；大文件显式下载路径 **优先改流式 `BlobReaderPort`**（实测 streaming RSS 恒定 ~6–10MiB、TTFB 拍平），改流式前对 >阈值的并发 full-buffer 拉取加信号量封顶。

### D7：`uc-cli` 去除 `uc-desktop` 依赖；业务命令走 daemon API，置备 in-process

- `uc-cli` 不再依赖 `uc-desktop`；按路径 spawn `uniclipd`。保留 `uc-bootstrap`（置备的 in-process wiring）+ `uc-daemon-client`/`uc-daemon-local`/`uc-daemon-contract`。
- **业务命令路由**：`send`/`recv`/`watch`/`search` 等统一走 daemon API；daemon 在跑 → 直接调；daemon 没跑 → **拉起 oneshot daemon 发完即退**（常驻仅由 `start` / 轻量模式 / GUI 显式建立）。
- **端点建设（评审补）**：`send`/`recv`/`watch` 当前走进程内自包含 facade，且 `dispatch_clipboard_snapshot`/`resend_entry`/`subscribe_inbound_clipboard_notices`/`cancel_inbound_transfer` 仅是 `app_facade` 进程内方法、**webserver 零暴露**。"统一走 daemon API"须 **先新建这批 daemon HTTP/WS 端点**，并排进实施路径（新增 P2.5 或在 P3 显式列 CLI 端点）。本决策是对 `refuse_if_daemon_running` 的 **反转**（拒绝→转发），须交代 refuse 在 `send`/`recv`/`watch` 上的退役顺序（与 D11 降级一致）。
- **oneshot 生命周期（评审补）**：`DaemonRunMode` 当前无 oneshot 模式（需新建）。按命令分类定生命周期：`send`/`search` 等请求 - 响应类"发完/查完即退"；`recv`（等第一个含 free-file 入站）/`watch`（无限订阅）无天然完成信号，detached-spawn 后 CLI 对其无句柄、退出后全功能 daemon 会成 **孤儿**（绑 iroh endpoint、持 sqlite 写锁，撞本决策要避免的独占冲突）。故长连接类要么不允许在无常驻 daemon 时拉 oneshot（提示先 `start`），要么定义退出条件（CLI 断 WS / 收一次后 / 超时）与回收责任方（CLI 复用 `stop.rs` 的 SIGTERM + pid metadata 终止其拉起的 oneshot）。收口见 OQ-cli-oneshot-lifecycle。
- **例外**：`init` / `join`（置备 / 改身份）保留 in-process（ADR-007 §2.7 置备模型；`refuse_if_daemon_running` 对它们仍正确）。
- 现有 `uc-cli/src/local_daemon.rs` 的 detached-spawn + health-probe 逻辑上移为 GUI 与 CLI 共享（放 `uc-desktop` 或 `uc-daemon-local`，均 GUI-agnostic）。
- 避免撞独占资源（sqlite 写锁 / iroh `BIND_LOCK` / 同 identity 双绑）：这正是"业务命令走 daemon、不再进程内起第二套 facade"的根本理由。

### D8：GUI 重连必须 reconnect + resync

- GUI 重开后必须重新订阅 WS topics 并 **拉取一次全量状态快照**，否则界面为空。
- 复用 M003 已有的重连机制（max 30s / 10 次），补全 attach 既有 daemon 时的初始 resync。
- **session 刷新（评审补，blocker）**：现状 `daemon-ws.ts` 重连直接读 `currentSession?.token` 从不 refresh；daemon 重启后 `jwt_secret` 由 `OsRng` 每次随机生成、never persisted，旧 JWT 全失效 → WS 握手 401 → 用同一陈旧 token 循环重连 → 耗尽 10 次后 **永久放弃**（无机制复活）。**故 reconnect 契约须含"重连前先 `refreshSession()` 再开 socket"**（或收到 401 close code 时触发一次刷新后重连），并在成功重连时 **重置 `_reconnectAttempt`**（区分本轮断连重试 vs 历史累计，避免长驻轻量模式重试预算被累计耗尽）。与 D5 session 续期通道协同。D3 轻量核心路径（daemon 独立重启后 GUI 冷重连）正是此场景。
- **resync 覆盖面（评审补）**：`build_snapshot_event` 当前对 CLIPBOARD/FILE_TRANSFER/ENCRYPTION 返回 `Ok(None)`，剪贴板全量走独立 HTTP `getClipboardEntries`（绑 mount/加密态变化，非 WS 重连）。"subscribe→拉全量快照"对 **最核心的剪贴板面板当前无此通路**。P3(c) 须二选一落地：(a) 给 CLIPBOARD/FILE_TRANSFER topic 在 `build_snapshot_event` 补 snapshot 分支；或 (b) `daemonWs` 重连成功时暴露 `onReconnect` 信号、consumers 据此重走 HTTP 全量拉取。区分 **冷启动 attach**（mount 全量拉）与 **运行中重连**（reconnect 触发 refetch）。补一致性要求：snapshot 带版本游标/last-event-id，订阅与拉取先后约定，窗口内事件 buffer 后 dedupe replay。

### D9：解锁契约——attended / unattended，禁止"无人值守自启 + auto_unlock=false"

- `uniclipd` 区分两种 **启动契约**（由拉起方传入，如 `--unattended` flag / env，**不是** run mode）：
  - **attended**（GUI 会来 / 用户在场）：尊重 `auto_unlock`；`false` → 保持 locked 等 GUI 解锁（GUI 必来，不卡死）。
  - **unattended**（自启 / headless / 轻量常驻）：要求 keyring auto-unlock 可用。
- **互斥校验**：用户同时配置"无人值守自启" + `auto_unlock=false` 是语义互斥（既要"每次手动解锁"又要"无人值守"），**禁止**——在配置层拦截。
- **校验权威**：写成纯函数（`uc-daemon-local` / `uc-bootstrap`，单一事实源）；GUI 设置页 + CLI 调它做 **友好前置报错**；**唯一硬边界是 `uniclipd --unattended` 启动自检**（任何拉起路径都必经的瓶颈），互斥则 **fail-fast 退出 + 写机器可读状态文件**，供下次 GUI 打开读取并红条提示。
- unattended 启动若 keyring 不可用（密钥被删 / keyring 锁死），daemon **显式启动失败并写明原因**，不假装活着。
- **瓶颈完备性（评审补）**："唯一硬边界 = `--unattended` 自检"成立的前提是 **所有能拉起 operational daemon 的路径都必经该自检**。须枚举并逐条说明各路径如何携带并触发 `--unattended`：GUI spawn / `cli start` / service-manager 单元（D10：ExecStart **必须固定带 flag**）/ oneshot 升常驻 / D16 setup→operational 重启（**须透传 flag**）。D11"没跑→CLI 直写文件"写入触发自启单元变化的设置（`auto_unlock` 等）时 **必须经本节纯函数前置校验**，不得靠后续自检兜底。互斥校验的左操作数是 **"unattended 自启开关"**（随 D10 per-profile 单元投影新增的字段），而非现有驱动 GUI 登录项的 `general.auto_start`。

### D10：autostart = settings 的派生投影，投影目标 = OS 原生自启/保活载体

- settings 是 single source of truth；autostart 开关是其 **派生写动作**：改 settings → 同步重写 / 删除投影；关自启 → 删除投影，杜绝"settings 说不自启、plist 还在"的幽灵自启。
- **投影目标 = OS 原生自启/保活载体**（macOS launchd / Linux systemd-user 单元 / Windows Task Scheduler 任务，见 D17/OQ-windows；在 `uc-platform` 抽象为 `StartupIntegrationProvider`），而非"启动文件夹 / 注册表 Run 键"——这样"登录自启"与"崩溃保活"尽量用一份配置解决（Windows 保活弱，见 D17）。
- 自启开关 **per-profile**（settings 本就 per-profile 隔离），默认仅主 profile 开启（见 D19）。

### D11：settings 单一 writer——终态 daemon API，本期分层落地

- **终态（方向）**：daemon 是唯一 writer，GUI/CLI 改设置一律 `PATCH /settings`，daemon 串行写，无跨进程文件竞争。
- **本期落地**：daemon 在跑 → 走 `PATCH /settings`；daemon 没跑 → 允许 CLI 直接写文件（置备路径）。ADR-007 §2.7 置备契约 **基本不动**。
- `refuse_if_daemon_running` 从"写设置拦路虎"降级为只拦真正互斥操作（如 `join` 改身份）；写设置在 daemon 在跑时走 API 而非被拒。
- **OS 进程独占副作用的写者归属（评审补）**：daemon 单一 writer 只覆盖 **纯设置落盘**；**GUI 进程独占的副作用（全局快捷键 OS 注册必须在 Tauri 主线程）不能由 daemon 承担**（§5.3#1 禁 daemon 依赖 GUI 框架）。这类 key 写盘后须由 daemon 发 `SettingsChanged` 推给在场 GUI 本地 rebind，归属分类见 D12；autostart 投影由 daemon 侧 settings 订阅者执行（D10）。**CLI 没跑直写文件须先持有该 profile 启动锁**（D22），闭合与并发启动 daemon 之间的 TOCTOU 写竞争。

### D12：settings 热生效——通用事件总线 + 全字段生效分类表

- `PATCH /settings` 写完后，daemon 发内部 `SettingsChanged{ changed_keys }` 事件；各子系统（mobile_lan lifecycle / clipboard / iroh / …）订阅自己关心的 key，自行 reload / rebind / ignore。复用既有 `MobileLanLifecycleController`（ADR-007 §2.3 "mobile_lan 生命周期由 settings 驱动"）。
- 每个 settings 字段标一个 **生效类别**（hot-reload / restart-required / daemon-irrelevant），维护一张 **全字段生效分类表**（同时是"改哪个设置会发生什么"的活文档）。
- **副作用执行进程归属（评审补，正交维度）**：除上述类别外，每个会触发 OS 副作用的字段再标一维 **daemon-side / gui-side / service-manager-side**。`gui-side`（全局快捷键）的字段在 daemon 写盘后经 `SettingsChanged` 推给在场 GUI 由 GUI 本地 rebind，并交代原子性（daemon 写盘成功但 GUI OS 注册失败时的回滚/告警，对应现有 `KeyboardShortcutsUpdateLock` 回滚）；**轻量模式 GUI 不在时这类 key 标 `requires_gui`、延迟到下次 GUI 起来生效**。`service-manager-side`（autostart 投影）由 daemon 侧订阅者执行（D10）。
- `PATCH /settings` 响应显式回 `{ applied: [...], requires_restart: [...] }`，CLI 打印"已生效 / 需重启"，GUI 弹对应提示——不静默不生效。

### D13：版本——捆绑分发 + 双分发形态 + 控制面/peer 兼容性正交

- **桌面**：`uniclipd` **捆绑进 GUI 安装包**，与 GUI 同版本、同发布、同更新渠道（`tauri-plugin-updater`）。磁盘 `uniclipd` 版本永远 = GUI 版本；唯一"漂移"是"运行中旧进程 vs 磁盘新二进制"，下次 daemon 重启自然收敛。既有 `terminate_incompatible_daemon` + 版本严格匹配整套保留——危险路径（杀活进程）在捆绑分发下几乎永不触发。
- **headless**：`uniclipd` 走独立 Docker image / 二进制 release（ADR-007 §2.8 既有 image CI）；容器内 CLI + daemon 同 image → 同版本，无跨机漂移（VPS 上无外部 GUI 来连）。
- **关键澄清（写进边界）**：版本一致性是 **机器内** 不变量，**非全局**。一个空间里桌面节点 v1.3、VPS 节点 v1.2 完全允许——它们是 P2P **peer**，靠 iroh / 业务协议的 **跨版本兼容**。`DAEMON_API_REVISION` 只管 **本机 client ↔ 本机 daemon** 的控制面协议，**与 P2P peer 之间的 wire 兼容是完全独立的两套**。捆绑分发解决前者；后者由各自协议版本协商负责，不在本 ADR 范围。
- **降级 / 回滚（评审补）**：D13 论证的是 **单向升级** 收敛；`tauri-plugin-updater` 存在用户回滚路径。磁盘 daemon 降级但 service-manager 可能正跑更高版本 detached `uniclipd` → `DAEMON_API_REVISION` 不匹配触发 `terminate_incompatible_daemon` **杀活进程**（§5.3#8 警惕的危险路径，"捆绑分发几乎不触发"论证 **在降级方向失效**）；高版本写过的 `app_version_state`（`schema_version` 不匹配直接报 corrupt）被降级后低版本读到。须定降级时 detached `uniclipd` 与磁盘低版本的 **收敛方向**（谁先停谁 / 是否允许低版本 client 杀高版本 daemon，还是反过来拒启并红条提示）、settings/数据 `schema_version` 前向不兼容降级行为。收口见 OQ-downgrade-rollback。

### D14：安全——威胁模型显式化 + 生产收紧 CORS

- **root of trust** = `daemon.token`（`0o600`）+ loopback 绑定。**显式声明威胁模型：本机同 UID 进程视为可信，daemon 不防御同 UID 攻击者**（能读 `daemon.token` 即可全控；这是 Unix 本地服务常态，与 Docker socket / ssh-agent / clash 9090 同级）。轻量模式把暴露时间从"分钟级"拉为"常驻"，故必须在 ADR 显式记录而非默默继承。
- **生产 CORS 只留 Tauri webview 平台 origin 集合**（评审订正）：现网 `is_allowed_cors_origin` 放行三平台 webview 源 `tauri://localhost`(macOS) + `http://tauri.localhost`(Windows WebView2) + `https://tauri.localhost`(Linux WebKitGTK)——**这三条须全保留**，否则打掉 Windows/Linux webview。降级为 **dev-only** 的仅是 `http://localhost:*` / `http://127.0.0.1:*` / `http://[::1]:*` 三条通配（`cfg(debug_assertions)` 或显式 dev flag 包起），杜绝任意本地网页对常驻 daemon 发起 CORS 请求。
- 口令端点（现网真实路由 `/v2/setup/initialize|redeem|switch-space`，含 D15 退役"口令不出进程"后新增的口令 unlock 端点）继续留在 L2+ 受保护层——"强制 session JWT、拒裸 bearer"**已是全局不变量**（`auth_extractor_middleware`），非口令端点专项；**审计日志绝不记口令 body**。（注：`/encryption/unlock` 是 keyring-only 自动解锁、不收 passphrase body。）

### D15：setup / unlock 口令走 loopback，退役"口令不出进程"不变量

- 现状 `space_setup.rs` 注释"passphrase 绝不上 socket，in-process 直调是唯一安全通路"是 `GuiInProcess` 时代产物——当时同进程，"不上 socket"是免费的。
- D2 拆分后该不变量不再免费（保留它要么破坏 Tauri 壳纯净度、要么引入密钥交换），且在 D14 "同 UID 即可信"模型下 **零增量收益**（能嗅探 loopback 的攻击者 = 同 UID = 已能 dump daemon 内存里的 master key）。
- **决策**：setup / unlock 口令 **走 loopback HTTP**（`POST /v2/setup/*` 等），在本 ADR 正式退役该不变量。配套加固见 D14（口令端点强制 session JWT + 不记 body）。`space_setup.rs` 那条注释在 P3 删除 / 改写。

### D16：daemon 两阶段生命周期——setup-mode / operational

- `uniclipd` 启动读 `is_setup_complete()` 分流：
  - `false` → **setup-mode**（轻装，**精确到子系统级**：起 HTTP server + `/setup/*` 路由；**clipboard capture / sync dispatch / mobile_lan 不构造**）。
  - `true` → **operational**（全功能 + D9 解锁契约）。
- **iroh 边界（评审订正，原"不构造 iroh"不准确）**：`/v2/setup/*` 由 `SpaceSetupFacade` 驱动，装配时无条件 `IrohNodeBuilder::bind` 并装 pairing/presence ALPN；**配对（join/redeem）本质依赖 iroh**（`issue_invitation` 有 `NetworkNotStarted`、`redeem` 有 `SponsorUnreachable`，邀请码内含 sponsor 的 iroh 地址）。故 setup-mode 须区分两条路径：
  - **new-Space（`initialize`，全新单设备）**：本地路径，可不 bind iroh。
  - **join-Space（`redeem`，加入既有 Space）**：**必须启动 iroh 网络栈**——GUI 经 D2/D5/D7 永久 client 化、无 in-process facade，其 join 必经 daemon `/setup/*`；joiner 未 setup-complete 进 setup-mode 若无 iroh 则配对端点直接 503，主流 onboarding 撞墙。
- 全新用户流程：开 GUI → 拉起 daemon 进 setup-mode → 前端经既有 `/setup/*` HTTP + setup WS 驱动（`uc-daemon-client/http/setup.rs` 已就绪）→ setup 完成。
- **setup-mode → operational 转换 = 重启 daemon**（不热升级；setup 一次性，热升级要处理"半装配状态"不值；GUI 经 D8 reconnect 一次即可；重启须透传 `--unattended` 等启动契约 flag，见 D9）。
- `check_setup_complete` 闸门语义精化为"**拦 operational 启动，不拦 setup-mode**"。
- **新 OQ**：setup-mode 下 iroh 何时 bind、用什么身份 bind（setup 期设备身份尚在生成中）——见 OQ-setup-iroh。

### D17：保活——OS service manager + 崩溃可见性兜底，不自建 watchdog

- 轻量模式下 `uniclipd` 无父进程监督（GUI 已退），其崩溃 / OOM kill 是 **今天不存在的新失效模式**。保活交给 **OS 原生自启/保活载体**：
  - macOS：`launchd` LaunchAgent（`KeepAlive`，用户级免 root）。
  - Linux：`systemd --user`（`Restart=on-failure`）；非 systemd 发行版 fallback 见 OQ。
  - Windows：**每用户 Task Scheduler 任务（`schtasks` AtLogOn，免管理员）** 作 autostart 投影目标，不走 Windows Service；保活在 Windows **显式降级**（Task Scheduler 仅登录自启，崩溃保活靠下述 crash 可见性 + 任务有限重试）。可选：安装期已接受 UAC 时注册真 Service 作高保真档，默认免管理员。（OQ-windows 已收敛）
  - headless：Docker `restart: unless-stopped`（ADR-007 既有）。
- **不自建 watchdog**（避免重新发明 systemd + 崩溃循环风险）；不靠 GUI 保活（轻量模式 GUI 不在）。
- **崩溃可见性兜底（评审订正方向）**：OOM/SIGKILL/`panic=abort`（release profile 已设）**落不了"退出时 crash marker"**——而这恰是轻量常驻最可能的死法。改为可靠的 **反向模式：启动写 start marker、graceful shutdown（D21）才清除；下次启动检测到 PID 文件残留 + 无 clean-shutdown sentinel = 上次异常退出**。"重启 N 次"计数源在 service-manager 模型下不存在（launchd/systemd 不传重启序号），须 `uniclipd` 自维护持久计数器 + 清零策略（稳定运行≥T 秒归零 / 用户显式彻底退出归零），或降级承诺为"仅提示近期异常、不报次数"。下次 GUI/CLI 起来读到 → 红条提示；**轻量模式长期不开 GUI，须另定主动通知路径**（如 systemd `OnFailure` 单元发系统通知）覆盖"轻量中途死亡"，与 D3"进入轻量"一次性通知区分。区分 launchd（仅 ~10s throttle、无 `StartLimitBurst` 硬熔断）与 systemd 语义——绝不静默。

### D18：投递语义精化——尽力投当下在线者 + 离线显式报告（与 ADR-005 §2.5 对齐）

- **评审订正**：原稿给常驻路径写了"presence monitor / keepalive 探到 peer 上线 **补投**"，但代码核实——`presence_monitor` 只桥接 WS `peers.changed` 快照、`peer_keepalive` 收 `Online` 仅 reset backoff + 暖连接拨号，**均不重投 entry**；全仓 **唯一** 重投路径是用户主动 `ResendEntryUseCase`。ADR-005 §2.5 亦 **显式否决** 自动补投（不引入新表/新 Port/不挂 presence 上线钩子）。故两条路径都是"**尽力投当下在线者 + 不自动补投**"，真实差异 **不是** 最终一致与否：
  - **常驻 daemon 路径**：保留 offline 落本地（`EntryDeliveryRecord(Failed{Offline})`）+ **长驻 UI/CLI 手动 resend 入口**；keepalive 仅暖连接、**不自动补投**。
  - **oneshot / CLI-only 路径**：`uniclip send` 即时尽力投当下在线者，对离线目标 **显式报告**"设备 X 离线、本次未投递"，**不补投、不落 pending、不静默**；进程退出即无后续 resend 入口。（澄清：oneshot 对离线目标是否仍写 `Failed{Offline}` 记录，决定事后常驻 daemon 接管 resend 是否可行——须在落地时定。）
- 即两条路径均 **不做自动最终一致**——"想让离线设备最终收到"靠 **用户主动 resend**（常驻多一个长驻 UI/CLI 入口 + 暖连接），而非 presence 自动补投。CLI 输出必须把投递分布讲清楚。

### D19：多 profile × 轻量

- **实例模型**：N profile = **N 个独立 `uniclipd` 进程**（数据目录 / 端口 / keychain / iroh identity 全隔离，`BIND_LOCK` 防同 identity 双绑——单进程多 profile 与现有隔离模型相悖，架构逼定）。
- **自启 / 保活粒度**：per-profile service manager 单元（unit 名带 profile）+ per-profile 自启开关（D10）。**默认仅主 / 默认 profile 开启轻量自启**，非主 profile 默认前台、显式开启才注册单元（避免 Windows 服务注册 ×N）。
- **托盘**：托盘归 GUI；轻量模式无托盘（D3）。
- **GUI 运行期切 profile（评审补）**：D19 未定 GUI 内切 profile 的运行期连接语义——是热切换（断当前 `uniclipd` WS、按新 profile 重走端口+token 发现+session+resync、必要时拉起目标 profile 未运行的 `uniclipd`）还是切 profile 强制 GUI 冷启动。须在 D5/D19 落地时定（与 OQ-downgrade-rollback 同属运行期连接语义补全）。

### D20：可观测性——每进程独立日志文件 + daemon 为 analytics 唯一权威

- **日志**：每进程角色独立日志文件（复用 `scope.rs` 已有 `gui-host` / daemon / cli 角色维度做文件名前缀，如 `uniclipboard-gui.json.<date>` / `uniclipboard-daemon.json.<date>`），消除两进程并发 append 同一 `uniclipboard.json` 的竞争与混淆；跨进程因果链靠既有 `CorrelationLayer` 的 correlation id 关联。`dual-side-debug` 思路从"两台机器"延伸到"一台机器两进程"。
- **telemetry**：`uniclipd` 为 product analytics **唯一权威发送方**——进程内幂等（`compose_event_context` OnceLock）跨不了进程，两进程各发会导致 PostHog DAU / 设备计数翻倍。本期至少钦定 **设备级信号（`active_device_count` / `is_first_run` / heartbeat）只由 daemon 发**；**oneshot 抑制设备级事件、只发动作级事件**（否则每次 `uniclip send` 算一次设备活跃）；GUI 纯 UI 交互事件 **经 daemon 代发**（OQ-gui-ui-analytics 收敛：新增 `POST /analytics/capture`（session JWT），daemon 用自己的 sink + EventContext + gate 统一上报；过渡期 GUI 进程内 sink 临时续用，P3(c) 切换）。

### D21：进程终止契约——跨进程 graceful 关停（评审新增）

- D2 删除了唯一的进程内 graceful 通道（in-process `DaemonHandle::shutdown` + `FRONTEND_SHUTDOWN_EVENT`）。拆进程后 GUI 永不再持有 daemon handle，D3"彻底退出→停"、D13 Windows `taskkill`、D17 service-manager stop 全是 **外部信号** 路径，须有 daemon 侧 graceful handler 承接，否则 §5.3#8"先 graceful"落空。
- **契约**：`uniclipd` 注册 graceful-shutdown handler（SIGTERM / Windows `CTRL_CLOSE_EVENT`）→ 停收新任务、排空在途 transfer/sync、flush `EntryDeliveryRecord`、释放 `BIND_LOCK`（D22）/ iroh endpoint，**带超时**；超时后才允许外层 SIGKILL / `taskkill /F`。
- **统一载体**：该 handler 是兑现 §5.3#8 的 **唯一载体**。D3"彻底退出→停"、D13 Windows 路径、D17 service-manager stop **全部重定向到"先 SIGTERM、等 handler、超时再强杀"**。复用既有 `cli stop` 的 PID-based SIGTERM + graceful-wait（随 detached-spawn 逻辑一并上移共享）。D3"彻底退出"显式复用此路径只停本 GUI spawn 的 daemon。
- **前端协调（待落地 / 可留 OQ）**：跨进程下"前端先关 WS"由谁触发须定——daemon 收 SIGTERM 后 `with_graceful_shutdown` 自等 WS 排空 vs GUI 彻底退出前先发 detach RPC；graceful 超时具体值同。

### D22：单实例 authority——daemon 为 per-profile singleton（评审新增，OQ-single-instance 升格）

- **问题**：现状单实例保护住在 GUI 侧 Tauri 插件（轻量模式 GUI 缺席即失效），daemon 侧只有进程内 iroh `BIND_LOCK`（非跨进程互斥）；而 D3/D7/D11/D19 已把"同 profile 全局唯一 daemon 且可靠 attach"当不证自明的地基。叠加 `terminate_local_daemon_pid` **仅校验 `pid!=0` 即 kill**，detach + 频繁崩溃重启放大 PID 回收窗口 → **stale-PID 误杀无辜进程**。
- **决策**：
  1. **daemon 侧 per-profile 跨进程互斥** 用 OS advisory file-lock（`flock LOCK_EX|LOCK_NB` / Windows `LockFileEx`），**在绑端口与 iroh bind 之前** 获取；**锁（非端口）成为跨进程 mutex**，loser 干净退出而非 `AddrInUse` crash。
  2. **任何 terminate**（incompatible 替换 / `cli stop` / service-manager 接管）**必须先做 PID-liveness + identity 校验**：`kill(pid,0)` 探活 + 比对进程名/可执行路径 = `uniclipd` + 比对 `started_at_ms`；任一不符即视为 stale PID 文件、**删文件而非发信号**（→ §5.3 铁律#11）。
  3. **D11 直写文件前先持有该 profile 启动锁**，闭合 TOCTOU 写竞争。
  4. `DaemonProcessMode::InProcess` 去掉 D2 删除后不可达的活分支、**保留为 legacy-read-only 变体**（旧 PID 文件仍正确识别并拒杀，见 OQ-migration）；`started_at_ms` 升为 lock-reclaim tiebreaker。
- **GUI/CLI 单实例降为 UX-only**（修 Tauri 插件 focus 现有窗口）；`UC_DISABLE_SINGLE_INSTANCE` 保留为 GUI-only 逃生阀，新增 `UC_DISABLE_DAEMON_SINGLE_INSTANCE` 给 daemon 锁。抢占 = cooperative-exit（incumbent 默认胜），唯一 sanctioned takeover 是既有 incompatible-version 替换（graceful-first，§5.3#8 / D21）。

## 3. 待决问题（Open Questions）

> 本轮对抗式评审（见 [`adr-008-review-2026-05-30.md`](./adr-008-review-2026-05-30.md)）已收敛原 10 条 OQ，并新增 5 条。状态：**收敛**=评审已给结论、落地照办；**收敛 (待实测)**=结论方向定、数值/产物待 P0/P4 验证；**开放**=须落地阶段定。

### 3.1 原 Open Questions（评审收敛）

| ID | 状态 | 结论 |
|---|---|---|
| OQ-crate-shape | 收敛 | 单 crate 带 `[[bin]] name="uniclipd"`（lib 承载 runtime、main.rs 极薄加行数上限 lint）。仓内 `uc-cli`/`p2p-bench` 均单 crate 先例；拆两 crate 经典理由全不成立。 |
| OQ-desktop-residue | 收敛 | `build_process_runtime`/`ProcessRuntimeContext`/`ProcessRuntimeHandles` 整组随 D1 迁入 `uc-daemon`；`uc-desktop` 只留 client 胶水，`ProcessRuntimeHandles` 在 uc-desktop 删除。**硬条件：迁移必须与 D2/D5 同切片**（否则 uc-desktop 同时持已迁走的类型与仍用它的 GuiInProcess 路径，矛盾态）。 |
| OQ-compat | 收敛 | 不保留 `uniclip daemon` 别名；P2 直接删 `Commands::Daemon`，spawn 一次性切 `uniclipd` + 配套等价替换（改 spawn 解析、删 current_exe 自调、加拒绝断言，照搬 #912）。 |
| OQ-perf-gate | **收敛（已实测）** | P0 spike 完成（M4，见 [`adr-008-perf-spike-results.md`](./adr-008-perf-spike-results.md)）：**loopback 不是瓶颈**（full-buffer TTFB 256MiB 仍 <10ms、吞吐 8–12GiB/s，超原拟门槛 1–2 数量级）；**瓶颈是 full-buffer 内存×并发**（64MiB×4=328MiB、256MiB×4=1.29GiB，且 256MiB×4 p99 飙到 162ms）。裁定：**自动内联预览阈值=8MiB**，>8MiB 转显式下载并优先改流式 `BlobReaderPort`（实测 streaming RSS 恒定 ~6–10MiB、省 35–127×、TTFB 拍平），改流式前对 >阈值的并发拉取加信号量封顶。残余：`BlobReaderPort` 流式读未在 spike 验证。 |
| OQ-packaging | 收敛 (待实测) | 一个 `uniclipd` 二进制 + 三管线（桌面 Tauri sidecar / CLI 同 matrix tarball / headless Docker + 裸 musl Release）；updater 仅服务桌面捆绑包，headless 走 Docker tag/裸二进制重拉。残余：sidecar 公证、musl 可能命中 `aws-lc-sys`、snap 沙箱 spawn AppArmor。 |
| OQ-single-instance | 收敛→**升格 D22** | 见 D22（OS file-lock + PID identity 校验 + §5.3 铁律#11）。 |
| OQ-migration | 收敛 | 零迁移工具（复用同 `AppPaths`、存量数据目录不变）；D2 删 `GuiInProcess` 时保留 `DaemonProcessMode::InProcess` 为 legacy-read-only（≥1 发布周期）；危险边界"老 GUI 仍存活其 in-process daemon 抢占同 profile 端口"靠既有 Incompatible→InProcess 拒 SIGTERM 链路兜底。与 D22 联合。 |
| OQ-windows | 收敛 | autostart=每用户 Task Scheduler（`schtasks` AtLogOn 免管理员）、不走 Service；保活显式降级。`uc-platform` 抽象 `StartupIntegrationProvider`。见 D10/D17。 |
| OQ-gui-ui-analytics | 收敛 | 经 daemon 代发：新增 `POST /analytics/capture`（session JWT），daemon 成 D20 唯一权威发送方。排期 P3(c)，过渡期 GUI 进程内 sink 临时续用。 |
| OQ-lightweight-discoverability | 收敛 | 进轻量由 `uc-tauri` 用 `tauri-plugin-notification` 发一次性系统通知、零留守；触发点=轻量 handler 最前（发完 await→再 detach+ 退出），去重用 `app_data_root` 自愈 JSON 标志文件（tempfile+rename，不塞 settings.json），per-profile，文案"它还在跑 + 如何唤回"（中英双版）。 |

### 3.2 评审新增 Open Questions

| ID | 内容 |
|---|---|
| OQ-session-refresh | webview session JWT（5min TTL）周期续期通道的传输形态（Tauri event push vs `get_daemon_session` 命令 pull）、续期失败的 GUI 降级行为。（D5/D8） |
| OQ-setup-iroh | setup-mode 下 iroh 何时 bind、用什么身份 bind（setup 期设备身份尚在生成中）；new-Space 与 join-Space 两条路径的 iroh 需求差异。（D16） |
| OQ-cli-oneshot-lifecycle | `recv`/`watch` 长连接类 oneshot 的退出条件与回收责任方；是否禁止无常驻 daemon 时拉 oneshot。（D7） |
| OQ-uninstall-cleanup | 卸载/重装时 service unit + autostart 投影 + crash marker 的清理责任方与触发机制；macOS pkg 无标准 hook。（D10/D17/§5.3#12） |
| OQ-downgrade-rollback | 降级/回滚时 detached `uniclipd` 与磁盘低版本的收敛方向、settings/数据 `schema_version` 前向不兼容降级行为。（D13/D19） |

### 3.3 定稿前须人最终确认的措辞 / 数值

- §1.2 现状订正与 §5.3"现状基线"标注的 **具体表述** 是否采纳（方向已定，见报告 §2.1 / §6）。
- D21 graceful handler 超时值、前端 WS 优雅关闭由谁触发（daemon 自等 vs GUI 先发 detach RPC）。
- OQ-packaging CI 产物——P4 实测落定。（OQ-perf-gate 已由 P0 spike 实测落定，见 §3.1 与 [`adr-008-perf-spike-results.md`](./adr-008-perf-spike-results.md)。）

## 4. 实施路径（分阶段切片，每阶段独立可发布、revert-safe）

| 阶段 | 内容 | 用户可见行为 | 提交类型 |
|---|---|---|---|
| **P0 · spike** | ① 实测大负载过 localhost 性能，定 D6 / OQ-perf-gate；② 抽 1–2 条 GUI 流程走 WS 直连验证可行性（接 `direct-daemon-ws`） | 无 | spike |
| **P1 · 抽库** | `uc-desktop/src/daemon/` runtime 抽进新建 `uc-daemon`（含 `build_process_runtime`/`ProcessRuntimeHandles`，OQ-desktop-residue）；三方可共享；**不改运行行为**，`GuiInProcess` 与 `Standalone` 照旧 | 无 | `arch:` |
| **P2 · `uniclipd` 二进制 + CLI 解耦** | 新增 `uniclipd` bin；`Standalone` / headless / oneshot spawn 从 `uniclip daemon` 改为 `uniclipd`（删 `Commands::Daemon`，OQ-compat）；`uc-cli` 去 `uc-desktop` 依赖；start/stop/probe 解析 `uniclipd`；daemon 两阶段（D16）；解锁契约（D9）；单实例 file-lock + PID identity 校验（D22）；进程终止契约（D21） | CLI / headless 行为等价 | `arch:` / `refactor:` |
| **P2.5 · 业务命令 daemon 端点** | 新建 `send`/`recv`/`watch` 派发 / resend / inbound-notice 订阅 / cancel 的 daemon HTTP/WS 端点（D7）；oneshot 生命周期（OQ-cli-oneshot-lifecycle） | CLI 在 daemon 在跑时走 API | `feat:` |
| **P3 · GUI 转 client** | `uc-tauri` + 前端从 in-process facade 改走 `uc-daemon-client` / WS；删 `GuiInProcess`；setup/unlock 走 loopback（D15）；token 注入收口 + bearer 止于原生 + session 续期（D5/OQ-session-refresh）。细分：(a) 只读视图 → (b) 写操作 + settings `PATCH`（D11/D12，含副作用归属）→ (c) 事件流 + resync（D8，含 clipboard snapshot）+ analytics 代发（D20）→ (d) 大负载按需拉（D6） | GUI 改为跨进程（行为变化集中点） | `feat:` / `refactor:` |
| **P4 · 轻量模式 + 保活 + 可观测性** | `DaemonOwnership` 重做（D3 新增 spawn-owned 态）关窗 detach / 重开 attach；三态 UX（D3）；autostart → `StartupIntegrationProvider`（D10/D17/OQ-windows）；多 profile（D19）；每进程日志 + analytics 单源（D20）；崩溃可见性（D17 反向 marker）+ 卸载清理（OQ-uninstall-cleanup） | **交付轻量模式** | `feat:` |

- **依赖序**：P1 是后续一切前提；P2、P2.5 在 P1 后可串/并；P3 在 P2.5 后（GUI 走的端点须先在）；**P4 必须在 P3 之后**（GUI 先成为 client 才谈得上 detach 留守）。
- **风险隔离**：P1 / P2 对用户零行为变化（纯结构迁移 + 等价替换）；行为变化集中在 P3；功能交付在 P4。

## 5. 后果

### 5.1 正向

- `uc-cli` 不再背桌面宿主全家桶，二进制更小、依赖图更干净（Scope B 目标）。
- 无头部署有了名正言顺的 `uniclipd`（替换 ADR-007 "单二进制自启"权宜）。
- daemon runtime 归位独立 crate，多 shell（`uc-tauri` / 未来 native）可复用。
- 轻量模式落地：GUI 可来去、后台常驻，对标 clash-verge / mihomo。

### 5.2 反向 / 成本 / 已知风险

- GUI 后端整条线重接到 client 路径（P3 工作量最大）；token 边界 / session 续期 / clipboard resync 是 P3 的净新工作（评审揭示，非"保持现状"）。
- normal 模式下 GUI 也走跨进程，大负载需按需拉取兜底（D6，且现有 blob 端点全量 buffer、大文件需改流式）。
- 双形态打包 / snap / updater / CI 编排成本（OQ-packaging）；service manager 在 Windows 落地有管理员门槛、保活弱（OQ-windows）。
- 进程所有权与生命周期 UX 复杂度（detach / attach / 彻底退出三态 + 单实例抢占 + graceful 关停）。
- **轻量模式"完全隐形"风险**（D3）：进轻量后无窗口 / 无托盘 / 无快捷键，用户可能忘了它在跑或不知如何唤回。最低缓解：首次进入轻量模式发 **一次性系统通知**（"UniClipboard 将在后台继续运行，从应用图标重新打开窗口"），靠 `tauri-plugin-notification` 在 GUI 退出前发，不引入留守组件（见 OQ-lightweight-discoverability）。轻量中途崩溃的可见性单独靠 D17。

### 5.3 边界铁律（落地后违反即偏离本 ADR）

> **现状基线标注（评审补，防完成度误判）**：[已成立]=今天代码已满足、保持即可；[需反转]=今天代码恰好违反、本 ADR 落地前须主动反转才成立；[净新建]=今天不存在、本 ADR 新建。下列 **#3–#6 描述的是 P3 后的目标态**，现状代码恰好违反它们（GuiInProcess 持 in-process facade、连接信息由 GUI 进程命令轮询下发、bearer 进 webview）——这正是本 ADR 要消除的现状，**勿读成"现状护栏"**。

1. [已成立] 在 `uc-daemon` 依赖任何 GUI / UI 框架。
2. [净新建] 为 GUI↔daemon 另起一套 IPC / 协议而非复用现有 HTTP+WS（违反 D4）。
3. [需反转] 为追求延迟而恢复 GUI in-process 托管 daemon 的"快路径"（违反 D2）。
4. [需反转] 在 GUI 进程内重新持有业务 `AppFacade`（违反 D5）。
5. [需反转] 把 daemon 端口 / token 发现重新耦合到"必须由 GUI 进程内下发"（破坏 D3 跨重启重连）。
6. [需反转] 把 bearer `daemon.token` 注入 webview（违反 D5，必须只下发短命 session）。
7. [净新建] 允许"无人值守自启 + auto_unlock=false"组合落地（违反 D9）；或把该校验的硬边界放在 GUI/CLI 而非 daemon 自检。
8. [净新建] 对承载活跃同步状态的 daemon 用 SIGKILL / 无条件 terminate 而不先 graceful（D21 graceful handler 是兑现本条的唯一载体；捆绑分发下危险路径本应几乎不触发，但 terminate 仍须优先 graceful；降级方向例外见 D13）。
9. [净新建] 让 GUI 与 daemon 两进程各发设备级 analytics（违反 D20，污染 DAU）。
10. [净新建] 把两进程日志写进同一个 `uniclipboard.json`（违反 D20）。
11. [净新建] 对未做 PID-liveness + identity 校验的裸 PID 发杀信号（违反 D22，防 stale-PID 误杀）。
12. [净新建] app 卸载后残留可自启的 service unit / autostart 投影（幽灵自启镜像版，见 D10 / OQ-uninstall-cleanup）。

## 6. 决策记录

本 ADR 由 2026-05-30 的设计 grilling 推导（20 轮决策），核心取舍：

- 恢复 `uc-daemon`，runtime 从 `uc-desktop` 迁出，产出 `uniclipd`；GUI 永久 client 化，删 `GuiInProcess`，不留双路径。
- 轻量模式 = GUI 进程整体退出（无托盘无快捷键）、daemon detach 留守、重启 app 唤回（比 clash-verge 更激进）。
- 不新开 IPC：复用 127.0.0.1 HTTP+WS（前端 M003 已直连），统一收口 `uc-daemon-client`。
- 解锁用 attended/unattended 契约，禁止"无人值守自启 + 手动解锁"互斥组合，daemon 自检为唯一硬边界。
- settings 单一 writer（终态 daemon API，本期分层）+ 通用事件总线热生效。
- 版本捆绑分发（机器内一致）+ 双分发形态；控制面 `api_revision` 与 P2P peer wire 兼容正交。
- 安全接受"同 UID 即可信"但显式记录威胁模型 + 生产收紧 CORS；退役"口令不出进程"不变量。
- daemon 两阶段（setup-mode / operational）；保活交 OS service manager + 崩溃可见性兜底。
- 投递语义精化：oneshot 即时尽力投 + 离线显式报告，常驻才最终一致。
- 多 profile = N 进程 + per-profile 单元（默认仅主 profile 自启）；每进程独立日志 + daemon 为 analytics 唯一权威。

**2026-05-30 对抗式评审追加**（65 agent / 六阶段，详见 [`adr-008-review-2026-05-30.md`](./adr-008-review-2026-05-30.md)，总评 needs-revision）：

- **现状核实订正**：§1.2 两处与代码相反——C1 `start_in_process` 实为 `pub(crate)` 进程内共享装配函数（非"对外暴露"）；C7 连接信息为前端 ~500ms 轮询 `get_daemon_connection_info`（无 `daemon://` 事件）、**RAW BEARER 当前进 webview**（故 D5/§5.3#6 是 **反转非保持**），均已订正并加纪律声明。
- **新增 D21 进程终止契约**：D2 删了唯一进程内 graceful 通道，新立跨进程 graceful handler 兑现 §5.3#8（SIGTERM→排空 iroh/sync、flush 记录、释放锁、带超时）。
- **新增 D22 单实例 authority**：OS advisory file-lock 作 per-profile 跨进程 mutex + terminate 前强制 PID-liveness+identity 校验（防 `terminate_local_daemon_pid` 仅校验 `pid!=0` 的 stale-PID 误杀），OQ-single-instance 升格。
- **D18 改为与 ADR-005 §2.5 对齐**：删原稿虚构的"presence 上线自动补投"（代码核实 `presence_monitor`/`peer_keepalive` 均不重投、全仓唯一重投是用户主动 resend），两路径均靠手动 resend；§1.3 引述同步修正。
- **D3 ownership 重做**：拆分后 GUI 恒为 External，新增"本 GUI spawn 的可停 standalone daemon"态（与 D22 协同）。
- **多处 factual 订正**：D6（blob 全量 buffer、大文件需流式）、D8（WS 重连须刷 session + clipboard topic 无 snapshot）、D14（CORS 三平台 origin + 口令端点真实路由）、D16（setup-mode iroh 精确到子系统级、join 须 iroh）、D17（crash marker 改启动检测反向模式 + Windows Task Scheduler）。
- §5.3 铁律加 **现状基线标注** + 2 条新铁律（#11 PID identity、#12 卸载清理）。原 10 OQ 全部收敛、新增 5 OQ（session-refresh / setup-iroh / cli-oneshot-lifecycle / uninstall-cleanup / downgrade-rollback）。实施路径补 P2.5。**决策总数 D1–D22。**
- **OQ-perf-gate 已由 P0 spike 实测落定**（M4，见 [`adr-008-perf-spike-results.md`](./adr-008-perf-spike-results.md)）：loopback 传输非瓶颈，瓶颈是 full-buffer 内存×并发；钦定自动内联预览阈值=8MiB + 大文件优先改流式 `BlobReaderPort`。bench 在 `src-tauri/crates/p2p-bench/src/bin/http_blob_bench.rs`（throwaway，iroh 依赖已 feature-gate）。

任何对上述取舍的修订需更新本节并视情况新建后续 ADR。

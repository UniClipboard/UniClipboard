# ADR-011: 本地连接发现改为动态端口 + `daemon.conn` 连接信息文件（否决新增 UDS / 独立 IPC 通道）

- **状态**：Accepted（2026-08-11）
- **日期**：2026-08-11
- **相关文档**：[`adr-008-uniclipd-split-gui-as-client.md`](./adr-008-uniclipd-split-gui-as-client.md)（D4/D5/D13/D14/D22）、[`adr-008-review-2026-05-30.md`](./adr-008-review-2026-05-30.md)（C7 现状订正）、`docs/uat/direct-daemon-ws.md`、`docs/development/config.md`、`crates/uc-daemon-process/AGENTS.md`

## 1. 决策

1. **维持 ADR-008 D4**：不引入 UDS / Windows named pipe / 第二套 IPC 协议；GUI↔daemon 传输仍为 `127.0.0.1` HTTP + WebSocket 单通道。本 ADR 是对"新增一套 sock 通道"议题的正式评审结论落档。
2. **loopback 监听改为动态端口**（`127.0.0.1:0`，由内核分配），废弃 `UC_PROFILE` hash 固定端口解析。
3. **连接信息收敛为单一 `daemon.conn` 文件**（`<app_data_root>/daemon.conn`，`0o600`，temp+rename 原子写），内容含 host / port / token / pid / startedAtMs；`.daemon-token` 文件与 hash 端口解析退役。
4. **所有本地客户端（CLI / Tauri 原生 / daemon probe / health-wait）改为"读 `daemon.conn` + PID 身份校验"**；前端 webview 数据通路不变（仍由原生侧注入连接信息）。

## 2. 背景

- 现状（代码已核实）：端口由 `crates/uc-daemon-process/src/socket.rs` 以 FNV-1a hash 派生（默认 `42715`，profile 落入 `42719+` 区间）；该文件注释自认"会与无关本地服务碰撞"。碰撞后 `crates/uc-webserver/src/api/server.rs` 的 `run_http_server` 重试 5 秒，超时即 daemon 启动失败。
- 前端连接信息经 ~500ms 轮询 `get_daemon_connection_info` 获取（`src/lib/daemon-connection-info.ts`，60s 超时上限）；RAW bearer 经该命令进入 webview（ADR-008 C7 已订正的现状）。
- 评审触发：评估"在 HTTP+WS 之外新增一套 socket 通道"（动机：Windows 防火墙 / 端口冲突 / 原生侧连接可靠性 / 安全加固）。

## 3. 被否掉的方案

### 3.1 UDS / named pipe / 新 IPC 协议（新增独立通道）

- **违反 ADR-008 D4 锁定决策**：D4 明确"再加一条 IPC = API 面 fork 成两套，违反单一来源"，只保留"同 API 换传输"的出口，且"本期不做传输替换"。
- **webview 物理约束**：前端是浏览器（fetch/WebSocket），**无法连接 UDS / named pipe**——任何 UDS 方案必然双通道（原生侧走 UDS、webview 留 TCP），API 面分裂成两套客户端契约。
- **安全增量≈0**：D14 威胁模型已显式声明"本机同 UID 进程视为可信"，UDS 的 `0o600` 权限模型只挡跨用户进程，在该模型下无增量收益；若主张更强的跨用户隔离，须先改写威胁模型而非换传输。
- **Windows 成本**：UDS 不存在，须 named pipe + DACL + hyper 兼容层，跨平台工作量翻倍。
- **仓库历史教训**：`.planning/milestones/v0.4.0-phases/41-*` 曾实现 UDS + JSON-RPC 控制面，phase 45 整体替换为 HTTP+WS——已走过一次完整弯路。
- **动机错配**：端口冲突只涉及 TCP 传输层，新通道并存解决不了它；防火墙弹窗大概率来自 iroh magicsock 的 UDP 端口（loopback 监听不触发 Windows Firewall），与 HTTP 通道无关，UDS 同样解决不了。

### 3.2 固定端口 + 冲突时换端口 fallback

- 复杂度≈动态端口（同样要写连接信息文件、同样要客户端改读文件），却多保留一段 hash 解析死代码，否决。

## 4. 决策细节

1. **绑定顺序（防 race）**：先同步 `TcpListener::bind("127.0.0.1:0")` → 写 `daemon.conn` → 开始 serve。客户端读到文件时端口必已可连。该"先 bind 后写连接信息"模式在 `crates/uc-webserver/src/mobile_lan/server.rs`（`endpoint_info`）已有先例。
2. **`daemon.conn` 文件格式**（JSON，camelCase，`format` 字段版本化）：

   ```json
   {
     "format": 1,
     "host": "127.0.0.1",
     "port": 43127,
     "token": "<bearer>",
     "pid": 12345,
     "startedAtMs": 1720000000000
   }
   ```

   - 写入 `0o600`（复用 `uc-daemon-local/src/auth.rs` 既有写文件 + temp+rename 模式）；daemon 每次启动重写，token 沿用既有 `load_or_create_auth_token` 语义（跨重启持久，不每次换新；每次换新的只是 JWT session secret）。
   - token 迁移：切换时可直接把旧 `.daemon-token` 内容搬入 conn（load-or-create 来源换路径），或直接换新——捆绑分发（D13）下客户端一律读 conn，换新亦安全。
   - graceful shutdown 删除；崩溃残留由客户端 PID 身份校验（D22 `verify_pid_identity`）识别为 stale，不依赖文件存在性。
3. **退役内容**：`resolve_daemon_http_addr` / hash 端口解析 / `resolve_daemon_token_path` / `.daemon-token` 全部删除；`uc-daemon-process/src/socket.rs` 新增 `resolve_daemon_conn_path` + `read_daemon_conn`（serde_json 已在允许依赖面内，`uc-daemon-process` 薄 crate 约束不破坏）。
4. **客户端改造点**（均已核实存在）：
   - `crates/uc-daemon-client/src/lib.rs`：`resolve_connection_info_from_env` / base URL 解析改读 conn 文件。
   - `crates/uc-desktop/src/daemon_probe.rs`：probe 改"读 conn 文件 + `verify_pid_identity`"（比"连 hash 端口可达"更可靠，且天然复用 D22 机制）。
   - `apps/cli/src/local_daemon.rs`：health-wait 改"等 conn 文件出现 → 校验 → 连接"。
   - `src-tauri/crates/uc-tauri/src/commands/startup.rs`（`get_daemon_connection_info`）：数据源改读 conn 文件，每次调用读到最新端口/token，不再依赖进程内 state 设置时序。
   - 前端 `daemon-connection-info.ts` / `daemon-ws.ts`：**不动**；原生侧数据源新鲜后，轮询读到的信息自动跟随端口/token 轮换，60s 超时兜底保留。
5. **版本与互操作**：
   - conn 文件 `format` 字段未知 / 文件缺失 → 视为 Incompatible → 走既有 `terminate_incompatible_daemon` 替换路径（与 `DAEMON_API_REVISION` 联动，D13 捆绑分发下同版本收敛）。
   - **双轨过渡（P1，revert-safe）**：daemon 写 conn 文件 + 仍绑 hash 端口；客户端优先读 conn、缺失时 fallback hash+token（兼容旧 daemon / 旧 client 混跑）。**P2 单轨**：daemon 只绑 `:0`，删 hash 解析、`.daemon-token`、AddrInUse 重试（bind `:0` 永不冲突）与 fallback。
6. **多 profile**：conn 文件位于 app_data_root，天然 per-profile，无需额外隔离。
7. **范围外**：bearer 不进 webview 的 D5 反转（原生侧 `/auth/connect` 换 session + 续期通道）仍属 ADR-008 P3 计划内工作，本文档不扩不缩其范围；conn 文件使该落地更顺（原生侧读文件即可拿到 bearer）。

## 5. 后果

**正面**

- 端口冲突归零（内核分配），删 AddrInUse 重试路径。
- 连接发现收敛为单一事实源（一份文件承载 port + token + pid），客户端不再各自推导。
- probe / health-wait 语义升级为"文件 + PID 身份校验"，天然复用 D22，且对 stale 文件鲁棒。
- 前端轮询问题消解（读到的信息永远新鲜），为 D5 收口铺路。

**代价**

- 日志中的端口值随进程变化（不再稳定可记忆）。
- 依赖固定端口的文档与工具须迁移：`docs/uat/direct-daemon-ws.md`（42715 示例）、`crates/AGENTS.md` CONVENTIONS、`crates/uc-daemon-process/AGENTS.md`。
- 既有固定端口断言类测试须改写为 conn 文件语义。

**不做的事**

- 不引入 UDS / 新 IPC（D4 保持）；不改 webview 数据通路；不并入 LAN mobile（其动态端口模式仅作先例引用；LAN mobile 已 demote 为 deprecated）。

## 6. 实施路径

| 阶段 | 内容 | 用户可见行为 | 提交类型 |
|---|---|---|---|
| **P1 · 双轨** | daemon 写 `daemon.conn`（保留 hash 监听）；客户端读 conn、缺失 fallback hash+token；单测固定 conn 路径与格式 | 无 | `arch:` |
| **P2 · 单轨** | daemon 改绑 `127.0.0.1:0`；删 hash 解析 / `.daemon-token` / AddrInUse 重试 / fallback；probe 与 health-wait 改文件+pid 校验；同步清理上述文档 | 端口冲突消失 | `arch:` / `refactor:` |
| **P3 ·（可选，与 D5 合并）** | 前端轮询收口为事件推送；原生侧 `/auth/connect` 换 session（ADR-008 P3 既有计划） | bearer 不进 webview | `feat:` |

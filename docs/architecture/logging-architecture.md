# 日志架构

本文描述桌面端当前的日志事实：谁写哪些文件、启动顺序、记录格式、级别与开关、请求关联、诊断包。Engine 自身的诊断运行时见 [`engine-diagnostic-runtime.md`](./engine-diagnostic-runtime.md)；写日志的代码规范见 [`../agent/rust-rules.md`](../agent/rust-rules.md)。

## 进程与日志文件

桌面端的业务进程是 Rust 后台服务 `uniclipd`（下称 daemon）。Go/Wails 图形界面宿主、Go 命令行 `uniclip` 和快捷面板都通过本机 HTTP/WebSocket 访问它。

日志目录由 `uc_app_paths::app_log_dir()` 唯一决定，与数据根分离：

- macOS：`~/Library/Logs/app.uniclipboard.desktop[-<profile>]/`
- Linux：`~/.local/state/app.uniclipboard.desktop[-<profile>]/logs/`
- Windows：`%LOCALAPPDATA%\app.uniclipboard.desktop[-<profile>]\logs\`
- 便携版：`<exe>/data/logs/`

| 文件 | 写入方 | 内容 |
| --- | --- | --- |
| `uniclipboard-daemon.json.<日期>` | daemon 的宿主层 | 宿主进程自己的记录：启动、HTTP、桌面平台适配、诊断导出等 |
| `engine.<日期>.jsonl` | daemon 内的 Engine 诊断运行时 | Engine 的连接、配对、同步诊断记录；由 Engine 的采集模式控制，与宿主 Debug 模式无关 |
| `network/daemon/<运行 ID>/` | daemon 内的 Engine 网络日志 | 第三方网络库（iroh、noq-proto）的脱敏连接事件，与上面所有文件独立，见 [独立网络日志](#独立网络日志)；默认不存在 |
| `uniclipboard-quick-panel.json.<日期>` | 快捷面板进程 | 快捷面板自己的记录 |
| `uniclipboard-gui.json.<日期>` | Go 图形界面宿主 | 宿主的生命周期事件，以及标准库 `log` 的全部输出（同时仍回显到 stderr） |
| `uniclipboard-cli.json.<日期>` | Go 命令行 `uniclip` | 每次调用的开始与结果（退出码、耗时）；参数可能含口令，从不记录 |
| `daemon-startup-failure.json` | daemon | 最近一次在日志可用之前失败的启动，见下文，只保留一份 |
| `service.stdout.log`、`service.stderr.log` | macOS launchd 用户服务 | 服务的标准输出和错误输出，见下文 |

按日轮转，保留 7 天，启动时清理更早的文件。Go 进程（图形界面宿主、命令行）通过 `packages/desktop-host-go/hostlog` 写自己的文件，格式与 daemon 的宿主层一致：一行一个 JSON 对象，含 `timestamp`、`level`、`target`（`go.host`）、`message`、`run_id`、`pid`；单条消息上限 8 KiB；目录 `0700`、文件 `0600`；只清理自己角色的过期文件。日志目录无法创建或打开时只退回控制台输出，不阻止启动。

前端（React）日志经 `src/lib/logger.ts` 进入控制台和 Sentry（受遥测开关控制），不写入本地日志目录，因此不在本地诊断包内。

## 启动顺序

daemon 启动按下面的顺序进行，顺序本身是约束：

1. 解析目录（`resolve_desktop_process_paths`）。
2. 抢占实例锁（`profile_claim::claim_instance_lock`）。
3. 安装 tracing（宿主层 + Engine 诊断运行时；网络日志配置在同一次安装里传入，见 [独立网络日志](#独立网络日志)）。
4. 准备宿主能力并启动 Engine，随后进入服务阶段。

实例锁必须先于任何日志安装。Engine 诊断运行时会打开共享的 `engine.<日期>.jsonl`，一个仍在等待锁的 daemon 不能与正在运行的 daemon 交错写入。

第 3 步之前发生的失败无法写入日志文件，因此写入有界的 `daemon-startup-failure.json`；日志安装之后、daemon 就绪之前的失败（宿主准备、Engine 启动、服务启动）同样写入它，并另记一条 ERROR。它是固定结构，只含 `runId`、`pid`、`step`（`path_resolution`、`instance_lock`、`tracing`、`host_preparation`、`engine_start`、`service_start`）、`class`（`lock_timeout`、`io`、`internal`）和时间，不含错误文本；文件上限 4 KiB，新的失败覆盖旧的。诊断导出会收录它，即使此时没有其他日志。

崩溃标记（`daemon-run.json`、`daemon-last-exit.json`）只用于发现上次异常退出，写入或清除失败只记 WARN，不阻止启动或把一次正常退出变成失败。

Windows 上图形界面停止 daemon 时先调用 `POST /lifecycle/graceful-stop`，最多等待 3 秒，超时才终止进程，因此正常退出不会再被下次启动判为异常退出。macOS 和 Linux 发送 SIGTERM，本来就走优雅关闭。

## 宿主记录格式

宿主 JSON 文件层使用 `uc_observability::format::FlatJsonFormat`：一行一个 JSON 对象，span 字段摊平到顶层。

- 固定字段：`timestamp`、`level`、`target`、`message`、`span`。
- 进程身份：`run_id`（本次进程运行的 UUID）和 `pid` 出现在每一条记录里，由格式化器拥有；span 或事件里同名字段会被改名为 `span_*`、`event_*`。两个 daemon 共用日志目录时可据此区分。`engine_run_id` 在启动成功后记录一次，用来对应 Engine 记录里的运行编号。
- 标识符：`device_id`、`peer_id`、`session_token_jti`、`token_jti`、`jti` 不以原值写入，改为 `device_ref`、`peer_ref`、`session_token_ref`、`token_ref`。引用是进程内随机密钥的哈希，同一次运行内可关联，不能反推原值，也不能跨运行关联。这是唯一的出口，调用点不需要各自处理。
- span 与事件字段同名时，span 字段加 `parent_` 前缀。

Engine 的自有事件、以及它依赖的网络栈（iroh、noq、quinn、netwatch、hickory 等）不会交给宿主层，所以宿主的日志级别设置对它们不起作用。第三方网络库的事件只可能进入独立网络日志，不会出现在宿主控制台、宿主 JSON 文件、Sentry 或业务日志里。

## 级别、Debug 模式与采集

宿主日志级别由 `LogProfile` 决定，优先级依次为：`RUST_LOG`、`UC_LOG_PROFILE`、设置里持久化的 Debug 模式、构建默认值（调试构建 `dev`，发布构建 `prod`）。无论哪个入口，OpenMLS 等敏感目标都被强制关闭。

- **Debug 模式** 只提高宿主进程（daemon、平台适配、Web 服务、守护进程客户端等）的日志详细度，重启后才生效，不改变 Engine 记录。
- **详细记录（Detailed capture）** 是 Engine 的限时采集，控制 `engine.<日期>.jsonl` 的内容，1–15 分钟。
- **网络日志的限时详细记录** 与上一项互相独立：各自开关、各自到期、各自失败，开启一个不会开启另一个。
- `GET /diagnostics/debug` 的 `effectiveLogProfile` 报告进程 **实际安装** 的 profile（`uc_observability::installed_log_profile()`），而不是设置值；`debugMode` 是持久化的设置，两者不同说明需要重启，或被环境变量覆盖。

控制台层用于开发。`UC_CONSOLE_LOG=off` 关闭它。macOS 的 launchd 服务把标准输出写进不会轮转的 `service.stdout.log`，所以生成的 plist 设置了这个变量，记录仍然进入按日轮转的 JSON 文件；Linux 的 systemd 单元把输出交给 journal，由 journal 限制大小，不设置。

## 请求关联

daemon 的 HTTP 入口（`request_tracing_middleware`）给每个请求一个 id：

- 调用方发送的 `x-request-id` 只有在 1–64 个 `[A-Za-z0-9._-]` 字符时才保留，否则由 daemon 生成，避免日志注入。
- id 写入请求 span（日志里的 `request_id`）、响应头 `x-request-id`，并通过 CORS 的 `Access-Control-Allow-Headers` 与 `Access-Control-Expose-Headers` 让浏览器能发送和读取。
- 4xx/5xx 的 JSON 对象响应体由中间件统一加上 `requestId`，不依赖各处错误构造代码；前端的 `DaemonApiError.apiRequestId` 保存它。

Engine 的操作记录是否带这个 id，取决于 Engine 是否提供不透明的观察上下文入口。已用真实进程核验（Engine 固定在 `5fc94529cd7b78509f31dc6046142908b85bf998`）：

- `Engine::execute(operation)` 没有上下文参数。Engine 文档中的“不透明 `ObservationContext`”只在 Application 的完整流程负责人之间延续，并且基于 OpenTelemetry 的 trace/span，不接受宿主的字符串 id。
- 对隔离 profile 里的真实 daemon 连续发 6 个带 `x-request-id` 的请求：36 条宿主记录中 13 条带这些 id；159 条 Engine 记录中 **0** 条带，Engine 记录的顶层字段里没有任何可承载请求 id 的位置。

因此 daemon 请求 id 与 Engine 记录之间 **没有** 关联，宿主也没有伪造关联：不拿时间邻近、也不拿 trace id 去猜。接入需要 Engine 先提供接收宿主关联的入口，不在宿主里自造接口。

## 诊断包

导出由 `uc_observability::startup_logs::export_diagnostic_logs` 统一组装（daemon 在线导出），图形界面在 daemon 不可用时用 Go 的 `packages/desktop-host-go/diaglogs` 做离线导出。两处使用同一份文件名规则，由 `packages/desktop-host-go/diaglogs/testdata/log-source-patterns.json` 约束，Go 与 Rust 的测试读取同一个夹具。

包内容：`logs/` 下的受管文件、`logs/daemon-startup-failure.json`（存在时）和 `manifest.json`。清单包含：

- `mode`（在线/离线）、`exportedAt`、时间窗口；
- `host`：应用版本、`buildCommit`、操作系统、架构、profile、是否便携、已安装的日志 profile、遥测状态、时区偏移（离线导出只含能在图形界面侧确定的字段）。不含设备、对等端或账户标识。`buildCommit` 是构建时的 Git HEAD 提交（daemon 由 `apps/daemon/build.rs` 写入，可用环境变量 `UC_BUILD_COMMIT` 覆盖；Go 宿主取工具链记录的 `vcs.revision`），没有时为 `unknown`，不检测未提交的修改。Engine 的来源提交在 Engine 准备结果和网络日志的 `run.json` 里；
- `collection`：`includedFiles`、`unreadableFiles`、`truncatedFiles`、`missingSources`（当前进程会写、但包里没有文件的来源，原因 `missing` 或 `unreadable`），以及独立的 `network` 块（见下节）。

失败时 `POST /diagnostics/log-export` 返回 `code: diagnostic_export_failed` 与 `details.cause`（`source_read_failed`、`archive_write_failed`、`destination_unavailable`、`unknown`），并记录一条带相同 cause 的 ERROR。设置页在回退到离线导出时显示原因和缺失的来源。

## 独立网络日志

Engine 把第三方网络库（iroh、noq-proto 的审定事件）记到自己独立的目录、队列、预算、状态和导出来源里，与业务日志互不牵连：任何一方写入失败、队列满或被删除，都不改变另一方，也不改变业务结果。**只升级 Engine 不会打开它**，宿主必须在安装观测运行时之前配置。

### 配置

- daemon 在取得实例锁之后、`install_with_host_layers` 之前，用 `Daemon` 角色调用 `with_network_logs`，并登记 `known_network_alpns()`（`crates/uc-bootstrap/src/observability/network_log.rs`）。GUI、CLI、快捷面板不安装第二个运行时，也没有网络日志写入方。
- **没有产品默认预算。** Engine 不提供默认值，桌面端的预算、保留期尚待产品决定，所以不带显式预算时网络日志是 `NotConfigured`：状态接口明确返回 `availability: notConfigured`，开始/停止采集返回 409 `network_log_not_configured`，诊断包的网络块是 `notConfigured`，不会伪装成“零事件”。
- 显式预算只用于开发与验证：环境变量 `UC_NETWORK_LOG_BUDGET`，格式 `record_bytes=…,queue_records=…,file_bytes=…,run_bytes=…,root_bytes=…,retention_secs=…`，六项必须齐全且自洽，否则整体拒绝并保持 `NotConfigured`（启动时记一条 WARN）。测试里用的数值不是产品承诺。

### 接口与界面

| 接口 | 作用 |
| --- | --- |
| `GET /diagnostics/network` | 状态、预算、计数（已接受、已写入、策略过滤、限速、超长、队列丢弃、关闭后丢弃、写入失败、淘汰文件数、路径恢复次数、未审查调用点）、各库观察数、当前窗口 id 与剩余时间 |
| `POST /diagnostics/network/start` | 开启 1–900 秒的限时详细记录；已有窗口时返回原窗口，不延长 |
| `POST /diagnostics/network/stop` | 用窗口 id 结束窗口；id 不匹配返回 409 `network_capture_mismatch` |

开始、停止、状态和导出准备都在阻塞线程池里执行，不占用异步工作线程。设置页“日志”分组里有独立的“网络连接日志”开关（窗口长度常量 `NETWORK_CAPTURE_SECONDS`，待产品确认）和计数、预算、覆盖范围说明；视觉由用户验收。

### 诊断包里的网络来源

- 包内路径 `network/<角色>/<运行 ID>/`，只收 `run.json` 与 `network.<UTC 日期>.<序号>.jsonl`。`lease.lock`、`.cleanup.lock`、其他文件、符号链接、非法角色或运行目录名一律不收，只计入 `ignoredEntries`，不跟随链接。
- 每个文件记录实际复制的字节数和这些字节的 SHA-256；当前运行的分段还与 Engine 刷新后给出的摘要比对（`matchesPrepared`），文件在刷新后继续增长时只比对刷新时的前缀。
- `collection.network.outcome`：`complete`（当前运行全部文件读全且与刷新时一致、无其他原因）、`partial`、`missing`（已配置但没有文件）、`notConfigured`、`unknown`；`reasons` 逐项列出未覆盖的原因：`engineOffline`、`flushFailed`、`flushTimedOut`、`stateNotReady`、`recordsDropped`、`fileUnreadable`、`fileTruncated`、`fileGrewAfterPrepare`、`fileChangedAfterPrepare`、`otherProcessesNotFlushed`、`entriesIgnored`。
- 在线导出的准备结果（`enginePreparation.network`）分别给出刷新结果、状态与计数、文件摘要，`otherProcessesFlushed` 恒为 `false`。网络日志刷新失败不改变业务日志的 `flush`，反之亦然。
- daemon 不可用时，Go 离线导出收录磁盘上已有的网络运行目录，结果恒为 `partial` 或 `unknown` 并带 `engineOffline`：没有刷新，计数与丢弃数未知；`otherProcessesNotFlushed` 在其他运行目录最近 5 分钟内仍有写入时给出。设置页把这些结果显示出来。

### 覆盖范围

只覆盖 iroh 与 noq-proto 的审定结构化事件。不覆盖 `log` crate 来源（noq-udp）、quinn、原始 qlog 和 TLS keylog，也不包含地址、公钥、URL、令牌、邀请或剪贴板内容。日志为空不代表网络没有活动：先看丢弃与写入失败计数，再看覆盖说明。pre-hello 阶段没有跨端统一关联（Engine 已取消该方案）。

### 退出

Engine 在自己的线程里排空网络队列，耗时不占用业务关闭预算；但进程可能先于该线程退出，所以 daemon 在业务 Engine 停止之后、观测运行时关闭之前，对已配置的网络日志做一次最长 1 秒的刷新（`NETWORK_SHUTDOWN_FLUSH_DEADLINE`），保留最后几秒的连接记录。强杀或崩溃仍可能丢失尾部，导出里对应为 `partial`。

## 验证

下列路径用真实进程覆盖，见 `tests/e2e/tests/daemon_logging.rs`（`cargo test -p uc-e2e-tests --test daemon_logging -- --ignored`）：同一 profile 上的第二个 daemon 失败并留下启动失败记录且不写任何日志；宿主记录的 `run_id`、`pid` 一致；请求 id 经过 CORS、400/401 响应体和宿主日志；在线导出收录启动失败记录并带 `host` 与 `missingSources`。

网络日志的真实进程用例见 `tests/e2e/tests/daemon_network_log.rs`（`cargo test -p uc-e2e-tests --test daemon_network_log -- --ignored`）：未配置、显式预算下的状态/窗口控制/导出清单与非白名单文件排除、来源提交等于 Cargo.toml 固定值、优雅停止耗时，以及两个真实 daemon 配对时的事件文件、摘要比对和无标识符检查。

仍须在对应环境单独验证的部分：Windows 的优雅停止与 `TerminateProcess` 回退、macOS launchd 服务实际的标准输出大小、图形界面的离线导出对话框与提示文案。

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
3. 安装 tracing（宿主层 + Engine 诊断运行时）。
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

Engine 的自有事件、以及它依赖的网络栈（iroh、noq、quinn、netwatch、hickory 等）不会交给宿主层，所以宿主的日志级别设置对它们不起作用。

## 级别、Debug 模式与采集

宿主日志级别由 `LogProfile` 决定，优先级依次为：`RUST_LOG`、`UC_LOG_PROFILE`、设置里持久化的 Debug 模式、构建默认值（调试构建 `dev`，发布构建 `prod`）。无论哪个入口，OpenMLS 等敏感目标都被强制关闭。

- **Debug 模式** 只提高宿主进程（daemon、平台适配、Web 服务、守护进程客户端等）的日志详细度，重启后才生效，不改变 Engine 记录。
- **详细记录（Detailed capture）** 是 Engine 的限时采集，控制 `engine.<日期>.jsonl` 的内容，1–15 分钟。
- `GET /diagnostics/debug` 的 `effectiveLogProfile` 报告进程 **实际安装** 的 profile（`uc_observability::installed_log_profile()`），而不是设置值；`debugMode` 是持久化的设置，两者不同说明需要重启，或被环境变量覆盖。

控制台层用于开发。`UC_CONSOLE_LOG=off` 关闭它。macOS 的 launchd 服务把标准输出写进不会轮转的 `service.stdout.log`，所以生成的 plist 设置了这个变量，记录仍然进入按日轮转的 JSON 文件；Linux 的 systemd 单元把输出交给 journal，由 journal 限制大小，不设置。

## 请求关联

daemon 的 HTTP 入口（`request_tracing_middleware`）给每个请求一个 id：

- 调用方发送的 `x-request-id` 只有在 1–64 个 `[A-Za-z0-9._-]` 字符时才保留，否则由 daemon 生成，避免日志注入。
- id 写入请求 span（日志里的 `request_id`）、响应头 `x-request-id`，并通过 CORS 的 `Access-Control-Allow-Headers` 与 `Access-Control-Expose-Headers` 让浏览器能发送和读取。
- 4xx/5xx 的 JSON 对象响应体由中间件统一加上 `requestId`，不依赖各处错误构造代码；前端的 `DaemonApiError.apiRequestId` 保存它。

Engine 的操作记录是否带这个 id，取决于 Engine 是否提供不透明的观察上下文入口。当前固定的 Engine 版本没有，所以 daemon 请求 id 与 Engine 记录之间 **尚未** 关联（计划在升级到含该入口的 Engine 后接入，不在宿主里自造接口）。

## 诊断包

导出由 `uc_observability::startup_logs::export_diagnostic_logs` 统一组装（daemon 在线导出），图形界面在 daemon 不可用时用 Go 的 `packages/desktop-host-go/diaglogs` 做离线导出。两处使用同一份文件名规则，由 `packages/desktop-host-go/diaglogs/testdata/log-source-patterns.json` 约束，Go 与 Rust 的测试读取同一个夹具。

包内容：`logs/` 下的受管文件、`logs/daemon-startup-failure.json`（存在时）和 `manifest.json`。清单包含：

- `mode`（在线/离线）、`exportedAt`、时间窗口；
- `host`：应用版本、操作系统、架构、profile、是否便携、已安装的日志 profile、遥测状态、时区偏移（离线导出只含能在图形界面侧确定的字段）。不含设备、对等端或账户标识，也没有构建提交号（桌面构建没有提交号标记，Engine 的来源提交在 Engine 准备结果里）；
- `collection`：`includedFiles`、`unreadableFiles`、`truncatedFiles`、`missingSources`（当前进程会写、但包里没有文件的来源，原因 `missing` 或 `unreadable`）。

失败时 `POST /diagnostics/log-export` 返回 `code: diagnostic_export_failed` 与 `details.cause`（`source_read_failed`、`archive_write_failed`、`destination_unavailable`、`unknown`），并记录一条带相同 cause 的 ERROR。设置页在回退到离线导出时显示原因和缺失的来源。

## 验证

下列路径用真实进程覆盖，见 `tests/e2e/tests/daemon_logging.rs`（`cargo test -p uc-e2e-tests --test daemon_logging -- --ignored`）：同一 profile 上的第二个 daemon 失败并留下启动失败记录且不写任何日志；宿主记录的 `run_id`、`pid` 一致；请求 id 经过 CORS、400/401 响应体和宿主日志；在线导出收录启动失败记录并带 `host` 与 `missingSources`。

仍须在对应环境单独验证的部分：Windows 的优雅停止与 `TerminateProcess` 回退、macOS launchd 服务实际的标准输出大小、图形界面的离线导出对话框与提示文案。

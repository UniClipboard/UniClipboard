# 0014 · 客户端可观测性：OpenTelemetry / OTLP 与动态日志等级

- 状态：已采纳，待实现；本文中的配置、CLI、HTTP API 和诊断接口均为目标设计，当前尚未提供。
- 日期：2026-11-09。
- 关联：[0002 配置热加载](0002-config-file.md)、[0003 Engine SDK](0003-engine-sdk.md)、[0005 仓库布局](0005-monorepo-layout.md)、[Engine 契约](../engine-contract.md)、[流水线契约](../pipeline-contract.md)、[同步后端契约](../sync-backend-contract.md)。

## 背景

unic 是常驻客户端，需要区分剪贴板观察、本机提交、后台发送、远端接收与系统应用各阶段的问题。断网、挂起、用户等待和遥测接收端故障都不能阻塞业务。设备地址、票据和剪贴板内容仍遵守现有秘密边界。

Engine 已支持注入 `*slog.Logger`，要求日志不阻塞且不包含内容、地址、票据或路径；桌面宿主尚未将 Logger 接入 Engine。现有 Engine 日志使用 `context.Background()`，尚无业务 trace 关联。桌面还使用 `Logf` / `log.Printf`，没有统一的等级控制。`engine/privacy_test.go` 已验证部分日志和错误不泄露秘密，可作为扩展验收的基础。

除了 OTLP 接入，必须允许外部在运行时调整日志等级，无需重启。首版“外部”指配置文件、本机 CLI 和本机 socket API；服务端远程下发等级不在本次范围。

## 决策

### 1. 进程内 SDK，OTLP/HTTP 上报，Collector 放在接收端

使用标准库 `log/slog` 作为业务日志接口，使用官方 OpenTelemetry Go SDK 采集 traces、metrics 和经过筛选的 logs，通过 OTLP/HTTP + Protobuf 主动上报。发行客户端的远程遥测默认关闭，启用后使用 HTTPS。

```text
Engine / 插件 / 宿主
        │ 类型化诊断、slog
        ▼
进程内诊断适配
        ├── 有界本机日志
        └── OTel SDK：过滤、采样、有界队列、批处理
                       │ HTTPS OTLP
                       ▼
                接收网关 / Collector
                       │
                       ▼
                 遥测存储与查询
```

客户端不随包安装 Collector，不开放 Prometheus 抓取端口。gRPC exporter、自动 SQL 埋点、跨设备 trace propagation 和可靠磁盘补传暂不引入。

采用以下组件；具体兼容版本在实现时固定到 `go.mod` / `go.sum`，不依赖“最新版本”作为运行条件：

| 用途 | 组件 |
|---|---|
| 日志接口、动态门槛 | `log/slog`、`slog.LevelVar` |
| Trace | `go.opentelemetry.io/otel/sdk/trace` |
| Metrics | `go.opentelemetry.io/otel/sdk/metric` |
| Trace HTTP 导出 | `go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp` |
| Metrics HTTP 导出 | `go.opentelemetry.io/otel/exporters/otlp/otlpmetric/otlpmetrichttp` |
| slog 桥接 | `go.opentelemetry.io/contrib/bridges/otelslog` |
| Logs SDK / HTTP 导出 | `go.opentelemetry.io/otel/sdk/log`、`go.opentelemetry.io/otel/exporters/otlp/otlplog/otlploghttp` |

`otelhttp` 和 Go runtime instrumentation 按实际需要选择性接入，不作为首批业务埋点的前置条件。原生崩溃采集另行设计，不把 OTLP 日志视为完整崩溃报告。

### 2. 宿主拥有遥测生命周期，Engine 保持公开边界

- Engine 保留 `Options.Logger`。关键日志传入对应业务 context，使用稳定事件消息和错误码，不直接格式化底层错误。
- 核心增加项目自有、类型化的诊断 hooks，使用标准库和公开 engine 类型；未提供时为 no-op。具体签名须先补入 Engine 公共契约，不能将 OTel Provider、exporter 或 internal 类型放进公开 Options。
- 平台无关的适配放在 `internal/observability`；桌面配置、本机日志目录、CLI/API 和装配放在 `app/desktop/`。核心不读取 TOML、环境变量或平台凭据。
- 宿主创建并持有 Provider、exporter 和日志输出，负责配置更新、停用、关闭及诊断状态查询。依赖显式注入，避免依靠进程全局 Provider 或修改全局默认 Logger。
- 诊断 hooks 和日志 Handler 不在业务路径执行网络或磁盘 I/O，也不在 Engine 锁内调用可能阻塞的输出；通过有界异步队列交给工作线程。队列满时丢弃诊断记录，记录丢弃数量。
- builtin 与外部插件由宿主/Engine 在调用边界记录阶段和稳定错误码。插件的任意 stdout/stderr、Lua 自由文本和用户通知不能自动成为远程日志。

诊断必须在业务执行点产生，不依赖 GUI 事件订阅重建阶段耗时。业务通知丢失、消费者过慢或重新获取快照，不应改变遥测语义。

### 3. 日志等级可在外部动态调整

持久配置沿用 0002 的目录监听、防抖和“坏配置保留上一份有效值”。所有等级只接受小写 `debug`、`info`、`warn`、`error`；不接受任意数值或自定义等级。

```toml
[logging]
level = "info"

[telemetry]
enabled = false
# endpoint = "https://collector.example.com"  # OTLP/HTTP 基地址，无信号路径

[telemetry.logs]
enabled = false
level = "warn"

[telemetry.traces]
enabled = true
sample_ratio = 0.05

[telemetry.metrics]
enabled = true
```

上例同时给出默认值。`telemetry.enabled` 是远程遥测总开关；各信号的 `enabled` 只有在总开关打开时才生效。仅启用总开关时默认上报 metrics 和采样 traces，日志仍需单独开启。

| 设置 | 生效规则 |
|---|---|
| `logging.level` | 热更新，本机日志门槛 |
| `telemetry.logs.level` | 热更新，远程日志门槛 |
| `telemetry.enabled`、三个信号的 `enabled` | 热更新；关闭立即停止接纳相应远程记录并取消导出 |
| `telemetry.endpoint` | 创建 exporter 时使用，修改需重启；关闭后再开启也不能偷偷应用待重启值 |
| `telemetry.traces.sample_ratio` | 范围 0..1，修改需重启，首版不增加运行时采样覆盖 |

等级门槛使用独立的 `slog.LevelVar`。组合 Handler 的 `Enabled` 在任一已启用输出接受该等级时返回 true，`Handle` 再分别检查各输出门槛；`WithAttrs` / `WithGroup` 派生的 Logger 必须共享动态等级控制。不能在入口统一按本机等级丢弃远程输出本来接受的记录。

等级决定后续记录是否接纳，不承诺清除已经接纳的记录，也不补回过去被过滤的记录。日志等级独立于 trace 采样率和 metrics 导出周期。打开 `debug` 不会开启远程上报，不改变秘密过滤。

#### 本机临时覆盖

CLI 经现有本机 socket 调用诊断 API，不直接改写配置文件：

```sh
unic logs level                                  # 查询本机与远程日志等级
unic logs level debug --for 15m                   # 临时覆盖本机等级
unic logs level debug --target remote --for 15m   # 临时覆盖远程等级，不开启上传
unic logs level reset                            # 清除本机临时覆盖
unic logs level reset --target remote             # 清除远程临时覆盖
```

`--target` 默认 `local`，仅接受 `local` / `remote`；`--for` 默认 15 分钟，解析后必须为 1..3600 整秒。每个目标最多一个覆盖，新设置替换旧设置，操作按宿主控制器接纳顺序生效。持久修改通过配置文件完成。

| HTTP API | 语义 |
|---|---|
| `GET /v1/diagnostics/log-level` | 查询两个目标的 configured / effective 等级、source、到期时间与日志丢弃计数 |
| `PUT /v1/diagnostics/log-level` | body `{target, level, duration_seconds}`，三字段必填；成功返回更新后的查询快照 |
| `DELETE /v1/diagnostics/log-level?target=local` | target 必填；清除该目标覆盖，幂等；成功返回更新后的查询快照 |

`PUT` 不接受未知字段；等级、目标或时长非法返回 `400 invalid_argument`，不改变任何状态。三个 API 仅暴露在现有本机受目录权限保护的 socket，不增加远程监听或绕过本机访问边界。

每个目标的状态包含 `configured_level`、`effective_level`、`source`（`config` / `override`）、`expires_at_ms`（无覆盖时为 null）和 `dropped_records`（该输出自本进程启动以来的累计丢弃数）。查询同时返回远程总开关与日志开关的有效状态；无论远程日志是否启用，都允许调整并查询其门槛。

覆盖不落盘，进程重启后清除。覆盖有效期间，合法配置热更新仍更新 configured 值；有效等级继续由覆盖决定。到期或 reset 后，恢复当时最新的有效配置值，不恢复覆盖创建时的旧值。非法配置不影响覆盖或既有配置。

到期使用单调时钟，在读取/判断有效等级时检查，定时器只负责及时发布状态变化；旧覆盖的定时器不能清除新覆盖。墙上时间只用于显示 `expires_at_ms`。进程被系统挂起期间不能保证准时执行回调，恢复后在接纳下一条日志前重新检查。

配置、设置覆盖、清除和过期的状态变化按宿主控制器串行处理；查询返回一致快照。新增 `diagnostics` 状态失效事件，仅通知重新查询，不带日志正文。`GET /v1/config` / `unic config` 展示持久配置；当前有效等级和临时覆盖通过上述诊断入口查询。

现有 `Logf` / `log.Printf` 逐步替换为明确等级的结构化日志；暂未替换的调用只能映射为固定等级，不能根据自由文本猜测严重程度。第三方日志另设过滤适配，不能将全量网络调试输出直接送往远程。

### 4. 业务阶段分别计时，保留既有取消和成功语义

| 阶段 | 首批诊断 |
|---|---|
| 观察与接纳 | 实际变化的接纳、基线忽略、防回环和策略过滤次数；不为每次轮询建 span |
| 处理与提交 | 处理器执行、历史提交的耗时与结果；commit 成功和同步派发分别记录 |
| 后台发送 | 按后端、目标建立内部工作 span，记录排队、发送、取消、被替代与失败 |
| 接收与提交 | 准入、校验、去重、received 处理、存储与发布结果 |
| 系统应用 | 应用耗时与结果，区分权限、格式和资源错误 |
| 配对与连接 | 有限阶段的结果和重连次数；等待用户批准不记网络错误 |

目标标识可以用于进程内关联，但不作为远程属性。发送完成、远端接收完成和系统应用成功分别统计，不能推导当前协议没有证明的结果。

异步工作只继承发起操作的 trace context / 因果关联，执行使用 Engine 生命周期控制的 context；不能让调用者取消导致已接纳的提交或后台工作被额外取消。长时间用户等待和 SSE 不建立贯穿整个等待期的业务 span。

首版不修改 wire / 插件 RPC 以传播 trace，不使用内容 hash、历史 ID 或设备 ID 生成 trace ID。跨进程、跨设备和原生语言边界的传播另行补齐相应契约。

### 5. 客户端有界、允许丢失、不拖慢业务

Trace 使用非阻塞批处理，不启用 `WithBlocking()`；Logs 同样使用有界批处理；Metrics 限制属性基数并周期导出。本机日志使用有界异步写入及轮转，总大小和保留期均有限额，文件位于应用私有目录。

以下是首轮实现和压测的起始参数，不是已测得的性能保证。调整默认值须记录测量结果：

| 项目 | 初始值 |
|---|---|
| Trace 队列 / 单批 | 1024 spans / 128 spans |
| 远程 Logs 队列 / 单批 | 1024 records / 128 records |
| Trace / Logs 批处理等待 | 5 秒 |
| Metrics 导出周期 | 60 秒 |
| 单次网络导出超时 | 3 秒，包含该次导出的重试预算 |
| 正常 trace 头部采样率 | 5%，再限制每进程每分钟最多 60 个本地根 trace；内部子 span 随该根 trace |
| 本机日志队列 | 1024 records |
| 本机日志轮转 | 总计不超过 20 MiB，保留不超过 7 天，以先到者为准 |
| 单条日志编码大小 | 不超过 8 KiB；超限丢弃并计数，字段先经白名单筛选 |
| Trace 属性 / 事件限制 | 每 span 最多 32 个属性、16 个事件；每字符串最多 256 字节 |
| 正常退出遥测关闭预算 | 独立 context，总计不超过 2 秒 |

实现须为各指标设置属性白名单和基数上限，包含 OTel 内部属性与事件的限制。队列条数并不等于内存字节上限；实际内存、CPU、空闲唤醒和上传量必须测量。超过队列、速率或属性限制时丢弃遥测并增加本机计数，不拖慢剪贴板或同步。

SDK 队列只作为临时缓冲，不承诺断网、崩溃或重启后的可靠投递。服务端 Collector 持久队列只保护已到达服务端的数据。首版不增加磁盘 spool，也不回放旧 metrics；本机日志保留用于排障，诊断包由用户主动导出，导出范围须另行定义。

关闭远程遥测或某个信号时，先关闭相应接纳和导出入口，再取消在途导出并丢弃尚未发送的缓冲；不能使用会再次发送缓冲的默认 shutdown/flush 路径替代停用。关闭成功返回后不再发起相应网络请求；已经发出的请求和接收端已收到的数据无法撤回。重新开启只采集之后的数据，不补发关闭前积压。

正常退出可以在独立总预算内尝试 flush；业务关闭和资源释放不等待遥测无限重试。遥测初始化失败、磁盘写入失败、DNS/TLS 错误、限流或接收端不可用都只令诊断降级，不令 Engine 启动失败或改变同步结果。exporter 自身错误进入受限本机通道，不再次送入同一远程 exporter。

头部采样丢弃的 trace 无法在结束时因错误补回。错误计数不受 trace 采样影响，错误日志按自身等级、限额和开关接纳；首版不承诺保留全部错误 trace，也不加入尾部采样。

### 6. 属性白名单和用户授权

远程 Resource 显式构造，只允许产品服务名、构建版本、OS 类型、架构等经过审核的发布信息；不自动探测主机名、用户名、进程参数或本机路径。

业务属性仅允许阶段、有限操作类型、结果、内置后端类型、内容大类、稳定错误码等受控枚举。大小和数量如需上报先分桶；插件实例 ID、第三方插件名称和其他用户可扩展值不能直接成为指标标签。

以下数据在任何日志等级下都禁止进入远程遥测；内容、秘密和路径同时遵守本机日志的既有约束：

- 剪贴板正文、摘要、预览、HTML、文件内容、文件名和绝对路径。
- 设备名、设备 ID、指纹、公钥、配对码、地址、票据、传输凭据及内容 hash。
- 请求/响应正文、URL 查询串、含历史或设备 ID 的原始路由、SQL 参数。
- 原始第三方错误、插件任意自由文本及 exporter 凭据。

先在产生处控制字段，再由适配层白名单防御；Collector 脱敏只能作为补充。HTTP 埋点使用有限路由模板；不假定 `otelhttp` 的默认属性天然满足上述边界。

`telemetry.endpoint` 是无用户信息、无查询串、无 fragment 的 HTTPS 基地址；启用总开关时必须有可用端点。开发测试的 HTTP loopback 例外通过显式测试/开发装配提供，不作为发行配置选项。OTel 环境变量不能绕过宿主开关、采样限制或秘密过滤，exporter 参数由宿主显式提供。

凭据从平台秘密存储或受控宿主注入获得，不写入普通 TOML 或查询响应；具体存储接入在实现前确定。配置 API 和错误只返回安全分类或“已设置”状态，不回显凭据。公开发行包中的共享写入令牌不能视为可信客户端身份；接收网关必须限制写入范围、流量和请求大小。

### 7. 开发环境与接收端

本地开发采用 `grafana/otel-lgtm` 验证埋点与三类信号；它只作为开发/测试工具。生产客户端只绑定标准 OTLP，不绑定查询平台或厂商 SDK。

自托管接收端优先评估 SigNoz；已有 Grafana 平台时复用 Collector、Tempo/Loki 和指标存储。接收端的部署、存储保留期、成本和服务配置另行决定，不作为客户端首批实现的前置交付。

未来 Android/iOS 复用 Engine 的诊断语义，网络调度、挂起恢复和上报预算由各自宿主控制。原生 UI 与 Go 核心避免重复统计；移动平台可用性需真实构建和设备验证，不由本决策推定已支持。

## 实施顺序与验收

1. **本机日志与动态控制。** 接通 Engine Logger，统一关键日志等级和 context，加入有界输出、配置热加载、CLI/API、临时覆盖和状态查询。补齐 Engine 诊断 hooks 与桌面 API 契约后再实现，状态变化不包含正文。
2. **Metrics / Trace 最小闭环。** 覆盖 commit、send、receive、apply，使用开发 Collector 验证阶段结果、异步关联和错误分类；默认关闭远程遥测。
3. **远程 Logs。** 接入经过审核的 slog 桥接和独立等级门槛，验证 local/remote 分支分别过滤，配置/覆盖均不能绕过上传开关。
4. **故障与资源验收。** 验证队列满、断网、DNS/TLS 失败、接收端限流、磁盘满、退出超时、反复启停和系统恢复；确认同步不等待遥测，Provider/工作线程不泄漏。

动态控制至少覆盖：既有及派生 Logger 立即生效、两个输出互不误过滤、非法输入不改变状态、覆盖替换、旧定时器不清除新覆盖、覆盖期间配置更新、到期/reset 恢复最新配置、重启清除覆盖、查询一致性及挂起恢复后重新检查。

隐私验收扩展现有测试，捕获本机日志、导出的 spans / metrics / logs、Resource、exporter 错误和诊断响应，搜索地址、票据、配对码、正文、路径和身份等测试秘密。所有等级和临时覆盖模式使用同一套边界。

性能记录至少比较遥测关闭、正常采样、debug 和队列满四种模式下的空闲 CPU、内存、唤醒次数、业务延迟与上传量。测试使用内存 exporter / 假时钟 / 本机接收端，不依赖真实外部服务。实现阶段按项目要求运行 `go test -race ./...`、lint 和桌面构建。

## 后果与范围

客户端增加 SDK、异步队列和诊断控制器，但遥测故障与业务结果隔离。接受采样、队列溢出和离线丢失，以本机有界记录补充排障。可在不重启进程的情况下提高日志详细程度，临时覆盖会自动到期。

本次不实现服务端远程控制、按模块任意配置等级、可靠磁盘补传、完整崩溃报告、跨设备 trace 传播、尾部采样或生产查询平台部署。后续增加这些能力须补齐各自的授权、资源和协议边界。

## 参考

- [Go slog：动态等级与 Handler](https://pkg.go.dev/log/slog)。
- [OTel Go Trace SDK：批处理、队列与非阻塞行为](https://pkg.go.dev/go.opentelemetry.io/otel/sdk/trace)。
- [Collector resiliency：内存队列、持久队列与丢失边界](https://opentelemetry.io/docs/collector/resiliency/)。

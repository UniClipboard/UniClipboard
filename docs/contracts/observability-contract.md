# Client observability contract: logs, metrics and product events

- Status: Logs, metrics and PostHog product events are implemented in the workspace. Local process acceptance and hosted limitations are recorded in the [0019 receipt](../exec/0019-posthog-integration.md). Traces are not implemented.
- Date: 2026-11-10.
- Scope: Local structured logs, independent log levels, configurable OTLP logs/metrics, PostHog product events, lifecycle and resource bounds.
- 依据：[Engine 契约](engine-contract.md)、[流水线契约](pipeline-contract.md)、[历史契约](history-contract.md)、[同步后端契约](sync-backend-contract.md)。
- 执行与证据：[0014 执行计划](../exec/0014-client-observability-otlp.md)。

## 1. 范围与不变量

This implementation provides logs, metrics and product events. Traces、跨设备传播、远程控制、崩溃采集、诊断包、磁盘补传和生产 Collector 部署不在本次范围。
本次不解析 `telemetry.traces` 配置。该配置按未知键拒绝，不提供无效开关。

1. Uploads default enabled. The TOML master and each signal switch can disable upload. Log level changes never change these switches.
2. 遥测失败不改变业务结果，不阻止 Engine 启动。
3. 配置非法仍按现有配置规则处理：启动时报错；热加载时保留上一有效配置。
4. 诊断公开签名只依赖标准库和公开 Engine 类型。公开 Options 不出现 OTel 类型。
5. 宿主持有 Provider、exporter、输出队列和控制器。不得修改全局 Provider 或全局默认 Logger。
6. 业务线程不执行遥测网络或磁盘 I/O，不等待遥测队列空间。
7. 诊断在业务执行点生成，不从 GUI/SSE 事件反推业务结果。
8. Logs and metrics contain no identity, content, path or user-extensible text. Product events may contain only the independent random analytics identifier specified in section 10.
9. Metrics 是允许丢失的诊断统计，不是计费账本或可靠业务审计。

The UI-free service in `app/service/` owns configuration, private state, socket routes and diagnostics assembly. The platform-independent adapter remains in `internal/observability/`.

## 2. Engine 诊断接口

### 2.1 公开签名

`engine.Options` 提供以下可选字段。既有 `Logger *slog.Logger` 保留。
下列类型均定义在公开 `engine` 包中，且只使用标准库类型。

```go
type DiagnosticStage string
type DiagnosticResult string
type DiagnosticBackend string
type DiagnosticContentKind string

type DiagnosticRecord struct {
    Stage       DiagnosticStage
    Result      DiagnosticResult
    Backend     DiagnosticBackend
    ContentKind DiagnosticContentKind
    Code        Code
    Duration    time.Duration
}

// Options 的新增字段；不是完整 Options 定义。
Diagnostics func(context.Context, DiagnosticRecord)
```

`Diagnostics == nil` 表示不采集业务 metrics。Logger 与 Diagnostics 分别注入。
记录不含 `error`、自由文本、map、业务 ID、字节内容或文件引用。
`Code` 成功时为空。失败时只使用公开 `engine.Code` 常量，禁止将底层错误转为字符串。
适配层将未知 Code 映射为 `internal`；不得把未知值导出。

回调必须快速返回。回调只校验固定字段并尝试放入有界队列。
Engine releases its state mutex before invoking the callback. Lifecycle/commit guards may remain held. The callback must not call Engine methods or wait for business/lifecycle completion.
宿主不得保存回调 context。回调 panic 必须在诊断调用边界隔离，不改变已经确定的业务结果。
本次不使用 context 生成 trace、标签或后台任务取消关系。

### 2.2 有限取值

| 字段 | 允许值 |
|---|---|
| Stage | `observe`、`process`、`commit`、`send_queue`、`send`、`receive`、`apply` |
| Backend | `none`、`tailcat`、`external` |
| ContentKind | `text`、`image`、`files`、`mixed`、`unknown` |
| Result | 只允许下节中对应阶段的结果 |

`none` 表示没有后端维度。内置 tailcat 映射为 `tailcat`，其他同步插件全部映射为 `external`。
禁止把插件 ID、实例 ID 或第三方插件名称放入 Backend。
多个表示属于同一内容大类时只记该大类。文本与 HTML 属于 `text`。跨大类组合记 `mixed`。
无法在准入前确认大类时记 `unknown`，不得为了分类读取正文。
未知枚举或非法阶段组合使该条诊断失效，增加本机 `invalid_record` 计数，不生成新标签值。

### 2.3 统计边界

| 阶段 | 一条记录的单位与时点 | 允许结果 | Duration |
|---|---|---|---|
| observe | 一次已进入 Engine 的观察判定结束；不统计宿主空轮询 | `accepted`、`baseline`、`loop_prevented`、`policy_filtered`、`failed` | 不使用 |
| process | 一次处理器调用结束；每个实际调用分别记 | `succeeded`、`failed`、`cancelled` | 调用开始至结束，包含该调用内等待，不含调用前排队 |
| commit | 一次唯一提交工作结束；所有幂等调用共享这一条 | `succeeded`、`failed`、`cancelled` | Engine 接纳工作至历史提交结果确定 |
| send_queue | 一个后端、一个目标的发送工作离开队列 | `started`、`cancelled`、`superseded` | 进入发送队列至开始执行或未执行终止 |
| send | 一个后端、一个目标的实际发送调用结束 | `succeeded`、`failed`、`cancelled`、`superseded` | 开始发送至该调用结束，不含排队 |
| receive | 一次进入 Engine 接收入口的入站工作结束 | `succeeded`、`duplicate`、`policy_filtered`、`rejected`、`failed`、`cancelled` | 准入开始至校验、处理、存储和发布完成，或在中途终止 |
| apply | 一张成功创建的应用票据终结 | `succeeded`、`failed`、`cancelled`、`deferred` | BeginApply/BeginApplyHistory 成功创建票据至结果报告或票据失效，包含宿主调度等待 |

每个计数单位最多生成一条终态记录。重试的新发送调用单独计数。
幂等查询、重复等待、历史浏览和 GUI 事件重放不增加提交计数。
相同内容经不同真实入站调用到达时，可分别生成 receive 记录；去重命中的结果必须为 `duplicate`。
提交工作包含其处理阶段，因此 commit 与 process 耗时可以重叠，禁止相加作为端到端耗时。
入站内容生成 receive 记录及实际 process 记录，不再生成本机 commit 记录。
同步关闭、无目标或策略过滤不能把已提交历史的 commit 结果改成失败。
未开始发送的工作只记 send_queue，不伪造 send。
首次报告后不再接受同一票据的终态统计。未报告的票据在失效或 Engine 停止时以 `cancelled` 终结。
等待用户选择内容、配对审批、SSE 和没有成功创建票据的应用都不创建阶段计时。
apply 是票据生命周期耗时，不声称只测量系统 API 写入耗时。

`failed` 和 `rejected` 必须携带非空 Code。`cancelled` 使用 `cancelled`。
`deferred` 允许携带已有稳定错误码。其余结果不携带 Code。
耗时使用进程单调时钟。除 observe 外 Duration 必须大于等于零。
Engine 取消只能沿既有生命周期传播；诊断不得额外取消已接纳的工作。

发送 succeeded 只表示现有发送契约证明的成功。它不证明远端接收、远端应用或用户粘贴。
接收 succeeded 只表示本机接收发布完成。应用 succeeded 只表示宿主报告系统写入成功。
Connection and session observations follow section 11. CPU, RSS and scheduler wakeups remain external acceptance measurements.

## 3. Metrics 规格

### 3.1 指标目录

所有指标使用固定 instrumentation scope `unic.observability`，scope 版本固定为 `1`。
OTLP 单位采用下表的值。计数为非负单调 Counter，耗时为显式边界 Histogram。

| 名称 | 类型 | 单位 | 标签 | 记录规则 |
|---|---|---|---|---|
| `unic.pipeline.operations` | Counter | `{operation}` | `stage`、`result`、`backend`、`content_kind` | 每条有效诊断加 1 |
| `unic.pipeline.duration` | Histogram | `s` | `stage`、`result`、`backend` | 除 observe 外，每条有效诊断记录 Duration 秒值 |
| `unic.pipeline.errors` | Counter | `{error}` | `stage`、`backend`、`code` | 仅 failed/rejected；每条加 1 |
| `unic.observability.dropped` | Counter | `{record}` | `signal`、`reason` | 本节定义的本机丢弃数变化量；允许远程导出自身丢失 |
| `unic.observability.export_failures` | Counter | `{export}` | `signal`、`code` | 一次完整导出调用最终失败时加 1，不按内部重试次数加 |

Histogram 边界固定为 `0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10, 30, 60` 秒。
大于 60 秒的值进入最后的正无穷桶。耗时不加原始大小、目标数或业务 ID 标签。
`errors` 是 `operations` 失败结果的子集，不能将两者相加作为操作总数。
一个业务工作可经过多个阶段，不能跨 stage 求和解释为用户操作数。

`code` 仅允许现有公开 Engine Code。Exporter 错误单独映射为 `dns`、`tls`、`timeout`、`rate_limited`、`unauthorized`、`unavailable`、`internal`。
遥测禁用引起的主动取消不计 export_failures。
`signal` accepts `local_logs`, `remote_logs`, `metrics`, and `analytics`.
`reason` 仅为 `queue_full`、`oversize`、`invalid_record`、`sink_failed`、`disabled_discard`。
等级过滤、正常关闭时未采集的业务、停用期间的新记录都不计丢弃。

### 3.2 聚合、丢失与重启

每个 instrument 最多 1024 个属性集合，包含 SDK overflow 集合。
业务属性只使用本契约的有限枚举。不得通过 OTel 环境变量扩大标签或属性上限。
如 SDK 使用 overflow，只允许额外属性 `otel.metric.overflow=true`。必须在接入固定 SDK 版本时验证该行为。
不接入自动 Resource 探测或其他自动 instrumentation。

PostHog `/i` endpoints use delta counters and histograms, collected every 60 seconds. Failed exports lose that interval; there are no exporter retries. Pathless Collector endpoints retain cumulative temporality and must compute per-stream differences instead of summing repeated totals.
进程重启或 metrics 重新开启时使用新的聚合起始时间，不继承旧累计值。
本机状态中的丢弃数和导出失败数始终按进程累计，停用或重开不能清零。

仅在 metrics 有效开启时接纳新业务诊断。关闭期间不缓存，不回放，不重建 GUI 事件。
关闭时丢弃队列和聚合器。重新开启时只采集新记录。
同一 exporter 的重试沿用同一批数据及起始时间，不生成新的业务计数。
业务 hooks 队列满会使远程统计少计。适配层必须提供本机累计丢弃计数。
Collector 故障、进程强杀和最后一次未导出的累计增量仍可能丢失。
当多个进程具有相同 Resource 时，禁止在接收端直接合并成单一累计流。生产多客户端的流区分方案须在生产接收端交付中补充；本次只验收每个隔离接收端对应一个被测进程的累计语义，不增加设备或安装 ID。

## 4. Logs 规格

### 4.1 结构与筛选

业务日志入口为 `slog.Logger`。每条日志由时间、等级、固定事件名和审核属性构成。
本机采用 JSON Lines。远程 Body 只使用固定事件名，不导出格式化自由文本。
本机固定键为 `time`、`level`、`msg`，其他键只来自属性白名单。
time 使用 UTC RFC3339Nano；level 使用 `DEBUG`、`INFO`、`WARN`、`ERROR`；msg 使用下列固定事件名。
不输出源码文件位置、调用栈或进程参数。

首批事件名为 `engine.created`、`engine.started`、`engine.stopped`、`engine.closed`、`pipeline.completed`、`pipeline.failed`、`host.failed`、`config.reloaded`、`config.rejected`、`diagnostics.changed`、`telemetry.degraded`。
四个生命周期事件和成功配置加载使用 info；配置拒绝、可恢复业务失败和遥测降级使用 warn；无法继续的宿主故障使用 error。
`pipeline.completed` 使用 debug，只在实际 commit/apply 终态生成；`host.failed` 使用 error。
`diagnostics.changed` 使用 info。新增事件必须先更新本节，不得将任意 msg 自动转为远程 Body。
正常业务结果由 metrics 统计，不要求每条成功操作都写日志。

远程属性只允许 `stage`、`result`、`backend`、`content_kind`、`code`、`target`、`source`。
前五项沿用本文枚举。`target` 只为 `local`/`remote`，`source` 只为 `config`/`override`。
本机允许相同字段及日志等级状态，不记录正文、地址、票据、身份、路径或原始错误。
Exporter 降级日志只进入本机，按错误分类每分钟最多一条；禁止回送同一远程 exporter。

`WithAttrs`、`WithGroup` 和每条 Record 的属性必须经过同一筛选。
分组属性不得绕过白名单。只按规定的平面字段导出，未知分组或字段直接丢弃。
在调用任意 `LogValuer`、字符串转换或编码前拒绝未允许的类型。允许的值只限已校验的枚举及固定状态值。
不得把任意对象、第三方 `error`、插件 stdout/stderr、Lua 文本或用户通知传给远程桥接。
本机也不得借错误日志输出原始配置文本或非法配置值。现有宿主 Logf 调用映射为固定事件和等级，不根据自由文本推断严重程度。

编码后的单条日志最多 8 KiB。超限整条丢弃，不截断 JSON，不截断秘密后继续输出。
最终导出 Body、属性及 Resource 必须再次校验。固定字段字符串最多 256 字节。
远程 logs 关闭时不得创建有后台网络活动的日志 exporter。

### 4.2 等级与输出分支

等级只接受小写 `debug`、`info`、`warn`、`error`。
两个输出使用独立的动态门槛。组合 Handler 的 Enabled 是两个有效输出的逻辑或。
Handle 对两个输出分别判断。派生 Logger 共享控制器，不复制过期等级。
本机 error、远程 debug 的配置不得提前丢弃远程可接纳的 debug 记录。

等级只影响后续接纳。调整等级不清理已接纳队列，也不补回过去记录。
上传开关停用则执行第 7 节的取消与丢弃语义。

## 5. 持久配置与凭据

```toml
[logging]
level = "info"

[telemetry]
enabled = true
# endpoint = "https://collector.example.com"

[telemetry.logs]
enabled = true
level = "warn"

[telemetry.metrics]
enabled = true

[telemetry.analytics]
enabled = true
```

以上是本次默认值。`telemetry.enabled && telemetry.logs.enabled` 决定远程 logs 的请求开启状态。
`telemetry.enabled && telemetry.metrics.enabled` 决定 metrics 的请求开启状态。
`telemetry.enabled && telemetry.analytics.enabled` controls product events. Actual export also requires successful initialization; requested enabled does not prove delivery.

等级、总开关和信号开关热更新。endpoint 修改需重启。
endpoint 是不含用户信息、查询串和 fragment 的 HTTPS 基地址，不接受信号路径。
The base path accepts empty, `/`, `/i`, or `/i/`; exporters append `/v1/logs` and `/v1/metrics`. An absent deployment endpoint is allowed so developer builds remain usable; signals report `not_configured`. An explicitly malformed endpoint is invalid even when upload is disabled.
本机测试使用显式开发装配允许 HTTP loopback，发行 TOML 没有不安全传输选项。
HTTPS loopback 可用于与发行配置完全相同的验收路径。

当前运行 endpoint 固定为进程启动时的值。更改后通过既有 restart_required 展示。
停用再启用继续使用启动时的值。启动时无 endpoint 的进程，新增 endpoint 后必须重启才能上传。
在此之前信号状态保持 degraded，安全错误码为 `restart_required`。
配置查询展示已接纳的持久配置，不表示待重启 endpoint 已经运行。

Host-controlled injection supplies exporter credentials. OTLP uses `Authorization: Bearer`; the pinned PostHog Go SDK uses its standard `/batch/` API with the ingestion token in the JSON `api_key` field. Tokens never enter query strings or diagnostics.
Release assembly may embed the approved ingestion-only `phc_` project token. Developer builds without deployment metadata do not guess a region or destination. Unauthenticated pathless Collectors remain supported; PostHog paths and product events require an ingestion token.
本次不新增凭据 CLI、普通环境变量、TOML token 键或系统凭据存储界面。
Only the ingestion-only project token may be distributed. Personal, secret and query/admin API credentials remain prohibited. The release token is bound to the exact release endpoint; an endpoint override never forwards it to another destination.
令牌按每次信号初始化读取，运行时轮换通过停用再启用生效。读取失败使该信号降级，不回显令牌或底层错误。
查询只返回 `credential_configured` 布尔值。不得输出请求头。

Exporter 必须显式设置 endpoint、协议、TLS、头部、超时和资源。
`OTEL_*` 环境变量不得改变上传开关、目的端点、凭据、采集范围或 Resource。
禁止自动 Resource detectors，禁止 HTTP 重定向。不得把 Authorization 转发到其他端点。
SDK 版本固定后必须验证其环境变量优先级和 HTTP 客户端行为，不能凭默认值推定满足本契约。

## 6. 临时等级覆盖与本机 API

### 6.1 控制语义

每个 target 最多一个覆盖，最新接纳的设置替换旧覆盖。
target 只接受 `local`、`remote`；等级取值见第 4 节。
duration_seconds 必须是 1..3600 的 JSON 整数。零、负数、小数、字符串及 null 均非法。
CLI `--for` 默认为 `15m`；解析结果必须是同一范围内的整秒。

覆盖不落盘，重启清除。覆盖期间配置更新 configured_level，覆盖仍决定 effective_level。
到期或 reset 恢复最新合法配置。旧覆盖的定时器不得清除新覆盖。
配置更新、覆盖设置、清除、过期和查询按控制器接纳顺序串行化。
到期使用单调时钟，每次日志等级判断与状态查询前重新检查；定时器只用于及时通知。
挂起恢复后须在接纳下一条日志前检查。expires_at_ms 仅为墙上时间的显示值。

### 6.2 HTTP

所有 API 只使用现有私有本机 socket，不新增 TCP 监听。

| 方法与路径 | 输入 | 成功响应 |
|---|---|---|
| `GET /v1/diagnostics/log-level` | 无 | 200，LogLevelState |
| `PUT /v1/diagnostics/log-level` | `{ "target": "local", "level": "debug", "duration_seconds": 900 }` | 200，更新后的 LogLevelState |
| `DELETE /v1/diagnostics/log-level?target=local` | target 必填 | 200，更新后的 LogLevelState；无覆盖也成功 |
| `GET /v1/diagnostics/telemetry` | 无 | 200，TelemetryState |

PUT 的三个字段全部必填。拒绝未知字段、重复字段、多个 JSON 值、非对象正文和超过 4 KiB 的正文。
DELETE 拒绝缺失、重复或非法 target。非法输入返回 400，复用现有 `{ "error": "固定安全说明", "reason": "invalid_argument" }` 错误结构，不改变状态。
这里的 reason 使用稳定诊断错误码，不沿用配对专属分类。无需改变其他 API 的错误结构。
关闭过程中的控制请求返回 503，reason 为 `unavailable`，不得阻塞到遥测 flush 完成。

LogLevelState 示例：

```json
{
  "local": {
    "configured_level": "info",
    "effective_level": "debug",
    "source": "override",
    "expires_at_ms": 1794226500000,
    "dropped_records": 0
  },
  "remote": {
    "configured_level": "warn",
    "effective_level": "warn",
    "source": "config",
    "expires_at_ms": null,
    "dropped_records": 0
  },
  "telemetry_enabled": false,
  "remote_logs_enabled": false
}
```

`telemetry_enabled` 是已生效的总开关；`remote_logs_enabled` 表示远程 logs 的接纳入口已开启。
初始化失败、停用及初始化中均不能返回 remote_logs_enabled=true。已就绪组件因导出失败而降级时，仍可保持接纳。
dropped_records 是该日志输出已确认丢弃的记录总数，按进程累计。

TelemetryState 的固定结构：

```json
{
  "configured_enabled": false,
  "effective_enabled": false,
  "restart_required": [],
  "credential_configured": false,
  "logs": {
    "requested_enabled": false,
    "state": "disabled",
    "last_error_code": null,
    "dropped_records": 0,
    "export_failures": 0
  },
  "metrics": {
    "requested_enabled": false,
    "state": "disabled",
    "last_error_code": null,
    "dropped_records": 0,
    "export_failures": 0
  },
  "analytics": {
    "requested_enabled": false,
    "state": "disabled",
    "last_error_code": null,
    "dropped_records": 0,
    "export_failures": 0,
    "pending_records": 0
  }
}
```

state 只接受 `disabled`、`starting`、`active`、`degraded`、`stopping`。
active 表示本机组件就绪，不表示 Collector 已收到记录。
网络导出失败使 state=degraded，但已就绪的接纳入口可保持开启并接受有界记录；下次导出成功恢复 active。
初始化失败的 degraded 没有接纳入口。日志 API 的 remote_logs_enabled 以入口实际状态为准。
effective_enabled 表示总开关已应用，不表示所有信号健康。
restart_required 是待重启键名数组，当前只允许 `telemetry.endpoint`。
`last_error_code` accepts the exporter categories in section 3 plus `initialization_failed`, `restart_required`, `not_configured`, and `identity_unavailable`. Healthy state uses null. The `analytics` object includes the same signal state fields plus `pending_records`, bounded to 0..1024. It exposes no installation identifier.
查询不返回 endpoint、凭据、请求头、原始错误或日志正文。
本机计数独立于 metrics 是否上传，不能从可能丢失的远程 metrics 反推。

状态变化发布既有宿主事件形状的 `diagnostics` 失效通知。Peer 和 Info 均为空。
事件不携带 DTO 或正文；消费者重新查询。逐条日志、逐次计数和每次导出不发送事件，避免事件洪泛。
事件仅在配置/覆盖变化、过期及信号 state 转换时发送。

### 6.3 CLI

```sh
unic logs level
unic logs level debug --for 15m
unic logs level debug --target remote --for 15m
unic logs level reset
unic logs level reset --target remote
unic diagnostics telemetry
```

`--target` 默认为 local。level 无位置参数时查询两个目标。
没有服务运行时，CLI 按[本地服务契约](local-service-contract.md#service-lifecycle)启动临时服务并在命令结束后停止；独立服务和桌面服务只被借用，不会被停止。
临时服务下，带 `--for` 的设置命令持有该服务直到自己创建的覆盖到期、被 reset 或被更新的覆盖替换（比较 `expires_at_ms`），并在 stderr 提示持有时长；覆盖不会比临时服务活得更久。持久服务下命令在接受后立即返回。
较早命令的退出不得撤销较新命令创建的覆盖。
设置和 reset 返回与查询相同的状态信息。CLI 不直接写 TOML，不读取日志目录来推断状态。
本次不增加日志正文读取或远程开关修改命令。持久配置仍通过既有配置入口修改。

## 7. 生命周期、资源与停用屏障

### 7.1 资源上限

| 项目 | 上限或初始参数 |
|---|---|
| 本机日志待处理队列 | 1024 条，每条编码不超过 8 KiB |
| 远程日志待处理队列 | 1024 条，每条编码不超过 8 KiB；所有串联队列合计计算 |
| 业务 metrics 待处理队列 | 1024 条固定字段诊断，不持有 context 或业务对象 |
| 远程日志单批/批处理等待 | 128 条 / 5 秒 |
| Metrics 导出周期 | 60 秒 |
| 单次导出超时 | 3 秒，首版关闭 exporter 内部重试 |
| 正常退出总关闭预算 | 2 seconds shared by logs, metrics and analytics，不分别叠加 |
| 本机文件 | `$UNIC_HOME/logs/`，目录 0700、文件 0600；受系统权限能力约束 |
| 本机日志保留 | 轮转文件及当前文件合计不超过 20 MiB；最长 7 天 |
| 单个轮转分段 | 最多 4 MiB；写入前回收旧段；旧文件按创建时间回收 |

异步队列容量以组件实际总保留量核算。不得给自建队列和 SDK 队列各分配 1024 条后宣称符合上限。
单批、编码缓冲和正在处理的记录须另计入内存测量，不把队列条数当作完整字节上限。
磁盘回收由输出 worker 执行，写入前及至少每小时执行一次；进程未运行时不承诺主动清理。
旧文件回收失败时停止写新记录并降级，不继续突破上限。不删除日志目录外的文件，不跟随符号链接逃逸私有目录。
本机输出失败时仍保留状态查询及累计丢弃计数；错误报告不得递归写入失败的输出。

### 7.2 停用

1. 控制器关闭相应信号的接纳入口，并使旧 generation 失效。
2. 关闭导出入口，取消在途请求及重试。
3. 丢弃旧 generation 队列；metrics 同时丢弃旧聚合器。
4. 确认旧 worker 不会再次发起请求，再将配置应用标记为完成。

配置“停用已生效”的状态是上述屏障完成后的状态。重新开启必须在旧 generation 屏障后执行。
不得使用会 flush 旧队列的默认 Shutdown 路径代替停用。
已经发出的请求和接收端收到的数据无法撤回。
停用成功后不得再发起对应信号的请求，包括重试与后台初始化请求。
仅修改日志门槛不执行停用屏障，也不丢弃旧队列。
Disabling the master switch stops all three remote signals. Local logging continues at its independent threshold.

正常退出允许在独立的 2 秒总预算内尝试 flush，再取消并释放资源。
遥测不能延长业务资源的关闭预算。强杀不承诺 flush。
反复启停不得泄漏 Provider、worker、timer、HTTP 连接或文件句柄。

### 7.3 丢弃计数

每条记录对每个输出最多计一次丢弃。两个日志输出分别计数。
已过滤的记录从未被接纳，不计丢弃。
停用丢弃队列时计 `disabled_discard`；不可导出的编码记录计 `oversize`；队列拒绝计 `queue_full`。
日志最终导出失败时按该批记录数计 `sink_failed`，不按每次 HTTP 重试重复计数。
Metrics 导出失败增加 export_failures，不把同一累计点反复计成丢弃记录。
Metrics 的 dropped_records 只统计入口记录丢失及停用时未处理队列记录，不声称计量未上传聚合增量。
SDK 内部队列必须提供可验证的拒绝计数，或由适配层接管唯一的有界批处理入口；不得静默漏计却声称计数完整。
具体实现以固定 SDK 的公开扩展点为准，不依赖其私有字段或日志文本。

## 8. 隐私与远程 Resource

Resource 只允许 `service.name=unicd`、`service.version`、`os.type`、`host.arch`。
构建版本只能来自发布元数据，开发版本使用固定 `dev`，不能填本机路径或脏工作区摘要。
OS 与架构使用构建平台的有限值。禁止默认探测主机名、用户名、进程参数、环境变量和安装路径。
OTLP 使用标准协议版本；本次不修改 wire、插件 RPC 或数据库业务协议。

以下数据禁止进入日志、业务诊断和远程 Resource/属性：正文、摘要、预览、HTML、内容 hash、文件名、文件路径、设备名、设备 ID、指纹、公钥、配对码、地址、票据和传输凭据。
此外，远程禁止原始 URL、查询串、SQL、请求/响应正文、第三方错误和任何任意自由文本。
稳定 Code 和有限枚举在产生处确定，适配层再次校验。Collector 脱敏不能代替客户端过滤。
诊断 API 只提供安全状态。既有配置 API 的路径字段沿用既有契约，不复制到诊断 API。

## 9. 验收要求

以真实 `unic`/`unicd` 多进程和可控 OTLP 接收端验证本契约。
测试先定义场景，再实施功能。不在实现后补写单元测试。
不使用内存 exporter 的通过结果代替 OTLP/HTTP 或真实进程证据。
隔离测试如确实必要，必须先记录全部相关失败方式，并说明 E2E 不能覆盖的原因。

验收必须保存来源身份、二进制摘要、构建参数、配置摘要、准确命令、退出码、脱敏请求时间线、解码后的 OTLP 数据、断言报告和重放入口。
场景、通过标准和验收工件结构以[执行计划](../exec/0014-client-observability-otlp.md)为准。
未实现、未运行、未通过和原生验收未授权必须分别登记。文档检查通过不代表本契约已实现。


## 10. PostHog product events

The service owns the sole SDK instance and analytics identity for a profile. Desktop registration may include a 16-character alphanumeric `launch_id`, generated once per adapter process and omitted after the first ready acknowledgement. The service retains only the last ready launch key in memory; no launch key enters an event, log, metric or ordinary diagnostics output.
`engine.Options.ProductEvents func(context.Context, ProductEvent)` is an optional nonblocking callback.
It uses public Engine/std-library types only. Panic is isolated; no caller context or business object is retained. Callbacks run outside `e.mu`, but lifecycle/commit guards may remain held; callbacks must not call Engine methods or wait for lifecycle work.
The adapter validates finite names and properties before SDK enqueue and filters serialized properties again after SDK enrichment. The SDK system-context cache is warmed on the lifecycle/control path, so the first business enqueue performs no OS detection I/O.
It does not install SDK request-context middleware, identify users, alias identities, poll feature flags or capture exceptions.

| Event | Authoritative emission boundary |
|---|---|
| `app_started` | Successful service startup (`product=cli`), or first ready for a pending Desktop process launch (`product=desktop`). A local launch key suppresses repeated ready calls and same-process reconnects; the client clears its pending key after acknowledgement. |
| `pairing_completed` | Successful atomic approval changes an untrusted device to trusted. Approval failure, pending requests and reconnects do not emit it. |
| `history_applied` | A successful, non-dead application ticket with `ApplyHistoryRequest.UserRestore=true`. Duplicate apply reports do not emit again. Action application sets UserRestore false. |
| `manual_send_completed` | One explicit send operation reaches a terminal outcome across all targets. A submission with no operation emits `skipped`, its commit failure, or `failed` with the sync error code when commit succeeded but sync admission failed. Automatic observations, Action dispatch and restore-triggered synchronization are excluded. |
| `action_completed` | The unique idempotent Action submission finishes execution/commit. An apply failure after that commit does not change its execution result. |

The public ProductEvent has `Name`, `Product`, `Result`, `Code`, `ContentKind`, `ActionCategory` and `SourceKind`.
Allowed names are the five entries above plus `remote_content_applied` from section 11. Product is `cli` or `desktop` (empty maps to cli).
Result accepts empty, `succeeded`, `failed`, `cancelled`, `superseded`, `partial`, or `skipped`.
Code uses the existing safe Engine code vocabulary; content category uses the existing diagnostic content vocabulary.
Action category is empty, `builtin`, or `external`. No plugin/action key is exported.
Common properties are schema version 1, published component version (or dev), OS and architecture. `product` identifies the emitting component: service completions use `cli`; only adapter startup uses `desktop`. It does not assert which UI originated a service action.
The SDK processing flags are always `$process_person_profile=false` and `$geoip_disable=true`.
No device identity, content, search term, path, target list or business request/operation identifier enters an event.
An event UUID is SDK-generated delivery metadata, not the business request identity.

### 10.1 Identity and disable lifecycle

After Engine acquires the profile lock, effectively enabled analytics may create `$UNIC_HOME/analytics-id`.
The identifier is a random 128-bit value written atomically with mode 0600.
No identifier is created when deployment credentials are absent or analytics is disabled.
The identifier persists across enabled restarts. Desktop does not receive it through IPC.

Master disable or analytics disable closes admission, cancels delivery, discards SDK work and waits for SDK cleanup.
The service invalidates and removes the identifier. Startup with a false setting performs the same cleanup before any analytics initialization.
A failed unlink leaves an invalidation marker where possible; re-enable never treats that marker as an identifier.
Identity errors degrade analytics with `identity_unavailable`; they do not select device identity or a shared fallback.
Read operations reject nonregular identity files and use `os.Root` to prevent profile escape.
Deletion removes a symlink itself and does not change its target.
Re-enable generates a new identifier and never replays disabled-period activity.
Previously transmitted data cannot be retracted by changing local settings.

### 10.2 SDK resource and transport bounds

The pinned SDK is `github.com/posthog/posthog-go v1.42.0`.
A single admission counter caps all retained events across SDK queues, partial batches and in-flight batches at 1024.
SDK settings use a message queue of 896, a prepared-batch queue of 1, batches of at most 64 and a five-second interval.
An encoded transmitted event is at most 2 KiB. Requests time out after three seconds and do not retry.
Response reads are capped at 4 KiB. The transport only accepts the configured origin and `/batch/` without a query, so redirects and ancillary SDK destinations cannot receive credentials.
SDK debug/error output is suppressed because it may contain arbitrary HTTP details.
Delivery failures and queue refusal update safe loss counters; pending records are exposed in diagnostics.
A cancelled transport precedes the SDK's flushing Close path during disable. All three signals share one two-second normal exit budget.

### 10.3 Release metadata

`tools/package` reads `UNIC_POSTHOG_HOST`, `UNIC_POSTHOG_PROJECT_TOKEN`, and optional `UNIC_PRODUCT_VERSION` at build time.
Host and token must be supplied together. Host selects the exact US or EU Cloud ingestion origin.
The tool rejects non-ingestion tokens and records only the host, version and credential-presence boolean in its manifest.
These are build inputs, not runtime environment overrides. Tokens are extractable from distributed binaries and provide no client authentication guarantee.
The daemon binds the compiled token to the compiled OTLP base, ending in `/i`.
A custom endpoint needs separately controlled host credential injection; no TOML token key or token CLI is added.

Disable all uploads with:

```toml
[telemetry]
enabled = false
```

Disable only events with:

```toml
[telemetry.analytics]
enabled = false
```

Both changes hot reload. Invalid edits retain the previous valid settings.
Local process acceptance does not prove Cloud ingestion, project Metrics availability or native UI acceptance.

## 11. Tailcat sync diagnostics (0020)

The built-in assembly uses `internal/syncdiag.Observer`. Records contain an internal
collection epoch, finite categories, durations and nonnegative byte counts only.
The epoch is local admission metadata and never a metric attribute. External plugin
RPC and Engine transport options do not change. Observer panic is isolated.

Connection phases are `network_start` / direction `none`, `dial` / `outgoing`, and
`identity_handshake` / `incoming|outgoing`. Results are `succeeded|failed|cancelled|pending|rejected`.
A completed pending/rejected handshake is not an admitted session. Error reasons are
`timeout|cancellation|unavailable|invalid_address|unusable_credentials|identity_mismatch|protocol_mismatch|permission_rejection|remote_close|io_failure|local_stop|suspension|revocation|state_replacement|unknown`.
Only typed errors and local lifecycle decisions select a reason. No string matching.

| Instrument | Unit | Labels | Maximum reachable series |
|---|---|---|---|
| `unic.sync.connection.attempts` | `{attempt}` | phase,direction,result | 20 |
| `unic.sync.connection.duration` | `s` | phase,direction,result | 20 |
| `unic.sync.connection.errors` | `{error}` | phase,reason | 45 |
| `unic.sync.session.ends` | `{session}` | reason,expected | 15 |
| `unic.sync.session.duration` | `s` | expected | 2 |
| `unic.sync.session.observed_time` | `s` | none | 1 |
| `unic.sync.recovery.ends` | `{episode}` | result | 3 |
| `unic.sync.recovery.duration` | `s` | none | 1 |
| `unic.sync.recovery.pending` | `s` | none | 1 |
| `unic.sync.path.samples` | `{sample}` | direction,path | 8 |
| `unic.sync.path.transitions` | `{transition}` | from,to | 6 |
| `unic.sync.path.upgrade_duration` | `s` | none | 1 |
| `unic.sync.transfer.bytes` | `By` | direction,result | 6 |
| `unic.sync.transfer.size` | `By` | direction,content_kind | 10 |
| `unic.sync.transfer.duration` | `s` | direction,result,start_path,size_class | 144 |
| `unic.sync.remote_applications` | `{application}` | none | 1 |

Time histograms use `.001,.005,.01,.025,.05,.1,.25,.5,1,2.5,5,10,30,60,120,300,900,3600` seconds.
Size histograms use `0,1024,65536,1048576,16777216,67108864,1073741824,4294967296` bytes.
Size classes use binary boundaries: `empty|le_64k|le_1m|le_16m|gt_16m|unknown`.
Transfer results are `succeeded|failed|cancelled`. Direction is `incoming|outgoing`.
All new instruments retain the existing 1024-series cap. Invalid records increment
`invalid_record` and create no label. Unknown errors become `unknown` before export.

An admitted session has one terminal claim. Local stop, suspension, revocation and
duplicate replacement are expected. EOF alone means remote_close. Connected exposure
is monotonic session time inside the current metrics epoch. Re-enable resets baselines.
An unexpected terminal starts one recovery episode only for a still eligible peer.
Repeated dial failures share that episode. Admission in either direction recovers it.
Loss of eligibility abandons it; stop or collection epoch replacement interrupts it.
No disabled-period time or cross-process duration is replayed. A crash can lose a final
observation. Pending ages are interval samples, not successful recovery durations.

A service-owned collector samples every 10 seconds while sessions or recoveries exist.
Its worker is the existing metrics generation worker; it performs no active probes.
Transfer-start paths use only its cached sample, at most 10 seconds old. Unsupported
outgoing status is unknown. Incoming tcnet status maps the connection's virtual
remote IP using v0.7.0's ULA /48 plus the first 80 node-key bits. Status lacks
TailscaleIPs in this revision. Ambiguous key-prefix matches remain unknown. Active status requires LastWrite within 30
seconds and a nonfuture timestamp. A valid CurAddr wins over Relay, then PeerRelay,
then a configured relay with fresh active status. Missing or ambiguous mapping is unknown.
Unknown breaks transition continuity. Upgrade timing starts at the first DERP sample
in a continuous known sequence, has up to 10 seconds sampling uncertainty, and excludes
sessions first seen direct. Start-path labels do not describe every payload packet.

Declared authorized/admitted size is recorded once before body I/O. Bytes count actual
successful payload writes/reads, including partial failure and admitted-body draining.
Control headers, rejected-body draining and network overhead are excluded. Body duration
runs from the first payload I/O start to the last payload I/O completion; storage after
that completion is excluded. An empty body records zero bytes and zero duration. Receive
success requires the Engine's accepted completion, not just a complete body read.
A multi-target submission produces one transfer per target. No receiver ACK is added.

### 11.1 Product value and daily admission

`ProductEvent.SourceKind` adds `local|remote|external|action|migration|unknown`.
`history_applied` classifies the selected stored origin when creating its application
ticket: local_system/explicit map to local; remote_device maps to remote. A retained
ticket preserves classification even if the history row is deleted later.
Successful explicit history application retains its existing per-operation frequency.

`remote_content_applied` is emitted only after successful accepted BeginApply receipt
completion for a remote device. BeginApplyHistory never emits it. Duplicate reports,
failed/dead tickets and external-channel receipts do not emit it. Each valid automatic
completion also increments `unic.sync.remote_applications` independent of analytics.
These signals describe clipboard availability and deliberate history retrieval; neither
proves paste, people, time saved or monetary ROI.

The analytics worker loads a private identity/day high-water marker before admission. Clock rollback does not admit a date at or before the marker. It coalesces
automatic-completion notifications in bounded memory. It writes and syncs a 0600 temporary
marker, then atomically renames it before SDK enqueue. The marker contains only analytics
identity epoch and UTC day. A failed marker read/write suppresses daily delivery and
increments sink_failed. It never blocks the business callback. Persist-before-enqueue
can lose the event if enqueue/delivery/crash fails. It is not replayed. A restart normally
suppresses same-day delivery. Disable waits for the worker and clears the marker before
identity rotation; stale markers cannot suppress a new identity. No history or peer
identifier, hash or content is persisted. Exactly-once hosted delivery is not promised.

Operational queries and value-cohort coverage limits are specified in the
[0020 decision](../decisions/proposed/0020-tailcat-sync-observability.md).

### 11.2 Query definitions, version 1

These are aggregation specifications. Hosted execution/publication remains a
separate acceptance layer. All numerator and denominator streams use the same
release, enabled-collection window and backend population. Export loss is visible
beside every result. Never join metrics with installation identifiers.

| Query ID | Definition | Required accompanying data |
|---|---|---|
| `sync-v1-dial-failure` | failed dial attempts / all completed dial attempts | cancelled dials shown separately; phase=dial only |
| `sync-v1-handshake-failure` | failed handshakes / all completed handshakes | pending/rejected/cancelled shown separately; never divide by dials |
| `sync-v1-disconnect-frequency` | unexpected session ends / (observed_time / 3600) | session exposure, not installation uptime; expected replacements excluded |
| `sync-v1-recovery` | p50/p95/p99 of recovered duration | pending-age distribution and abandoned/interrupted counts; no success-only outage claim |
| `sync-v1-passive-direct` | direct samples / (direct + derp + peer_relay samples) | unknown / all samples, direction coverage and 10-second cadence; not bandwidth share |
| `sync-v1-payload` | body duration percentiles by direction, result, start_path and size_class | declared size histogram and partial bytes; empty bodies excluded from throughput |
| `sync-v1-availability-dau` | distinct analytics identifiers with remote_content_applied per UTC day | telemetry coverage and daily persistence/export losses |
| `sync-v1-history-dau` | distinct analytics identifiers with history_applied and source_kind=remote per UTC day | explicit retrieval only; missing source_kind is unknown, not remote |
| `sync-v1-availability-D1-D7` | fixed day-D availability cohort active on D+1 or D+7 through the same availability event / day-D cohort | require complete UTC days through D+1 or D+7; exclude incomplete windows |
| `sync-v1-history-D1-D7` | same fixed cohort definition using successful remote history retrieval | do not mix event families or rotating identities |
| `sync-v1-history-reach` | distinct identifiers with remote history retrieval / distinct identifiers with remote availability in the same window | history-only recipients and telemetry loss make this coverage-limited, not a complete conversion funnel |

Availability/retrieval repeat use is installation retention, not people retention.
No query estimates pasted items, time saved or monetary ROI. Engineering ROI remains
an ordered cost/benefit assessment: failure stages first, passive-path coverage next,
payload attribution next, and independent availability/retrieval reach next. Reassess
resource and series cost before rollout. No per-retry product event is added.

The passive collector reads one transport snapshot per collection, then classifies
all incoming sockets against its private reduced map. It does not perform one full
status read per session. Session terminals carry their close-time collection epoch;
delayed cleanup after re-enable cannot create recovery from a disabled-period close.
The daily marker uses one reserved temporary filename. Startup and disable remove
that temporary, bounding crash residue and clearing old identity state.

## 12. Offline local log export

`unic logs export [--output FILE]` runs entirely in the CLI. It needs no running
service, clipboard access or network. The default is a unique UTC-timestamped
`unic-logs-*.tar.gz` file in the current directory. Success prints its absolute path.
`--output` selects a file, not a directory. An existing file or symlink is never
replaced. The destination parent must exist and must not be the managed logs directory.

The archive contains only regular `logs/segment-<positive UnixNano>.jsonl` entries
and `manifest.json`. The manifest has format_version=1, UTC creation time and each
entry's relative name, byte count and SHA-256. It contains no profile path, config,
credentials, database, analytics identifier or clipboard data. It copies existing
sanitized log bytes without adding a second logging policy or exporting remote metrics.

The CLI rejects symlinked logs directories, matching symlinks/nonregular entries,
segments larger than 4 MiB and retained input larger than 20 MiB. Source and output
operations use rooted handles; opened source identity must match its selected regular
file. Missing/empty logs fail without creating an archive. Unrelated files are ignored.
Archive and member permissions are private (0600 on POSIX).

Export streams one segment at a time with bounded copy buffers. Each selected file
has a fixed length: appends after selection are excluded. Rotation may remove an
unopened segment, which is skipped; an opened unlinked file remains readable. A
replaced/truncated/unreadable segment fails the export. On error/cancellation, remove
only the archive created by this command. Propagate archive/gzip/output close failures.

This is a best-effort snapshot of already-written bytes. It does not flush service
queues or claim an atomic cross-file view; the last JSONL record can be incomplete
if export overlaps its write. No new service endpoint or configuration key is added.

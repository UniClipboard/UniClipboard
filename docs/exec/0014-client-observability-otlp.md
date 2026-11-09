# 0014 执行计划：Logs 与 Metrics

- 状态：logs + metrics 已接入工作区；验收结果与未覆盖边界见文末收据。Traces 未实现。
- 日期：2026-11-09。
- 设计：[0014 决策](../decisions/proposed/0014-client-observability-otlp.md)。
- 规格：[可观测性契约](../contracts/observability-contract.md)、[Engine 契约](../contracts/engine-contract.md)。

## 1. 交付边界与当前基线

交付本机日志、动态等级、远程 logs、业务 metrics、本机诊断状态和可重复的真实进程验收。
Traces 留待后续实施。本次不实现其 Provider、配置开关、采样器或传播。
不部署生产接收端，不修改 GUI，不启动原生界面，不改变协议和业务结果。

2026-11-09 核对基线：分支 `v2`，HEAD `29286ce50f04d69a087b3e4d6140da8326849dfa`。
工作区已有大量 UI 改动。此身份只用于本次文档调查，不作为未来实现或二进制验收的来源证明。

| 入口 | 当前事实 | 本次目标 |
|---|---|---|
| `engine/options.go` | 已有可选 Logger | 增加标准库类型的 Diagnostics 回调 |
| `engine/engine.go` | 已有少量 slog 日志，使用后台 context | 改为固定事件名；业务日志传递对应 context |
| `app/desktop/internal/host/node.go` | 创建 Engine 时未注入 Logger | 宿主装配日志、诊断和生命周期 |
| `app/desktop/internal/config/` | 已有 TOML 热加载，无 logging/telemetry | 增加严格配置、默认值和待重启键 |
| `app/desktop/internal/api/` | 已有私有 socket 和宿主事件 | 增加诊断 DTO、状态查询和限时覆盖 |
| `app/desktop/cmd/unicd/daemon.go` | 使用 log.Printf | 接入受控结构化输出，移除敏感格式化错误 |
| `go.mod` | 没有 OTel 依赖 | 固定兼容版本，仅引入 logs/metrics 所需组件 |
| `e2e/cli/` | 已有真实进程、目录剪贴板和工件支持 | 扩展 OTLP 接收端和下表场景 |

前置文档阶段不包含业务实现。本轮已按下列阶段接入代码，实施登记和验收证据见文末。未覆盖目标不得写成已验收现状。

## 2. 实施前故障清单

开始编写实现前，先将下列失败方式转为 E2E 场景和可观察断言。

| 类别 | 必须排除的失败方式 |
|---|---|
| 接线 | Engine Logger 未注入；Diagnostics 只订阅 GUI 事件；初始化晚于 Engine.New 而丢失创建日志 |
| 统计 | 幂等重试重复计数；空轮询刷计数；send 推导 receive/apply；过滤或去重计成失败；阶段耗时相加产生伪端到端延迟 |
| 应用 | 一张票据多次上报重复计数；失效或停止时漏掉未报告的票据；将票据生命周期误称为纯系统写入耗时 |
| 等级 | 本机门槛提前过滤远程记录；派生 Logger 保留旧门槛；debug 隐式开启上传 |
| 覆盖 | 非法输入改变状态；旧 timer 清除新覆盖；reset 恢复旧配置；到期依赖墙上时间；挂起后仍用过期覆盖 |
| 配置 | 坏配置清空有效状态；端点热替换；先关闭再开启偷偷应用待重启端点；OTEL 环境变量绕过宿主 |
| 停用 | Shutdown 又 flush 旧记录；HTTP 重试越过停用屏障；旧 worker 污染新 generation；metrics 重新开启带回旧累计值 |
| 累计流 | 将 cumulative 数据逐批相加；导出失败清零；进程重启复用旧起始时间；多个无身份 Resource 流错误合并 |
| 队列 | SDK 和自建队列叠加超预算；队列满等待；字段白名单前保留大对象；丢弃计数漏计或重试重复计数 |
| 存储 | 写盘阻塞业务；轮转超限；回收失败继续写入；符号链接删除私有目录外文件；磁盘失败递归日志 |
| 网络 | DNS/TLS/401/429/5xx 阻止业务；超时不含重试预算；重定向泄露头部；导出错误回送自身 |
| 隐私 | 原始错误、派生属性、LogValuer、Resource detector 或 SDK 环境变量泄露测试秘密 |
| 资源 | 反复启停泄漏 goroutine、timer、连接或句柄；flush 预算按信号叠加；关闭等待无上限 |
| 证据 | 二进制与源码不对应；测试只检查内存 exporter；工件没有实际断言、缺重放命令或只保存成功摘要 |

## 3. 分阶段实施

每个阶段先写真实进程场景及断言，再增加实现。每阶段保留上一阶段可工作的路径。
不在编写实现后补写单元测试。不创建预防性框架、配置层或独立插件系统。

### P0：冻结规格与验收入口

1. 按契约冻结事件、指标、统计边界、DTO、开关和停用语义。
2. 核对现有 E2E 目录剪贴板、工件保存和二进制选择入口。
3. 编写 OTLP 本机接收端的进程控制和解码断言；通过真实 HTTP 接收 Protobuf。
4. 在测试装配中提供受限 HTTPS loopback、受控失败输出和固定诊断夹具。发行配置不暴露测试选项。
5. 查询固定版本官方 SDK 文档与源代码。验证队列、重试、超时、停用、基数和环境变量控制能力。
6. 检查项目现有依赖。优先复用现有库；日志轮转若需新依赖，先核对维护状态和边界。
7. 将选定版本、扩展点和限额证据登记到本计划，随后固定 go.mod/go.sum。不得引用 SDK 私有字段。

退出条件：场景与失败断言已存在；接收端能保存解码请求；SDK 适配方案有可核对依据。
此阶段不要求尚未实现的产品场景通过。记录预期失败和实际失败原因。

### P1：本机日志与动态控制最小闭环

1. 在创建 Engine 前创建宿主控制器，注入 Logger。
2. 接通固定事件名、有界本机输出、私有文件轮转和累计丢弃状态。
3. 使用现有配置 Manager 接入 logging 与 telemetry 结构、严格校验和默认值。
4. 增加等级查询、设置、reset、本机 CLI 及 diagnostics 失效事件。
5. 实现两个独立等级门槛与共享派生 Logger 状态；远程分支此时保持关闭。
6. 通过真实 daemon 验证配置热加载、短时覆盖、覆盖替换和重启清除。

退出条件：本机日志文件可解析；控制 API 结果一致；轮转和拒绝输入符合契约；不产生遥测网络请求。

### P2：Metrics 最小端到端闭环

1. 在 Engine 增加契约规定的 Diagnostics 和固定字段 DTO。
2. 先接通唯一一次本机 commit。通过真实 CLI 提交并在 OTLP 接收端断言 counter 与 histogram。
3. 接通双节点 send_queue、send、receive 和系统 apply，再接通 observe/process。
4. 将失败、取消、被替代、去重和策略过滤映射为固定结果。
5. 实现有限标签、固定 Histogram、cumulative 起始时间及本机丢弃状态。
6. 实现总开关、metrics 开关、端点待重启状态和 metrics 停用屏障。

退出条件：两节点真实业务成功；独立接收端分别证明本机各阶段统计；无目标或同步关闭仍统计 commit succeeded。
幂等调用不增加提交计数。关闭期间零请求，重开没有旧聚合数据。

### P3：远程 Logs 闭环

1. 接入官方日志 SDK、OTLP/HTTP exporter 和受审核的 slog 桥接。
2. 在桥接前筛选固定事件与字段。验证 WithAttrs/WithGroup 无绕过路径。
3. 接入远程日志独立门槛、批处理及可核对的丢弃计数。
4. 实现远程 logs 停用屏障，隔离 exporter 自身错误。
5. 通过真实配置切换验证本机与远程门槛交叉、覆盖和上传开关。

退出条件：真实 OTLP/HTTP 请求包含允许的日志；测试秘密扫描通过；远程等级覆盖不能开启上传；停用不 flush 旧批次。

### P4：故障、资源与恢复验收

1. 执行队列满、慢写盘、磁盘错误、拒绝连接、DNS/TLS、401、429、5xx 和慢响应场景。
2. 使用真实同步操作证明遥测失败期间仍可提交、接收和应用。
3. 在相同负载下测量关闭、正常上传、debug 和队列满四种模式。
4. 完成反复启停、端点修改、关闭与正常退出竞态场景。
5. 运行项目现有必要回归、race、lint、桌面构建及文档检查。
6. 更新架构现状与 verified，仅标记已完成且有证据的内容。

退出条件：下表本次必需场景通过且有重放工件。任何未通过场景明确阻止对应能力的完成声明。
真实休眠、跨平台原生和生产接收端验收单独登记，不由模拟推定。

## 4. 端到端验收矩阵

下列名称是计划新增场景，不表示当前 TestCLI 已有这些用例。
沿用 `testdata/*.txtar`，通过现有生成入口更新 CASES.md，不手改生成目录。
测试只操作隔离 UNIC_HOME 与目录剪贴板，不触碰用户剪贴板。

| 场景 | 核心断言 | 工件 |
|---|---|---|
| `observability-default-off` | 默认没有遥测请求；本机日志有限且结构化；OTEL 环境变量不能开启上传 | 接收端零请求窗口、配置和日志 |
| `observability-log-level` | 既有/派生 Logger 立即生效；四个等级映射明确；local/remote 门槛交叉不误过滤 | API 快照、两分支事件集合 |
| `observability-level-override` | 默认 15m、短时到期、替换、reset、配置热更新和重启清除；远程覆盖不启用上传 | 控制时间线、查询快照和请求窗口 |
| `observability-invalid-input` | 非法等级/目标/时长、重复 JSON 键、未知字段、超大正文均拒绝；快照不变 | 请求夹具、HTTP 状态和前后快照 |
| `observability-metrics-pipeline` | commit/send_queue/send/receive/apply 分别计数；空轮询不计；幂等等待不重复；耗时非负且桶正确 | 两节点业务结果及分离接收端解码数据 |
| `observability-metrics-outcomes` | 同步关闭仍 commit succeeded；去重、过滤、失败、取消和被替代语义正确；重复应用报告不重复计数 | 确定性夹具、阶段结果集合 |
| `observability-metrics-cumulative` | 两次导出累计值不下降；失败不清零；重开和重启更新起始时间；不回放关闭期业务 | OTLP 起始/结束时间与累计差值 |
| `observability-remote-logs` | 总开关及 logs 开关同时开启才上传；Body 与属性白名单；两信号互不代开 | Protobuf 解码记录与开关时间线 |
| `observability-endpoint-restart` | 修改端点报告待重启；停用再启用仍用旧端点；首次配置端点须重启；凭据不回显 | 两接收端请求时间线、状态快照 |
| `observability-disable-barrier` | 在途请求取消；屏障后零新请求；重新开启不补发旧批次；metrics 聚合重置 | 请求发起/结束时间、屏障快照、OTLP 内容 |
| `observability-export-failures` | DNS/TLS/401/429/5xx/超时只使诊断降级；同步仍成功；错误不递归上传 | 故障脚本、请求时间线、业务结果 |
| `observability-queue-pressure` | 有界队列拒绝不阻塞；丢弃数准确；大字段先筛选；单条超限被拒绝；内部与外部队列合计合规 | 负载种子、丢弃对账、内存采样 |
| `observability-local-retention` | 写入前轮转；总量不超过 20 MiB；过期清理；失败不突破限额；不越过符号链接 | 文件清单、大小、时间及受保护夹具哈希 |
| `observability-privacy` | 四个等级、两种覆盖、失败路径、属性/Resource/原始错误及 API 全部不含测试秘密 | 分类扫描报告及解码数据 |
| `observability-lifecycle` | 反复启停 100 次无持续资源增长；独立退出预算不超过 2 秒；业务关闭不等待遥测 | 进程/句柄采样、退出时间和错误日志 |
| `observability-resume` | SIGSTOP 超过覆盖期限后 SIGCONT；下一条记录恢复配置等级 | 信号时间线、日志和查询结果 |

无法从公开 CLI 稳定触发的 superseded、重复票据报告、慢写盘及队列饱和，由专用测试宿主装配真实 Engine 与同一适配器，再通过其本机入口触发。
测试宿主仍是独立进程，并使用真实 OTLP/HTTP。不得在发行 CLI 增加故障注入参数。
固定日志夹具只使用契约允许的事件和属性。畸形诊断夹具必须经过生产白名单路径。
TTL 使用短时真实覆盖做主验收。假时钟仅可作为已说明失败方式的补充证据。
SIGSTOP/SIGCONT 只证明进程挂起恢复，不证明真实系统休眠。

零请求断言必须包括接收端请求开始记录，并覆盖至少一次正常导出周期及导出超时窗口。
生产 60 秒周期至少验收一次；其他场景可通过显式测试装配缩短周期，工件必须记录参数。
不得只观察 Collector 成功响应，因为主动取消的请求也可能已到达接收端。

## 5. 性能与资源记录

以同一源码和二进制、同一硬件、同一确定性业务负载比较四种模式。
每种模式至少执行三次；记录预热、样本数和原始数据，不只保存平均值。

| 模式 | 配置与负载 | 必须记录 |
|---|---|---|
| 关闭 | 总开关 false，本机 info | 基线资源和业务延迟 |
| 正常 | metrics + warn 远程 logs | SDK/队列增量、正常上传量 |
| debug | 两输出 debug，保持相同业务负载；另用固定诊断夹具压测日志 | 格式化成本、过滤与上传量 |
| 队列满 | 慢接收端/慢输出与受控高负载 | 丢弃数、峰值内存、业务延迟、恢复后资源 |

至少记录空闲 CPU、RSS、唤醒次数、goroutine/句柄、commit 与同步延迟 p50/p95/p99、网络请求数和字节数。
未取得的系统指标写“未采集”，不能以其他指标代替。
硬约束是契约中的队列、文件限额、超时、零越界请求和停用后无旧数据。
CPU、RSS 和业务延迟目前没有测得基线，不在文档中虚构百分比保证。
出现持续资源增长或与遥测 I/O 相同长度的业务停顿时，必须阻止完成声明并定位。
完成测量后将原始结果及建议预算写入本节；默认值变更必须附对应测量结果。

## 6. 验收工件

使用现有 `UNIC_E2E_ARTIFACTS` 入口，将工件写到用户数据目录之外。
每次运行至少包含以下内容：

```text
<run>/
  manifest.json
  summary.json
  commands.txt
  replay.sh
  <scenario>/
    assertions.json
    timeline.jsonl
    diagnostics.jsonl
    otlp-decoded.jsonl
    local-logs.jsonl
    resource-samples.csv
    fixtures.json
```

manifest 记录工作目录、分支/HEAD、脏文件状态、实际参与构建的源码文件摘要、Go 版本、平台、构建参数、二进制 SHA-256 和测试参数。
脏工作区不能只记 HEAD；测试结束后重新核对参与构建的文件摘要。
replay.sh 使用相同确定性夹具和已说明的构建入口。声明所需平台、开发 TLS 条件和命令。
summary/assertions 按场景记录 passed/failed/not_run、具体断言、期望值、实际值和退出码。
失败场景也保存已得到的工件。未执行的场景不得写为通过。

仅保存合成夹具。测试秘密可由固定种子重建，扫描报告只保存秘密类别与命中位置，不复制秘密值。
原始 HTTP 头、凭据、真实剪贴板、配对材料与地址不得进入工件。
解码 OTLP 必须在保存前扫描；出现合成测试秘密时保留安全的失败报告，清理不安全工件，并允许按相同种子复现。
需要核对请求体时使用脱敏后的规范化数据及原始字节摘要。摘要不代替内容断言。

## 7. 回归与文档收口

以下命令是后续实现的目标检查，本次文档交付不声称已执行这些功能检查：

```sh
go test -race ./...
go build -o /private/tmp/unic-otlp-bin/unic ./app/desktop/cmd/unic
go build -o /private/tmp/unic-otlp-bin/unicd ./app/desktop/cmd/unicd
UNIC_E2E=1 UNIC_E2E_BIN=/private/tmp/unic-otlp-bin UNIC_E2E_ARTIFACTS=/private/tmp/unic-otlp-evidence go test -race ./e2e/cli -run 'TestObservability' -count=1 -v
go run ./tools/docscheck
git diff --check
```

构建前创建隔离输出目录。若要验证子进程 race，两个二进制也须加 `-race`，并保存最终采用的准确命令。
Lint 使用执行时仓库 CI 的命令，不引入另一套标准。
复现基线已有失败时保存对应源码证据，分别报告本次新增失败与基线失败。不能把整个回归报告写为通过。

行为落地后补充 `docs/architecture/observability.md`，并更新桌面运行时页面与架构索引。
只有对应代码已实现并核对后，才执行 `go run ./tools/docscheck -bump <页>`。
0014 的 traces 目标仍未实施，因此本次不得将整个 0014 标为完成或归档。

## 8. 实施台账

- [x] 中文执行计划与目标契约已编写。
- [x] P0：场景、接收端、固定 SDK 与扩展点证据。
- [x] P1：本机日志、配置、等级 API/CLI 与覆盖。
- [x] P2：业务 metrics 及双节点最小闭环。
- [x] P3：远程 logs 与分支筛选。
- [ ] P4：故障、隐私、资源、生命周期及回归。
- [x] 真实 OTLP 验收工件已生成；重放入口与构建命令已核对。
- [x] 架构现状与 verified 已按代码更新。

功能实现已接入。原生界面和生产部署不在本次范围。P4 中未运行的项目保持未勾选，不能用已通过的其他场景代替。

## 9. 工作区实现登记

实现已提交，提交身份见文末收据。原有界面改动保持未提交。

- Engine 提供公开 Diagnostics 固定字段回调。各业务阶段在执行点记录，回调不持有 Engine 锁；回调 panic 不改变业务结果。
- 宿主在 Engine.New 前创建日志与遥测控制器。配置解析、模板、端点待重启标记、本机诊断 API 和 CLI 已接通。
- 本机日志使用有界队列、私有 JSON Lines 文件、轮转和保留清理。日志目录拒绝符号链接。
- 动态门槛和每目标限时覆盖共享控制器。派生 Logger 不缓存旧门槛，覆盖不写配置文件。
- 官方 SDK/exporter 版本固定：metrics、log SDK 与基础 SDK 为 v1.47.0，logs HTTP exporter 为 v0.23.0，otelslog 为 v0.21.0，OTLP Protobuf 为 v1.11.0。
- Metrics 使用 ManualReader 和单一宿主 worker。Logs 使用 SDK Processor 的公开扩展点，拥有唯一有界队列和批处理，直接调用官方 HTTP exporter。没有叠加 SDK BatchProcessor 队列。
- 两个信号各有可取消代际。停用先切断接纳，再取消、丢弃和等待 worker/SDK 清理。正常退出共享 2 秒 flush 预算。
- SDK 的 WithResource 会合并环境属性。Metrics 在 Export 前替换 Resource；Logs 在 HTTP 传输边界通过标准 OTLP Protobuf 替换 Resource。远程只保留四个审核字段。
- HTTP 状态码在传输边界采集。401/403、429 和其他失败映射为有限错误分类，不依赖 SDK 429 错误文本。

SDK 核对依据为上述固定版本的 `sdk/log/processor.go`、`sdk/log/provider.go`、`sdk/metric/config.go`、`sdk/metric/manual_reader.go` 与两个 HTTP exporter 的 `config.go`。实现只使用公开 API，不读取 SDK 私有字段。

验收入口调整：复杂 HTTP/Protobuf 断言直接使用同一 `e2e/cli` 包中的 `TestObservability*`，不伪造 txtar 场景。原 CASES.md 仍只登记现有 txtar 用例。
`unic_e2e` 构建标签提供无界面 unicd 入口，复用相同 host/config/API。诊断场景通过已有 Transport seam 使用本机 TCP，完整执行真实 session、握手、wire、Engine 与目录剪贴板；不依赖公共 DERP。
发行构建不包含这些夹具、短导出周期或测试路由。

## 10. 已确认的回归限制

`go test -race ./...` 的工作区首轮记录有 11 个失败。已在 HEAD `29286ce50f04d69a087b3e4d6140da8326849dfa` 的独立源码归档中逐项复现相同失败：

- Engine：`TestBackendsGetIndependentViewsAndThresholds`、`TestBackendPoliciesAreEffectiveValidatedAndHotUpdated`、`TestHistorySyncSummaryFollowsTheOperation`。
- Engine：`TestUserLimitsApplyToImport`、`TestNewRemovesOnlyItsOwnLeftovers`。
- Engine：`TestReceiveBetweenEngines`、`TestReceiverHonoursItsOwnSettings`、`TestReceivedFilesAreRemovedWithTheirContent`、`TestCloseAndStartupClearTheReceiveDirectory`。
- 宿主：`TestFileSync`。
- 迁移工具：`TestRustFixtureMigrationResumeAndSourceIndependence`。

这些失败不属于本次新增回归，但全量检查仍未通过。不得报告为全部测试通过。
固定版本 lint 还报告六条已有工作区问题，涉及 `observers_test.go`、`cleanup_gui.go`、`history_client.go`、`cli.go`、`gui.go` 和 `history_cleanup.go`。本次不修改这些无关逻辑。

## 11. 最终端到端收据

最终验收使用独立冻结源码快照，避免并行界面工作改变构建来源。

- 快照：`/private/tmp/unic-otlp-verified-source`，基线 HEAD 为 `29286ce50f04d69a087b3e4d6140da8326849dfa`，含本次未提交实现。
- 二进制：`/private/tmp/unic-otlp-verified-bin`；unic 与 unicd 均启用 race，unicd 另带 `unic_e2e` 标签。
- 工件：`/private/tmp/unic-otlp-verified-evidence`。
- 汇总：该目录的 `summary.json`；各 profile 保存 manifest、二进制摘要、OTLP 解码数据、断言、本机日志、资源采样和 replay.sh。
- 结果：5 个顶层 E2E、35 个叶场景全部通过，耗时 226.602 秒，退出码 0。
- 22 份 profile 工件全部通过。每份 manifest 的 `source_unchanged` 均为 true。
- 已逐项核对本次任务源码：当前工作区的实现文件与测试快照摘要一致。并行界面源码不据此声明原生验收通过。
- 默认周期验收包含 64 秒关闭观察窗口及正常 60 秒 metrics 周期。其余场景显式使用 500 ms metrics / 100 ms logs 周期。
- 100 次信号启停前后 goroutine 从 18 降至 16；堆采样保存在 observability/resource-samples.json。这是有界场景证据，不是所有负载下的零泄漏证明。
- 401、429、503、HTTP 超时、TLS 证书失败和拒绝连接场景通过；诊断故障期间提交仍成功，停用屏障后未发起新请求。
- 发行配置的正常 unicd 构建通过；没有启动 mygo.App、真实系统剪贴板或 GUI。

最终准确验收命令：

```sh
go build -race -tags unic_e2e -o /private/tmp/unic-otlp-verified-bin/unicd ./app/desktop/cmd/unicd
go build -race -o /private/tmp/unic-otlp-verified-bin/unic ./app/desktop/cmd/unic
TMPDIR=/private/tmp UNIC_E2E=1 UNIC_E2E_BIN=/private/tmp/unic-otlp-verified-bin UNIC_E2E_ARTIFACTS=/private/tmp/unic-otlp-verified-evidence go test -race ./e2e/cli -run '^TestObservability' -count=1 -v
```

每种模式运行三次，每次执行 50 次真实进程内 Engine 提交并经本机 HTTP 观察结果。下表为三次运行分位值的中位数，不把 HTTP 往返耗时称为纯 Engine 执行耗时。

| 模式 | 样本数 | p50（毫秒） | p95（毫秒） | p99（毫秒） |
|---|---|---|---|---|
| 关闭 | 150 | 5.801 | 8.061 | 9.117 |
| 正常上传 | 150 | 5.597 | 7.122 | 8.277 |
| debug | 150 | 5.734 | 7.268 | 11.409 |
| 队列压力 | 150 | 5.826 | 8.098 | 10.193 |

CPU 与 RSS 使用宿主 ps 采样，保留原始字段和采样时间。%CPU 是 ps 的进程统计值，不宣称为精确的稳定空闲占用。
未采集完整唤醒次数、文件句柄序列和同步延迟分位值。未据此设定新的性能百分比保证。

最终全量 race 回归仍未通过：除前述 11 项已复现的基线失败外，还出现一次 `TestSandboxBudgets` 时序失败，期望指令预算先耗尽，但实际先到超时。
该目录源码与 HEAD 完全相同。基线和最终快照分别隔离复跑五次均通过；只记录时序波动，不宣称已证明全部因果。
最终 lint 仍有前述六条已有工作区问题；新增代码与测试标签入口未报告 lint 问题。go mod verify 与 git diff --check 通过。

`go run ./tools/docscheck` 通过，保留五条过期警告。尝试 bump 被工具拒绝，因为源码尚未提交。没有为更新 verified 擅自提交；新架构页明确保留基线 verified 的含义。

未运行：真实 DNS 故障、强制文件系统 ENOSPC/unlink 故障、完整应用并发与停用竞态排列、真实系统休眠、原生剪贴板/UI、Windows/Linux 原生和生产接收端/认证部署。
这些条目使 P4 保持部分完成。Traces 仍未实现，0014 不归档。


## 12. 范围限定提交收据

用户授权提交后，代码与前置设计、契约、执行计划一并提交为 `0710594f0e20d22f9e2ff06bece61631eff6172d`。
提交标题为 `feat(observability): add bounded OTLP logs and metrics`。

- 提交只包含本次 44 个文件。`main.go` 只提交两个诊断帮助条目；GUI demo 导入、命令分支、UI Kit、布局和其他未相关改动保持未提交。
- 已从暂存区创建独立源码快照，排除未提交界面代码。快照构建 race 版本 unic、无界面 unicd，以及正常发行入口 unicd 均通过。
- 独立快照的 15 个控制场景和双节点流水线场景通过。工件位于 `/private/tmp/unic-otlp-commit-evidence`，汇总为该目录的 `summary.json`。
- 提交树为 `ccdcd7bea112d629c0976908e689c07e1e16583d`，与受测暂存区快照相同。
- 前一节 35 个场景的完整结果对应冻结工作区；本次独立快照验证补充证明了排除其他界面改动后的提交来源。两类证据分别保留。
- observability、pipeline、desktop-runtime 三个架构页的 verified 已更新为上述代码提交。

未推送，未部署。原生界面、生产接收端和 P4 未执行项的边界保持不变。

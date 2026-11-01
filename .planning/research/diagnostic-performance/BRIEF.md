# 诊断包中的 CPU 与内存数据

日期：2026-11-01。
状态：调研完成。方案待实施。
范围：桌面端。移动端不在本轮实施范围内。

## 建议

本端应统一使用 OpenTelemetry 数据模型。
诊断包应使用 OTLP JSONL 作为指标、日志和追踪的交换格式。
开发者应通过固定版本的 OpenTelemetry Collector 导入诊断平台。
这项选择替代本文初稿中自定义 `resources/samples.jsonl` 的交换格式。
下文的自定义字段用于说明采样语义，实施时应映射到 OpenTelemetry。

应在后台定期采集 CPU 与内存数据。用户导出诊断包时，应附带采样历史和导出前快照。
只在导出时采集一次，无法说明问题发生时的资源占用。

第一阶段应覆盖 daemon 进程，并打通在线导出与离线导出。
第二阶段应补齐 GUI、Quick Panel 和可确认归属的 WebView 进程。
第一阶段的诊断包应明确说明进程覆盖范围。

采样机制应与远程遥测分离。用户导出诊断包不应触发自动上传。
本方案中的频率、容量和开销目标均为建议值，尚未通过性能验收。

## 已验证事实

本轮核对的 Desktop 源码为 `81a11286d281a54625516a7d9dbf8c48b90fed09`。
Desktop 固定的 Engine 源码为 `d532978d8bc9740a64460ee08b6e97a85fe4b590`。
Engine 结论来自该版本的本机 Cargo 检出源码，不代表上游最新版本。

| 入口 | 当前行为 | 源码位置 |
| --- | --- | --- |
| 前端在线导出 | 调用 `/diagnostics/log-export` | `apps/gui-go/frontend/src/api/daemon/diagnostics.ts` |
| daemon 导出准备 | 请求 Engine 刷新业务日志和网络日志 | `apps/daemon/src/daemon/diagnostics.rs` 中的 `prepare_export` |
| 在线 ZIP 组装 | 写入日志、网络日志和清单 | `crates/uc-observability/src/startup_logs.rs` |
| GUI 离线导出 | 调用 Go 的 `diaglogs.ExportStartup` | `apps/gui-go/host_commands_files.go` 中的 `ExportStartupLogs` |
| 离线 ZIP 组装 | 收集已保留文件，不请求实时 Engine 快照 | `packages/desktop-host-go/diaglogs/diaglogs.go` |
| 现有 CPU 采样 | 性能工具用 `sysinfo` 采样指定 daemon PID | `crates/p2p-bench/src/bin/mobile_healthz_process_bench.rs` 中的 `sample_daemon_cpu` |

Engine 的 `LocalDiagnosticExportReport` 包含刷新结果、运行状态、文件计数和模块日志计数。
该报告没有 CPU 或内存时间线。
`prepare_local_diagnostic_export` 明确返回 `other_processes_flushed: false`。
不能将 daemon 刷新解释为 GUI、Quick Panel 或 WebView 已刷新。

Engine 证据位置：

- `crates/uc-observability-runtime/src/local_capture.rs` 中的 `LocalDiagnosticExportReport`。
- `crates/uc-observability-runtime/src/runtime.rs` 中的 `ProcessObservabilityHandle::prepare_local_diagnostic_export`。

仓库的 `Cargo.lock` 已包含 `sysinfo 0.38.4`。
`crates/p2p-bench/Cargo.toml` 直接依赖 `sysinfo 0.38`。
这不表示产品 daemon 已具备资源采集能力。

## 成熟产品与库的选择

VS Code 先按进程定位资源占用，再收集对应进程的性能剖析文件。
可复用这个诊断顺序：先确认进程和时间窗口，再决定是否需要性能剖析。
参见 [VS Code 性能问题指南](https://github.com/microsoft/vscode/wiki/Performance-Issues)。

Rust 侧宜优先复用 `sysinfo`，并限制为已确认身份的进程。
应使用指定字段的刷新接口，避免每次采集全机进程、命令行或环境变量。
本轮已核对本机 `sysinfo 0.38.4` 的 `Process` 源码。
实施时应以固定版本源码为准，避免照搬新版 API。

不宜启动独立监控服务或引入 Prometheus 数据库。
当前需求只需要有界时间线和诊断包。
不宜用 `ps`、`top` 输出作为产品文件格式。
命令输出的格式、权限和字段随平台变化。
第一阶段不需要为 Go 宿主引入第二套采集库。

## 数据口径

| 数据 | 建议字段 | 口径 |
| --- | --- | --- |
| 进程身份 | `role`、`pid`、`process_start`、`run_id` | 区分进程重启和 PID 复用 |
| 时间 | `seq`、`at_utc`、`elapsed_ms`、`interval_ms` | 用单调时钟计算实际采样间隔 |
| CPU | `cpu_time_ms`、`cpu_percent_one_core` | 一个逻辑核心满载为 100%，允许超过 100% |
| 常驻内存 | `resident_bytes` | Linux/macOS 为 RSS，Windows 为 working set |
| 覆盖与质量 | `state`、`reason`、`gap_ms` | 区分有效值、缺失和历史数据 |
| 导出关联 | `export_id`、`capture_id` | 关联同次导出及已有详细采集窗口 |

CPU 百分比应使用同一进程的两次累计 CPU 时间差计算：

```text
cpu_percent_one_core = 100 × delta_cpu_time_ms / delta_elapsed_ms
```

第一次 CPU 采样应标记为 `warming_up`。
进程重启后应重新建立基线。
系统休眠后应记录缺口，并重新建立 CPU 基线。
采样器不应补发一批没有实际观测的数据。
参见 [sysinfo 的 CPU 采样说明](https://docs.rs/sysinfo/latest/sysinfo/struct.Process.html#method.cpu_usage)。

不同平台的内存指标应保留独立字段：

| 平台 | 第一阶段 | 后续可补充 | 限制 |
| --- | --- | --- | --- |
| macOS | RSS | `phys_footprint`、压缩内存 | RSS 不等于活动监视器的所有内存口径 |
| Windows | working set | `PrivateUsage`，即 private commit | private commit 不等于物理内存 |
| Linux | RSS | `smaps_rollup` 的 PSS、`Private_Dirty` | 额外采样应控制频率并报告权限失败 |

内存字节数应保留原值，不应只记录百分比。
进程 RSS 相加会重复计算共享页。
诊断包不应将 RSS 总和命名为应用的独占物理内存。
单点 RSS 或持续高 RSS 均不能独立证明 Engine 内存泄漏。

## 采样与所有权

宜采用下列初始参数：

- 普通采样间隔为 5 秒。
- 详细采集间隔为 1 秒，并复用现有详细采集窗口。
- 历史保留目标为最近 30 分钟。
- 全部进程的历史文件总上限为 8 MiB。
- 单条记录上限为 2 KiB。

30 分钟的 5 秒采样包含每进程 360 条记录。
配额不足时应先移除最旧记录，并记录移除计数。
采样器应在独立阻塞任务中调用系统 API。
采样器应使用有界队列。
采样器不应阻塞 GUI 线程、网络循环或剪贴板捕获。

Engine 的 observability runtime 宜拥有 daemon 自身的采样、状态和有界历史接口。
Engine 内部变更应在 `UniClipboard/Engine` 实施。
Desktop daemon 应拥有启动、停止和 ZIP 组装。
Desktop 不应复制 Engine 内部包。

GUI 和 Quick Panel 的进程注册应由各自宿主负责。
注册应绑定当前资料、运行标识、PID 和进程启动标识。
宿主可在内存中核验可执行文件路径，但不应将原始路径写入资源记录。
采样器不应仅凭进程名或 PPID 判定归属。
macOS 的 WebView 辅助进程可能没有可直接使用的父子关系。
无法确认归属时，应标记 `unknown` 并排除在已确认进程合计之外。

## 持久化与隐私

`VISION.md` 规定新增持久化数据默认加密。
资源历史应在持久化前使用当前资料的 MasterKey AEAD 加密。
不应为资源历史另建普通文件密钥。
资料锁定或密钥不可用时，只保留有界内存数据，并报告持久化不可用。

用户主动导出时，在线路径可将已解锁历史转换成白名单 JSONL。
密文历史只能使用已有合法密钥生命周期读取。
离线导出不应为了解密而启动 daemon 或绕过资料锁定。
离线路径可携带版本化密文资源文件，并标明 `encrypted_unavailable`。
诊断方无法读取该密文时，清单应明确报告资源历史不可读。
如果产品要求“daemon 不可用时仍导出可读历史”，应先确定密钥接入方案。
明文历史方案需要按仓库规则单独论证，不能默认采用。

资源数据只应包含白名单数值和有限枚举。
不应包含剪贴板内容、设备名、文件路径、窗口标题、命令行、环境变量或凭据。
第一阶段不应自动附带堆转储或 CPU 调用栈剖析文件。

## 诊断包契约

下列文件名为建议契约，不是当前实现：

```text
manifest.json
resources/summary.json
resources/samples.jsonl
resources/history.enc
```

`summary.json` 应包含单位、CPU 口径、实际时间范围和进程覆盖范围。
每个来源应报告 `complete`、`partial`、`missing`、`unavailable` 或 `unknown`。
每个指标应另报缺失原因，例如 `unsupported`、`permission_denied` 或 `warming_up`。
历史记录应标记 `stale` 和最后采样时间。
不可用值应使用 `null` 和原因，不应使用零替代。
清单应记录文件大小、SHA-256、保留上限和裁剪计数。

在线导出应按顺序执行：

1. 生成本次导出的关联标识。
2. 在 ZIP 压缩前请求资源快照。
3. 在固定期限内刷新资源历史。
4. 冻结本次导出的文件清单与读取长度。
5. 组装日志与资源文件。
6. 写入覆盖范围和失败原因。

导出前快照可以复用最近 CPU 区间，不应把两次紧邻刷新当作有效 CPU 百分比。
压缩后的资源占用不能替代导出前快照。
某个资源来源失败时，其余日志仍应导出。
离线导出只应使用已保留历史，并明确说明没有实时刷新。
只有资源文件而没有普通日志时，两套归档实现也应允许生成诊断包。
现有 `ErrNoLogs` 判断需要随资源文件契约更新。

## 实施顺序

1. 在 Engine 增加 daemon 自身资源采样与状态接口。
2. 接入有界内存历史和加密持久化。
3. 扩展 Desktop 在线 ZIP 与清单。
4. 扩展 Go 离线 ZIP 与清单。
5. 打通 daemon 的最小端到端验收。
6. 补齐 GUI、Quick Panel 与可归属的 WebView。
7. 按实际诊断需求增加缓存、队列和连接池聚合计数。

## 端到端验收计划

实施前先运行 `bun run e2e:gui list`。
复用现有诊断包验收入口，不另建独立测试框架。
macOS 入口为 `apps/gui-go/e2e/diagnostics_acceptance_run.py`。
Windows 入口为 `apps/gui-go/e2e/windows_diagnostics_acceptance.ps1`。

| 场景 | 验收条件 |
| --- | --- |
| 普通导出 | ZIP 同时包含日志、资源时间线、导出前快照和单位说明 |
| CPU 负载 | 已确认进程的区间 CPU 随受控负载上升，首样本不伪装为零 |
| 内存负载 | 已触碰的受控内存分配能反映在常驻内存时间线中 |
| 释放阶段 | 保留负载前、负载中、释放后的记录，不强制要求 RSS 立即回落 |
| 进程重启 | 同 PID 或新 PID 的新运行不沿用旧 CPU 基线 |
| 关闭界面 | daemon 继续采集，已退出 GUI 不继续显示有效样本 |
| 离线导出 | 标记历史时间、密文可读性和未实时刷新 |
| 休眠与时钟变化 | 记录缺口，CPU 分母不使用 UTC 差值 |
| 配额与失败 | 保留上限生效，权限失败和刷新超时不阻断其余日志 |
| 隐私 | 白名单校验通过，文件不含合成敏感标记 |

验收工件应包含 ZIP、解析后的资源记录、源码 SHA、执行命令、结果和 SHA-256。
开销验收应比较相同构建、负载和时长下的启用组与禁用组。
初始目标为平均额外 CPU 小于一个逻辑核心的 0.5%，额外 RSS 小于 10 MiB。
该目标不是已验证结果，也不是对所有平台的性能承诺。

本轮没有运行产品、原生 GUI、跨平台或性能验收。
本轮没有编写产品代码或单元测试。

## OTLP 离线导出方案

### 已验证基础

当前固定版本的 Engine 已依赖 OpenTelemetry Rust SDK。
`uc-observability-runtime/src/telemetry.rs` 已构建日志和追踪的 OTLP/HTTP 二进制导出器。
`uc-observability-runtime/Cargo.toml` 尚未启用 SDK 的 `metrics` 功能。
现有在线导出器不能直接视为离线资源历史功能。

OTLP 是交换协议，不是诊断 ZIP 的文件格式。
OpenTelemetry 提供 OTLP JSON 文件序列化规范。
该规范仍处于 Development 状态。
Collector 的 `otlp_json_file` 接收器可读取 OTLP JSON 文件。
该接收器的日志、指标和追踪能力目前标为 Alpha。
应固定并验证 Collector 版本，不应承诺任意版本兼容。

### 数据流与文件

```text
CPU / memory sampler + logs + spans
  -> OpenTelemetry records
  -> bounded encrypted local history
  -> user-requested export and decryption
  -> OTLP JSONL files in diagnostic ZIP
  -> developer-side Collector
  -> diagnostic backend and visualization
```

应使用以下结构替代前文中的资源文件结构：

```text
manifest.json
otel/metrics.jsonl
otel/logs.jsonl
otel/traces.jsonl
import/collector.yaml
import/README.md
```

每行应包含一种信号的完整 OTLP 导出请求。
序列化应遵循 OTLP JSON 规则，不能只给现有 JSON 日志更改扩展名。
应使用成熟序列化实现，并验证 trace ID、纳秒时间戳和整数编码。
未采集的信号应在清单中标为缺失，不应补造记录。
普通模式可限制追踪量，详细模式可扩大追踪窗口。

`import/collector.yaml` 应按信号配置文件接收和 OTLP 转发。
导入配置应从文件开头读取。
导入器应在全部文件发送成功后结束，避免重复导入。
开发者端应先解压 ZIP，再执行导入。
开发者端可以使用 Collector，不要求用户安装 Collector。
导入目标地址和凭据应由开发者提供，不应写入用户诊断包。

### 指标与关联

宜优先采用标准进程指标，例如 `process.cpu.time` 与 `process.memory.usage`。
CPU 时间单位应为秒，内存单位应为字节。
标准 `process.cpu.utilization` 按可用 CPU 数归一化，不能直接填入单核心百分比。
如果底层库无法提供标准指标要求的 CPU 模式，应补齐系统采样能力或使用明确的应用自定义指标。
不应将总 CPU 时间伪装成 `user` 或 `system` 时间。
进程指标语义约定尚未全部稳定，应记录采用的约定版本。

每个进程应具有独立的 Resource 身份。
Resource 应记录产品版本、平台、进程角色和运行标识。
记录不应包含真实设备身份、主机名、用户名、命令行或环境变量。
日志和追踪应保留真实的 `trace_id`、`span_id` 关联。
跨 Go、Rust 或 WebView 的调用应传播追踪上下文，不能仅靠时间接近建立父子关系。
CPU 与内存指标可先按进程和时间窗口关联，不应为每个样本创建追踪。

### 历史导入限制

诊断平台需要接受原始时间戳范围内的历史记录。
平台可能因保留期、乱序或过旧样本策略拒绝导入。
导入器应报告接收、拒绝和重复记录数。
原始文件应保留原时间戳。
如果提供时间平移模式，应标记平移量并保留原时间。
这项能力不能作为默认导入行为。

诊断 ZIP 仍应保留启动失败和覆盖范围清单。
没有完成迁移的现有日志可暂时作为原始附件。
长期目标是一个诊断记录来源和 OTLP 交换格式。
迁移验收完成后应移除重复的历史写入路径。
产品分析事件不属于本地诊断记录，不应强制并入该迁移。

### 新增验收条件

端到端验收应导出真实 ZIP，并通过固定版本 Collector 导入选定的诊断后端。
验收应查询 CPU、内存、日志和一条完整追踪。
查询结果应与 ZIP 内的记录数量、时间范围和运行标识匹配。
验收还应覆盖过旧样本、重复导入、追踪裁剪、离线加密历史不可读和部分文件缺失。
工件应包含 Collector 配置、后端查询结果、导入计数和全部版本信息。
本轮仅核对源码与官方文档，尚未执行该导入链路。

## 外部资料

- [OTLP 规范](https://opentelemetry.io/docs/specs/otlp/)：传输与 JSON 编码。
- [OTLP 文件规范](https://github.com/open-telemetry/opentelemetry-specification/blob/main/specification/protocol/file-exporter.md)：JSONL 文件约定。
- [Collector 文件接收器](https://github.com/open-telemetry/opentelemetry-collector-contrib/blob/main/receiver/otlpjsonfilereceiver/README.md)：离线导入与组件成熟度。
- [OpenTelemetry 进程指标](https://opentelemetry.io/docs/specs/semconv/system/process-metrics/)：指标类型、单位与 CPU 归一化口径。
- [VS Code 性能问题指南](https://github.com/microsoft/vscode/wiki/Performance-Issues)：按进程定位与按需性能剖析。
- [sysinfo Process](https://docs.rs/sysinfo/latest/sysinfo/struct.Process.html)：CPU 差分和 RSS。网页对应最新版本，实施时核对固定版本。
- [Apple task_vm_info_data_t](https://developer.apple.com/documentation/kernel/task_vm_info_data_t?language=occ)：`resident_size`、`phys_footprint` 和压缩内存字段。
- [Windows PROCESS_MEMORY_COUNTERS_EX](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex)：working set 与 private commit。
- [Linux /proc](https://docs.kernel.org/filesystems/proc.html)：进程状态、RSS 和 `smaps_rollup`。

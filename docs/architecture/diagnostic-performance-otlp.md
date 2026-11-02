# 诊断采集与 OTLP 导出

## 当前范围

daemon 在独立线程采集自身和已登记宿主的累计 CPU 时间及常驻内存。
普通间隔为 5 秒，详细采集间隔为 1 秒。
采集不依赖自动远程遥测开关。用户导出不会触发上传。
GUI 常驻宿主、GUI 子进程和 Quick Panel 通过当前资料的已认证本地通道登记。
Engine 在内存中绑定 PID、内核进程创建标识和可执行文件路径。
macOS 使用微秒创建时间，Windows 使用 FILETIME，Linux 使用内核启动时钟计数。
这些精确标识不写入归档。归档使用随机运行标识。
采样时再次核对身份。进程退出或身份改变后停止输出有效样本。
Wails 当前公开 API 不提供可确认归属的 WebView PID。未确认进程标为 unknown。
清单必须报告实际进程覆盖，不能将进程采样称为全应用资源占用。

Engine 的观测运行时拥有采样、白名单、运行身份、有界编码队列和分段文件。
Desktop 负责启动采样、用户导出和 ZIP 组装。

## 文件契约

在线导出包含：

```text
otel/manifest.json
otel/metrics.jsonl
otel/logs.jsonl
otel/traces.jsonl
import/collector.yaml
import/import.py
import/README.md
```

每行包含一种信号的完整 OTLP 请求。
序列化使用固定版本 `opentelemetry-proto`。
资源包含产品版本、平台、进程角色、PID、进程创建时间和随机运行标识。
文件不包含真实设备身份、命令行、环境变量或剪贴板内容。
日志只使用已通过现有隐私过滤的记录。追踪只导出已完成的 span。
OTLP 日志只补充明确白名单中的本地关联元数据。`peer_ref`、连接、路径和采集引用保留原值，并由原记录的 `run_id` 划分作用域。新运行的该标识与 SDK Resource 共享同一所有者。旧记录保留原标识。不得复制任意顶层字段。
历史裁剪可能导致追踪不完整。清单不能将存在追踪文件解释为所有追踪完整。

诊断记录产生后持续写入 `diagnostics/engine/<run_id>/`。Logs 保留本地 JSONL，Metrics 与已结束 Traces 保存 OTLP JSONL。Logs 只在导出时转换，不长期双写。
每个运行三类信号合计最多 16 MiB；每段最多 4 MiB，最多保留 64 段。根目录目标为 100 MB/7 天，只回收非活动运行。多活动进程及清理失败可使根目录超过目标。
记录含换行最多 64 KiB。编码前取得单个串行编码槽。队列最多 256 条，并按实际入队副本的 512 KiB 字节上限拒绝超额；writer 和编码槽各最多 64 KiB 编码负载。这不是整个进程的堆或 RSS 上限。
采集不接入资料密钥、AAD 或密文恢复。旧诊断密文不读取、附带或删除。
正常关闭排空队列。突然退出会丢失排队记录与未结束 span；离线计数可能不完整。文件写入不保证断电零丢失，不逐条 fsync。

在线调用 `prepare_local_diagnostic_export`，完成一次必要采样、SDK 和队列刷新。准备结果只包含状态、覆盖与计数，不返回历史正文。
在线和离线都逐文件打开，从句柄取得截止长度，只读有限前缀。完整 JSONL 行才进入 OTLP。
清单记录截止长度、实际读取、残行、坏行、截短、不可读和缺失。SHA-256、字节和请求数对应实际 ZIP 内容。
导出不是跨进程事务快照。其他进程未刷新必须明示。后台继续追加不延长本次导出。

daemon 不运行时，同一格式的文件历史仍可由 Rust 或 Go 导出。只有 Metrics/Traces 时也可导出。
离线导出不启动 daemon、不解锁资料。只有旧密文时仍返回没有日志。
新日志保留源头可用的纳秒时间；旧微秒记录不能恢复已丢失的纳秒。
Logs 的导出转换共用 `packages/desktop-host-go/diaglogs/log-attributes.json` 的字段和静态值白名单，不复制任意字段或错误正文。

## 数值口径

`process.memory.usage` 采用进程指标语义约定 1.37.0。该约定仍为 Development。
该指标导出为累计、非单调 Sum，对应 UpDownCounter。单位为字节。Unix 使用 RSS，Windows 使用 working set。
RSS 不等于独占物理内存。
`uc.process.cpu.time` 是累计总 CPU 秒数，没有伪造 user/system 分类。
`uc.process.cpu.utilization` 是单逻辑核心比例，允许超过 1。
它不等于按核心数归一化的标准 `process.cpu.utilization`。
CPU 首样本、进程重启和较长采样缺口不输出使用率值。
使用率分母来自单调时钟，不能用 UTC 差值替代。

## 开发者导入

用户无需安装 Collector。
开发者解压 ZIP，再按包内说明使用 Collector Contrib `0.139.0`。
该版本的 OTLP JSON 文件接收器为 Alpha。
配置显式从文件开头读取，并关闭文件名属性注入。
导入器校验文件摘要，核对接收、发送和拒绝计数，随后停止 Collector。
重复导入必须由开发者显式选择，不能在失败结果未知时自动重试。
传输成功与后端查询成功是两个验收步骤。
后端必须接受原始时间戳。导入器不平移时间。
目标地址和凭据由开发者提供，不得写入用户诊断包。

本地 Logs 是唯一常驻日志事实来源。OTLP Logs 是导出表示。产品分析事件不属于本地诊断流。

## 验收入口

继续使用 `apps/gui-go/e2e/diagnostics_acceptance_run.py`。
该入口核对在线与离线清单、摘要和进程指标文件。
使用同一入口运行隔离的受控负载验收：

```bash
python3 apps/gui-go/e2e/diagnostics_acceptance_run.py --performance-only --out target/diagnostic-performance/load-run
```

夹具登记本任务拥有的合成宿主，不代表真实 GUI 验收。
该场景覆盖 CPU 负载、内存分配、进程退出、重新建立基线、调度缺口和登记配额。
Collector 导入回执与平台查询结果必须另行保留。
Windows 继续使用 `apps/gui-go/e2e/windows_diagnostics_acceptance.ps1`。
未执行的平台、休眠场景、受控负载与开销对比不能记为通过。

持续文件流和两个离线归档入口通过现有入口验收：

```bash
python3 apps/gui-go/e2e/diagnostics_acceptance_run.py --streaming-only --out target/diagnostic-performance/streaming
python3 apps/gui-go/e2e/diagnostics_acceptance_run.py --streaming-only --duration-seconds 1860 --out target/diagnostic-performance/streaming-long
```

场景使用真实隔离 daemon，检查未导出时增长、在线有限导出、正常停止与突然退出后的 Rust/Go 离线导出、只有 Metrics 的目录和旧密文不变。
回执保存源码、二进制、RSS 时间线、ZIP 独立哈希和清理结果。长时间、超旧 8 MiB、故障、堆峰值与原生平台范围必须分别记录实际执行状态。

完整的主机候选运行使用同一入口。`ENGINE_CHECKOUT` 指向已核实的 Engine checkout；它不是产品配置。

```bash
python3 apps/gui-go/e2e/diagnostics_acceptance_run.py --streaming-only --duration-seconds 1860 --load-bytes 10485760 --engine-root "$ENGINE_CHECKOUT" --engine-probe "$ENGINE_CHECKOUT/target/debug/examples/process_owner_probe" --collector target/diagnostic-performance/collector-0.139.0/otelcol-contrib --out target/diagnostic-performance/streaming-final
```

该场景还验证损坏行、超长行、残行、读取拒绝、目标写入失败、原目标保留，以及两个真实 daemon 共享根目录时的运行身份和活动租约保护。
为验证到期回收，场景只暂停本任务拥有的 daemon，并只修改本任务运行文件的修改时间；它不访问日常产品资料。
`recording` 是在线准备状态；`runs` 是原始运行的来源及最后一次计数；两者均不包含历史正文。
预留队列字节高水位包含入队候选，失败立即回退。它不是已接受记录数，也不是整个进程的堆或 RSS 上限。

编码器保留单个有界缓冲。队列先预留字节，再复制为精确长度的独占记录；候选副本计入预留。因此小记录可以共用字节预算，256 个结构槽不会把编码负载扩大为 256 × 64 KiB。
长时间负载夹具按认证接口返回的 `refreshAtSecs` 更新测试会话，不修改产品的认证期限。

编码槽只执行有界编码与非阻塞入队。槽内不执行磁盘 I/O。

# 诊断性能记录与 OTLP 离线交换

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

Engine 的观测运行时拥有采样和有界历史。
Engine 组装层连接现有资料加密适配器。
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
OTLP 日志只补充明确白名单中的本地关联元数据。`peer_ref`、连接、路径和采集引用保留原值，并由原记录的 `run_id` 划分作用域。该运行标识不等同于 SDK Resource 的随机运行标识。不得复制任意顶层字段。
历史裁剪可能导致追踪不完整。清单不能将存在追踪文件解释为所有追踪完整。

内存历史最多保留 30 分钟、8 MiB。
持久化使用现有资料加密适配器和固定 AAD。
持久文件为日志目录下的 `otel-history/history.enc`。
明文持久化输入最多 512 KiB，密文文件最多 8 MiB。
清单分别报告内存裁剪、持久裁剪、拒绝记录和最近指标时间。
密钥不可用时仅保留内存记录。不得生成替代文件密钥。
确定损坏或不支持的历史格式不会永久阻断写入。采集器先成功加密新历史，再将旧密文保存到单个 `history.quarantine.enc` 证据槽，最后原子替换历史。证据最多保留 8 MiB；超限时只保留前缀，并报告原大小及裁剪状态。恢复元数据随新历史加密保存，并出现在导出摘要中。密钥不可用、锁定、未知加密错误或 IO 错误均不触发隔离和覆盖。

在线导出先请求导出前快照，再冻结 OTLP 记录，最后压缩。
快照等待在阻塞线程中执行，等待期限为 1 秒。
快照超时不阻止其余记录导出。

离线导出不启动 daemon，不解锁资料，也不读取密钥。
离线 ZIP 可以附带 `otel/history.enc`，其状态为 `encrypted_unavailable`。
只有密文历史而没有普通日志时，也允许生成 ZIP。
文件清单记录实际复制内容的大小和 SHA-256。

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

现有原始日志附件暂时保留，供旧诊断工具读取。
所有进程的 OTLP 覆盖与诊断工具迁移通过后，移除重复的本地记录写入路径。
产品分析事件不属于这条迁移链路。

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

加密历史恢复验收仍使用现有入口：

```bash
python3 apps/gui-go/e2e/diagnostics_acceptance_run.py --history-recovery-only --out target/diagnostic-performance/history-recovery
```

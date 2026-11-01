# 离线诊断导入

固定使用 OpenTelemetry Collector Contrib 0.139.0。文件接收器为 Alpha。
先解压诊断 ZIP。查看 `otel/manifest.json` 的覆盖、缺失和裁剪状态。
文件按信号保存完整 OTLP JSON 请求，每行一个请求。空文件表示没有该信号。
用户端无需安装 Collector。导出不会上传数据。

从可信源码工作区运行一次性导入器。参数中的 ZIP 为原诊断包：

```bash
OTEL_IMPORT_ENDPOINT=http://127.0.0.1:4318 python3 packages/desktop-host-go/diaglogs/import/import.py diagnostics.zip --collector /path/to/otelcol-contrib --receipt import-receipt.json
```

客户提供的 ZIP 属于外部输入。包内脚本仅作版本参考。
运行包内脚本前，先核对它与可信源码副本的 SHA-256 一致。
导入器校验固定版本、文件大小、容量上限和 SHA-256。
导入器使用临时配置读取每个文件一次。
接收和发送计数均与 ZIP 一致后，导入器停止 Collector。
发送失败或超时会返回失败，并保留计数。
已有回执时拒绝再次导入。确认目标平台状态后才能显式使用 `--allow-duplicate`。
目标平台查询仍需单独执行。发送计数不证明平台查询成功。
`collector.yaml` 仅供手动诊断。该配置会持续运行，修改输入文件会触发重复读取。

目标平台须支持 OTLP/HTTP，并接受记录中的原始时间戳。
此配置不含凭据。由开发者在本机配置目标鉴权。
一次性导入器支持从 `OTEL_IMPORT_AUTHORIZATION` 读取 Authorization 请求头。
临时配置只保存环境变量引用，不保存请求头值。
OTLP 不提供后端去重查询。回执将重复记录数标为未知。
手动运行 Collector 时，文件不得在导入期间修改。
重复启动会重复读取文件。不要将进程退出视为平台已经接收成功。
查询目标平台，核对进程、时间范围、指标点、日志和完整追踪。
保留查询结果及拒绝计数。部分追踪可能因历史裁剪而缺少 span。

当前清单报告 daemon 和已登记宿主的覆盖。
未确认归属的 WebView 标为 unknown，不计入已确认进程。
每个进程具有独立 Resource。归档不提供全应用独占物理内存总量。
`uc.process.cpu.time` 是总累计 CPU 秒数，没有伪造 user/system 分类。
`uc.process.cpu.utilization` 是一个逻辑核心的使用比例，允许超过 1。
它不是按可用核心数归一化的 `process.cpu.utilization`。
首样本和休眠缺口没有使用比例值，不能解释为零。
`process.memory.usage` 使用字节，Unix 为 RSS，Windows 为 working set。

# PR #2180：关联元数据与加密历史恢复

## 决定与源码身份

两项发现均已对照当前源码确认，Decision: accept。实现前的失败清单见 P2_RECOVERY_PLAN.md。

- 开工 Desktop：e461eea50235f700268b7e6d9bfc2eba413e9378。保留了原 rebase 和审查修复，没有回退到父消息中的旧提交。
- 开工 Engine：403f3a72ff062760635d6c3407ed5591daf957f7。
- Engine 修复：3d3ecb22166f0edab545ce8b4d0b6c71397ea471，分支 feat/diagnostic-performance-main-integration，已推送。
- Desktop 产品集成：73785cef90254a1d5ec1282e3358247f8dc8c701，固定上述 Engine Git revision 与 Cargo.lock，无 path 依赖。
- 最终验收脚本源码：ced856b8bab71e122d13eb920a5ef79e434e5797。后续提交仅记录本报告。
- 回执内的 head 记录启动时 Git HEAD；编写期的脚本曾含未提交改动。最终脚本均已提交，工件清单另记录脚本摘要。产品二进制摘要保存在每个回执中；没有把脚本 Git HEAD 当作二进制构建来源。

## P2-1：关联元数据

OfflineHistory::log 原先只复制 fields，遗漏记录顶层的本地关联元数据。修复为明确白名单，保留原值和类型，包括可空值。不复制任意顶层字段，不生成新的业务 ID。

不透明引用属于 LocalRecordingState 的运行作用域。OTLP 中保留原记录 run_id；它与 Resource 的 service.instance.id 分开。加密历史重载保留原运行作用域，不给旧记录重新分配当前运行 ID。trace_id/span_id 继续进入 OTLP 正式关联字段。

修改前工件：target/diagnostic-performance/native-run-4/export1.zip。其来源为该目录 assertions.json 记录的旧实施版本，不冒充本轮基线导出。按原始时间和事件字段匹配，416 条 OTLP 全部找到原记录，白名单元数据产生 4925 处缺失。原日志的额外记录可能在 OTLP 快照冻结之后写入，不能只按重复事件名认定为同一记录。

修复后的真实双端传输结果：

| 导出 | 全部 OTLP 日志匹配 | peer_ref | connection_id | path_ref | capture_id | 运行作用域 | trace/span 对照次数 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| before-restart | 451/451 | 121 | 72 | 30 | 143 | 1 | 376 |
| after-restart | 328/328 | 109 | 57 | 31 | 161 | 2 | 162 |

所有匹配记录逐值、逐类型对照通过。白名单差异和未匹配 OTLP 均为 0。属性键只来自原 fields 与明确顶层白名单；原始 trace/span 保持一致。重启包包含旧运行与新运行。

命令：

```bash
python3 apps/gui-go/e2e/diagnostics_acceptance_run.py --correlation-only --out target/pr2180-p2/correlation-after-2
```

## P2-2：历史恢复

持久化所有者现在区分确定不可读取的旧文件和可重试错误。assembly 边界保留 BlobCipherError 的 source，并检查 V3 ContentProtectionError 与旧版密钥错误。V3 KeyUnavailable 不再被误判为确定损坏。

只有新历史加密成功后才写入密文证据，再原子替换 history.enc。history.quarantine.enc 只有一个槽，最多 8 MiB。超限文件仅保留前缀，并报告原大小、保留大小与裁剪。恢复原因与元数据随新历史加密保存，并在主动导出的摘要中显示。损坏历史的旧记录数未知；不能把新历史的裁剪计数解释为旧数据没有丢失。

锁定、缺失历史密钥和 IO 错误不隔离或覆盖原文件。未知保护错误也保守重试。没有新增明文历史、文件密钥或密钥存储路径。

修改前真实损坏场景在多次快照后仍为 locked_or_io_failure，未恢复写入，证据位于 target/pr2180-p2/corruption-before/。修改后以下场景均通过：

- 无效密文头、认证失败和超过 8 MiB 的历史恢复新密文。
- 旧密文证据与输入逐字节一致；超限时与输入的有界前缀一致。重复损坏后证据槽数量仍为 1。
- 每次恢复后停止并重启，旧运行指标和恢复元数据均重载。
- 缺失历史密钥时，真实 V3 解密回调报告 key_unavailable，原文件摘要不变，没有证据槽。
- 通过产品 API 锁定真实 Engine 会话后，无效文件不被覆盖或隔离；解锁后正常持久化。
- 文件权限拒绝时，原 inode 和内容均不变；恢复权限后重新持久化。
- 所有正常回执 cleanupFailures 为空；失败场景均标为 failed，原场景异常不会被清理异常覆盖。启动失败也执行停止检查。

命令：

```bash
python3 apps/gui-go/e2e/diagnostics_acceptance_run.py --history-recovery-only --out target/pr2180-p2/history-after-2
```

## 其他验证与工件

- PASSED：Engine workspace all-targets locked、Rust 格式、风格、仓库架构与隐私检查。
- PASSED：Desktop 相关 all-targets 编译和原生 GUI E2E 构建；宿主结构检查、56 项现有路由测试、33 对文档及锚点检查、Python 语法与 diff 检查。没有新增单元测试。
- PASSED：真实新 ZIP 的摘要和包内导入器核对；固定 Collector 0.139.0 导入 Prometheus/Loki/Jaeger，再经 Grafana 查询。8 个指标点、74 条日志、1 个完整根 span，失败和拒绝均为 0，保留原始时间戳。

最终导入 ZIP：target/pr2180-p2/daemon/online.zip。SHA-256:2638f3da99a3c25c0c2a4884e90ed1a380fa6db9a1901fd5a2f6d8be16fd1438。

逐个场景的 ZIP 和密文证据摘要见 target/pr2180-p2/history-after-2/result.json；双端 ZIP 摘要见 correlation-after-2/result.json；导入回执和平台查询见 backend/。交付工件清单另保存所有选定文件的 SHA-256。原始失败目录不覆盖。

## 限制

- 原生窗口验收因当前系统无法截取窗口而中断，native-after/assertions.json 保持 passed=false，未冒充通过。截图/原生 GUI 本轮没有完成；数据回归通过同一入口的真实 daemon 场景完成。
- 本轮没有通过加密适配器注入“认证成功但 JSON/版本损坏”的文件。相关分类代码经过编译与审查；不能把密文认证失败测试称为覆盖了这两条分支。
- Windows/Linux 原生、旧 V1/V2 密钥错误的真实运行、未知加密错误、进程崩溃或断电窗口没有运行。未知错误保持保守策略。不能宣称跨平台或断电持久性已验收。
- 本轮恢复的是新历史持久化能力，不是恢复旧损坏内容。超过证据上限的原文件不会被完整保留。
- 旧版本已经丢弃的 OTLP 元数据不会被伪造或回填；本轮对新采集和重载记录验证作用域保真。
- 当前构建仍有既存 macOS 部署目标链接警告。远端 CI 状态单独核对，不以本地验收替代。没有发布产品或合并主分支。

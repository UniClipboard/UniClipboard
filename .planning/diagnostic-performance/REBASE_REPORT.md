# PR #2180 冲突解决与复验

## 集成

- 遵循 CONTRIBUTING.md 的要求，rebase 到 main `ea1bbcde39f11be158349d2847c194e24a1ef6d8`。
- 原 PR head：`c3aae693bee45b7b4cda7ff524612d197796db79`。本地保留 backup/pr-2180-before-rebase-c3aae693。
- 实际文本冲突仅在 Cargo.toml 和 Cargo.lock。UI 生命周期、CI 路由、领域术语和目标分支其他变更均保留。
- Engine 诊断分支在独立新分支 feat/diagnostic-performance-main-integration 上 rebase 到目标版本 `8c68da61322d7aa7f64bba5667227251d0ffd6f2`。原 Engine 分支没有改写。
- Engine 集成提交：`403f3a72ff062760635d6c3407ed5591daf957f7`，已推送。五个诊断提交的 range-diff 均为内容相同；目标分支的单次启动密钥读取、文件接收进度和 blob 回收变更均在其祖先中。
- Desktop 使用集成提交，Cargo.lock 从目标分支锁文件重新解析，避免继承无关依赖更新。没有临时 path 依赖。

## 审查修复

五项意见均已核对，Decision: accept。没有跳过项。根因与修改前失败清单见 REVIEW_FIXES.md。

- 清理步骤分别记录异常；缺失或不可读二进制保留摘要失败；全部尝试后再写 result.json。原场景异常继续传播。
- 内容锁客户端列表纳入 quick-panel，保留 gui/helper 和 CLI 原有规则。
- Collector 本地指标轮询使用关闭代理的独立 opener。
- Collector 版本探测上限为 5 秒；超时、启动失败和非零退出均使用参数错误退出。
- Collector 在临时目录清理前停止；成功、拒绝和轮询超时均保留回执。
- 正常负载复验暴露了固定 3 秒基线等待的时序缺陷。改为等待真实基线样本，未降低 CPU 或内存断言。

## 验证

- PASSED：Engine metadata locked、workspace all-targets locked、格式、Rust 风格与仓库所有权/隐私检查。
- PASSED：Desktop daemon/webserver all-targets 编译、原生 GUI 构建、Go 共享宿主构建、Rust 格式。
- PASSED：宿主结构检查、当前 56 项路由测试、33 对中英文文档及锚点检查、Python 语法检查、git diff --check。
- PASSED：冲突集成后的原生 GUI 诊断验收，含 GUI 双进程覆盖、重开、完全退出、加密历史重载与离线导出。证据为 target/pr2180-rebase/native/assertions.json；源码 ab24b7b4a60913fb534f3e19976d933ed9cfd908。本轮使用 --no-expiry-wait，没有重跑 600 秒到期等待。
- PASSED：审查修复后的真实 daemon 内容锁。quick-panel/gui 撤销授权后均为 423 content_locked，重新授权后可读；CLI 可读规则保持不变。证据为 target/pr2180-review/content-gate-2/result.json。
- PASSED：真实负载正常路径，所有清理与二进制摘要完成。证据为 target/pr2180-review/load-2/result.json。
- PASSED：测试侧注入恢复信号不确定结果及 PermissionError/FileNotFoundError，验证后续清理与摘要继续执行、原异常保留、子进程回收。故障场景的 result.json 按预期为 passed=false；对应 fault-assertions.json 为 passed=true。证据位于 target/pr2180-review/cleanup-fault/ 和 cleanup-fault-missing-2/。
- PASSED：真实导入 CLI 的版本路径不存在、版本非零退出、版本超时、不可用代理、后端拒绝和轮询超时。固定 Collector 的成功、拒绝、超时三种情况都在临时输入仍存在时停止。证据为 target/pr2180-review/importer-3/result.json。
- PASSED：最终真实 ZIP 内导入器与可信源码摘要一致，再经 Collector 0.139.0 导入 Prometheus/Loki/Jaeger，并由 Grafana 数据源查询。接收与发送均为 8 个指标点、74 条日志、1 个完整根 span；失败与拒绝均为 0；保留原始时间戳。证据为 target/pr2180-review/backend/。

最终诊断 ZIP：target/pr2180-review/daemon/online.zip。
SHA-256:450d7433b27a44040e39771eee5135d05f9e53f95bbe656013df54c75ce929f2。

首轮夹具失败均保留：importer/ 和 importer-2/ 的观察进程时序；content-gate/ 的错误信封字段；load/ 与 cleanup-fault-missing/ 的未观察到基线。产品断言没有减弱，最终成功工件使用新目录。

Windows/Linux 原生、真实系统休眠、强制同 PID 复用、采集开销 A/B 和完整保留上限压力验收仍为 NOT_RUN。原报告中的限制继续适用。当前构建仍有既存 macOS 部署目标链接警告。没有将本地检查视为远端 CI 通过。

后续推送使用原 PR head 的精确 force-with-lease；若远端已改变则拒绝覆盖。推送后须核对远端 SHA 与 PR head，并确认 GitHub 冲突状态。

最终可执行变更与验收脚本源码：`44d93550589ef9cb7eebd836eb80b95f2fc1125d`。后续提交仅记录本报告。

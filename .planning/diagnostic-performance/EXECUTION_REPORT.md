# 诊断性能采集实施报告

## 任务

- Task ID、Workflow ID：未提供。
- 目标：采集进程 CPU 和内存；加密保存有界历史；在诊断 ZIP 中导出三种 OTLP 信号；通过固定 Collector 导入诊断平台。
- Desktop 基线：`81a11286d281a54625516a7d9dbf8c48b90fed09`。
- Desktop 实施验收源码：`53ab967a0c29ee2ef9440849c1cb4372b6b772b8`，分支 `research-diagnostic-performance`，已推送。
- Engine 基线：`d532978d8bc9740a64460ee08b6e97a85fe4b590`。
- Engine 交付：`dce4ebeb83c2e2043cd81d7d722787fc4bcc476c`，分支 `feat/diagnostic-performance`，已推送。
- Engine 使用专用源码 worktree。Desktop 使用固定 Git revision 和 Cargo.lock，无临时 path 依赖。
- 原研究 BRIEF 保持未提交状态。实施阶段没有合并主分支、创建 PR 或发布产品。后续 PR 文档提交不改变本报告的验收源码。

## 实现

1. Engine 的 `uc-observability-runtime` 拥有独立采样线程、明确登记的进程资源和有界历史。普通采样间隔为 5 秒，详细采样间隔为 1 秒。历史上限为 30 分钟和 8 MiB。
2. Engine 组装层复用资料加密适配器。常驻文件为 `otel-history/history.enc`。密钥不可用时报告失败，不生成替代文件密钥。加密适配器使用弱引用，不由全局观测历史延长密钥生命周期。
3. 进程登记绑定 PID、内核创建标识和可执行文件路径。精确标识与路径仅留在内存。每次采样再次核对。首样本、进程更换和调度缺口不输出 CPU 使用率零值。
4. GUI 常驻宿主、GUI 子进程和真实 Quick Panel 使用已认证的本地连接登记。进程登记失败不阻断业务鉴权。进程数量上限为 32。
5. Desktop 在线路径取得导出前快照，再写入三种 OTLP JSONL、清单和导入工具。等待期限为 1 秒，在阻塞工作线程中执行。离线路径不启动 daemon，不读取密钥，附带密文时标为 `encrypted_unavailable`。
6. Rust 和 Go 归档共用导入资产。清单包含实际覆盖、裁剪、拒绝、快照状态、文件大小与 SHA-256。仅有密文历史时也能导出。
7. 开发者使用可信源码中的一次性导入器。固定 Collector Contrib 0.139.0。OTLP JSON 文件接收器仍为 Alpha。导入器校验文件上限与摘要，关闭自动重试，核对接收、发送和失败计数。已有回执时拒绝自动重复导入。
8. `process.memory.usage` 使用累计、非单调 Sum，单位为字节。`uc.process.cpu.utilization` 使用单逻辑核心比例，可超过 1。归档不提供全应用独占物理内存总量。

主要变更入口：

- Engine：`crates/uc-observability-runtime/src/{offline,process_identity,local_log_processor,local_recording,telemetry}.rs`，Engine assembly 的资料加密连接。
- Desktop：`apps/daemon/src/daemon/diagnostics.rs`、`crates/uc-observability/src/startup_logs.rs`、`packages/desktop-host-go/diaglogs/`。
- 宿主登记：`apps/gui-go/diagnostic_processes.go`、GUI 生命周期入口、`crates/uc-webserver/src/security/connect.rs`、Quick Panel daemon client。
- E2E：现有 `apps/gui-go/e2e/diagnostics_acceptance_run.py` 及其 `diagnostics_performance.py` 场景。
- 文档：`docs/architecture/diagnostic-performance-otlp.md`。CI 路由已包含共享导入资产。

## 验证

环境：当前 macOS 原生宿主。没有新增单元测试。执行了现有路由测试和真实端到端场景。

| 状态 | 检查 | 证据 |
| --- | --- | --- |
| PASSED | Engine workspace all-targets locked、Rust 风格与提交前检查 | `target/diagnostic-performance/delivery-final/checks/engine-workspace.log`，Engine 最终提交 |
| PASSED | Desktop daemon、daemon-client、Quick Panel all-targets 编译 | `target/diagnostic-performance/delivery-final/checks/desktop-check.log` |
| PASSED | 原生 GUI E2E 构建 | `target/diagnostic-performance/delivery-final/checks/native-build.log` |
| PASSED | Go 共享宿主构建、宿主结构检查、85 项现有路由测试 | `target/diagnostic-performance/delivery-final/checks/host-contract.log`、`target/diagnostic-performance/delivery-final/checks/routing.log` |
| PASSED | 真实 daemon 导出、首样本省略、加密历史检查 | `target/diagnostic-performance/daemon-run-4/result.json` |
| PASSED | CPU/内存负载、退出、新进程基线、调度缺口、真实 Quick Panel、登记配额 | `target/diagnostic-performance/resource-load-run-4/result.json` |
| PASSED | 最终原生 GUI、双 GUI 进程覆盖、关闭与重开、加密历史重载、离线导出 | `target/diagnostic-performance/native-run-4/assertions.json`；使用 `--no-expiry-wait`，本轮没有重跑 600 秒等待 |
| PASSED | 最终 ZIP 经 Collector 导入 Prometheus、Loki、Jaeger，再经 Grafana 数据源查询 | `target/diagnostic-performance/backend-run-4/` |
| PASSED | 重复回执、缺失文件、摘要不符、容量超限、离线不可读、后端 503 | `target/diagnostic-performance/import-cases-final/result.json`；使用最终版本 ZIP 和导入器 |
| PASSED | 只有密文历史的 Go 离线导出 | `target/diagnostic-performance/offline-resource-only/result.json`；归档源码与最终版本一致 |
| PASSED | 完整 600 秒自动到期验收 | `target/diagnostic-performance/native-run-2/assertions.json`；Desktop `14d57203`、Engine `a5cd9d32`，属于较早版本 |

最终受控负载：单逻辑核心比例峰值 `0.9569541399315332`；常驻内存增量 `139542528` 字节。此结果证明响应受控负载，不是采集开销测量。

最终导入：8 个指标点、74 条日志、1 个 span。Collector 接收与发送计数相同，失败和拒绝均为 0。平台查询返回 5 组指标、74 条日志和完整根追踪 `761f5deb48ebaaf561c1ac66830c64f3`。保留原始时间戳。

ZIP SHA-256:`e584386dff82b2c4a9593729c6fcb1325fbb751d9250a814df014ef20fcafd6f`。

平台固定版本：Collector 0.139.0、Prometheus 3.7.3、Loki 3.5.7、Jaeger 2.22.0、Grafana 12.2.1。原生后端均已正常关闭。Docker socket 不可用，因此使用原生二进制。版本和二进制摘要单独保存。

导入回执中的 `backendQuery` 保留导入器自己的 `NOT_RUN` 状态。独立平台查询结果由 `backend-run-4/result.json` 和 `grafana-*-query.json` 证明。后端重复记录数无法通过 OTLP 观测，保持未知。

## 重现命令

从上述固定 Desktop 提交构建并执行。Engine 由 Cargo.lock 拉取固定提交。

```bash
bash apps/gui-go/build.sh e2e
python3 apps/gui-go/e2e/diagnostics_acceptance_run.py --performance-only --out target/diagnostic-performance/load-replay
python3 apps/gui-go/e2e/diagnostics_acceptance_run.py --out target/diagnostic-performance/native-full-replay
```

平台验收脚本位于交付工件中的 `daemon_acceptance.py` 与 `backend_acceptance.py`。它们使用 `target/diagnostic-performance/tools/` 下的固定版本原生二进制。重跑前将输出目录改为新的目录。脚本拒绝复用已有输出目录。保留 ZIP 和可信导入器的摘要一致检查。

## 限制与风险

- Wails 当前公开 API 不提供可确认归属的系统 WebView PID。清单报告 `unknown`，不使用进程名猜测归属，不宣称覆盖全应用。
- Windows 与 Linux 原生验收：NOT_RUN。当前 macOS 通过不证明其他平台通过。
- 强制同一 PID 复用、真实系统休眠、墙上时钟跳变：NOT_RUN。已执行新 PID 基线重建和受控调度缺口；不能将其称为完整替代。
- 同构建启用/禁用 A/B 开销对比：NOT_RUN。不得宣称满足额外 CPU 小于 0.5% 或额外 RSS 小于 10 MiB。
- 30 分钟时间上限、8 MiB 饱和裁剪、强制快照超时、不同系统权限失败：没有逐项完成原生压力验收。实现报告裁剪、拒绝和超时状态，但不能将代码检查当作这些场景通过。
- 追踪历史可能被裁剪。已查询的验收追踪完整，不代表每个用户诊断包都包含完整追踪。
- 当前构建有既存的 macOS 链接目标版本警告。当前机器可运行，不构成 macOS 11 兼容证明。
- 原始日志附件暂时保留。现有文档规定，在全部进程覆盖和工具迁移验收后移除重复记录路径。

## 状态与后续

状态：PARTIAL。代码实施、固定版本集成和 macOS 主链路已完成。跨平台、系统 WebView 归属与上述未执行验收仍未完成。

下一步：独立审查两仓提交；按同一工件契约运行 Windows/Linux；确认原生宿主可用的 WebView 归属接口；执行采集开销 A/B 对比。不要将本报告作为发布或合并批准。

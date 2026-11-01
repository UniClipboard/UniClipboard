# Engine 共享编译缓存

修改 Rust CI、缓存或预热时读取。PR 条件路由见[工作流规则](../agent/workflow-rules.md#ci-与-pr-检查)。

## 入口与所有权

- Engine 提供 MBX 实现。`scripts/build-cache/engine-tools.sh` 固定自动化提交并逐文件校验 SHA-256；产品 Engine 版本由 `Cargo.toml` / `Cargo.lock` 独立管理。
- Desktop 的 `.github/actions/engine-build-cache` 负责 CI 装配。编译始终只使用本地 CAS；R2 只传输快照。快照缺失、损坏、不可用或未恢复时，执行本地冷编译，禁止逐对象远端回退。
- 本地入口为 `bun mbx:exec -- <command>`。不修改全局 Cargo 配置，不叠加 sccache，不启用增量编译、target views、target seed 或 `share_workspace_root`。
- Windows 使用原生 MBX 和私有 `cargo.exe`。Node 启动的 daemon/helper 编译也须分别进入包装器，并分别保存统计。
- 工具版本、自动化 pin 和协议参数以入口脚本为准；本页不重复维护。

## 权限与存储边界

- 私有桶为 `uniclipboard-build-cache`；快照前缀为 `desktop/mbx-snapshots/v1/<scene>/`。不得操作历史逐对象缓存或 sccache 对象。
- 消费者使用 `desktop-build-cache-reader`；只有受保护 `main` 的生产者可使用 `desktop-build-cache-writer` 发布。PR 和其他分支只恢复。
- 只传递 `BUILD_CACHE_R2_ENDPOINT`、`BUILD_CACHE_R2_ACCESS_KEY_ID`、`BUILD_CACHE_R2_SECRET_ACCESS_KEY`。嵌套可复用工作流须显式声明和传递，不继承无关发布凭据。
- 无凭据时使用本地 MBX；部分凭据或非 HTTPS 端点拒绝装配。凭据只进入 runner 临时受限文件，Unix 权限为 `0600`；`GITHUB_ENV` 只保存路径。
- `UC_MBX_SNAPSHOT_ENV_FILE` 指向传输身份；`UC_ENGINE_MBX_ENV_FILE` 指向无凭据的本地编译策略。容器只读挂载所需文件，不把凭据、端点或 SDK 异常正文写入日志、摘要、命令参数或工件。
- 快照只含已校验的 CAS、action result 和原生导出清单；不得包含 target、Cargo workspace state、测试结果、用户数据或凭据。macOS/Linux 不从 Actions 恢复 target；容器使用独立 target。

## 场景与共享编译入口

预热与消费者必须使用相同的编译入口、工具链、features、profile、flags 和容器环境。

| 场景 | 共享入口 |
| --- | --- |
| release sidecar（按目标、组件、构建模式和 target 模式隔离） | `.github/workflows/build-target.yml`、`.github/workflows/build-cli.yml` |
| `linux-gui` | `apps/gui-go/e2e/linux/build_daemon_release.sh` |
| `e2e-macos` | `.github/actions/build-e2e-rust` |
| `pr-check-linux`、`quick-panel-windows` | `.github/actions/build-pr-rust` |
| `coverage-linux` | `.github/actions/rust-coverage` |

- 场景键隔离操作系统、架构、编译器、依赖、Cargo 配置、缓存工具和协议、共享构建动作。容器场景还包含镜像定义与系统身份；musl 包含 C 工具链和 flags；覆盖率包含插桩工具版本。
- 快照键只选择基线。MBX 按完整实际输入校验产物，源码、版本、渠道、features 或 flags 变化仍可能重编译。不得修改实际源码、关闭优化或复用其他版本二进制来制造命中。
- 发布与覆盖率基线可在内存中归一化本地包版本；不归一化外部依赖或实际编译输入。GUI/CLI sidecar 和 Linux 独立打包使用 `fetch-depth: 0`，供 MBX 继承历史锁文件的预测记录。
- coverage 每次仍执行完整测试并生成新 LCOV；消费者上传 Codecov，预热不上传。没有快照场景的测试保留本地编译策略，不另建预热。

## 接入与预热

1. 复用表中的共享编译入口。设置 `MBX_CACHE_EXPORT_GROUP=<scene>`，在实际编译环境中恢复快照、编译并保留统计。
2. 使用 `.github/actions/mbx-snapshot` 和 `scripts/ci/mbx-snapshot.py` 传输。脚本使用 MBX 原生导出/导入确定依赖闭包，不自行实现动作键或对象遍历。
3. 上传不可变的 SHA-256 归档后再更新索引。恢复须校验大小和摘要，并拒绝路径穿越、链接、状态附件及超限内容（压缩上限 16 GiB，解包上限 40 GiB）。传输失败写入 `snapshot-restore.json` / `snapshot-save.json` 并发出警告，不能伪装成命中或普通未命中。
4. 由受保护 `main` 的 `cache-warmup.yml` 建立基线。工具链、依赖或协议变化后重新预热；没有基线时允许冷编译，发布不等待同提交的预热。
5. sidecar/Linux GUI 生产者仅在 `build_only && save_cache` 且构建源码 SHA 等于工作流 SHA 时发布；coverage 还须测试成功。`build_only` 保留源码证明，但不进入签名、安装器、GUI 验收或生产符号上传。
6. 构建历史源码时，先按工作流 `GITHUB_SHA` 固定传输脚本，再检出源码；用 `UC_MBX_SNAPSHOT_SOURCE_ROOT` 指定实际源码目录。容器 Git 信任仅限明确目录，不设置全局通配符。
7. 成功发布后仅清理当前场景超过 14 天的索引和归档；清理异常单独报告，不修改桶级生命周期。

自动预热由 `scripts/ci/cache-warmup-needed.py`判断依赖变化；手动和定时运行始终预热。sidecar 按平台依次预热，平台内组件并行，辅助场景随后运行。不得把此调度描述为发布优先级保证。

## 验证与证据

```bash
python3 scripts/ci/mbx-snapshot.test.py -v
```

本机有对应 MBX 时，设置 `UC_MBX_TEST_BINARY` 运行真实导出、空 CAS/空 target 恢复、产物执行和输入失效验证。测试使用临时 fixture 与内存 S3，不证明真实 R2 权限或完整产品验收。跨平台入口为 `.github/workflows/mbx-snapshot-validation.yml`。

真实缓存变更按以下顺序验收：

1. 在受保护 `main` 成功发布基线。核对 `snapshot-save.json` 和实际生产 SHA。
2. 使用独立 CAS 和空 target 恢复。核对 `snapshot-restore.json` 的命中状态与 `source_sha`，以及 MBX 的 `hits`、`restored_output_files`。
3. 确认快照消费者实际下载且未上传，原生逐对象远端读写为零，传输与后台错误为零。快照传输统计与 MBX 编译统计分别检查。
4. 执行恢复出的产物；coverage 重新运行测试并检查新报告。缓存命中不替代产品、容器或原生平台验收。
5. 用 `scripts/ci/summarize-mbx.py` 生成摘要，保留会话 JSON、快照记录、源码身份和执行结果。历史源码缺少汇总脚本时保留原始统计。

评估收益时固定源码和编译参数，计入工具安装、快照传输、编译及完整任务耗时；生产者另计导出、压缩、上传、存储与清理成本。首次冷编译不能作为命中样本，逻辑请求数不等于 Cloudflare 账单，单平台结果不能外推到全部发布。

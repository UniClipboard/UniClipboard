# Engine 共享编译缓存

## 入口与所有权

macOS/Linux/Windows sidecar、musl CLI daemon、Rust PR/E2E、coverage、warmup 和 Linux 独立打包使用 Engine 的 MBX 1.18.0 实现。`scripts/build-cache/engine-tools.sh` 从 Engine 自动化提交 `a8e21a0505159e7b71630b2178746d72f985ac76` 获取五个缓存脚本，逐文件校验 SHA-256。产品依赖的 Engine rev 仍由 Cargo.toml/Cargo.lock 管理；自动化 pin 独立维护，避免升级业务依赖时偷偷改变缓存实现。`.github/actions/engine-build-cache` 只负责共同的 CI 装配和无凭据 CAS 存储，没有复制另一套 MBX/R2 策略。

本地 `bun mbx:exec -- <command>` 也使用该入口。只在 Cargo 子进程移除冲突的 sccache wrapper，关闭增量编译、target views 和 target seed，不修改全局 Cargo 配置或共享 target。

Windows 使用校验后的原生 MBX x64/arm64 包和私有 cargo.exe，不叠加 sccache。原生双架构 E2E 已验证编译、恢复产物执行及关键输入变化；完整产品工作流的结论应以对应候选 SHA 的 CI 工件为准。

## 后端与权限

复用私有桶 `uniclipboard-build-cache`，MBX namespace 为 `engine/mbx`，对象位于 `engine/mbx/v1/`。历史 Windows `desktop/sccache/v1/windows-*` 对象不删除，也不能转换成 MBX 命中。

使用现有 `desktop-build-cache-reader` / `desktop-build-cache-writer` environments 内的三个 secret 名称：

- `BUILD_CACHE_R2_ENDPOINT`
- `BUILD_CACHE_R2_ACCESS_KEY_ID`
- `BUILD_CACHE_R2_SECRET_ACCESS_KEY`

sidecar 只在受保护 main 的 push 请求 read-write，其他事件请求 read-only；MBX 原生策略和实际桶令牌仍是最终约束。Linux 独立打包、PR、coverage 和 membership 绑定现有 reader，只请求读取；历史 file-key-store 回归继续不接触 secrets，只使用本地 MBX。secret 名称存在不证明令牌能访问 `engine/mbx/v1/`，需实际 R2 恢复验证；本改动不修改远端权限。

没有凭据时继续本地 MBX；部分凭据或非 HTTPS 端点拒绝装配。凭据只进入 runner 临时 0600 文件，GITHUB_ENV 保存路径。Linux daemon 容器只读挂载此文件和固定工具快照，写入单独统计目录；原始值不写入缓存、产物或命令参数。

## 存储边界与发布

macOS/Linux 不从 Actions 恢复 target。sidecar 无 R2 时按 ref 保存本地 CAS；Linux 缓存只含 Cargo registry/git 下载和 MBX CAS。容器仍使用自己独立的 target。构建参数、Engine rev、签名、Sentry 符号上传、安装包和发布检查不变。

共享桶不保证任意构建互相命中。MBX 按编译器身份、完整编译参数、输入内容、发现的环境输入和受支持链接器/SDK 身份分键。不同 target、features、profile、RUSTFLAGS 或源码可以合法未命中。Engine 作为 workspace member 与 Desktop 的 git dependency 还可能有源路径、Cargo metadata、依赖图或 profile 差异；不启用 `share_workspace_root` 去改变调试路径、`file!()` 或 panic 语义。

MBX 的 Cargo 预测清单按 `Cargo.lock` 内容区分；升版本即使只修改工作区包版本，也会产生新的清单标识。GUI sidecar、CLI sidecar 和 Linux 独立打包的编译 checkout 使用 `fetch-depth: 0`，让 MBX 能通过 Git 历史找到旧锁文件并继承预测记录；预热复用相同入口。浅克隆只有当前提交，会阻断该继承路径。继承的预测仍需按当前输入重新计算动作键，不会绕过缓存有效性校验，也不要求发布等待同一提交的预热完成。

## 验证

每次 Cargo 会话 JSON 上传为 `mbx-sidecar-*` 或 `mbx-linux-package-*`。实际复用须使用两个空 target 并检查 `hits`、`restored_output_files`；R2 验证还必须使用两个独立 CAS，消费者 `downloaded_bytes > 0`、`uploaded_bytes == 0`，远端及后台错误为零，最后运行恢复出的产物。

本地 macOS 小型真实 Engine 内容哈希消费者已验证跨目录恢复和源码/feature/profile/RUSTFLAGS 分键，不能替代完整 daemon、Linux 容器或 Windows CI 验收。自动化脚本下载也计入首次调用耗时，不把安装成本排除后宣称整条发布流水线的收益。

Windows 原生入口已通过 x64/arm64 空 target 恢复和产物执行。CI 配置和统计由 Engine 包装器加载；Node sidecar 脚本对 daemon 与 quick-panel 分别进入包装器，避免原生子进程绕过 shell cargo，也避免两次编译覆盖同一统计文件。自动化 pin 独立于产品 Engine pin。

Warmup 仅调用 `build.yml` 的 `build_only: true`，复用正式编译参数并保留 staging/provenance，不进入签名、安装器或 GUI smoke，也不上传生产调试符号。默认 `build_only: false`，正式发布的签名与包校验策略不变。手动验证直接运行该编译入口，不调用缓存维护/删除流程。

## R2 请求控制与 macOS E2E 快照

PR Check 按 PR 编号分组，新推送取消该 PR 的过时运行。编译任务通过 `scripts/ci/summarize-mbx.py` 把现有会话统计汇总到任务摘要与原证据工件；逻辑读取是 manifest、action、blob、blob pack 请求之和，不等同于 Cloudflare 账单，未包含 SDK 内部重试。快照传输的请求、字节和错误单独记录，不能只看 MBX 下载量判断总流量。旧源码缺少汇总脚本时，发布任务保留原统计工件并跳过汇总。

macOS E2E 使用私有桶中独立的 `desktop/mbx-snapshots/v1/e2e-macos/` 前缀，不占用 Actions 缓存容量。场景键包含目标宿主、实际 Rust 编译器、依赖锁文件、Cargo 配置、缓存工具 pin、快照协议脚本和共享构建动作。源码变化仍由 MBX 重新校验，不依赖快照键判断编译产物有效性。

`cache-warmup.yml` 的 macOS E2E 任务在受保护 main 上生成快照，使用现有 writer 环境；PR 只读现有 reader 环境。快照预热复用 `.github/actions/build-e2e-rust`，与 PR 使用相同的 daemon、开发 CLI、Engine host 和测试二进制编译命令，保留开发 CLI 的独立 target 与 feature 图。构建本身继续使用 Engine 固定入口，原生 MBX 对象写入限制不变。快照发布是独立的 CI 归档传输，允许受保护 main 的 push、定时预热和手动预热，不允许 PR 或任意分支写入。

快照由 `scripts/ci/mbx-snapshot.py` 编排：

1. 为本次任务的命令设置 `MBX_CACHE_EXPORT_GROUP=e2e-macos`，通过原生分组导出选取实际构建记录。
2. 删除导出格式中的可选 Cargo workspace state 附件，再用原生 MBX 导入到隔离 CAS；原生导入器确定传递闭包、校验对象并丢弃无引用附件。编排脚本不实现动作键或对象遍历逻辑。
3. 只打包已校验的 CAS、action result 与原生导出清单，不携带 target、凭据文件、用户数据或完整历史预取缓存。原生导出临时阶段仍会捕获 Cargo 状态，最终发布的归档不包含它。
4. 先上传不可变、按 SHA-256 命名的压缩归档，再更新场景索引。消费者校验归档大小和 SHA-256，拒绝路径穿越、链接、额外目录、状态附件与超限内容，再交给原生 MBX 导入。
5. 成功发布后只清理该试点前缀下超过 14 天的索引和归档，不修改桶级生命周期或 `engine/mbx/` 对象。清理异常单独报告。

PR 的读取策略是：**快照完整导入成功后只使用本地 CAS；变化的输入在本地编译，避免远端历史预测触发额外预取。** 快照缺失、损坏或不可用时，记录状态并继续原来的逐对象 R2 读取。快照命中后的停止读取通过清除本次 Engine 配置文件中的远端选择实现，不使用 MBX 不支持的预取配置，也不修改全局 Cargo 配置。预热仍保留逐对象读取，以填充下一份快照。

快照上传和恢复使用 runner 临时虚拟环境中的固定 boto3，凭据从已有 0600 临时文件加载；原始凭据、端点与 SDK 异常正文不进入摘要。压缩归档上限为 16 GiB，解包总量上限为 40 GiB；临时导出、隔离导入与压缩仍需要额外磁盘空间。优化失败会在任务中发出警告并写入 `snapshot-restore.json` 或 `snapshot-save.json`，不能把错误标记为普通命中或未命中。

首次接入后，在 main 运行 Cache Warmup 生成基线；工具链、依赖或快照协议变动后同样需要重新预热。在快照成功生成前，PR 保持原 R2 行为。快照在成功预热时清理，长期不运行预热不会主动清理过期对象。

## 请求成本对照验证

手动运行 `mbx-cache-benchmark.yml`，三个独立 macOS runner 对同一源码分别使用原始 R2、快照加逐对象补齐、快照命中后仅本地缓存。三者使用相同构建动作并实际执行 daemon 合同测试，不写 R2。快照未命中或失败时，第三组会退回原策略，必须结合 `snapshot-restore.json` 判断实验是否有效。

比较完整 job 耗时、快照安装/下载/导入耗时、快照与 MBX 下载字节、逻辑请求、命中/重编译和错误；快照生成的时间、R2 写入与存储也计入总体成本。当前没有完整 macOS 产品运行的降幅数据，不能把本地小型闭包验证当作性能结论。后续根据对照结果决定是否保留快照命中后的本地策略，以及是否推广到发布 sidecar。

本地检查脚本边界：

```bash
python3 scripts/ci/mbx-snapshot.test.py -v
```

已有本机 MBX 1.18.0 时，可通过 `UC_MBX_TEST_BINARY` 指向其二进制，运行真实导出、压缩传输、空 target 导入、命中、产物执行、源码/feature 失效及损坏拒绝验证。该测试只操作自己的临时 fixture，不修改项目 target 或远端缓存。

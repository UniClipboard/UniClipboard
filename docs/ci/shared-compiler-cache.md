# Engine 共享编译缓存

## 入口与所有权

macOS/Linux/Windows sidecar、musl CLI daemon、Rust PR/E2E、coverage、warmup 和 Linux 独立打包使用 Engine 的 MBX 1.18.0 实现。`scripts/build-cache/engine-tools.sh` 从 Engine 自动化提交 `a8e21a0505159e7b71630b2178746d72f985ac76` 获取五个缓存脚本，逐文件校验 SHA-256。产品依赖的 Engine rev 仍由 Cargo.toml/Cargo.lock 管理；自动化 pin 独立维护，避免升级业务依赖时偷偷改变缓存实现。`.github/actions/engine-build-cache` 只负责共同的 CI 装配和无凭据 CAS 存储，由 Desktop 在 CI 装配时强制编译仅使用本地 CAS，快照传输独立持有身份。

本地 `bun mbx:exec -- <command>` 也使用该入口。只在 Cargo 子进程移除冲突的 sccache wrapper，关闭增量编译、target views 和 target seed，不修改全局 Cargo 配置或共享 target。

Windows 使用校验后的原生 MBX x64/arm64 包和私有 cargo.exe，不叠加 sccache。原生双架构 E2E 已验证编译、恢复产物执行及关键输入变化；完整产品工作流的结论应以对应候选 SHA 的 CI 工件为准。

## 后端与权限

复用私有桶 `uniclipboard-build-cache`，CI 仅传输 `desktop/mbx-snapshots/v1/` 下的快照，不再读写 `engine/mbx/v1/` 中的逐对象缓存。历史 Windows `desktop/sccache/v1/windows-*` 对象不删除，也不能转换成 MBX 命中。

使用现有 `desktop-build-cache-reader` / `desktop-build-cache-writer` environments 内的三个 secret 名称：

- `BUILD_CACHE_R2_ENDPOINT`
- `BUILD_CACHE_R2_ACCESS_KEY_ID`
- `BUILD_CACHE_R2_SECRET_ACCESS_KEY`

快照继续使用现有 reader / writer 环境和发布门控，只有受保护 main 上的生产者可发布。编译本身不加载远端身份，不论生产者还是消费者都只使用本地 MBX。secret 名称存在不证明快照可读，需实际恢复验证；不修改远端权限。

没有凭据时继续本地 MBX；部分凭据或非 HTTPS 端点拒绝装配。凭据只进入 runner 临时 0600 文件，GITHUB_ENV 保存路径。Linux daemon 容器只读挂载此文件和固定工具快照，写入单独统计目录；原始值不写入缓存、产物或命令参数。

## 存储边界与发布

macOS/Linux 不从 Actions 恢复 target。sidecar 无 R2 时按 ref 保存本地 CAS；Linux 的 Actions 缓存只含 Cargo registry/git 下载；私有 MBX CAS 使用 R2 快照传输。容器仍使用自己独立的 target。构建参数、Engine rev、签名、Sentry 符号上传、安装包和发布检查不变。

共享桶不保证任意构建互相命中。MBX 按编译器身份、完整编译参数、输入内容、发现的环境输入和受支持链接器/SDK 身份分键。不同 target、features、profile、RUSTFLAGS 或源码可以合法未命中。Engine 作为 workspace member 与 Desktop 的 git dependency 还可能有源路径、Cargo metadata、依赖图或 profile 差异；不启用 `share_workspace_root` 去改变调试路径、`file!()` 或 panic 语义。

MBX 的 Cargo 预测清单按 `Cargo.lock` 内容区分；升版本即使只修改工作区包版本，也会产生新的清单标识。GUI sidecar、CLI sidecar 和 Linux 独立打包的编译 checkout 使用 `fetch-depth: 0`，让 MBX 能通过 Git 历史找到旧锁文件并继承预测记录；预热复用相同入口。浅克隆只有当前提交，会阻断该继承路径。继承的预测仍需按当前输入重新计算动作键，不会绕过缓存有效性校验，也不要求发布等待同一提交的预热完成。

## 验证

每次 Cargo 会话 JSON 上传为 `mbx-sidecar-*` 或 `mbx-linux-package-*`。实际复用须使用两个空 target 并检查 `hits`、`restored_output_files`；R2 验证还必须使用两个独立 CAS，消费者 `downloaded_bytes > 0`、`uploaded_bytes == 0`，远端及后台错误为零，最后运行恢复出的产物。

本地 macOS 小型真实 Engine 内容哈希消费者已验证跨目录恢复和源码/feature/profile/RUSTFLAGS 分键，不能替代完整 daemon、Linux 容器或 Windows CI 验收。自动化脚本下载也计入首次调用耗时，不把安装成本排除后宣称整条发布流水线的收益。

Windows 原生入口已通过 x64/arm64 空 target 恢复和产物执行。CI 配置和统计由 Engine 包装器加载；Node sidecar 脚本对 daemon 与 quick-panel 分别进入包装器，避免原生子进程绕过 shell cargo，也避免两次编译覆盖同一统计文件。自动化 pin 独立于产品 Engine pin。

sidecar 预热调用 `build.yml` 的 `build_only: true`，复用正式编译参数并保留 staging/provenance，不进入签名、安装器或 GUI smoke，也不上传生产调试符号。默认 `build_only: false`，正式发布的签名与包校验策略不变。手动验证直接运行该编译入口，不调用缓存维护/删除流程。

## R2 请求控制与跨平台快照

PR Check 按 PR 编号分组，新推送取消该 PR 的过时运行。编译任务通过 `scripts/ci/summarize-mbx.py` 把现有会话统计汇总到任务摘要与原证据工件；逻辑读取是 manifest、action、blob、blob pack 请求之和，不等同于 Cloudflare 账单，未包含 SDK 内部重试。快照传输的请求、字节和错误单独记录，不能只看 MBX 下载量判断总流量。旧源码缺少汇总脚本时，发布任务保留原统计工件并跳过汇总。

快照使用私有桶中独立的 `desktop/mbx-snapshots/v1/<scene>/` 前缀，不占用 Actions 缓存容量。场景键包含宿主操作系统与架构、实际 Rust 编译器、依赖锁文件、Cargo 配置、缓存工具 pin、快照协议脚本和对应的共享构建动作。Linux PR 场景还包含容器定义与 `/etc/os-release`，生产和消费都使用 bookworm 镜像。Windows x64 与 ARM64 的键独立；不同场景、平台和架构不混用快照。源码变化仍由 MBX 重新校验，不依赖快照键判断编译产物有效性。

| 场景 | 消费者 | 共享编译入口 |
| --- | --- | --- |
| `release-sidecar` | Windows / macOS daemon 与 helper；Linux sidecar daemon、musl CLI daemon | `.github/workflows/build-target.yml` 或 `build-cli.yml`，每个组件独立编译 |
| `linux-gui` | GTK4 Linux GUI 打包容器内的 daemon | `apps/gui-go/e2e/linux/build_daemon_release.sh`，两种架构分别预热 |
| `e2e-macos` | macOS CLI E2E | `.github/actions/build-e2e-rust`：daemon、独立开发 CLI、Engine host、E2E 测试 |
| `pr-check-linux` | bookworm 容器内 Rust Check | `.github/actions/build-pr-rust`：工作区检查、独立 Quick Panel 检查、OpenAPI 生成器 |
| `quick-panel-windows` | Windows x64 / ARM64 Quick Panel | `.github/actions/build-pr-rust`：独立 Quick Panel 检查、字体回退测试编译 |

所有消费者保留原有运行验证。musl 的场景键包含目标 C 编译器、CFLAGS、Rust flags 和链接器；Linux GUI 在实际打包容器内恢复、编译、保存，键包含打包镜像定义、基础镜像和容器系统身份，不与 bookworm sidecar 混用。membership 仍使用原有缓存策略。

`cache-warmup.yml` 的 E2E 与 PR Rust 预热任务在受保护 main 上生成快照，使用现有 writer 环境；PR 只读现有 reader 环境。快照预热复用 `.github/actions/build-e2e-rust`，与 PR 使用相同的 daemon、开发 CLI、Engine host 和测试二进制编译命令，保留开发 CLI 的独立 target 与 feature 图。构建本身继续使用 Engine 固定入口，原生 MBX 对象写入限制不变。快照发布是独立的 CI 归档传输，允许受保护 main 的 push、定时预热和手动预热，不允许 PR 或任意分支写入。

快照由 `scripts/ci/mbx-snapshot.py` 编排：

1. 为本次任务的命令设置 `MBX_CACHE_EXPORT_GROUP=<scene>`，通过原生分组导出选取实际构建记录。
2. 删除导出格式中的可选 Cargo workspace state 附件，再用原生 MBX 导入到隔离 CAS；原生导入器确定传递闭包、校验对象并丢弃无引用附件。编排脚本不实现动作键或对象遍历逻辑。
3. 只打包已校验的 CAS、action result 与原生导出清单，不携带 target、凭据文件、用户数据或完整历史预取缓存。原生导出临时阶段仍会捕获 Cargo 状态，最终发布的归档不包含它。
4. 先上传不可变、按 SHA-256 命名的压缩归档，再更新场景索引。消费者校验归档大小和 SHA-256，拒绝路径穿越、链接、额外目录、状态附件与超限内容，再交给原生 MBX 导入。
5. 成功发布后只清理该场景前缀下超过 14 天的索引和归档，不修改桶级生命周期或 `engine/mbx/` 对象。清理异常单独报告。

PR、正式 sidecar 消费者与快照预热采用同一策略：**编译始终只使用本地 CAS；快照缺失、损坏、不可用或根本未执行恢复时，均在本地冷编译。** `engine-tools.sh --ci` 在暴露编译入口前，生成不含凭据、不引用身份文件的本地策略。`UC_MBX_SNAPSHOT_ENV_FILE` 保留原始受限身份文件的路径，仅供传输使用；`UC_ENGINE_MBX_ENV_FILE` 指向本地策略，清除全部 `MBX_REMOTE_*` 及 R2 选择变量。直接 CI 包装器调用也强制该策略。Linux 容器只读挂载两个独立文件；历史源码检出继续使用装配时固定的编译入口。没有命中后的策略切换，也没有逐对象回退开关。

快照上传和恢复使用 runner 临时虚拟环境中的固定 boto3；Windows 使用 `Scripts/python.exe` 并将 Git Bash 路径转换为原生路径，Debian 缺少虚拟环境支持时安装 `python3-venv`。凭据从现有 Engine 临时文件加载，Unix 文件权限为 0600；原始凭据、端点与 SDK 异常正文不进入摘要。压缩归档上限为 16 GiB，解包总量上限为 40 GiB；临时导出、隔离导入与压缩仍需要额外磁盘空间。优化失败会在任务中发出警告并写入 `snapshot-restore.json` 或 `snapshot-save.json`，不能把错误标记为普通命中或未命中。

首次接入后，在 main 运行 Cache Warmup 生成基线；工具链、依赖或快照协议变动后同样需要重新预热。在快照成功生成前，PR 使用本地冷编译。快照在成功预热时清理，长期不运行预热不会主动清理过期对象。

没有匹配快照场景的 membership 与历史 file-key-store 测试也使用同一 CI 本地策略，保留全部测试命令；不新增独立预热。Linux GUI 消费作业自身绑定现有 reader environment，生产作业仅在受保护 main 的 `build_only && save_cache` 下绑定 writer；调用方未使用 `secrets: inherit` 不改变作业内环境 secret 的选择，本次不扩大传递的凭据范围。

## 正式发布 sidecar 基线

正式发布与缓存预热共用 `build-target.yml` 的 `build-component` 作业，在原有 `stage-daemon.mjs` 编译命令前恢复快照。目标 triple、daemon/helper、release/test 优化模式、原生/显式 `--target` 分别隔离，前缀为 `desktop/mbx-snapshots/v1/sidecar-<target>-<component>-<build-mode>-<target-mode>/`；每个组件只导出自己的实际构建闭包。

发布基线选择在内存中归一化根 manifest 与 lock 中的本地包版本，保留 registry/git 依赖、Engine rev、编译器、Cargo 配置和实际 profile/Rust flags 的差异。`APP_ENV` 不参与基线选择，让 alpha 与 stable 共享依赖基线。真实源码、manifest 和 lock 不变，原生 MBX 仍按完整输入校验动作，包括 `CARGO_PKG_VERSION`、渠道与 feature；版本或渠道变化时，受影响的产物必须重编译。快照键只选择基线，不决定产物有效性。

传输脚本按工作流的 `GITHUB_SHA` 固定下载到 `RUNNER_TEMP`，随后再 checkout 被构建源码；`UC_MBX_SNAPSHOT_SOURCE_ROOT` 指向实际源码目录。历史 tag 不需要自带新协议，源码目录也不会因缓存工具产生未提交改动。脚本下载、恢复或发布失败均保留本地编译路径。

容器中的源码目录可能由不同用户检出。启动脚本通过进程级 Git 配置只信任明确的源码目录，保留已有 Git 配置，不写入全局配置或信任通配符。该配置仅供快照进程及其子进程读取源码身份，不改变发布权限或源码 SHA 校验。

只有受保护 main 上的 `build_only && save_cache` 任务、且构建源码 SHA 与工作流 SHA 相同时才发布基线；脚本再次校验这一源码边界并记录实际生产 SHA。正式发布和其他分支只恢复，编译始终禁用逐对象远端读取，变化的内容在本地编译。预热也始终禁用原生对象读取与写入，变化的输入在本地重编译，随后使用原始传输身份发布新闭包。新动作进入发布快照，不再同时填充逐对象池；缺失或损坏时也只在本地编译，再发布完整快照。生产者仍有本地编译、导出、压缩、上传与清理成本，不能把整个发布体系描述成只需两次读取。

首次合并后，现有 main 自动预热会为各平台组件建立基线，同时更新协议变更后的 PR 基线。正式发布的降幅须以预热成功后的实际构建统计验证；尚无基线时会本地冷编译，不能当作快照命中样本。

## 验证与历史对照

一次性三模式对照工作流已移除，常规 CI 保留快照生产、只读消费、请求统计和跨平台原生回归测试。共享构建动作继续作为预热与 PR 的单一编译入口。逐对象回退开关已删除；预热和消费者在所有恢复结果下均只使用本地 CAS。

2026-10-22 的 [macOS E2E 三模式对照](https://github.com/UniClipboard/UniClipboard/actions/runs/39786130655)中，原始 R2 与快照仅本地模式分别为 27,264 / 2 次逻辑读取、29,172,597,769 / 2,964,122,625 字节下载、931 / 590 秒完整任务时长。该样本不代表所有平台、完整产品构建或 Cloudflare 账单；这些是 PR/E2E 编译图的数据，不能直接推算正式 release 构建的降幅。

重新评估时可从实验提交 `7c2c0cfd5` 恢复临时对照工作流，在专用分支对固定源码比较原始 R2、快照加逐对象补齐、快照仅本地三种模式，结束后删除。必须先确认受保护 main 已成功发布对应场景的基线，恢复记录为 `hit` 且 `source_sha` 对应预期生产者，再比较包含快照传输的总请求、总下载、完整任务耗时、实际产物执行、消费者上传量与错误。快照未命中后的回退不能作为快照模式的有效样本；生产快照的时间、写入和存储需要单独计入成本。

本地检查脚本边界：

```bash
python3 scripts/ci/mbx-snapshot.test.py -v
```

已有本机 MBX 1.18.0 时，可通过 `UC_MBX_TEST_BINARY` 指向其二进制，运行真实导出、压缩传输、空 target 导入、命中、产物执行、源码/feature 失效、release 版本/渠道变化及损坏拒绝验证。测试还覆盖“预热命中旧闭包 → 改变源码并本地重编译 → 发布新闭包 → 空缓存消费者命中并执行新产物”，以及原始凭据文件不变、策略文件不含凭据、传输仍能读取原始身份。该测试使用内存 S3，只操作自己的临时 fixture，不修改项目 target 或真实远端缓存。

`mbx-snapshot-validation.yml` 在 Linux、macOS、Windows x64 / ARM64 上运行小型原生往返测试，不需要 R2 凭据。它验证可移植性和缓存正确性，不代替受保护 main 的真实发布与三模式性能对照。修改快照脚本会使已有快照键失效，包括 macOS；合并后的自动预热会生成新基线。

预热命中后停止对象回源的策略需要在合并后验证：先由受保护 main 发布当前协议基线，再核对命中生产者的 `snapshot-restore.json`、原生对象读取为零、`snapshot-save.json` 成功且实际生产 SHA 正确，最后以独立只读消费者恢复并验证产物。协议更新后的首次未命中预热必须本地冷编译并发布基线，原生逐对象读写应为零；冷编译成本不能计作命中收益。

## 覆盖率编译快照

覆盖率生产和消费统一调用 `.github/actions/rust-coverage`，使用固定的 cargo-llvm-cov 0.9.1、相同工具链配置、系统依赖和完整的 `cargo llvm-cov --lcov --output-path lcov.info --workspace` 测试命令。消费者继续向 Codecov 上传本次生成的报告，预热不上传 Codecov。

`coverage-linux` 是独立的 MBX 导出分组与快照场景，不混用普通 check、E2E 或发布编译图。除了共享协议输入，场景键还包含 cargo-llvm-cov 实际版本、宿主 `/etc/os-release` 和显式 Rust/profile flags；插桩的实际编译输入继续由原生 MBX 校验。只恢复编译动作闭包，每次仍完整执行测试并重新生成 LCOV、profraw/profdata；这些运行结果和 Cargo target 状态不进入快照。

消费者仅恢复；预热只有在受保护 main 且测试成功后才发布。两者始终禁用逐对象 R2 读写，缺失或损坏时只进行本地冷编译。快照上传、清理及必要的本地重编译仍存在，不能把覆盖率报告当成编译缓存复用。

新增场景修改了共享协议脚本，因此现有场景的键也会一次性失效。合并后的自动预热会重新建立基线；应在基线保存后验证覆盖率生产者命中、再次保存及独立消费者命中，不能把首次回退作为优化效果。Linux 原生回归另外验证：插桩编译 → 导出及精简闭包 → 空 CAS/空 target 导入 → 再次执行测试 → 新 LCOV 含已覆盖的库代码，且运行标记确实增加。

## 发布调度与后台预热

`build.yml` 固定源码 SHA 后，一次调用 `build-target-plan.mjs` 生成全部平台的编译、打包和原生验收矩阵，再交给 `build-target.yml` 执行。后者不再单独启动 `setup-matrix` runner；Windows runner 状态也只查询一次。每个平台仍有独立的组件、组装和打包依赖，不会因其他平台未完成而提前使用其产物。

后台预热先由 `cache-warmup-needed.py` 比较实际提交的依赖输入。只修改本地包版本的 Cargo 元数据不会触发整套预热；外部依赖、profile、工具链、Cargo 配置和构建脚本变化仍会预热。手动和定时运行始终预热。覆盖率的依赖基线同样归一化本地包版本，实际版本相关编译输入继续由 MBX 校验。

release sidecar 预热一次只启动一个平台（该平台的 daemon/helper 仍并行），PR、E2E、coverage 等辅助预热随后启动。musl 和 Linux GUI 使用 `build_only` 入口，只生成编译基线，不执行正式签名、打包或安装验收。此策略减少后台争用，不代表 GitHub 提供发布作业优先级，也不保证其他 PR 或平台容量不会产生排队。GitHub 的额度与矩阵并发行为见[官方限制](https://docs.github.com/en/actions/reference/limits)与[工作流语法](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax)。

最终二进制仍可能需要重新编译：`unportable-native-link`、不支持的 crate 类型和实际输入变化都不能用依赖快照强行绕过。运行 39900895332 中 macOS x64 的最终 `uniclipd` 动作耗时 320.27 秒；Cargo timing 没有进一步区分此动作内部的链接和代码生成。不会通过关闭 ThinLTO、复用不同版本二进制或在 CAS 快照中加入 Cargo target 来宣称消除了这段时间。

本轮修改 CI 策略后，已有键会一次性更新；受保护主线必须先完成冷预热。效果验证依次检查快照保存、独立消费者命中、每组件两次逻辑读取、原生逐对象读写为零以及实际编译/打包成功。首次冷构建和未经主线发布的 PR 不能作为命中效果样本。

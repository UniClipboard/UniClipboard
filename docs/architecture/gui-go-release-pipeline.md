# Go 宿主发布流水线：对外写入前的门禁与离线验收

本文记录 `release.yml` 在 Go/Wails 宿主上的整合：标准流程、门禁、失败模型、离线端到端验收，以及仍然阻塞的项目。它接续 [gui-go-tauri-retirement.md](gui-go-tauri-retirement.md)、[gui-go-updater-signatures.md](gui-go-updater-signatures.md)、[gui-go-windows-packaging.md](gui-go-windows-packaging.md) 与 [gui-go-distribution-channels.md](gui-go-distribution-channels.md)。

## 标准流程（未改动）

1. 维护者手动触发 `prepare-release.yml`（版本由维护者选择；该工作流 bump 版本、重新生成 Go `buildinfo`、刷新 `Cargo.lock`、生成更新日志，并创建 release PR）。
2. 合并 release PR。
3. `tag-on-merge.yml` 自动创建并推送注解 tag `v<version>`。
4. tag push 触发 `release.yml`。

本任务不选择、不设置、不递增版本，不改 `prepare-release.yml`、`tag-on-merge.yml`、`bump-version.js`。tag 是 `release.yml` 的输入，不属于“对外写入”；所有检查发生在 tag 触发之后的构建、签名与资产阶段，并且早于任何 tag（仅 `workflow_dispatch` 路径会在此创建 tag）、GitHub Release、R2、FlareRelease 与渠道写入。

没有生产证书时，tag 可以按现有流程存在，但 `release.yml` 的第一个门禁会失败，正式包不会发布。

## 变更

| 位置 | 变更 |
| --- | --- |
| `release.yml` `validate` | 原无条件 `exit 1` 改为 `scripts/ci/release_gate.py prerequisites`：必须恰好配置一个生产 Windows 签名后端（`WINDOWS_SIGN_BACKEND` 为 `azure`/`pfx`，**或** SignPath 生产：变量 `SIGNPATH_PRODUCTION_POLICY_SLUG` 与 `SIGNPATH_PRODUCTION_CERT_THUMBPRINT`），且有更新签名私钥，否则失败。测试签名（self-test、SignPath test-signing）永远不满足；两个后端同时存在也失败。随后 `release_gate.py source` 固定 `source_sha`，并要求 tag 版本与 `package.json`、`apps/gui-go/app.json`、`Cargo.toml`、`buildinfo.go`、`Cargo.lock` 工作区成员一致，且 `Cargo.toml` 与 `Cargo.lock` 的 Engine 版本固定一致。 |
| `release.yml` `build` / `build-cli` / `create-release` | 一律检出并构建 `needs.validate.outputs.source_sha`（完整提交 SHA），而不是移动的分支或 tag 名。 |
| `release.yml` `create-release` | 收集、签名、清单、登记改为一次调用 `scripts/ci/assemble_release_assets.py`（见下），其后才是原有的 SHA256SUMS、tag（仅 dispatch）、Release、R2、FlareRelease。 |
| `release.yml` 渠道分发 | Snap：alpha 仍异步触发，但 `release=false`，只产出 artifact；COPR：alpha 仍触发，但 `dry_run=true`，只产出 SRPM。推送商店与提交 COPR 需要人显式 `workflow_dispatch`。npm 与 GitCode 镜像入口不变（见下）。 |
| `apps/gui-go/e2e/package_linux.py` | deb 的 `Control: Version` 把预发布分隔符换成 `~`（与 rpm 既有约定一致）；`Conflicts`/`Replaces`/`Provides` 同样使用该写法。文件名与更新语义版本不变。 |

### 共享的发布阶段

`scripts/ci/assemble_release_assets.py` 依次执行：收集具名发行物（`collect-release-assets.py`）→ `release_gate.py assets` → 更新签名（`updater-sign`，使用 `app.json` 里的生产公钥）→ 不带任何密钥的签名复验 → `assemble-update-manifest.js --require-all-platforms` → `build-flare-release-registration.js` → 写出 SHA-256 索引（`assembly-index.json`）。它不访问网络服务，也不写任何外部系统。

`--mode release` 只允许生产路径；`--mode fixture` 只多出“指定一次性签名器与公钥”的能力，阶段、顺序与检查相同。release 模式下传入这些参数会被拒绝。

### 门禁 `release_gate.py assets` 检查什么

- 必需文件集合（版本 `V`）：两个 macOS 更新归档、两个 dmg、两个 deb、两个 rpm、两个 AppImage 更新归档、两个 setup 与两个 portable，即六个平台；至少一个 CLI 归档。缺失、带有其他版本、出现未知文件都失败。
- 下载的 workflow artifact 中出现测试模式或测试签名构建（名称含 `-test`、`-signing-selftest`、`-signpath-test`）即失败，无论它是否贡献了文件。
- 每个发布文件的字节必须等于某个 workflow artifact 内的文件，索引里记录来源 artifact 名。
- 每个平台的打包证据（`package-manifest.json`、`provenance.json`）必须来自固定的 `source_sha`、非脏检出、版本等于 `V`；Windows 证据还必须是 `signing.provider == "signed"`（生产后端）。升级验收用的 `newer` 包刻意使用另一版本，被跳过。

## Windows 生产签名：SignPath 接线

项目确定使用 SignPath 签名。`WINDOWS_SIGN_BACKEND` 只表达 `sign.py` 的本地后端（`azure`/`pfx`），不足以描述 SignPath（SignPath 是远程提交，没有本地私钥），所以前提门禁只认 `azure`/`pfx` 是不够的；现在 SignPath 生产由一组没有默认值的仓库变量选择，不新造 `sign.py` 后端：

| 名称 | 类型 | 作用 |
| --- | --- | --- |
| `SIGNPATH_PRODUCTION_POLICY_SLUG` | 仓库变量 | 生产签名策略名。`build.yml` 在 release 调用（`require_signing`）且该变量非空时进入 `signpath` 模式并使用 Environment `signpath-production`；值为 `test-signing` 被拒绝。 |
| `SIGNPATH_PRODUCTION_CERT_THUMBPRINT` | 仓库变量 | 生产证书的 40 位 SHA-1 指纹。所有验证器 `--expect-thumbprint` 固定该签名者；**不**设置 `SIGNING_TEST_CERT`，也**不**放宽证书链信任。 |
| `SIGNPATH_API_TOKEN` | Environment `signpath-production` 的 secret | 提交签名请求。`validate` 作业读不到 Environment 密钥，所以由 `package-windows-gui` 作业的 “SignPath preconditions” 步骤再次检查并失败关闭。 |
| 构件配置（artifact configuration）`go-stage1`、`go-stage2-setup` | SignPath 内 | 与 test-signing 相同的两阶段配置；需要维护者在 SignPath 中为生产策略保存并确认。 |

接线内容（`build.yml`）：新模式 `signpath`（与 `unsigned`/`selftest`/`signpath-test`/`signed` 并列）；阶段 1（GUI exe、daemon、卸载器、CLI 可执行文件）与阶段 2（setup）的提交步骤在 `signpath` 与 `signpath-test` 下共用，策略名在生产模式取自上述变量；证据记录 `testCertificate: false`、策略与固定指纹；冒烟作业同样固定指纹。同时配置 `WINDOWS_SIGN_BACKEND` 与生产策略视为错误。release 调用拒绝任何测试模式的规则不变。

Windows CLI 归档：`build-cli` 作业只会用 `sign.py` 本地后端签名。SignPath 生产下 Windows GUI 作业已在阶段 1 签名并打包 CLI（`cli-package`），因此该作业把已签名的 CLI zip 上传为 `cli-x86_64-pc-windows-msvc`（收集器读取的同名工件），`setup-matrix` 在该配置下把 Windows 从 `build-cli` 矩阵剔除。

发布门禁 `release_gate.py assets`（及只做证据检查的 `release_gate.py evidence`）接受 `signing.provider` 为 `signed`（本地后端）或 `signpath`（要求证据里 `testCertificate` 为 false、策略不是 `test-signing`、有 40 位固定指纹）；其余 provider 一律拒绝。

**状态：结构已写好，从未与真实 SignPath 生产策略运行过；策略、证书、Environment 与 token 都未配置（只读核对见 `production-blocked` 证据）。生产签名验收 blocked，未配置时仍失败关闭。**

## 失败模型与验证

离线验收：`python3 -I apps/gui-go/e2e/release_assembly_run.py --out <新的空目录>`。它在合成的 `download-artifact` 目录树上运行上述同一批脚本，再用真实 Go 消费者经本地 HTTP 下载并验证六个平台。进程环境移除所有令牌，代理指向无效端口。`scope.json` 明确声明：包是合成的、更新密钥是一次性的、生产 Windows 签名未运行。

| 失败方式 | 用例 |
| --- | --- |
| 缺平台 | 缺 Linux arm64 AppImage 归档；缺 Windows arm64 安装包 |
| 错版本 | 文件名为 9.9.9 的 deb；CLI 归档；平台证据版本不符 |
| 错源 | 证据来自另一提交；证据来自脏检出；源记录对应另一 SHA；预发布证据来自另一提交 |
| 重复 | 同名 deb 出现在第二个 artifact |
| 中间工件 | SignPath 阶段 2 输入与证据内的 CLI 副本不被收集（字节比对） |
| 测试签名 / 未签名 | `-signpath-test`、`-signing-selftest`、`-test` artifact；Windows 证据 provider 为 `signpath-test`、`selftest`、`unsigned`、`unspecified` |
| 版本载体漂移 | 陈旧的 `buildinfo.go`；`Cargo.lock` 的 Engine 版本漂移；单个载体版本不同；错误的版本或 SHA |
| 签名后篡改 | 篡改字节后复验与真实消费者均拒绝；互换两个 `.sig` 被拒绝 |
| 绕过生产公钥 | release 模式用一次性私钥签名，被 `app.json` 的生产公钥拒绝，且没有任何 `.sig` 写出 |
| 缺少生产前提 | 无前提、后端为 `signpath-test`/`selftest`、缺更新私钥，均失败；只有两者都存在才通过（仅验证存在性） |

预发布拼写 `1.3.0-alpha.1` 另在一份载体副本上整条链路重跑（`prerelease-*` 用例），因为仓库当前载体的版本由 prepare-release 决定。

deb 排序验证：`python3 -I apps/gui-go/e2e/linux/deb_version_order_run.py --out <新的空目录>`，在 Debian 容器里用真实 `dpkg` 与 `build_deb`：`1.3.0~alpha.1` → `alpha.2` → `1.3.0` 依次升级，稳定版之上装 alpha 被 `dpkg` 报告为降级。用修复前的 `package_linux.py`（`--baseline`）运行同一脚本会失败。

两个入口都挂在 `updater-e2e.yml`（PR 与手动触发，只读，不写任何外部系统）。

## 三类证据

交付物分三类，互不替代：

1. **真实输入**（`release_real_inputs_run.py`、`verify_package_set_prerelease_run.py`）：用早先真实 CI run 的打包证据 artifact（Linux run 37894007183、Windows run 37875320922、macOS run 37638284857，均为只读下载）核对门禁：真实布局被识别；真实测试模式/测试签名 artifact 因正确原因被拒绝；换成别的提交或版本被拒绝。另用真实 arm64 Linux 包的载荷、以随包的 `build_deb`/`build_rpm` 重建成 `1.3.0-alpha.1`（派生集，已标注），验证 `verify_package_set.py` 接受 `~` 的 deb Version 并拒绝保留 `-` 的 deb。
2. **合成夹具**（`release_assembly_run.py`、`deb_version_order_run.py`）：机制验证，包是合成的，更新密钥一次性。
3. **生产阻塞**（`release_production_blocked_run.py`）：只读记录当前仓库没有生产签名配置，`prerequisites` 失败关闭，并列出需要维护者提供的内容。

真实输入发现的问题：macOS 与 Windows 的打包证据都记录 `dirty: true`（macOS 的 `porcelain` 是 `?? sidecar-artifact/`，即 `build.yml` 把 sidecar 工件下载到检出目录内）。按门禁的规则这会让每次真实发布失败；已在 `.gitignore` 增加 `/sidecar-artifact/`（Windows 记录里没有 porcelain，原因推断为同一个目录，未证实）。

## 渠道触发边界

被调用 workflow 对实际传入参数的支持（只读核对）：`snap.yml` 的 `workflow_dispatch` 有 `channel`、`release`（boolean）、`branch`；`copr.yml` 有 `version`、`project`、`run_ids`、`dry_run`（boolean）；`gh api -f inputs[...]=` 传的字符串 `"false"`/`"true"` 会按 boolean 输入解析，`copr.yml` 的 `dry_run` 路径不读取 COPR 凭据，且仍会构建并上传 SRPM。两个 workflow 在 tag 提交处都包含这些输入（dispatch 用 `ref=v<version>`）。

行为变化：此前 alpha 发布会自动向 Snap Store edge 推送、向 COPR `uniclipboard-alpha` 提交构建；现在两者只产 artifact。需要真实推送时由人显式 dispatch（Snap `release=true`；COPR `dry_run=false`）。

**不存在“非生产 tag 因而安全”**：任何 `v*` tag 推送都会触发 `release.yml`，其成功路径会创建 GitHub Release、上传 R2、登记 FlareRelease，并在 alpha 时触发 npm 发布与 GitCode 镜像。因此没有授权就不做 tag 演练；预演只能用上面的离线入口。

| 渠道 | 本次状态 |
| --- | --- |
| Snap | alpha 触发 `release=false`；`snap.yml` 默认同样不推送。推送需人显式 dispatch。 |
| COPR | alpha 触发 `dry_run=true`。Go rpm 从未真实提交过 COPR（见渠道文档），故不自动提交。 |
| npm | 不变：alpha 仍通过 `repository_dispatch` 自动发布 CLI 包（既有行为，保留；npm 发布不可撤销，所以这条自动路径只能由真实的 tag 发布触发）。 |
| GitCode 镜像 | 不变：alpha 在 Release、R2 与 FlareRelease 登记成功之后 dispatch；`mirror` environment 仅允许 `main`。 |
| 稳定渠道 | 不变：Release 为 draft，FlareRelease 需显式 Promote；Tauri 用户的稳定渠道不受影响。 |

## 仍然阻塞或未验证

- 生产 Windows 代码签名后端（`WINDOWS_SIGN_BACKEND` 与对应凭据、SignPath 生产策略或 Azure Artifact Signing）不存在：生产签名验收为 blocked，未运行。
- 在 GitHub Actions 上的真实运行（tag 触发、构建、全平台包）未做：没有 push、没有 dispatch。所有证据来自本地。
- 生产更新私钥、`MINISIGN_RELEASE_PRIVATE_KEY`、R2 / FlareRelease 凭据的真实可用性未验证。
- `workflow_dispatch` 路径（`release.yml` 里的 `bump`）不是标准入口，本任务未改动，其二次 bump 与 `buildinfo` 不重新生成的问题见报告。

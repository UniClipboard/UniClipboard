# Release Workflow

本文档说明如何使用项目的版本管理和发布系统。

> **当前状态：** 标准流程是手动 `prepare-release` → release PR → 合并 → `tag-on-merge` 推送版本 tag → `release.yml`。`release.yml` 现在在对外写入前执行生产签名前提、固定源提交、版本一致性与资产门禁；缺少生产 Windows 签名后端时它失败关闭。Go/Wails 宿主的流水线、门禁和离线验收见 [`docs/architecture/gui-go-release-pipeline.md`](architecture/gui-go-release-pipeline.md)。下文关于安装包、签名与渠道的部分仍是旧流水线的设计记录，以前述文档为准。版本号脚本仍然可用。

## 版本管理脚本

项目提供了自动化的版本管理脚本 `scripts/bump-version.js`，用于统一管理版本号。

### 本地使用

```bash
# Patch 版本升级 (0.1.0 -> 0.1.1)
bun run version:bump --type patch --channel stable

# Minor 版本升级 (0.1.0 -> 0.2.0)
bun run version:bump --type minor --channel stable

# Major 版本升级 (0.1.0 -> 1.0.0)
bun run version:bump --type major --channel stable

# 创建 alpha 预发布版本 (0.1.0 -> 0.1.0-alpha.1)
bun run version:bump --type patch --channel alpha

# 继续发布 alpha 版本 (0.1.0-alpha.1 -> 0.1.0-alpha.2)
bun run version:bump --type patch --channel alpha

# 一步设置到指定版本 (例如: 0.1.0-alpha.2)
bun run version:bump --to 0.1.0-alpha.2

# 从预发布版本升级到稳定版 (0.1.0-alpha.5 -> 0.1.0)
bun run version:bump --type patch --channel stable

# 预览变更（不实际修改文件）
bun run version:bump --type patch --channel alpha --dry-run
```

### 脚本功能

该脚本会自动更新以下文件中的版本号：

- `package.json`
- `apps/gui-go/app.json`
- `Cargo.toml`
- `Cargo.lock`

参数说明：

- `--type <patch|minor|major>` + `--channel <stable|alpha|beta|rc>`: 按规则升级版本
- `--to <version>`: 直接设置目标版本（语义化版本），不能与 `--type/--channel` 同时使用
- `--dry-run`: 仅预览，不修改文件

## 发布渠道

项目支持以下发布渠道：

### Stable（稳定版）

- **用途**: 正式发布版本，推荐给所有用户使用
- **版本格式**: `X.Y.Z` (例如：`1.0.0`)
- **GitHub Release**: 标记为正式版本（非 prerelease）

### Alpha（内测版）

- **用途**: 早期功能测试，可能包含未完成的功能或已知问题
- **版本格式**: `X.Y.Z-alpha.N` (例如：`0.1.0-alpha.1`)
- **GitHub Release**: 标记为 prerelease，带有警告说明
- **建议**: 仅供开发者和高级用户测试使用

### Beta（公测版）

- **用途**: 功能基本完成，进行更广泛的测试
- **版本格式**: `X.Y.Z-beta.N` (例如：`0.1.0-beta.1`)
- **GitHub Release**: 标记为 prerelease
- **建议**: 可供愿意帮助测试的用户使用

### RC（候选版）

- **用途**: 发布候选版，即将成为稳定版
- **版本格式**: `X.Y.Z-rc.N` (例如：`1.0.0-rc.1`)
- **GitHub Release**: 标记为 prerelease
- **建议**: 适合最终验证和回归测试

## GitHub Actions 发布流程

### 触发发布

1. 访问 GitHub 仓库的 Actions 页面
2. 选择 "Release" 工作流
3. 点击 "Run workflow"
4. 配置以下参数：
   - **发布分支 (branch)**: 要发布的分支，通常是 `main`
   - **构建平台 (platform)**:
     - `all` - 所有平台（推荐用于正式发布）
     - `macos-aarch64` - macOS Apple Silicon
     - `macos-x86_64` - macOS Intel
     - `ubuntu-22.04` - Linux
     - `windows-latest` - Windows
   - **版本升级类型 (bump)**:
     - `patch` - 修复版本 (0.1.0 -> 0.1.1)
     - `minor` - 次版本 (0.1.0 -> 0.2.0)
     - `major` - 主版本 (0.1.0 -> 1.0.0)
   - **发布渠道 (channel)**:
     - `stable` - 稳定版
     - `alpha` - 内测版
     - `beta` - 公测版
     - `rc` - 候选版

5. 点击 "Run workflow" 开始发布

### 工作流执行步骤

1. **版本验证 (validate)**
   - 自动运行版本升级脚本
   - 提交版本更改到代码仓库
   - 检查标签是否已存在
   - 获取上一个版本的标签

2. **构建 (build)**
   - 根据选择的平台进行编译
   - 生成安装包（.dmg, .deb, .AppImage, .msi, .exe）
   - 生成签名文件（.sig）

3. **创建发布 (create-release)**
   - 创建 Git 标签
   - 生成发布说明（含桌面端直接下载链接，以及移动端仓库 [UniClipboard/UniClip](https://github.com/UniClipboard/UniClip) 的 iOS 公测与安卓下载链接；桌面预发布会链接到移动端最新预览版，正式发布会链接到移动端最新正式版）
   - 上传所有构建产物
   - 创建 GitHub Release 草稿
   - 把不可变安装包上传到现有 R2 路径
   - 通过 FlareRelease 登记 Release，并保持为待推广状态

## FlareRelease 发布边界

FlareRelease 是发布信息和 Channel 的唯一管理方。客户端继续访问 `release.uniclipboard.app`，安装包仍使用 `https://release.uniclipboard.app/artifacts/v<version>/<filename>`，这些公开地址没有变化。

Desktop 工作流完成构建后只做两件事：上传不可变安装包，然后通过受 Cloudflare Access 保护的管理接口登记 Release。登记成功后 Release 状态为 Ready，但不会自动改变 Stable 或 Alpha Channel。维护者确认后，必须在 FlareRelease 中显式 Promote。

CI 使用专门的 Cloudflare Access service token。以下凭据由 `UniClipboard` 组织的 GitHub Actions organization secrets 统一管理，并授权给 Desktop 与 UniClip 仓库：

- `FLARE_RELEASE_ACCESS_CLIENT_ID`
- `FLARE_RELEASE_ACCESS_CLIENT_SECRET`

发布流程仍通过 `secrets.*` 读取组织密钥，不需要在每个仓库重复创建同名 repository secrets。个人登录信息和通用 Cloudflare 管理令牌不得用于 Release 登记。

切换后，Desktop CI 不再写入 R2 中的 `manifests/*.json`、`release-notes/index/*.json` 或 GitHub Pages 的 Channel manifest。旧 R2 JSON 只作为迁移备份保留。`workers/update-server` 的部署入口已经停用，但代码会保留到生产验证完成且约定的回滚窗口结束；窗口内如需回退，使用 Cloudflare Worker version rollback，不重新启用两套长期并行的发布状态。

## GitCode 镜像

R2 始终是安装包的权威来源。已登记到 FlareRelease 的 Desktop 安装包可以额外拥有一份经校验的 GitCode/AtomGit 副本，中国大陆的下载请求会被重定向到该副本；公开的安装包地址和更新清单保持不变。镜像契约与 Mobile（线程 t-0153）共用同一套 FlareRelease `PUT /api/mirrors` / `POST /api/mirrors/revoke` 接口，但 GitCode 目标仓库不同：Desktop 镜像到 GitCode 上的 `UniClipboard/UniClipboard`，与 Mobile 的镜像仓库彼此独立。

**覆盖范围**：FlareRelease 的 `PUT /api/mirrors` 只会在已登记的 `(product, version, filename)` 三元组上生效——Desktop 每个平台目前只登记一个更新用安装包（macOS `.app.tar.gz`、Linux `.AppImage(.tar.gz)`、Windows `.nsis.zip`/`.exe`，见 `scripts/assemble-update-manifest.js` 的平台选择规则）。`.dmg`、`.deb`、`.rpm` 以及 Windows 便携版 zip 仍然只上传到 R2 和 GitHub Release，**不会** 被镜像，因为 FlareRelease 目前没有为它们登记制品记录。如果要覆盖这些安装包，需要先扩展 Desktop 的 FlareRelease 登记（`scripts/build-flare-release-registration.js`）把它们也加入 `artifacts` 数组——FlareRelease 的登记 schema 本身已支持任意数量的制品，不需要改 FlareRelease 代码。

**传输位置**：GitCode 的上传入口在中国大陆，GitHub 托管的 runner 出站到它只有几十 KiB/s，所以上传不在 CI 里直接做，而是由已发布的第三方 Action `mkdir700/gitcode-release-mirror`（工作流里固定到完整提交 SHA，当前为 v1.0.0 的 `5055ef17b7302e27ca53cbe5f6e44722f7e55f6b`）通过 SSH 登录上海中转机（专用无权限账号 `gitcode-mirror`，严格校验 host key）。机器上该账号的强制命令（`/opt/gitcode-release-mirror-1.0.0-5055ef1/host.mjs`）只接受一行 JSON 请求，并按机器上由 root 维护的白名单配置（`/etc/gitcode-release-mirror/config.json`）执行：只允许从 `https://release.uniclipboard.app/artifacts/` 下载、只允许 5 类更新包文件名（`.app.tar.gz`、`.AppImage`、`.AppImage.tar.gz`、`.nsis.zip`、`.exe`）、最大 300 MiB、只允许上传到 `UniClipboard/UniClipboard`。机器上不保存任何凭据，GitCode token 只随请求进入单个进程的内存；Action 随请求发送核心文件的 SHA-256，机器上的核心文件不一致会直接拒绝。机器侧文件由维护者一次性安装，工作流不会创建或修改机器上的任何文件。Mobile 仍使用 `/opt/uniclip-mirror/` 下自己的旧入口（`gitcode-mirror-host.py`、`mirror-android-apk-to-gitcode.mjs`，其 key 在 UniClip 仓库），两者互不影响。

**实现**：工作流用 `registration.json`（`scripts/build-flare-release-registration.js` 生成，含每个已登记安装包的 `sha256`）生成来源列表（R2 地址 + `sha256`）交给 Action。Action 对每个文件独立执行：主机从 R2 下载并校验 SHA-256（不一致则不接触 GitCode）→ 确保 GitCode 上存在该 tag 的 Release（只在不存在时创建，新建的预发布标为 prerelease）→ 已有同名文件则按字节比对决定复用或失败（**不覆盖、不删除**）→ 否则上传 → 匿名回读校验 size + SHA-256。单个文件失败不影响其余文件。Action 输出 `receipt`（每个文件 `mirrored`/`reused`/`failed`），随后 `scripts/register-flare-release-mirrors.mjs` 在 runner 上读取 receipt，对状态为 `mirrored`/`reused` 且摘要与登记一致的文件调用 `PUT /api/mirrors` 登记（FlareRelease 凭据不离开 GitHub），`failed` 的文件不登记。receipt 与登记结果作为 `gitcode-mirror-receipt-<tag>` artifact 保留。

**触发方式**：`mirror` environment 的部署分支策略只允许 `main`（见下表），而 `release.yml` 的 job 运行在发布 tag 这个 ref 上——如果像早期实现那样用 `uses: ./.github/workflows/mirror-desktop-gitcode.yml` 内联调用，mirror job 会在任何 step 执行前就被 environment protection rule 拒绝（`Tag "vX.Y.Z" is not allowed to deploy to mirror due to environment protection rules`），`non_blocking: true` 对这一层完全不起作用——它只能处理 job 已经开始跑之后、某个 step 内部的失败。v1.1.1（#1835）上实际触发过这个拒绝。

现在的触发链路改为"始终从受信的 main ref 发起"：
- **alpha**：`release.yml` 用 `GITHUB_TOKEN` 直接把 Release 创建为已发布状态，这类事件不会触发 `release.published`（GITHUB_TOKEN 产生的事件不会级联触发其他 workflow）。`release.yml` 的 `dispatch-mirror-alpha` job（与 `dispatch-copr-alpha`/`dispatch-snap`/`dispatch-npm-alpha` 同构）在 `create-release` 成功后，用 `gh api .../mirror-desktop-gitcode.yml/dispatches -f ref=main` 异步触发一次独立的 workflow run，不等待镜像完成。
- **stable / beta / rc**：这些渠道的 Release 先以 `draft: true` 创建，需要维护者在 GitHub UI 上手动点击发布，这才是真正的 `release.published` 事件（人工操作，会正常级联触发其他 workflow）。`mirror-desktop-gitcode.yml` 新增了 `on: release: types: [published]` 入口，由一个不声明 `environment` 的小 job（`redispatch-from-release`）接住这个事件，从 tag_name 推导 version/channel 后，同样用 `gh api .../dispatches -f ref=main` 重新发起一次独立 run。这同时解决了"引用的 ref 不是 main"和"draft 还没发布就去镜像"两个问题——只有真正发布后才会触发，且触发时的 job ref 已经是 main。
- 两条路径最终都落到同一个 `mirror` job（`environment: mirror`），该 job 本身只接受 `workflow_dispatch`/`workflow_call` 的显式 `inputs`（`if: github.event_name != 'release'`，避免被 `release` 事件直接选中）；镜像的制品始终是 `inputs.tag_name` 指向的、已发布的那个不可变 Release，不是 main 分支当前内容。两个自动入口都不传 `non_blocking`（沿用默认值 `false`）：它们各自都是独立的 workflow run，不会影响 `release.yml` 自身的结论，也不影响其他渠道，所以没有理由再把真实的镜像失败在它自己的 run 里也用 `continue-on-error` 悄悄降级成 `::warning`——一次真正的镜像失败现在会让那次 dispatch 出来的 run 本身失败，可见。`non_blocking: true` 仍然保留给手动 `workflow_dispatch` 的人工重跑场景按需使用。
- 该工作流也支持直接 `workflow_dispatch` 手动重跑或补镜像旧 tag（ref 必须选 `main`）——此时它会从 GitHub Release 重新下载安装包，并用仓库里相同的两个脚本重新计算登记 payload，再通过同一条 SSH 中转路径执行。`mirror` job 在实际下载/镜像之前会先校验这次要镜像的 Release 确实已发布（非 draft）、且 `tag_name`/`version`/`channel` 三者互相一致、`channel` 是 `stable`/`alpha`/`beta`/`rc` 之一——这层校验对自动入口和手动入口一视同仁，手动填错参数或指向一个还在 draft 的 release 会在这一步被直接拒绝，不会走到下载/上传。
- **GitHub 官方文档没有说明 `release` 事件具体用哪个版本的 workflow 文件**（只明确写了该事件的 `GITHUB_SHA`/`GITHUB_REF` 指向被打标签的那个 commit，不是 main；不能照抄其他没有天然关联 ref 的事件小节"文件必须在默认分支"这条规则去类比，那条规则的前提对 `release` 不成立）。因此不能假设"把这次修复合并到 main 之后，所有未来 tag 的 `release.published` 都自动用新版 workflow"——稳妥的假设是 **新 tag 自己的提交要已经包含这次修复**（先合并到 main，再从 main 切新 tag）。给一个历史 tag（提交本身不包含修复）补做任何依赖新逻辑的操作，唯一确定路径是显式 `workflow_dispatch`（ref=main）；不要移动或重打那个历史 tag。

**前置条件**：FlareRelease 的登记 payload 必须包含每个制品的 `sha256`（`scripts/build-flare-release-registration.js` 已经计算并发送）；`PUT /api/mirrors` 要求制品的已登记 `sha256` 非空且与镜像上传的字节一致，否则拒绝（`Mirror sha256 does not match the artifact`）。

**配置**：

| 名称 | 类型 | 状态 | 说明 |
| --- | --- | --- | --- |
| `GITCODE_RELEASE_TOKEN` | `UniClipboard` 组织 secret（selected repositories） | 已配置，已包含本仓库 | GitCode 机器人 token，与 Mobile 共用同一枚 token，分别用各自的 `GITCODE_OWNER`/`GITCODE_REPO` 指向不同镜像仓库 |
| `GITCODE_OWNER` / `GITCODE_REPO` | repository variable | 已配置为 `UniClipboard` / `UniClipboard` | 镜像仓库，仓库默认分支需要至少一个 commit |
| `GITCODE_API_BASE` | repository variable | 旧流程使用，当前工作流不读取 | 新流程使用 Action 的默认 `https://api.gitcode.com/api/v5` |
| `GITCODE_TARGET_COMMITISH` | repository variable | 旧流程使用，当前工作流不读取 | 新流程使用 Action 的默认 `main` |
| `FLARE_RELEASE_ACCESS_CLIENT_ID` / `_SECRET` | 已有的组织 secret | 已配置 | 与 Release 登记共用 |
| `mirror` GitHub Environment | repository environment，限制只允许 `main` 分支使用 | 已配置（2026-10-03 创建，`deployment_branch_policy` 自定义为仅 `main` 这一个 branch 类型策略，已用 `gh api repos/.../environments/mirror` 核实） | Mobile 的 `mirror-android-gitcode.yml` 已在用同名 environment；这个 main-only 策略是本次修复要依赖、而不是放宽的既有规则 |
| `GITCODE_MIRROR_ACTION_SSH_KEY` | environment secret（在 `mirror` environment 下） | 已配置（2026-10-10，专用新 key，仅绑定 Action 的强制命令；值不可读） | 与旧的 `MIRROR_SSH_KEY`（Desktop 专用，绑定已退役的旧中转脚本）是两把不同的 key；旧 key 在回填验证后按下面的退役步骤删除 |
| `MIRROR_SSH_HOST` / `MIRROR_SSH_KNOWN_HOSTS` / `GITCODE_MIRROR_ACTION_SSH_USER` | repository / environment variable | 已配置（`GITCODE_MIRROR_ACTION_SSH_USER=gitcode-mirror` 在 `mirror` environment 下） | 与 Mobile 相同的机器，不同的账号 |
| 机器上的 Action 入口 | 机器侧文件（`/opt/gitcode-release-mirror-1.0.0-5055ef1`、`/etc/gitcode-release-mirror`、账号 `gitcode-mirror`），由维护者一次性安装 | 已安装并用新 key 验证强制命令（拒绝无效请求、任意命令、pty） | 升级 Action 版本需要同时更新工作流里的 SHA 与机器上的文件 |

上面几项配置已就位。v1.1.1（#1835）的失败不是"缺少配置"，而是 environment protection rule 在任何 step 执行前就拒绝了整个 job——即上方描述的触发方式问题。GitCode 目标仓库必须是普通仓库：它曾是"镜像仓库"类型，此时创建 Release 会返回 `HTTP 400 … repository is an image repository`（2026-10-10 在冒烟测试中观察到，关闭镜像后恢复）。新流程已用 `mkdir700/gitcode-release-mirror` 的冒烟测试（`.github/workflows/gitcode-release-mirror-smoke.yml`，见 `docs/ci/gitcode-release-mirror-action-smoke.md`）在真实 runner、真实机器、真实 GitCode 上验证；使用真实安装包大小的完整镜像仍需第一次真实运行确认。

**旧入口退役（回填验证通过后，各步骤需单独批准）**：旧的 Desktop 专用中转已不再被工作流使用。待新路径对真实发布验证通过后：删除本仓库 `mirror` environment 中的 `MIRROR_SSH_KEY`；在机器上删除 `/home/uniclip-mirror/.ssh/authorized_keys` 里 `gitcode-mirror-host-desktop.py` 那一行（注释 `uniclip-mirror-desktop-ci`），以及 `/opt/uniclip-mirror/gitcode-mirror-host-desktop.py`、`/opt/uniclip-mirror/mirror-desktop-installers-to-gitcode.mjs`。Mobile 的那一行（`gitcode-mirror-host.py`）和 `gitcode-mirror-host.py`、`mirror-android-apk-to-gitcode.mjs` 不能动。

**已知限制**（与 Mobile 一致）：302 重定向发生后服务器无法补救，镜像失败时客户端若不自动回退需手动切换下载源；撤回或下架只会停止重定向，不能召回已分享出去的镜像链接；GitCode 附件的大小上限未知，Desktop 安装包可能比 Mobile 的 APK 更大，第一次真实上传才能验证是否可行；一次 SSH 会话要串行传输本次发布的全部已登记安装包（通常 5 个），单个会话的总耗时会明显长于 Mobile 的单文件会话，具体时长同样需要第一次真实运行才能确定。

### 完成发布

工作流执行完成后：

1. 访问仓库的 [Releases](https://github.com/your-repo/releases) 页面
2. 找到新创建的草稿版本
3. 编辑发布说明，补充更新内容
4. 确认无误后，点击 "Publish release" 发布
5. 在 FlareRelease 中检查新版本为 Ready，并显式 Promote 到目标 Channel

## 版本升级策略

### Patch 版本 (X.Y.Z -> X.Y.Z+1)

适用于：

- Bug 修复
- 安全补丁
- 小的性能改进
- 文档更新

### Minor 版本 (X.Y.Z -> X.Y+1.0)

适用于：

- 新增功能
- 功能改进
- API 新增（保持向后兼容）
- 依赖库重要更新

### Major 版本 (X.Y.Z -> X+1.0.0)

适用于：

- 破坏性变更
- 架构重构
- 重要里程碑
- API 不兼容变更

## 发布示例

### 场景 1: 发布第一个 alpha 版本

```bash
# 本地测试
bun run version:bump --type patch --channel alpha --dry-run

# 确认无误后执行
bun run version:bump --type patch --channel alpha

# 提交并推送
git add .
git commit -m "chore: prepare alpha release"
git push

# 在 GitHub Actions 触发发布
# branch: main
# platform: all
# bump: patch
# channel: alpha
```

结果：`0.1.0` -> `0.1.0-alpha.1`

### 场景 2: 继续发布 alpha 版本

如果当前版本是 `0.1.0-alpha.1`，继续使用相同参数：

```bash
bun run version:bump --type patch --channel alpha
```

结果：`0.1.0-alpha.1` -> `0.1.0-alpha.2`

如果希望从稳定版直接到指定预发布号（例如 `0.1.0` -> `0.1.0-alpha.2`）：

```bash
bun run version:bump --to 0.1.0-alpha.2
```

### 场景 3: Alpha 测试完成，发布稳定版

```bash
bun run version:bump --type patch --channel stable
```

结果：`0.1.0-alpha.5` -> `0.1.0`

### 场景 4: 发布新的 minor 版本

```bash
bun run version:bump --type minor --channel stable
```

结果：`0.1.5` -> `0.2.0`

## 安装包命名规则

- macOS ARM64: `UniClipboard_X.Y.Z_aarch64.dmg`
- macOS Intel: `UniClipboard_X.Y.Z_x64.dmg`
- Linux Debian: `uniclipboard_X.Y.Z_amd64.deb`
- Linux AppImage: `uniclipboard_X.Y.Z_amd64.AppImage`
- Windows NSIS: `UniClipboard_X.Y.Z_x64-setup.exe`

所有安装包都附带 `.sig` 签名文件用于验证。

**注意**: Windows 使用 NSIS 安装程序而不是 MSI，因为 NSIS 支持完整的语义化版本号（包括预发布标识如 `-alpha.1`），而 MSI 只支持纯数字版本号。

## 故障排除

### 版本号格式错误

确保版本号符合语义化版本规范：

- 稳定版：`X.Y.Z` (例如 `1.0.0`)
- 预发布：`X.Y.Z-channel.N` (例如 `1.0.0-alpha.1`)

### 标签已存在

如果工作流提示标签已存在，说明该版本已经发布过。请更新版本号后重试。

### 构建缓存维护

- 桌面与 CLI 的 GitHub 临时构建机器在缓存恢复前只保留项目固定的 Rust 工具链。
  rust-cache 会把所有已安装编译器纳入匹配；清除未使用的预装工具链，可以避免其补丁升级造成无关的缓存失配。
  该操作拒绝在本机和自托管 runner 执行。首次切换到这套稳定匹配条件需要重新生成缓存。
- `build.yml` 手动构建默认采用 `build_mode=test`：只降低编译优化成本，
  保留 release 的安全功能开关、panic 策略和调试符号，适合功能验证。
  需要正式优化或测量正式版运行性能时选择 `build_mode=release`。
  发布工作流的可复用调用仍默认 `release`，`Cargo.toml` 的正式配置不变。
  测试缓存、上传产物和 Windows 免安装包带 `-test` 标识，不覆盖正式缓存。
- 2026-09-09 同提交 Windows x64 对照：依赖缓存命中时，正式优化 23m24s，
  快速模式 8m47s；首次无测试缓存仍为 22m39s。
  保留应用自身产物的实验没有避免应用重编，故不启用。
  详细数据见 [Windows 构建对照记录](ci/windows-build-benchmarks.md)。
- `build.yml` 的手动构建默认保存缓存；可用 `save_cache=false` 关闭。
  被其他发布工作流调用时仍默认只读，main 上的构建继续保存缓存。
- `cache-warmup.yml` 每周以及构建模式配置合入 main 后，预热 Windows x64 正式版与测试版、Windows CLI、Rust 检查、
  覆盖率和文档依赖。其他发布平台仍可正常构建，但不再同时预热全部平台，
  避免缓存总量超过仓库默认容量。
- 覆盖率检查关闭工具链安装步骤自带的缓存，由显式缓存步骤统一管理；
  PR 只读取已有缓存，main 和预热工作流负责写入。
- `cache-maintenance.yml` 每日、相关构建结束后及预热之前清理缓存。
  只处理已知的 Rust、Bun 和 CodeQL 缓存：删除旧 PR 覆盖率缓存、已结束 PR 的缓存，
  并对每个分支、用途和编译环境只保留最新一份。
  总量超过 8 GiB 时按最近使用时间清理，为下一次上传预留空间；
  优先保留 main 最新的 Windows x64 正式版和测试版构建缓存。未知用途的缓存不会自动删除。
- 缓存维护只处理 Actions 缓存，不删除构建产物或 Release 文件。
  手动触发 Cache Maintenance 默认仅预览；命令行等效操作：

```bash
GH_REPO=UniClipboard/UniClipboard node scripts/ci/maintain-actions-cache.mjs
# 检查预览结果后执行相同规则
GH_REPO=UniClipboard/UniClipboard node scripts/ci/maintain-actions-cache.mjs --apply
```

### 构建失败

1. 检查构建日志中的错误信息
2. 确认代码在本地可以正常编译
3. 检查依赖项是否有问题
4. 必要时重新运行工作流

## 相关文件

- 版本管理脚本：[`scripts/bump-version.js`](../scripts/bump-version.js)
- Codex changelog 提示词：[`.github/prompts/release-changelog.codex.md`](../.github/prompts/release-changelog.codex.md)
- Changelog 写作规则：[`docs/CHANGELOG_TEMPLATE.md`](./CHANGELOG_TEMPLATE.md)
- 发布工作流：[`.github/workflows/release.yml`](../.github/workflows/release.yml)
- 预发布准备工作流：[`.github/workflows/prepare-release.yml`](../.github/workflows/prepare-release.yml)
- 构建工作流：[`.github/workflows/build.yml`](../.github/workflows/build.yml)
- 发布控制服务：[`UniClipboard/FlareRelease`](https://github.com/UniClipboard/FlareRelease)

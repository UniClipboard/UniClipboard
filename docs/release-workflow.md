# Release Workflow

本文档说明如何使用项目的版本管理和发布系统。

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
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`

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

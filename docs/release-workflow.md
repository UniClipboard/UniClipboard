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
- **版本格式**: `X.Y.Z` (例如: `1.0.0`)
- **GitHub Release**: 标记为正式版本（非 prerelease）

### Alpha（内测版）

- **用途**: 早期功能测试，可能包含未完成的功能或已知问题
- **版本格式**: `X.Y.Z-alpha.N` (例如: `0.1.0-alpha.1`)
- **GitHub Release**: 标记为 prerelease，带有警告说明
- **建议**: 仅供开发者和高级用户测试使用

### Beta（公测版）

- **用途**: 功能基本完成，进行更广泛的测试
- **版本格式**: `X.Y.Z-beta.N` (例如: `0.1.0-beta.1`)
- **GitHub Release**: 标记为 prerelease
- **建议**: 可供愿意帮助测试的用户使用

### RC（候选版）

- **用途**: 发布候选版，即将成为稳定版
- **版本格式**: `X.Y.Z-rc.N` (例如: `1.0.0-rc.1`)
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
   - 生成发布说明（包含直接下载链接）
   - 上传所有构建产物
   - 创建 GitHub Release 草稿

### 完成发布

工作流执行完成后：

1. 访问仓库的 [Releases](https://github.com/your-repo/releases) 页面
2. 找到新创建的草稿版本
3. 编辑发布说明，补充更新内容
4. 确认无误后，点击 "Publish release" 发布

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

结果: `0.1.0` -> `0.1.0-alpha.1`

### 场景 2: 继续发布 alpha 版本

如果当前版本是 `0.1.0-alpha.1`，继续使用相同参数：

```bash
bun run version:bump --type patch --channel alpha
```

结果: `0.1.0-alpha.1` -> `0.1.0-alpha.2`

如果希望从稳定版直接到指定预发布号（例如 `0.1.0` -> `0.1.0-alpha.2`）：

```bash
bun run version:bump --to 0.1.0-alpha.2
```

### 场景 3: Alpha 测试完成，发布稳定版

```bash
bun run version:bump --type patch --channel stable
```

结果: `0.1.0-alpha.5` -> `0.1.0`

### 场景 4: 发布新的 minor 版本

```bash
bun run version:bump --type minor --channel stable
```

结果: `0.1.5` -> `0.2.0`

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

- 稳定版: `X.Y.Z` (例如 `1.0.0`)
- 预发布: `X.Y.Z-channel.N` (例如 `1.0.0-alpha.1`)

### 标签已存在

如果工作流提示标签已存在，说明该版本已经发布过。请更新版本号后重试。

### 构建失败

1. 检查构建日志中的错误信息
2. 确认代码在本地可以正常编译
3. 检查依赖项是否有问题
4. 必要时重新运行工作流

## 相关文件

- 版本管理脚本: [`scripts/bump-version.js`](../scripts/bump-version.js)
- Codex changelog 提示词: [`.github/prompts/release-changelog.codex.md`](../.github/prompts/release-changelog.codex.md)
- Changelog 写作规则: [`docs/CHANGELOG_TEMPLATE.md`](./CHANGELOG_TEMPLATE.md)
- 发布工作流: [`.github/workflows/release.yml`](../.github/workflows/release.yml)
- 预发布准备工作流: [`.github/workflows/prepare-release.yml`](../.github/workflows/prepare-release.yml)
- 构建工作流: [`.github/workflows/build.yml`](../.github/workflows/build.yml)

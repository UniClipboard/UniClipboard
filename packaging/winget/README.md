# winget 打包

本目录是提交到 [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) 的 manifest 源（3 文件 schema）：

- `UniClipboard.UniClipboard.installer.yaml`
- `UniClipboard.UniClipboard.locale.en-US.yaml`
- `UniClipboard.UniClipboard.yaml`（version）

最终落在 `manifests/u/UniClipboard/UniClipboard/0.15.0/`。

## 重要：winget 不进 Repology

已用 7zip 在 Repology 的完整源列表验证过——**Repology 不抓 winget**。所以这一步 **不会让 packaging-status 徽章变化**。做它的理由是补上 Windows 官方包管理器的安装入口：
```powershell
winget install uniclipboard
```

## 推荐路径：用 wingetcreate（自动算 hash + 自动提 PR）

手写 3 个 yaml 容易在 hash 和格式上出错。微软官方工具 `wingetcreate` 会下载安装器、自动算 sha256、生成 manifest 并直接提 PR——本目录的 yaml 仅作结构参考 / 手动 fallback。

```powershell
winget install wingetcreate

# 首次创建：交互式，自动下载两个 setup.exe 算 hash
wingetcreate new `
  https://github.com/UniClipboard/UniClipboard/releases/download/v0.15.0/UniClipboard_0.15.0_x64-setup.exe `
  https://github.com/UniClipboard/UniClipboard/releases/download/v0.15.0/UniClipboard_0.15.0_arm64-setup.exe

# 校验 + 提交（需 GitHub token，会自动 fork + 开 PR）
wingetcreate submit --token <GITHUB_TOKEN>
```

后续版本升级更省事：
```powershell
wingetcreate update UniClipboard.UniClipboard `
  --version <new> `
  --urls <x64-url> <arm64-url> `
  --submit --token <GITHUB_TOKEN>
```

## 手动路径（若不用 wingetcreate）

1. 把 `installer.yaml` 里两个 `InstallerSha256` 的全 `0` 占位换成真值（从 release `SHA256SUMS.txt` 取，或 `Get-FileHash <exe> -Algorithm SHA256`）。不要用本仓库环境产出的 hash。
2. 本地校验：
   ```powershell
   winget validate --manifest packaging\winget
   # 启用本地 manifest 安装测试（需开发者模式）
   winget settings --enable LocalManifestFiles
   winget install --manifest packaging\winget
   ```
3. 把三个文件复制到 winget-pkgs 的 `manifests/u/UniClipboard/UniClipboard/0.15.0/`，commit 开 PR。

## 后续版本自动化（CI）

首版人工提交后，`.github/workflows/winget-publish.yml` 会在每个 stable release 发布后用 wingetcreate 自动提 PR 到 `microsoft/winget-pkgs`。需在仓库配置 `WINGET_TOKEN`（GitHub PAT，classic，`public_repo`）。注意 wingetcreate `update` 要求包已存在——**首版必须先手动**（上面的 `wingetcreate new`），workflow 从第二版接管。

## 待确认

- **`Scope: user`**：依据 Tauri NSIS 默认 `currentUser` 安装模式。若实际打的是 perMachine 安装器，改成 `machine`。
- **升级匹配**：若 winget 升级检测不到已装版本，按 validation 提示补 `AppsAndFeaturesEntries`（DisplayName / ProductCode）。
- **未在 Windows 实测**：manifest 经事实校对（URL、installer 类型、locale 元数据），安装需你在 Windows 上验证。

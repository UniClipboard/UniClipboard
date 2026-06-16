# Chocolatey 打包

本目录是提交到 [Chocolatey Community Repository](https://community.chocolatey.org) 的包源：

- `uniclipboard.nuspec` — 包元数据
- `tools/chocolateyInstall.ps1` — 下载 NSIS 安装器并静默安装（`/S`）
- `tools/chocolateyUninstall.ps1` — 静默卸载
- `tools/VERIFICATION.txt` — 来源与校验说明

## 为什么进 Chocolatey

Repology 抓取 Chocolatey（源名 `chocolatey`），合并后徽章 +1 行。安装方式：
```powershell
choco install uniclipboard
```

> 提醒：Chocolatey community repo 有 **人工 moderation**，新包从提交到上架通常要数天到一两周，比 nixpkgs/Scoop 慢。元数据不全或 checksum 不对会被打回。

## 提交步骤

1. **填 checksum**（`chocolateyInstall.ps1` 里 `checksum64` 是 `REPLACE_WITH_SHA256` 占位）

   从 release 的 `SHA256SUMS.txt`（minisign 签名）取，或：
   ```powershell
   Get-RemoteChecksum https://github.com/UniClipboard/UniClipboard/releases/download/v0.15.0/UniClipboard_0.15.0_x64-setup.exe
   ```
   > 不要用本仓库环境产出的 hash——沙箱输出不可信。

2. **打包并本地实测**（必做）
   ```powershell
   cd packaging\chocolatey
   choco pack
   choco install uniclipboard --source . --yes   # 装、起、托盘、配对同步
   choco uninstall uniclipboard --yes             # 验证卸载干净
   ```

3. **推送到 community repo**
   ```powershell
   # 先在 community.chocolatey.org 注册账号，拿 API key
   choco apikey --key <YOUR_API_KEY> --source https://push.chocolatey.org/
   choco push uniclipboard.0.15.0.nupkg --source https://push.chocolatey.org/
   ```

4. **等 moderation**：自动校验（virus scan、安装测试）+ 人工 review。按 moderator 反馈改，直到 Approved。

5. **上架后**：Repology 下次抓取会显示 `chocolatey` 行。**后续版本由 CI 自动 push**——`.github/workflows/choco-publish.yml` 在每个 stable release 发布后下载安装器、算 checksum（与 `SHA256SUMS.txt` 交叉核对）、回填并 `choco push`。需在仓库配置 `CHOCOLATEY_API_KEY` secret。首版人工过 moderation 后即免手动。

## 待确认

- **WebView2 依赖**：`nuspec` 里以 XML 注释预留了 `<dependency>`，默认未启用。Win10/11 一般预装；若要强制，先确认 community 上 WebView2 的确切包 id（`webview2-runtime` 还是 `microsoft-edge-webview2-runtime`）再放开注释，避免引用不存在的包导致安装失败。
- **仅 x64**：ARM64 Windows 通过 x64 模拟运行该安装器。如需原生 ARM64，在 `chocolateyInstall.ps1` 增补 arm64 的 url/checksum。
- **未在 Windows 实测**：脚本经事实校对（URL、exe 名、silent 参数），安装/卸载需你在 Windows 上验证。

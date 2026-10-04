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

> **历史**：首版 0.1.2（2024-10-10 提交）于 2024-10-18 被拒，原因见
> [review 历史页](https://community.chocolatey.org/packages/uniclipboard/0.1.2)：
> `releaseNotes` 指向的 URL 当时不可达（Guideline，`cpmr0056`），另有
> `iconUrl`/`packageSourceUrl`/`projectSourceUrl` 几条 Guideline 未填。后三条已在
> 当前 nuspec 补齐；`releaseNotes` 现指向实际存在的 release 页，由下面步骤 1
> 在每次改版本时一并核对。拒绝后一直没人重新提交，`choco-publish.yml` 这个
> "后续版本自动 push" 的 CI 因此从未真正生效过——它依赖一个已被 approve 的首版。

## 提交步骤

1. **核对 nuspec 版本与 checksum**（`chocolateyInstall.ps1` 的 `checksum64`）

   从 release 的 `SHA256SUMS.txt`（minisign 签名）取，或：
   ```powershell
   Get-RemoteChecksum https://github.com/UniClipboard/UniClipboard/releases/download/v1.1.0/UniClipboard_1.1.0_x64-setup.exe
   ```
   > 不要用本仓库环境产出的 hash——沙箱输出不可信。同时确认 `uniclipboard.nuspec`
   > 的 `<version>`、`<releaseNotes>`（release 页必须已发布、非 draft）与
   > `VERIFICATION.txt` 里的版本号一致。

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
   choco push uniclipboard.1.1.0.nupkg --source https://push.chocolatey.org/
   ```

4. **等 moderation**：自动校验（virus scan、安装测试）+ 人工 review。按 moderator 反馈改，直到 Approved。

5. **上架后**：Repology 下次抓取会显示 `chocolatey` 行。**后续版本由 CI 自动 push**——`.github/workflows/choco-publish.yml` 在每个 stable release 发布后下载安装器、算 checksum（与 `SHA256SUMS.txt` 交叉核对）、回填并 `choco push`。需在仓库配置 `CHOCOLATEY_API_KEY` secret。首版人工过 moderation 后即免手动。

## 待确认

- **WebView2 依赖**：`nuspec` 里以 XML 注释预留了 `<dependency>`，默认未启用。Win10/11 一般预装；若要强制，先确认 community 上 WebView2 的确切包 id（`webview2-runtime` 还是 `microsoft-edge-webview2-runtime`）再放开注释，避免引用不存在的包导致安装失败。
- **仅 x64**：ARM64 Windows 通过 x64 模拟运行该安装器。如需原生 ARM64，在 `chocolateyInstall.ps1` 增补 arm64 的 url/checksum。
- **已在 Windows 实测**（t-0174，`choco` v2.3.0）：`choco pack` → `choco install uniclipboard --source . --yes`（下载安装器、hash 校验通过、注册表唯一一条 `UniClipboard 1.1.0` 卸载项）→ `choco uninstall uniclipboard --yes`（卸载成功，注册表项清除，`choco list` 不再列出）。
  遗留问题（不在本包脚本范围内）：卸载后 `%LocalAppData%\UniClipboard\uniclipboard-daemon.exe` 未被清除——这是应用自身 NSIS 卸载程序未跟踪该 sidecar 文件，Chocolatey 卸载脚本只是转调它；需要另开 Desktop 主应用的 issue 跟踪，不是这个包的问题。

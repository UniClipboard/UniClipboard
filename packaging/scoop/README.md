# Scoop 打包

`uniclipboard.json` 是提交到 [ScoopInstaller/Extras](https://github.com/ScoopInstaller/Extras) bucket 的 manifest 源。UniClipboard 是 GUI 应用，归 `Extras` bucket（`Main` 只收命令行工具）。

## 为什么进 Scoop

Repology 抓取 Scoop 官方 bucket（源名 `scoop`）。合并后徽章 +1 行，且这是 Windows 用户最省力的安装方式：
```powershell
scoop bucket add extras
scoop install uniclipboard
```

## manifest 要点

- **portable 模式**：用 `*-portable.zip`（内含 `UniClipboard.exe` + 必需的后台服务 `uniclipd.exe` + `portable.dat` 标记 + README）。zip 用 `Compress-Archive '$STAGE/*'` 打包，文件在根目录，因此 **不需要 `extract_dir`**。
- **`"persist": "data"`**：`portable.dat` 让应用把加密数据写到 exe 同目录的 `data/`。Scoop 把它持久化到 `~/scoop/persist/uniclipboard/data`，升级换版本目录时数据不丢。
- **`checkver` + `autoupdate`**：已配置跟随 GitHub Release。后续版本由 Extras 的 excavator bot 自动开升级 PR，无需手动维护。

## 提交步骤

1. **fork 并 clone Extras**
   ```powershell
   git clone https://github.com/<你的账号>/Extras
   cd Extras
   git checkout -b uniclipboard
   copy <本仓库>\packaging\scoop\uniclipboard.json bucket\uniclipboard.json
   ```

2. **填 hash**（manifest 里两个架构的 `hash` 目前是全 `0` 占位）

   用 Extras 自带工具自动下载 zip、算 sha256、顺带校验 `autoupdate` URL，一步到位：
   ```powershell
   .\bin\checkver.ps1 uniclipboard . -Update
   ```
   > 不要手填 hash——本仓库环境无法生成可信哈希，上面这条命令会用官方工具算真值替换占位。也可与 release 自带的 `SHA256SUMS.txt`（minisign 签名）交叉核对。

3. **本地验证**（必做）
   ```powershell
   scoop install .\bucket\uniclipboard.json
   # 验证：能装、能起、托盘正常、能配对同步、scoop uninstall 干净
   .\bin\formatjson.ps1 uniclipboard .     # 统一格式
   .\bin\checkurls.ps1   uniclipboard .     # URL 可达性
   ```

4. **提交并开 PR**（Extras 的 commit 规范）
   ```powershell
   git add bucket\uniclipboard.json
   git commit -m "uniclipboard: Add version 0.15.0"
   git push -u origin uniclipboard
   ```
   base 选 `ScoopInstaller/Extras:master`。

5. **合并后**：Repology 下次抓取（通常几天内）会显示 `scoop` 行。

## 已知缺口

- **WebView2 依赖**：靠 `notes` 提示。Win10/11 一般预装；如需强约束可后续改 `suggest`/`depends`，但要先确认 Scoop 里 WebView2 包名再写，避免引用不存在的依赖。
- **未在 Windows 实测**：manifest 经事实校对（zip 结构、exe 名、URL 均来自仓库真实值），但安装/运行需你在 Windows 上验证。

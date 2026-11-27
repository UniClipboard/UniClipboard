<div align="center">

<img src="app/desktop/internal/appicons/unic-logo.svg" alt="Unic" width="112" height="112" />

# 讓每一次複製更有用

<p>
  <strong>Unic（原名 UniClipboard）</strong> 是一款原生、本機優先的剪貼簿管理與跨裝置同步工具，涵蓋 Windows、macOS、Linux、iOS、Android 與終端機命令列。<br />
  結合本機歷史搜尋、快速面板、智慧預覽、外掛擴充與端對端直連同步 —— 無需帳號，不經雲端。
</p>

<p>
  <a href="https://uniclipboard.app"><img alt="官網" src="https://img.shields.io/badge/Website-0891B2?style=for-the-badge" /></a>
  <a href="https://docs.uniclipboard.app/zh"><img alt="文件" src="https://img.shields.io/badge/Docs-4F46E5?style=for-the-badge&logo=readthedocs&logoColor=white" /></a>
  <a href="#community"><img alt="WeChat 群組" src="https://img.shields.io/badge/WeChat-07C160?style=for-the-badge&logo=wechat&logoColor=white" /></a>
  <a href="#community"><img alt="QQ 群組" src="https://img.shields.io/badge/QQ-1688D8?style=for-the-badge&logo=qq&logoColor=white" /></a>
  <a href="https://t.me/uniclipboard"><img alt="Telegram 群組" src="https://img.shields.io/badge/Telegram-26A5E4?style=for-the-badge&logo=telegram&logoColor=white" /></a>
</p>

<p>
  <a href="https://github.com/UniClipboard/UniClipboard/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/UniClipboard/UniClipboard?style=flat-square&color=E5B650&logo=github&label=Stars" /></a>
  <a href="LICENSE"><img alt="AGPL-3.0 授權" src="https://img.shields.io/badge/License-AGPL--3.0-22c55e?style=flat-square" /></a>
</p>

<p>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="Windows" src="https://img.shields.io/badge/Windows-1f2937?style=flat-square&logo=data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAxNiAxNiIgZmlsbD0iIzM4YmRmOCI+PHBhdGggZD0iTTAgMGg3LjR2Ny40SDB6bTguNiAwSDE2djcuNEg4LjZ6TTAgOC42aDcuNFYxNkgwem04LjYgMEgxNlYxNkg4LjZ6Ii8+PC9zdmc+" /></a>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="macOS" src="https://img.shields.io/badge/macOS-1f2937?style=flat-square&logo=apple&logoColor=white" /></a>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="Linux" src="https://img.shields.io/badge/Linux-1f2937?style=flat-square&logo=linux&logoColor=facc15" /></a>
  <a href="https://testflight.apple.com/join/nyNQ8dQe"><img alt="iOS Beta" src="https://img.shields.io/badge/iOS-Beta-2563eb?style=flat-square&logo=apple&logoColor=white&labelColor=1f2937" /></a>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="Android" src="https://img.shields.io/badge/Android-1f2937?style=flat-square&logo=android&logoColor=3ddc84" /></a>
  <a href="#cli"><img alt="CLI" src="https://img.shields.io/badge/CLI-1f2937?style=flat-square&logo=gnubash&logoColor=white" /></a>
</p>

<p>
  <small>
    <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | <strong>繁體中文</strong> | <a href="README.ja.md">日本語</a> | <a href="README.pt-BR.md">Português (Brasil)</a> | <a href="README.ru.md">Русский</a>
  </small>
</p>

![Unic 跨裝置介面展示（桌面、平板與手機）](docs/assets/product-overview.png)

</div>

---

## 為什麼選擇 Unic

- **不只於同步，單機同樣順手**：多數剪貼簿管理器受限於單一系統或綁定付費雲端同步，而跨裝置傳輸工具往往缺少歷史檢索與快速貼上。Unic 即使只裝在一台裝置上也很好用——隨叫隨到的快速面板、本機歷史搜尋、自動分類與智慧連結預覽；連接多台裝置後，文字、圖片、檔案乃至整個資料夾都能在 Windows、macOS、Linux、iOS、Android 與終端機之間無縫同步，同區域網路或跨網路皆可用。
- **原生架構，不套殼 Electron**：以 Go 核心與原生桌面介面打造，不內建瀏覽器引擎。主視窗以獨立程序運作，關閉視窗即結束主介面程序；背景服務（`unicd`）本身不含介面相依，也能在無桌面環境的機器上獨立執行。
- **架構級隱私，不靠口頭承諾**：剪貼簿裡經常存著密碼、金鑰與私人檔案。Unic 無需註冊帳號、不設雲端儲存——裝置經指紋核對後端對端加密直連，移除裝置立即斷權，剪貼簿歷史始終只留在你自己的裝置上。
- **可擴充的剪貼簿工作流，不只複製貼上**：內建自動分類、可依網頁標題與簡介搜尋的智慧連結預覽、不覆寫原紀錄的 **Action**（去除格式、文字轉換、將圖片或檔案轉成臨時分享連結），支援從外掛市集一鍵安裝 **Go / Lua 外掛**，並提供完整命令列工具（`unic`），適用於終端機、SSH、指令碼與免配對的 6 位數取件碼臨時傳送。

<a id="features"></a>

## 功能

<table>
  <tr>
    <td>⚡ <strong>快速面板</strong></td>
    <td>在任何應用程式中按 <code>⌘ ⌃ V</code>（Windows / Linux 上是 <code>Ctrl Alt V</code>）開啟歷史搜尋，按 <code>Enter</code> 直接貼上，或按 <code>Shift+Enter</code> 以純文字貼上。</td>
  </tr>
  <tr>
    <td>🔍 <strong>快速搜尋</strong></td>
    <td>輸入關鍵字，或用 <code>#標籤</code>、<code>@裝置</code>、<code>/類型</code> 與收藏篩選。幾個月前複製的內容也找得回來，連結亦支援依網頁標題與簡介搜尋。</td>
  </tr>
  <tr>
    <td>🏷️ <strong>自動分類與智慧預覽</strong></td>
    <td>自動為連結、電子郵件、色碼、版本號、UUID、IP 位址等內容加上標籤（支援自訂正規表示式規則），並為複製的連結自動產生預覽卡片。</td>
  </tr>
  <tr>
    <td>🔄 <strong>跨裝置同步與資料夾</strong></td>
    <td>在已配對裝置間同步文字、富文字、圖片、檔案與完整保留目錄結構的資料夾，可同步給所有裝置或指定單台裝置。</td>
  </tr>
  <tr>
    <td>📮 <strong>免配對傳送</strong></td>
    <td>執行 <code>unic send "內容"</code> 會顯示傳輸連結。取件碼可用時，也會顯示 6 位數取件碼。接收方可使用任一方式接收，不會留下裝置紀錄。</td>
  </tr>
  <tr>
    <td>🧩 <strong>Action 與外掛擴充</strong></td>
    <td>執行不改寫原項目的 Action（文字轉換、去除格式、把圖片或檔案轉成分享連結），並可在設定中一鍵安裝 Go 或 Lua 外掛。</td>
  </tr>
  <tr>
    <td>⌨️ <strong>命令列</strong></td>
    <td>用 <code>unic</code> 在終端機、SSH 和指令碼中傳送、接收、執行 Action 及篩選歷史清單。</td>
  </tr>
  <tr>
    <td>🌍 <strong>多語言與主題</strong></td>
    <td>English、简体中文、日本語、Português、Русский，支援淺色與深色模式。</td>
  </tr>
</table>

<a id="quick-start"></a>

## 三步開始

**1. 下載並安裝**

- **桌面版**：從 [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases) 下載 Windows（`.exe`/`.zip`）、macOS（`.dmg`）或 Linux（`.deb`/`.rpm`/`.AppImage`）安裝檔。
- **iOS**：透過 [TestFlight](https://testflight.apple.com/join/nyNQ8dQe) 加入公開 Beta 測試。
- **Android**：從 [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases) 下載 APK 安裝（目前暫無上架 Google Play 的計畫）。
- **僅命令列**：執行 `npm install -g @uniclipboard/cli`。

[![Packaging status](https://repology.org/badge/vertical-allrepos/uniclipboard.svg)](https://repology.org/project/uniclipboard/versions)

**2. 立即開始使用**

照常複製內容，在任意應用程式中按 `⌘ ⌃ V`（Windows / Linux 上按 `Ctrl Alt V`）呼叫快速面板，搜尋歷史並直接貼上。

**3. 連接第二台裝置（選用）**

在第一台裝置上開啟 **裝置 → 新增裝置**，記下 6 位數配對碼，然後在第二台裝置上輸入。兩台裝置顯示的指紋一致後，在兩邊分別點擊 **接受**，之後複製的內容便會自動同步。

<a id="cli"></a>

## 命令列

```sh
unic send --to all "Hello"        # 傳送給所有裝置
unic send --to all -f ./photos    # 傳送檔案或資料夾
unic send "臨時內容"               # 免配對，顯示傳輸連結，並在可用時顯示取件碼
unic recv 482913                  # 用取件碼接收
unic history list                 # 查看歷史
```

完整指令請見 [命令列文件](app/service/README.md)。

## 隱私

- 裝置之間端對端加密傳輸，Unic 不經營儲存你剪貼簿內容的伺服器。
- 每台新裝置都必須由你核對指紋並手動核准。移除的裝置會立即失去存取權。
- 剪貼簿內容、檔案路徑和裝置位址不會寫入日誌。
- 匿名診斷資料不包含剪貼簿內容和裝置身分，可隨時在 **設定 → 診斷與隱私** 中關閉。

如發現安全問題，請依 [SECURITY.md](SECURITY.md) 私下回報。

## 從 UniClipboard v1 升級

v2 可以匯入 v1 的全部歷史紀錄，包括文字、圖片、檔案和標籤。第一次開啟 v2 時依照引導操作即可，也可以稍後在 **設定 → 儲存空間 → 從 v1 遷移** 中匯入。

v2 裝置無法與 v1 裝置同步，請把所有裝置都升級到 v2 後重新配對。v1 的程式碼保留在 [`v1` 分支](https://github.com/UniClipboard/UniClipboard/tree/v1)。

<a id="community"></a>

## 社群交流

歡迎加入 Unic 社群，與其他使用者及開發者交流心得、提出建議或回報問題：

- <img src="docs/assets/community/qq.svg" alt="" width="18" /> QQ 群組：`1080860954`
- <img src="docs/assets/community/wechat.svg" alt="" width="18" /> WeChat 群組：掃描下方 QR 碼加入維護者好友以邀請入群
- <img src="docs/assets/community/telegram.svg" alt="" width="18" /> [Telegram 群組 (@uniclipboard)](https://t.me/uniclipboard)

<table>
  <tr>
    <td align="center"><strong><img src="docs/assets/community/qq.svg" alt="" width="18" /> QQ 群組</strong></td>
    <td align="center"><strong><img src="docs/assets/community/wechat.svg" alt="" width="18" /> WeChat 群組</strong></td>
    <td align="center"><strong><img src="docs/assets/community/telegram.svg" alt="" width="18" /> Telegram 群組</strong></td>
  </tr>
  <tr>
    <td align="center"><img src="docs/assets/community/qq-qr.png" alt="Unic QQ 群組 QR 碼" width="240" /></td>
    <td align="center"><img src="docs/assets/community/wechat-qr.png" alt="Unic WeChat QR 碼" width="240" /></td>
    <td align="center"><a href="https://t.me/uniclipboard"><img src="docs/assets/community/telegram-qr.jpg" alt="Unic Telegram 群組 QR 碼" width="240" /></a></td>
  </tr>
</table>

## 參與開發

```sh
make build            # dist/
go test -race ./...
```

開始前請先閱讀 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [架構文件](docs/architecture/README.md)。撰寫外掛請參考 [外掛開發指南](plugins/README.md)。

## 授權

[AGPL-3.0](LICENSE)

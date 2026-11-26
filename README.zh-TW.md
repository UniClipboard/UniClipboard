<div align="center">

<img src="app/desktop/internal/appicons/unic-logo.svg" alt="UniClipboard" width="112" height="112" />

# 複製一次，每台裝置都能貼上

<p>
  <strong>UniClipboard</strong> 在你的裝置之間直接同步文字、圖片、檔案與資料夾。<br />
  端到端加密，無需帳號，不經雲端。
</p>

<p>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><strong>下載</strong></a>
  &nbsp;&nbsp;·&nbsp;&nbsp;
  <a href="#quick-start"><strong>快速開始</strong></a>
  &nbsp;&nbsp;·&nbsp;&nbsp;
  <a href="#features">功能一覽</a>
  &nbsp;&nbsp;·&nbsp;&nbsp;
  <a href="#cli">命令列</a>
</p>

<p>
  <code>Windows</code> &nbsp;<code>macOS</code> &nbsp;<code>Linux</code> &nbsp;<code>命令列</code>
</p>

<p>
  <small>
    <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | <strong>繁體中文</strong> | <a href="README.ja.md">日本語</a> | <a href="README.pt-BR.md">Português (Brasil)</a> | <a href="README.ru.md">Русский</a>
  </small>
</p>

![UniClipboard 跨裝置介面展示（桌面、平板與手機）](docs/assets/product-overview.png)

</div>

---

## 為什麼選擇 UniClipboard

- **跨系統**：Windows、macOS、Linux 之間互相同步。裝置在同一個 Wi-Fi 或不同網路都能連上。
- **不經過雲端**：裝置之間加密直連，不需要註冊帳號。剪貼簿歷史只存在你自己的裝置上。
- **記憶體用量低**：背景同步服務閒置時約佔 14 MB 記憶體<sup>[1](#benchmarks)</sup>。在 Windows 上，主視窗約佔 34–49 MB<sup>[2](#benchmarks)</sup>。關閉主視窗後，介面程序會結束並釋放記憶體。
- **都能同步**：文字、圖片、檔案和整個資料夾都能同步。

<a id="features"></a>

## 功能

<table>
  <tr>
    <td>⚡ <strong>快速面板</strong></td>
    <td>在任何應用程式中按 <code>⌘ ⌃ V</code>（Windows / Linux 上是 <code>Ctrl Alt V</code>）開啟歷史，選一筆，按 Enter 直接貼上。</td>
  </tr>
  <tr>
    <td>🔍 <strong>快速搜尋</strong></td>
    <td>輸入關鍵字，或用 <code>#標籤</code>、<code>@裝置</code>、<code>/類型</code> 篩選。幾個月前複製的內容也找得回來。</td>
  </tr>
  <tr>
    <td>🏷️ <strong>自動分類</strong></td>
    <td>自動為連結、電子郵件、色碼、版本號、UUID、IP 位址等內容加上標籤，也能新增自己的規則。</td>
  </tr>
  <tr>
    <td>📁 <strong>檔案與資料夾</strong></td>
    <td>傳送整個資料夾時完整保留目錄結構，也可以只傳給指定裝置。</td>
  </tr>
  <tr>
    <td>📮 <strong>免配對傳送</strong></td>
    <td>執行 <code>unic send "內容"</code> 會顯示傳輸連結。取件碼可用時，也會顯示 6 位數取件碼。接收方可使用任一方式接收。傳輸不會留下裝置紀錄。</td>
  </tr>
  <tr>
    <td>🧩 <strong>外掛</strong></td>
    <td>在設定中一鍵安裝外掛，例如把圖片或檔案轉成分享連結。</td>
  </tr>
  <tr>
    <td>⌨️ <strong>命令列</strong></td>
    <td>用 <code>unic</code> 在終端機、SSH 和指令碼中傳送、接收及篩選歷史清單。</td>
  </tr>
  <tr>
    <td>🌍 <strong>多語言</strong></td>
    <td>English、简体中文、日本語、Português、Русский，支援淺色與深色模式。</td>
  </tr>
</table>

<a id="quick-start"></a>

## 三步開始

**1. 下載並安裝**

從 [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases) 下載對應平台的安裝檔（macOS `.dmg`、Windows `.exe`/`.zip` 或 Linux `.deb`/`.rpm`/`.AppImage`），安裝後開啟 **UniClipboard**。如果只需要命令列版本，可執行 `npm install -g @uniclipboard/cli`。

**2. 連接第二台裝置**

在第一台裝置上開啟 **裝置 → 新增裝置**，記下 6 位數配對碼，然後在第二台裝置上輸入。兩台裝置顯示的指紋一致後，在兩邊分別點擊 **接受**。

**3. 複製**

在一台裝置上按 `Ctrl+C`，然後在另一台裝置上按 `Ctrl+V`。

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

<a id="benchmarks"></a>

## 實測數據

| | 記憶體 | 測試條件 |
|---|---:|---|
| 背景同步服務 `unicd` | **14 MB** | macOS（Apple Silicon），空歷史，閒置 20 秒後讀取，口徑與「活動監視器」的「記憶體」欄相同<sup>1</sup> |
| 主視窗 | **34 MB** / 49 MB | Windows 11 ARM64 原生版 / x64 版，60 筆文字歷史，工作管理員口徑<sup>2</sup> |
| 主視窗（含圖片） | 119 MB / 133 MB | 同上，另加 12 張 2048×1536 圖片<sup>2</sup> |

<sup>1</sup> 於提交 `e2cb68d` 以 `footprint <pid>` 量測。<sup>2</sup> 三次量測取中位數，詳見 [Windows 記憶體測試報告](docs/exec/windows-desktop-memory.md)。

## 隱私

- 裝置之間端對端加密傳輸，UniClipboard 不經營儲存你剪貼簿內容的伺服器。
- 每台新裝置都必須由你核對指紋並手動核准。移除的裝置會立即失去存取權。
- 剪貼簿內容、檔案路徑和裝置位址不會寫入日誌。
- 匿名診斷資料不包含剪貼簿內容和裝置身分，可隨時在 **設定 → 診斷與隱私** 中關閉。

如發現安全問題，請依 [SECURITY.md](SECURITY.md) 私下回報。

## 從 UniClipboard v1 升級

v2 可以匯入 v1 的全部歷史紀錄，包括文字、圖片、檔案和標籤。第一次開啟 v2 時依照引導操作即可，也可以稍後在 **設定 → 儲存空間 → 從 v1 遷移** 中匯入。

v2 裝置無法與 v1 裝置同步，請把所有裝置都升級到 v2 後重新配對。v1 的程式碼保留在 [`v1` 分支](https://github.com/UniClipboard/UniClipboard/tree/v1)。

## 參與開發

```sh
make build            # dist/
go test -race ./...
```

開始前請先閱讀 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [架構文件](docs/architecture/README.md)。撰寫外掛請參考 [外掛開發指南](plugins/README.md)。

## 授權

[AGPL-3.0](LICENSE)

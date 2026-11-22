<div align="center">

<img src="app/desktop/internal/appicons/unic-logo.svg" alt="UniClipboard" width="112" height="112" />

# 複製一次，每台裝置都能貼上

<strong>UniClipboard</strong> 在你的電腦之間同步剪貼簿。<br />
不用註冊帳號，不上傳雲端，背景只佔很少記憶體。

<a href="#quick-start"><img alt="開始使用" src="https://img.shields.io/badge/%E9%96%8B%E5%A7%8B%E4%BD%BF%E7%94%A8-→-000000?style=for-the-badge" /></a>
<a href="#features"><img alt="功能一覽" src="https://img.shields.io/badge/%E5%8A%9F%E8%83%BD%E4%B8%80%E8%A6%BD-555555?style=for-the-badge" /></a>

<table>
  <tr>
    <td align="center" width="170"><h2>0</h2>需要註冊的帳號</td>
    <td align="center" width="170"><h2>0</h2>儲存你內容的雲端伺服器</td>
    <td align="center" width="170"><h2>14 MB</h2>背景同步服務的記憶體用量<sup><a href="#benchmarks">1</a></sup></td>
    <td align="center" width="170"><h2>6 位數</h2>配對碼，輸入後核准即可連線</td>
  </tr>
</table>

Windows · macOS · Linux · 命令列　|　文字 · 圖片 · 檔案 · 資料夾

<a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | <strong>繁體中文</strong> | <a href="README.ja.md">日本語</a> | <a href="README.pt-BR.md">Português (Brasil)</a> | <a href="README.ru.md">Русский</a>

</div>

---

## 為什麼選擇 UniClipboard

- **跨系統**：Windows、macOS、Linux 之間互相同步。裝置在同一個 Wi-Fi 或不同網路都能連上。
- **不經過雲端**：裝置之間加密直連，不需要註冊帳號。剪貼簿歷史只存在你自己的裝置上。
- **記憶體用量低**：背景同步服務閒置時約佔 14 MB 記憶體<sup>[1](#benchmarks)</sup>。在 Windows 上，主視窗約佔 34–49 MB<sup>[2](#benchmarks)</sup>。關閉主視窗後，介面程序會結束並釋放記憶體。
- **都能同步**：文字、圖片、檔案和整個資料夾都能同步。

<a id="features"></a>

## 功能

| | |
|---|---|
| ⚡ **快速面板** | 在任何應用程式中按 `⌘ ⌃ V`（Windows / Linux 上是 `Ctrl Alt V`）開啟歷史，選一筆，按 Enter 直接貼上。 |
| 🔍 **快速搜尋** | 輸入關鍵字，或用 `#標籤`、`@裝置`、`/類型` 篩選。幾個月前複製的內容也找得回來。 |
| 🏷️ **自動分類** | 自動為連結、電子郵件、色碼、版本號、UUID、IP 位址等內容加上標籤，也能新增自己的規則。 |
| 📁 **檔案與資料夾** | 傳送整個資料夾時完整保留目錄結構，也可以只傳給指定裝置。 |
| 📮 **免配對傳送** | 執行 `unic send "內容"` 會得到 6 位數取件碼，對方輸入即可接收，用完不留下裝置紀錄。 |
| 🧩 **外掛** | 在設定中一鍵安裝外掛，例如把圖片或檔案轉成分享連結。 |
| ⌨️ **命令列** | 用 `unic` 在終端機、SSH 和腳本中傳送、接收和查詢歷史。 |
| 🌍 **多語言** | English、简体中文、日本語、Português、Русский，支援淺色與深色模式。 |

<a id="quick-start"></a>

## 三步開始

**1. 執行**

> 正式安裝檔即將推出。目前可以從原始碼執行，只需要安裝 [Go](https://go.dev/dl/)。

```sh
git clone https://github.com/UniClipboard/UniClipboard.git
cd UniClipboard
make run-desktop
```

**2. 連接第二台裝置**

在第一台裝置上開啟 **裝置 → 新增裝置**，記下 6 位數配對碼，然後在第二台裝置上輸入。兩台裝置顯示的指紋一致後，在兩邊分別點擊 **接受**。

**3. 複製**

在一台裝置上按 `Ctrl+C`，然後在另一台裝置上按 `Ctrl+V`。

## 命令列

```sh
unic send --to all "Hello"        # 傳送給所有裝置
unic send --to all -f ./photos    # 傳送檔案或資料夾
unic send "臨時內容"               # 免配對，產生取件碼
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

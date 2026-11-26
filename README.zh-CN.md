<div align="center">

<img src="app/desktop/internal/appicons/unic-logo.svg" alt="UniClipboard" width="112" height="112" />

# 复制一次，每台设备都能粘贴

<p>
  <strong>UniClipboard</strong> 在你的设备之间直接同步文字、图片、文件和文件夹。<br />
  端到端加密，无需账号，不经过云端。
</p>

<p>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="下载" src="https://img.shields.io/badge/%E4%B8%8B%E8%BD%BD-2563eb?style=for-the-badge&logo=github&logoColor=white" /></a>
  <a href="https://testflight.apple.com/join/nyNQ8dQe"><img alt="iOS Beta" src="https://img.shields.io/badge/iOS%20Beta-1f2937?style=for-the-badge&logo=apple&logoColor=white" /></a>
  <a href="#quick-start"><img alt="快速开始" src="https://img.shields.io/badge/%E5%BF%AB%E9%80%9F%E5%BC%80%E5%A7%8B-4b5563?style=for-the-badge" /></a>
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
    <a href="README.md">English</a> | <strong>简体中文</strong> | <a href="README.zh-TW.md">繁體中文</a> | <a href="README.ja.md">日本語</a> | <a href="README.pt-BR.md">Português (Brasil)</a> | <a href="README.ru.md">Русский</a>
  </small>
</p>

![UniClipboard 跨设备界面展示（桌面、平板与手机）](docs/assets/product-overview.png)

</div>

---

## 为什么选择 UniClipboard

- **跨平台**：支持 Windows、macOS、Linux、iOS 和 Android 互相同步。设备在同一 Wi-Fi 或不同网络里都能连上。
- **不经过云端**：设备之间加密直连，不需要注册账号。剪贴板历史只保存在你自己的设备上。
- **内存占用低**：后台同步服务空闲时约占 14 MB 内存<sup>[1](#benchmarks)</sup>。在 Windows 上，主界面约占 34–49 MB<sup>[2](#benchmarks)</sup>。关闭主窗口后，界面进程会退出并释放内存。
- **都能同步**：文字、图片、文件和整个文件夹都能同步。

<a id="features"></a>

## 功能

<table>
  <tr>
    <td>⚡ <strong>快捷面板</strong></td>
    <td>在任何应用里按 <code>⌘ ⌃ V</code>（Windows / Linux 上是 <code>Ctrl Alt V</code>）打开历史，选中一条，按回车直接粘贴。</td>
  </tr>
  <tr>
    <td>🔍 <strong>快速查找</strong></td>
    <td>输入关键词，或用 <code>#标签</code>、<code>@设备</code>、<code>/类型</code> 筛选。几个月前复制的内容也能找回。</td>
  </tr>
  <tr>
    <td>🏷️ <strong>自动分类</strong></td>
    <td>自动给链接、邮箱、颜色值、版本号、UUID、IP 地址等内容打标签，还能添加自己的规则。</td>
  </tr>
  <tr>
    <td>📁 <strong>文件和文件夹</strong></td>
    <td>发送整个文件夹时，目录结构会完整保留。也可以只发给指定的设备。</td>
  </tr>
  <tr>
    <td>📮 <strong>不配对也能传</strong></td>
    <td>运行 <code>unic send "内容"</code> 会输出传输链接。取件码可用时，也会输出 6 位取件码。接收方可以用其中一种方式接收。传输不会留下设备记录。</td>
  </tr>
  <tr>
    <td>🧩 <strong>插件</strong></td>
    <td>在设置里一键安装插件，例如把图片或文件转换成分享链接。</td>
  </tr>
  <tr>
    <td>⌨️ <strong>命令行</strong></td>
    <td>用 <code>unic</code> 在终端、SSH 和脚本中发送、接收和筛选历史列表。</td>
  </tr>
  <tr>
    <td>🌍 <strong>多语言</strong></td>
    <td>简体中文、English、日本語、Português、Русский，支持浅色和深色模式。</td>
  </tr>
</table>

<a id="quick-start"></a>

## 三步开始

**1. 下载并安装**

- **桌面端**：从 [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases) 下载 Windows（`.exe`/`.zip`）、macOS（`.dmg`）或 Linux（`.deb`/`.rpm`/`.AppImage`）安装包。
- **iOS**：通过 [TestFlight](https://testflight.apple.com/join/nyNQ8dQe) 加入公开 Beta 测试。
- **Android**：从 [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases) 下载 APK 安装（暂未计划上架 Google Play）。
- **仅命令行**：运行 `npm install -g @uniclipboard/cli`。

**2. 连接第二台设备**

在第一台设备上打开 **设备 → 添加设备**，记下 6 位配对码，然后在第二台设备上输入。两台设备显示的指纹一致后，在两边分别点击 **接受**。

**3. 复制**

在一台设备上按 `Ctrl+C`，然后在另一台设备上按 `Ctrl+V`。

<a id="cli"></a>

## 命令行

```sh
unic send --to all "Hello"        # 发送给所有设备
unic send --to all -f ./photos    # 发送文件或文件夹
unic send "临时内容"               # 无需配对，输出传输链接，并在可用时输出取件码
unic recv 482913                  # 用取件码接收
unic history list                 # 查看历史
```

完整命令见 [命令行文档](app/service/README.md)。

<a id="benchmarks"></a>

## 实测数据

| | 内存 | 测试条件 |
|---|---:|---|
| 后台同步服务 `unicd` | **14 MB** | macOS（Apple Silicon），空历史，空闲 20 秒后读取，统计口径与“活动监视器”的“内存”列一致<sup>1</sup> |
| 桌面主界面 | **34 MB** / 49 MB | Windows 11 ARM64 原生版 / x64 版，60 条文字历史，Windows 任务管理器口径<sup>2</sup> |
| 桌面主界面（含图片） | 119 MB / 133 MB | 同上，另加 12 张 2048×1536 图片<sup>2</sup> |

<sup>1</sup> 在提交 `e2cb68d` 上测量，测量方法是 `footprint <pid>`。<sup>2</sup> 三次测量取中位数，详见 [Windows 内存测试报告](docs/exec/windows-desktop-memory.md)。

## 隐私

- 设备之间端到端加密传输，UniClipboard 不运营存储你剪贴板内容的服务器。
- 新设备必须由你核对指纹并手动批准。设备移除后会立即失去访问权限。
- 剪贴板内容、文件路径和设备地址不会写入日志。
- 匿名诊断数据不包含剪贴板内容和设备身份。可以在 **设置 → 诊断与隐私** 中随时关闭。

如果发现安全问题，请按 [SECURITY.md](SECURITY.md) 私下报告。

## 从 UniClipboard v1 升级

v2 可以导入 v1 的全部历史记录，包括文字、图片、文件和标签。首次打开 v2 时，按照引导操作即可。也可以稍后在 **设置 › 存储 › 从 v1 迁移** 中导入。

v2 设备不能和 v1 设备同步，所以请把所有设备都升级到 v2，然后重新配对。v1 的代码保留在 [`v1` 分支](https://github.com/UniClipboard/UniClipboard/tree/v1)。

## 参与开发

```sh
make build            # dist/
go test -race ./...
```

开始之前，请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [架构文档](docs/architecture/README.md)。编写插件请参考 [插件开发指南](plugins/README.md)。

## 许可证

[AGPL-3.0](LICENSE)

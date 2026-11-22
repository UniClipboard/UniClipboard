<div align="center">

<img src="app/desktop/internal/appicons/unic-logo.svg" alt="UniClipboard" width="112" height="112" />

# 复制一次，每台设备都能粘贴

<strong>UniClipboard</strong> 在你的电脑之间同步剪贴板。<br />
不用注册账号，不上传云端，后台只占很少内存。

<a href="#quick-start"><img alt="开始使用" src="https://img.shields.io/badge/%E5%BC%80%E5%A7%8B%E4%BD%BF%E7%94%A8-→-000000?style=for-the-badge" /></a>
<a href="#features"><img alt="功能一览" src="https://img.shields.io/badge/%E5%8A%9F%E8%83%BD%E4%B8%80%E8%A7%88-555555?style=for-the-badge" /></a>

<table>
  <tr>
    <td align="center" width="170"><h2>0</h2>需要注册的账号</td>
    <td align="center" width="170"><h2>0</h2>保存你内容的云服务器</td>
    <td align="center" width="170"><h2>14 MB</h2>后台同步服务的内存占用<sup><a href="#benchmarks">1</a></sup></td>
    <td align="center" width="170"><h2>6 位</h2>配对码，输入后批准即可连接</td>
  </tr>
</table>

Windows · macOS · Linux · 命令行　|　文字 · 图片 · 文件 · 文件夹

<a href="README.md">English</a> | <strong>简体中文</strong> | <a href="README.zh-TW.md">繁體中文</a> | <a href="README.ja.md">日本語</a> | <a href="README.pt-BR.md">Português (Brasil)</a> | <a href="README.ru.md">Русский</a>

</div>

---

## 为什么选择 UniClipboard

- **跨系统**：Windows、macOS、Linux 之间互相同步。设备在同一 Wi-Fi 或不同网络里都能连上。
- **不经过云端**：设备之间加密直连，不需要注册账号。剪贴板历史只保存在你自己的设备上。
- **内存占用低**：后台同步服务空闲时约占 14 MB 内存<sup>[1](#benchmarks)</sup>。在 Windows 上，主界面约占 34–49 MB<sup>[2](#benchmarks)</sup>。关闭主窗口后，界面进程会退出并释放内存。
- **都能同步**：文字、图片、文件和整个文件夹都能同步。

<a id="features"></a>

## 功能

| | |
|---|---|
| ⚡ **快捷面板** | 在任何应用里按 `⌘ ⌃ V`（Windows / Linux 上是 `Ctrl Alt V`）打开历史，选中一条，按回车直接粘贴。 |
| 🔍 **快速查找** | 输入关键词，或用 `#标签`、`@设备`、`/类型` 筛选。几个月前复制的内容也能找回。 |
| 🏷️ **自动分类** | 自动给链接、邮箱、颜色值、版本号、UUID、IP 地址等内容打标签，还能添加自己的规则。 |
| 📁 **文件和文件夹** | 发送整个文件夹时，目录结构会完整保留。也可以只发给指定的设备。 |
| 📮 **不配对也能传** | 运行 `unic send "内容"` 会得到一个 6 位取件码，对方输入取件码就能收到内容，用完不会留下设备记录。 |
| 🧩 **插件** | 在设置里一键安装插件，例如把图片或文件转换成分享链接。 |
| ⌨️ **命令行** | 用 `unic` 在终端、SSH 和脚本中发送、接收和查询历史。 |
| 🌍 **多语言** | 简体中文、English、日本語、Português、Русский，支持浅色和深色模式。 |

<a id="quick-start"></a>

## 三步开始

**1. 运行**

> 正式安装包即将发布。现在可以从源码运行，只需要安装 [Go](https://go.dev/dl/)。

```sh
git clone https://github.com/UniClipboard/UniClipboard.git
cd UniClipboard
make run-desktop
```

**2. 连接第二台设备**

在第一台设备上打开 **设备 → 添加设备**，记下 6 位配对码，然后在第二台设备上输入。两台设备显示的指纹一致后，在两边分别点击 **接受**。

**3. 复制**

在一台设备上按 `Ctrl+C`，然后在另一台设备上按 `Ctrl+V`。

## 命令行

```sh
unic send --to all "Hello"        # 发送给所有设备
unic send --to all -f ./photos    # 发送文件或文件夹
unic send "临时内容"               # 不配对，生成取件码
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

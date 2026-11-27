<div align="center">

<img src="app/desktop/internal/appicons/unic-logo.svg" alt="Unic" width="112" height="112" />

# 让每一次复制更有用

<p>
  <strong>Unic（原名 UniClipboard）</strong> 是一款原生、本地优先的剪贴板管理与跨设备同步工具，覆盖 Windows、macOS、Linux、iOS、Android 与终端命令行。<br />
  集本地历史搜索、快捷面板、智能预览、插件扩展与端到端直连同步于一体 —— 无需账号，不经过云端。
</p>

<p>
  <a href="https://uniclipboard.app"><img alt="官网" src="https://img.shields.io/badge/Website-0891B2?style=for-the-badge" /></a>
  <a href="https://docs.uniclipboard.app/zh"><img alt="文档" src="https://img.shields.io/badge/Docs-4F46E5?style=for-the-badge&logo=readthedocs&logoColor=white" /></a>
  <a href="#community"><img alt="微信群" src="https://img.shields.io/badge/WeChat-07C160?style=for-the-badge&logo=wechat&logoColor=white" /></a>
  <a href="#community"><img alt="QQ 群" src="https://img.shields.io/badge/QQ-1688D8?style=for-the-badge&logo=qq&logoColor=white" /></a>
  <a href="https://t.me/uniclipboard"><img alt="Telegram 群组" src="https://img.shields.io/badge/Telegram-26A5E4?style=for-the-badge&logo=telegram&logoColor=white" /></a>
</p>

<p>
  <a href="https://github.com/UniClipboard/UniClipboard/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/UniClipboard/UniClipboard?style=flat-square&color=E5B650&logo=github&label=Stars" /></a>
  <a href="LICENSE"><img alt="AGPL-3.0 许可证" src="https://img.shields.io/badge/License-AGPL--3.0-22c55e?style=flat-square" /></a>
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

![Unic 跨设备界面展示（桌面、平板与手机）](docs/assets/product-overview.png)

</div>

---

## 为什么选择 Unic

- **不止于同步，单机同样好用**：多数剪贴板管理器局限于单一系统或绑定付费云同步，而跨设备传输工具往往缺少历史检索与快捷粘贴。Unic 即使只装在一台设备上也足够顺手——随叫随到的快捷面板、本地历史搜索、自动分类与智能链接预览；连接多台设备后，文字、图片、文件乃至整个文件夹都能在 Windows、macOS、Linux、iOS、Android 与终端之间无缝同步，局域网与跨网皆可用。
- **真正原生轻量，拒绝 Electron 臃肿**：核心由 Go 编写，桌面端采用原生界面。后台服务（`unicd`）空闲时仅占约 14 MB 内存；在 Windows 上，主窗口约占 34–49 MB，且**关闭主窗口后界面进程彻底退出**，不在后台残留内存开销。
- **架构级隐私，不靠口头承诺**：剪贴板里经常装着密码、密钥和私人文件。Unic 无需注册账号、不设云端存储——设备经指纹核验后端到端加密直连，移除设备立即断权，剪贴板历史始终只留在你自己的设备上。
- **可编程的剪贴板工作流，不止复制粘贴**：内置自动分类、可按网页标题与简介检索的智能链接预览、不改写原记录的 **Action**（去除格式、文本变换、将图片或文件生成临时分享链接），支持从插件市场一键安装 **Go / Lua 插件**，并提供完整命令行工具（`unic`），胜任终端、SSH、脚本与无需配对的 6 位取件码临时传输。

<a id="features"></a>

## 功能

<table>
  <tr>
    <td>⚡ <strong>快捷面板</strong></td>
    <td>在任何应用里按 <code>⌘ ⌃ V</code>（Windows / Linux 上是 <code>Ctrl Alt V</code>）呼出历史搜索，按 <code>Enter</code> 直接粘贴，或按 <code>Shift+Enter</code> 以纯文本粘贴。</td>
  </tr>
  <tr>
    <td>🔍 <strong>快速查找</strong></td>
    <td>输入关键词，或用 <code>#标签</code>、<code>@设备</code>、<code>/类型</code> 与收藏筛选。几个月前复制的内容也能找回，链接还支持按网页标题与简介检索。</td>
  </tr>
  <tr>
    <td>🏷️ <strong>自动分类与智能预览</strong></td>
    <td>自动给链接、邮箱、颜色值、版本号、UUID、IP 地址等内容打标签（支持自定义正则规则），并为复制的链接自动生成预览卡片。</td>
  </tr>
  <tr>
    <td>🔄 <strong>跨设备同步与文件夹</strong></td>
    <td>在已配对设备间同步文字、富文本、图片、文件和完整保留目录结构的文件夹，可同步给所有设备或指定单台设备。</td>
  </tr>
  <tr>
    <td>📮 <strong>不配对也能传</strong></td>
    <td>运行 <code>unic send "内容"</code> 会输出传输链接。取件码可用时，也会输出 6 位取件码。接收方可以用其中一种方式接收，不会留下设备记录。</td>
  </tr>
  <tr>
    <td>🧩 <strong>Action 与插件扩展</strong></td>
    <td>运行不修改原条目的 Action（文本转换、去除格式、将图片或文件转为分享链接），并可在设置中一键安装 Go 或 Lua 插件。</td>
  </tr>
  <tr>
    <td>⌨️ <strong>命令行</strong></td>
    <td>用 <code>unic</code> 在终端、SSH 和脚本中发送、接收、执行 Action 与筛选历史列表。</td>
  </tr>
  <tr>
    <td>🌍 <strong>多语言与主题</strong></td>
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

[![Packaging status](https://repology.org/badge/vertical-allrepos/uniclipboard.svg)](https://repology.org/project/uniclipboard/versions)

**2. 立即开始使用**

照常复制内容，在任意应用中按 `⌘ ⌃ V`（Windows / Linux 上按 `Ctrl Alt V`）呼出快捷面板，搜索历史并直接粘贴。

**3. 连接第二台设备（可选）**

在第一台设备上打开 **设备 → 添加设备**，记下 6 位配对码，然后在第二台设备上输入。两台设备显示的指纹一致后，在两边分别点击 **接受**，此后复制的内容就会自动同步。

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

## 隐私

- 设备之间端到端加密传输，Unic 不运营存储你剪贴板内容的服务器。
- 新设备必须由你核对指纹并手动批准。设备移除后会立即失去访问权限。
- 剪贴板内容、文件路径和设备地址不会写入日志。
- 匿名诊断数据不包含剪贴板内容和设备身份。可以在 **设置 → 诊断与隐私** 中随时关闭。

如果发现安全问题，请按 [SECURITY.md](SECURITY.md) 私下报告。

## 从 UniClipboard v1 升级

v2 可以导入 v1 的全部历史记录，包括文字、图片、文件和标签。首次打开 v2 时，按照引导操作即可。也可以稍后在 **设置 › 存储 › 从 v1 迁移** 中导入。

v2 设备不能和 v1 设备同步，所以请把所有设备都升级到 v2，然后重新配对。v1 的代码保留在 [`v1` 分支](https://github.com/UniClipboard/UniClipboard/tree/v1)。

<a id="community"></a>

## 社区交流

欢迎加入 Unic 社区，与其他用户和开发者交流使用心得、提出建议或反馈问题：

- <img src="docs/assets/community/qq.svg" alt="" width="18" /> QQ 群：`1080860954`
- <img src="docs/assets/community/wechat.svg" alt="" width="18" /> 微信群：扫描下方二维码添加好友，拉你进交流群
- <img src="docs/assets/community/telegram.svg" alt="" width="18" /> [Telegram 群组 (@uniclipboard)](https://t.me/uniclipboard)

<table>
  <tr>
    <td align="center"><strong><img src="docs/assets/community/qq.svg" alt="" width="18" /> QQ 群</strong></td>
    <td align="center"><strong><img src="docs/assets/community/wechat.svg" alt="" width="18" /> 微信群</strong></td>
    <td align="center"><strong><img src="docs/assets/community/telegram.svg" alt="" width="18" /> Telegram 群组</strong></td>
  </tr>
  <tr>
    <td align="center"><img src="docs/assets/community/qq-qr.png" alt="Unic QQ 群二维码" width="240" /></td>
    <td align="center"><img src="docs/assets/community/wechat-qr.png" alt="Unic 微信二维码" width="240" /></td>
    <td align="center"><a href="https://t.me/uniclipboard"><img src="docs/assets/community/telegram-qr.jpg" alt="Unic Telegram 群组二维码" width="240" /></a></td>
  </tr>
</table>

## 参与开发

```sh
make build            # dist/
go test -race ./...
```

开始之前，请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [架构文档](docs/architecture/README.md)。编写插件请参考 [插件开发指南](plugins/README.md)。

## 许可证

[AGPL-3.0](LICENSE)

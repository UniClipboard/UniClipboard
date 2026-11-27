<div align="center">

<img src="app/desktop/internal/appicons/unic-logo.svg" alt="Unic" width="112" height="112" />

# Make every copy more useful.

<p>
  <strong>Unic (formerly UniClipboard)</strong> is a native, local-first clipboard workspace and cross-device sync tool for Windows, macOS, Linux, iOS, Android, and the terminal.<br />
  Fast history search, Quick Panel, smart previews, plugins, and encrypted P2P sync — no account, no cloud.
</p>

<p>
  <a href="https://uniclipboard.app"><img alt="Website" src="https://img.shields.io/badge/Website-0891B2?style=for-the-badge" /></a>
  <a href="https://docs.uniclipboard.app"><img alt="Docs" src="https://img.shields.io/badge/Docs-4F46E5?style=for-the-badge&logo=readthedocs&logoColor=white" /></a>
  <a href="#community"><img alt="WeChat Group" src="https://img.shields.io/badge/WeChat-07C160?style=for-the-badge&logo=wechat&logoColor=white" /></a>
  <a href="#community"><img alt="QQ Group" src="https://img.shields.io/badge/QQ-1688D8?style=for-the-badge&logo=qq&logoColor=white" /></a>
  <a href="https://t.me/uniclipboard"><img alt="Telegram Group" src="https://img.shields.io/badge/Telegram-26A5E4?style=for-the-badge&logo=telegram&logoColor=white" /></a>
</p>

<p>
  <a href="https://github.com/UniClipboard/UniClipboard/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/UniClipboard/UniClipboard?style=flat-square&color=E5B650&logo=github&label=Stars" /></a>
  <a href="LICENSE"><img alt="AGPL-3.0 License" src="https://img.shields.io/badge/License-AGPL--3.0-22c55e?style=flat-square" /></a>
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
    <strong>English</strong> | <a href="README.zh-CN.md">简体中文</a> | <a href="README.zh-TW.md">繁體中文</a> | <a href="README.ja.md">日本語</a> | <a href="README.pt-BR.md">Português (Brasil)</a> | <a href="README.ru.md">Русский</a>
  </small>
</p>

![Unic across desktop, tablet, and phone](docs/assets/product-overview.png)

</div>

---

## Why Unic

- **More than sync — a complete clipboard workspace**: Most clipboard managers lock you into one OS or a paid cloud, while sync tools give you no real history or quick paste workflow. Unic is built to be worth using even on a single machine — instant Quick Panel, local history search, auto-tagging, and rich link previews — and seamlessly syncs text, images, files, and whole folders across Windows, macOS, Linux, iOS, Android, and terminals over the same Wi-Fi or across networks.
- **Native speed, not Electron bloat**: Written in Go with a native desktop UI. The background service (`unicd`) idles at ~14 MB of memory; on Windows, the main window uses ~34–49 MB and **exits completely when closed**, leaving zero UI overhead behind.
- **Private by architecture, not by promise**: Your clipboard carries passwords, API keys, and private files. Unic requires no account and runs no cloud storage — devices connect directly over end-to-end encrypted links after you verify their fingerprints, revoking a device cuts access immediately, and your history stays exclusively on your own devices.
- **Programmable, not just copy-paste**: Turn every copy into a workflow with built-in auto-classification, Smart Link Previews searchable by webpage title and description, non-destructive **Actions** (strip formatting, transform text, or generate share links), one-click **Go & Lua plugins**, and a full **CLI (`unic`)** for SSH, scripts, and no-pairing 6-digit transfers.

<a id="features"></a>

## Features

<table>
  <tr>
    <td>⚡ <strong>Quick Panel</strong></td>
    <td>Press <code>⌘ ⌃ V</code> (<code>Ctrl Alt V</code> on Windows / Linux) in any app to search history, press <code>Enter</code> to paste, or <code>Shift+Enter</code> for plain text.</td>
  </tr>
  <tr>
    <td>🔍 <strong>Instant local search</strong></td>
    <td>Search by keyword or filter with <code>#tag</code>, <code>@device</code>, <code>/type</code>, and favorites. Find what you copied months ago — including links matched by webpage title or description.</td>
  </tr>
  <tr>
    <td>🏷️ <strong>Auto-tagging &amp; smart previews</strong></td>
    <td>Links, emails, colors, version numbers, UUIDs, and IP addresses are tagged automatically (plus custom regex rules), and copied links render rich preview cards.</td>
  </tr>
  <tr>
    <td>🔄 <strong>Cross-device sync &amp; folders</strong></td>
    <td>Sync text, rich text, images, files, and whole folders with directory structure intact — to every paired device or just one.</td>
  </tr>
  <tr>
    <td>📮 <strong>Send without pairing</strong></td>
    <td><code>unic send "text"</code> prints a transfer link and a 6-digit pickup code when available. The receiver can use either. No device record is kept.</td>
  </tr>
  <tr>
    <td>🧩 <strong>Actions &amp; plugins</strong></td>
    <td>Run non-destructive Actions (transform text, strip styling, turn images or files into share links) and install Go or Lua plugins in one click from Settings.</td>
  </tr>
  <tr>
    <td>⌨️ <strong>Command line</strong></td>
    <td>Use <code>unic</code> to send, receive, run Actions, and filter history from a terminal, over SSH, or in scripts.</td>
  </tr>
  <tr>
    <td>🌍 <strong>Languages &amp; themes</strong></td>
    <td>English, 简体中文, 日本語, Português, Русский. Light and dark themes.</td>
  </tr>
</table>

<a id="quick-start"></a>

## Get started in 3 steps

**1. Download and install**

- **Desktop**: Download the installer for Windows (`.exe`/`.zip`), macOS (`.dmg`), or Linux (`.deb`/`.rpm`/`.AppImage`) from [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases).
- **iOS**: Join the public beta via [TestFlight](https://testflight.apple.com/join/nyNQ8dQe).
- **Android**: Download and install the APK from [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases) (not planned for Google Play at this time).
- **CLI only**: Run `npm install -g @uniclipboard/cli`.

[![Packaging status](https://repology.org/badge/vertical-allrepos/uniclipboard.svg)](https://repology.org/project/uniclipboard/versions)

**2. Use it right away**

Copy as usual, then press `⌘ ⌃ V` (`Ctrl Alt V` on Windows / Linux) in any app to open the Quick Panel, search your history, and paste.

**3. Connect a second device (optional)**

On the first device, open **Devices → Add device** and note the 6-digit code. Enter it on the second device. When both devices show the same fingerprint, click **Accept** on each — everything you copy now syncs automatically.

<a id="cli"></a>

## Command line

```sh
unic send --to all "Hello"        # send to all devices
unic send --to all -f ./photos    # send files or a folder
unic send "temporary"             # no pairing: get a transfer link and, when available, a pickup code
unic recv 482913                  # receive with a code
unic history list                 # browse history
```

See the [CLI guide](app/service/README.md) for all commands.

## Privacy

- Content is end-to-end encrypted between devices. Unic runs no server that stores your clipboard.
- You verify a fingerprint and approve every new device. A removed device loses access immediately.
- Clipboard content, file paths and device addresses never go into logs.
- Anonymous diagnostics contain no clipboard content or device identity. Turn them off anytime in **Settings → Diagnostics & Privacy**.

Report security issues privately as described in [SECURITY.md](SECURITY.md).

## Upgrading from UniClipboard v1

v2 imports your full v1 history: text, images, files and tags. Follow the prompt the first time you open v2, or import later from **Settings › Storage › Migrate from v1**.

v2 devices cannot sync with v1 devices. Upgrade every device to v2 and pair them again. The v1 code lives on the [`v1` branch](https://github.com/UniClipboard/UniClipboard/tree/v1).

<a id="community"></a>

## Community

Join the Unic community to share feedback, ask questions, report issues, and chat with other users and developers:

- <img src="docs/assets/community/qq.svg" alt="" width="18" /> QQ Group: `1080860954`
- <img src="docs/assets/community/wechat.svg" alt="" width="18" /> WeChat: scan the QR code below to add the maintainer and join the WeChat group
- <img src="docs/assets/community/telegram.svg" alt="" width="18" /> [Telegram (@uniclipboard)](https://t.me/uniclipboard)

<table>
  <tr>
    <td align="center"><strong><img src="docs/assets/community/qq.svg" alt="" width="18" /> QQ Group</strong></td>
    <td align="center"><strong><img src="docs/assets/community/wechat.svg" alt="" width="18" /> WeChat Group</strong></td>
    <td align="center"><strong><img src="docs/assets/community/telegram.svg" alt="" width="18" /> Telegram</strong></td>
  </tr>
  <tr>
    <td align="center"><img src="docs/assets/community/qq-qr.png" alt="Unic QQ Group QR Code" width="240" /></td>
    <td align="center"><img src="docs/assets/community/wechat-qr.png" alt="Unic WeChat QR Code" width="240" /></td>
    <td align="center"><a href="https://t.me/uniclipboard"><img src="docs/assets/community/telegram-qr.jpg" alt="Unic Telegram Group QR Code" width="240" /></a></td>
  </tr>
</table>

## Contributing

```sh
make build            # dist/
go test -race ./...
```

Read [CONTRIBUTING.md](CONTRIBUTING.md) and the [architecture docs](docs/architecture/README.md) first. To write a plugin, see the [plugin guide](plugins/README.md).

## License

[AGPL-3.0](LICENSE)

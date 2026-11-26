<div align="center">

<img src="app/desktop/internal/appicons/unic-logo.svg" alt="Unic" width="112" height="112" />

# Copy once. Paste on every device.

<p>
  <strong>Unic (formerly UniClipboard)</strong> syncs text, images, files, and folders directly across your devices.<br />
  End-to-end encrypted. No account. No cloud.
</p>

<p>
  <a href="https://github.com/UniClipboard/UniClipboard/releases"><img alt="Download" src="https://img.shields.io/badge/Download-2563eb?style=for-the-badge&logo=github&logoColor=white" /></a>
  <a href="https://testflight.apple.com/join/nyNQ8dQe"><img alt="iOS Beta" src="https://img.shields.io/badge/iOS%20Beta-1f2937?style=for-the-badge&logo=apple&logoColor=white" /></a>
  <a href="#quick-start"><img alt="Get started" src="https://img.shields.io/badge/Get%20started-4b5563?style=for-the-badge" /></a>
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

- **Cross-platform**: Sync between Windows, macOS, Linux, iOS, and Android, on the same Wi-Fi or across networks.
- **No cloud**: Devices connect directly over encrypted links. No sign-up. Your history stays on your own devices.
- **Lightweight**: The background sync service uses about 14 MB of memory when idle<sup>[1](#benchmarks)</sup>. On Windows, the main window uses about 34–49 MB<sup>[2](#benchmarks)</sup>. Close it and its process exits.
- **Everything syncs**: Text, images, files and whole folders.

<a id="features"></a>

## Features

<table>
  <tr>
    <td>⚡ <strong>Quick Panel</strong></td>
    <td>Press <code>⌘ ⌃ V</code> (<code>Ctrl Alt V</code> on Windows / Linux) in any app, pick an item and press Enter to paste it.</td>
  </tr>
  <tr>
    <td>🔍 <strong>Find anything</strong></td>
    <td>Type a keyword, or filter with <code>#tag</code>, <code>@device</code> or <code>/type</code>. Find what you copied months ago.</td>
  </tr>
  <tr>
    <td>🏷️ <strong>Auto-tagging</strong></td>
    <td>Links, emails, colors, version numbers, UUIDs and IP addresses are tagged automatically. Add your own rules.</td>
  </tr>
  <tr>
    <td>📁 <strong>Files and folders</strong></td>
    <td>Send a whole folder with its structure intact, to every device or just one.</td>
  </tr>
  <tr>
    <td>📮 <strong>Send without pairing</strong></td>
    <td><code>unic send "text"</code> prints a transfer link and a 6-digit pickup code when available. The receiver can use either. No device record is kept.</td>
  </tr>
  <tr>
    <td>🧩 <strong>Plugins</strong></td>
    <td>Install plugins in one click from Settings, for example to turn an image or file into a share link.</td>
  </tr>
  <tr>
    <td>⌨️ <strong>Command line</strong></td>
    <td>Use <code>unic</code> to send, receive, and filter the history list from a terminal, over SSH, or in scripts.</td>
  </tr>
  <tr>
    <td>🌍 <strong>Languages</strong></td>
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

**2. Connect a second device**

On the first device, open **Devices → Add device** and note the 6-digit code. Enter it on the second device. When both devices show the same fingerprint, click **Accept** on each.

**3. Copy**

Press `Ctrl+C` on one device, then `Ctrl+V` on the other.

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

<a id="benchmarks"></a>

## Measurements

| | Memory | Conditions |
|---|---:|---|
| Sync service `unicd` | **14 MB** | macOS (Apple Silicon), empty history, read after 20 s idle; same measure as the Memory column in Activity Monitor<sup>1</sup> |
| Main window | **34 MB** / 49 MB | Windows 11, native ARM64 / x64 build, 60 text items; Task Manager measure<sup>2</sup> |
| Main window with images | 119 MB / 133 MB | Same, plus 12 images at 2048×1536<sup>2</sup> |

<sup>1</sup> Measured at commit `e2cb68d` with `footprint <pid>`. <sup>2</sup> Median of three runs. See the [Windows memory report](docs/exec/windows-desktop-memory.md).

## Privacy

- Content is end-to-end encrypted between devices. Unic runs no server that stores your clipboard.
- You verify a fingerprint and approve every new device. A removed device loses access immediately.
- Clipboard content, file paths and device addresses never go into logs.
- Anonymous diagnostics contain no clipboard content or device identity. Turn them off anytime in **Settings → Diagnostics & Privacy**.

Report security issues privately as described in [SECURITY.md](SECURITY.md).

## Upgrading from UniClipboard v1

v2 imports your full v1 history: text, images, files and tags. Follow the prompt the first time you open v2, or import later from **Settings › Storage › Migrate from v1**.

v2 devices cannot sync with v1 devices. Upgrade every device to v2 and pair them again. The v1 code lives on the [`v1` branch](https://github.com/UniClipboard/UniClipboard/tree/v1).

## Contributing

```sh
make build            # dist/
go test -race ./...
```

Read [CONTRIBUTING.md](CONTRIBUTING.md) and the [architecture docs](docs/architecture/README.md) first. To write a plugin, see the [plugin guide](plugins/README.md).

## License

[AGPL-3.0](LICENSE)

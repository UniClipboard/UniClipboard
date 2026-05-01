![UniClipboard](https://socialify.git.ci/UniClipboard/UniClipboard/image?custom_description=A+privacy-first%2C+end-to-end+encrypted%2C+cross-device+clipboard+sync+built+with+Rust+and+Tauri.&description=1&font=KoHo&forks=1&issues=1&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto)

## Project Overview

English | [简体中文](./README_ZH.md)

UniClipboard is a **privacy-first**, cross-device clipboard synchronization tool.
It enables seamless and secure syncing of text, images, and files across multiple devices, whether on the same Wi-Fi or across different networks. Data is encrypted both in transit and at rest, and decrypted only on the user’s devices—neither servers nor the network layer can ever access plaintext data.

![Image](https://github.com/user-attachments/assets/8d339467-5bbe-4afa-9235-1d26cbff82c9)

<div align="center">
  <br/>

  <a href="https://github.com/UniClipboard/UniClipboard/releases">
    <img
      alt="Windows"
      src="https://img.shields.io/badge/-Windows-blue?style=flat-square&logo=data:image/svg+xml;base64,PHN2ZyB0PSIxNzI2MzA1OTcxMDA2IiBjbGFzcz0iaWNvbiIgdmlld0JveD0iMCAwIDEwMjQgMTAyNCIgdmVyc2lvbj0iMS4xIiB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHAtaWQ9IjE1NDgiIHdpZHRoPSIxMjgiIGhlaWdodD0iMTI4Ij48cGF0aCBkPSJNNTI3LjI3NTU1MTYxIDk2Ljk3MTAzMDEzdjM3My45OTIxMDY2N2g0OTQuNTEzNjE5NzVWMTUuMDI2NzU3NTN6TTUyNy4yNzU1NTE2MSA5MjguMzIzNTA4MTVsNDk0LjUxMzYxOTc1IDgwLjUyMDI4MDQ5di00NTUuNjc3NDcxNjFoLTQ5NC41MTM2MTk3NXpNNi42NzA0NTEzNiA0NzAuODMzNjgyOTdINDIyLjY3Njg1OTI1VjExMC41NjM2ODE5N2wtNDE4LjAwNjQwNzg5IDY5LjI1Nzc5NzUzek00LjY3MDQ1MTM2IDg0Ni43Njc1OTcwM0w0MjIuNjc2ODU5MjUgOTE0Ljg2MDMxMDEzVjU1My4xNjYzMTcwM0g0LjY3MDQ1MTM2eiIgcC1pZD0iMTU0OSIgZmlsbD0iI2ZmZmZmZiI+PC9wYXRoPjwvc3ZnPg=="
    />
  </a>
  <a href="https://github.com/UniClipboard/UniClipboard/releases">
    <img
      alt="MacOS"
      src="https://img.shields.io/badge/-MacOS-black?style=flat-square&logo=apple&logoColor=white"
    />
  </a >
  <a href="https://github.com/UniClipboard/UniClipboard/releases">
    <img
      alt="Linux"
      src="https://img.shields.io/badge/-Linux-purple?style=flat-square&logo=linux&logoColor=white"
    />
  </a>

  <div>
    <a href="./LICENSE">
      <img
        src="https://img.shields.io/github/license/UniClipboard/UniClipboard?style=flat-square"
      />
    </a >
    <a href="https://github.com/UniClipboard/UniClipboard/releases">
      <img
        src="https://img.shields.io/github/v/release/UniClipboard/UniClipboard?include_prereleases&style=flat-square"
      />
    </a >
    <a href="https://codecov.io/gh/UniClipboard/UniClipboard" >
      <img src="https://codecov.io/gh/UniClipboard/UniClipboard/branch/main/graph/badge.svg?token=QZfjXOsQTp"/>
    </a>
  </div>

</div>

> [!WARNING]
> UniClipboard is currently under active development and may have unstable or missing features. Feel free to try it out and provide feedback!

## Features

- **Cross-platform**: Windows, macOS, and Linux
- **Cross-network sync**: Real-time clipboard sync between devices on the same LAN, across different home/office networks, or across the internet — with automatic NAT traversal and relay fallback
- **Content types**: Text, images, and files (large files supported via streaming transfer)
- **Encrypted spaces**: Devices group into shared "spaces"; create one, join via an invitation code + passphrase, or switch spaces without losing local history
- **Local full-text search**: Encrypted local index for fast browsing across thousands of entries
- **Quick Panel**: Keyboard-shortcut overlay with inline preview for text, links, images, code, and files
- **Command-line tool**: A `uniclip` CLI that drives the full setup, pairing, and sync flow from the terminal
- **Secure encryption**: XChaCha20-Poly1305 AEAD keeps data encrypted in transit and at rest
- **Multi-device management**: Manage paired devices, presence, and per-device sync preferences

## Installation

### Download from Releases

Visit the [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases) page to download the installation package for your operating system.

### Build from Source

```bash
# Clone the repository
git clone https://github.com/UniClipboard/UniClipboard.git
cd UniClipboard

# Install dependencies
bun install

# Start development mode
bun tauri dev

# Build application
bun tauri build
```

## Usage

### First Device (Create a Space)

1. Launch the app and choose **Create a Space**
2. Set an encryption passphrase — this protects all data inside the space
3. Done. Copied content is stored encrypted in this space.

### Adding More Devices (Join via Invitation)

1. On an existing device, open the **Devices** page and **generate an invitation code** (short-lived, valid for several minutes)
2. On the new device, choose **Join an existing space**, enter the invitation code together with the space passphrase
3. Once verified, the device joins and syncing starts automatically.

> Already set up and want to move to another space? Use **Switch space** from the Devices page (or `uniclip switch-space` from the CLI) — your local clipboard history is re-encrypted and migrated.

### Main Pages

- **Dashboard** — Clipboard history with full-text search and detailed preview
- **Quick Panel** — Keyboard-shortcut overlay for fast clipboard access
- **Devices** — Manage paired devices and presence, generate invitation codes, switch spaces
- **Settings** — General, sync, security, network, storage, and search-index options

## Advanced Features

### Network Configuration

UniClipboard syncs directly between your devices over an encrypted peer-to-peer connection:

- **Same network**: Devices on the same Wi-Fi connect directly for the lowest latency
- **Different networks**: NAT traversal (hole-punching) lets devices on different home/office networks connect directly whenever possible
- **Fallback relay**: When a direct connection isn't possible, traffic falls back to a relay — still end-to-end encrypted; the relay only sees ciphertext
- **Resilient to changes**: Connections recover automatically after Wi-Fi switches, sleep/wake, or brief disconnects, with no re-pairing required

### Command-line Tool

The `uniclip` CLI mirrors the GUI flow and works headlessly (e.g. on servers):

```bash
uniclip init                    # Create a new encrypted space on this device
uniclip invite                  # Generate a short-lived invitation code
uniclip join <code>             # Join an existing space
uniclip members                 # List paired devices and presence
uniclip send "hello"            # Send clipboard content to other devices
uniclip watch                   # Stream incoming clipboard events
uniclip switch-space            # Move this device to another space
uniclip status / start / stop   # Daemon lifecycle
```

### Security Features

- **End-to-end encryption**: Data is encrypted in transit between devices and remains encrypted at rest in local storage
- **XChaCha20-Poly1305 encryption**: Modern AEAD cipher providing authenticated encryption
  - 24-byte random nonce effectively reduces nonce reuse risks
  - 32-byte (256-bit) encryption key
  - Provides ciphertext integrity and authenticity verification
- **Argon2id key derivation**: Securely derives encryption keys from user passphrase
  - Memory cost: 128 MB
  - Iterations: 3
  - Parallelism: 4 threads
  - Resistant to GPU/ASIC cracking attacks
- **Key management**: Layered key architecture protects data
  - MasterKey for clipboard content encryption
  - Key Encryption Key (KEK) derived from passphrase via Argon2id
  - KEK securely stored in system keyring (macOS Keychain, Windows Credential Manager, Linux Secret Service)
  - MasterKey encrypted and stored in KeySlot file
- **Per-space isolation**: Each space has its own MasterKey; switching to another space re-encrypts local history under the new space's MasterKey
- **Device authorization**: Precise control over each device's access permissions

## Contributing

Contributions of all kinds are welcome! If you're interested in improving UniClipboard:

1. Fork this repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Create a Pull Request

## License

This project is licensed under the AGPL-3.0 License - see the [LICENSE](./LICENSE) file for details.

## Acknowledgments

- [Tauri](https://tauri.app) - Cross-platform application framework
- [React](https://react.dev) - Frontend UI development framework
- [Rust](https://www.rust-lang.org) - Safe and efficient backend implementation language

---

**Have questions or suggestions?** [Create an Issue](https://github.com/UniClipboard/UniClipboard/issues/new) or contact us to discuss!

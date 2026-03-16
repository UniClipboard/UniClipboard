![UniClipboard](https://socialify.git.ci/UniClipboard/UniClipboard/image?custom_description=A+privacy-first%2C+end-to-end+encrypted%2C+cross-device+clipboard+sync+built+with+Rust+and+Tauri.&description=1&font=KoHo&forks=1&issues=1&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto)

## Project Overview

English | [简体中文](./README_ZH.md)

UniClipboard is a **privacy-first**, cross-device clipboard synchronization tool.
It enables seamless and secure syncing of text, images, and files across multiple devices. Data is encrypted both in transit and at rest, and decrypted only on the user’s devices—neither servers nor the network layer can ever access plaintext data.

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

- **Cross-platform support**: Supports Windows, macOS, and Linux operating systems
- **Real-time sync**: Instantly share clipboard content between connected devices
- **Content types**: Supports text, images, and files
- **P2P device discovery**: Automatic device discovery on LAN via mDNS (powered by libp2p)
- **Quick Panel**: Keyboard shortcut-triggered quick access panel for clipboard history
- **Preview Panel**: Detailed content preview for clipboard entries
- **Secure encryption**: Uses XChaCha20-Poly1305 AEAD encryption algorithm to ensure secure data transmission
- **Multi-device management**: Easily add and manage multiple devices
- **Flexible configuration**: Provides extensive customization options

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

### First Device (Create Encrypted Space)

1. Launch the application for the first time and select **Create Encrypted Space**
2. Set an encryption passphrase — this will be used to protect all synced data
3. Setup is complete. Copied text, images, and files will be stored in the encrypted space

### Adding More Devices (Join Encrypted Space)

1. Launch the application on the new device and select **Join Encrypted Space**
2. The app will automatically scan for available devices on the local network
3. Select the discovered device, then enter the encryption passphrase
4. Once the passphrase is verified, clipboard sync begins automatically

### Main Pages

- **Dashboard**: Overview of clipboard history and device connections
- **Devices**: Manage paired devices and access permissions
- **Settings**: Configure general, sync, security, network, and storage options

## Advanced Features

### Network Configuration

Supports multiple network connection modes that can be configured based on your network environment:

- **LAN sync (P2P)**: Automatic device discovery and direct sync over local network using libp2p with mDNS
- **WebDAV sync**: Under development

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
- **Device authorization**: Precise control over each device's access permissions

## Contributing

Contributions of all kinds are welcome! If you're interested in improving UniClipboard:

1. Fork this repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Create a Pull Request

## License

This project is licensed under the Apache-2.0 License - see the [LICENSE](./LICENSE) file for details.

## Acknowledgments

- [Tauri](https://tauri.app) - Cross-platform application framework
- [React](https://react.dev) - Frontend UI development framework
- [Rust](https://www.rust-lang.org) - Safe and efficient backend implementation language

---

**Have questions or suggestions?** [Create an Issue](https://github.com/UniClipboard/UniClipboard/issues/new) or contact us to discuss!

![UniClipboard](https://socialify.git.ci/UniClipboard/UniClipboard/image?custom_description=A+privacy-first%2C+end-to-end+encrypted%2C+cross-device+clipboard+sync+built+with+Rust+and+Tauri.&description=1&font=KoHo&forks=1&issues=1&name=1&owner=1&pattern=Floating+Cogs&pulls=1&stargazers=1&theme=Auto)

## 📝 项目介绍

[English](./README.md) | 简体中文

UniClipboard 是一款以**隐私优先**为核心理念的跨设备剪贴板同步工具。它支持在多台设备之间无缝、安全地同步文本、图片和文件，无论设备处于同一 Wi-Fi 还是不同网络环境。数据在传输与本地存储阶段均保持加密，仅在用户设备本地解密，服务器与网络层永远无法访问明文。

![Image](https://github.com/user-attachments/assets/8d339467-5bbe-4afa-9235-1d26cbff82c9)

<div align="center">

  <br/>

  <a href="https://github.com/UniClipboard/UniClipboard/releases">
    <img
      alt="Windows"
      src="https://img.shields.io/badge/-Windows-blue?style=flat-square&logo=data:image/svg+xml;base64,PHN2ZyB0PSIxNzI2MzA1OTcxMDA2IiBjbGFzcz0iaWNvbiIgdmlld0JveD0iMCAwIDEwMjQgMTAyNCIgdmVyc2lvbj0iMS4xIiB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHAtaWQ9IjE1NDgiIHdpZHRoPSIxMjgiIGhlaWdodD0iMTI4Ij48cGF0aCBkPSJNNTI3LjI3NTU1MTYxIDk2Ljk3MTAzMDEzdjM3My45OTIxMDY2N2g0OTQuNTEzNjE5NzVWMTUuMDI2NzU3NTN6TTUyNy4yNzU1NTE2MSA5MjguMzIzNTA4MTVsNDk0LjUxMzYxOTc1IDgwLjUyMDI4MDQ5di00NTUuNjc3NDcxNjFoLTQ5NC41MTM2MTk3NXpNNC42NzA0NTEzNiA0NzAuODMzNjgyOTdINDIyLjY3Njg1OTI1VjExMC41NjM2ODE5N2wtNDE4LjAwNjQwNzg5IDY5LjI1Nzc5NzUzek00LjY3MDQ1MTM2IDg0Ni43Njc1OTcwM0w0MjIuNjc2ODU5MjUgOTE0Ljg2MDMxMDEzVjU1My4xNjYzMTcwM0g0LjY3MDQ1MTM2eiIgcC1pZD0iMTU0OSIgZmlsbD0iI2ZmZmZmZiI+PC9wYXRoPjwvc3ZnPg=="
    />
  </a>  
  <a href="https://github.com/UniClipboard/UniClipboard/releases">
    <img
      alt="MacOS"
      src="https://img.shields.io/badge/-MacOS-black?style=flat-square&logo=apple&logoColor=white"
    />
  </a>
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
    </a>
    <a href="https://github.com/UniClipboard/UniClipboard/releases">
      <img
        src="https://img.shields.io/github/v/release/UniClipboard/UniClipboard?include_prereleases&style=flat-square"
      />
    </a>
    <a href="https://codecov.io/gh/UniClipboard/UniClipboard" >
      <img src="https://codecov.io/gh/UniClipboard/UniClipboard/branch/main/graph/badge.svg?token=QZfjXOsQTp"/>
    </a>
  </div>

</div>

> [!WARNING]
> UniClipboard 目前处于积极开发阶段，可能存在功能不稳定或缺失的情况。欢迎体验并提供反馈！

## ✨ 功能特点

- **跨平台支持**: 支持 Windows、macOS 和 Linux 操作系统
- **跨网络同步**: 同一 Wi-Fi、不同家庭/办公室网络、甚至跨广域网均可实时同步，自动 NAT 穿透与中继回落
- **内容类型**: 支持文本、图片和文件（大文件采用流式传输）
- **加密空间**: 设备以"空间"为单位加入；可创建新空间，也可通过邀请码 + 口令加入已有空间，或在不丢失本地历史的前提下切换到另一个空间
- **本地全文搜索**: 加密的本地索引让数千条历史条目也能秒级检索
- **快捷面板**: 通过键盘快捷键唤出，内嵌文本、链接、图片、代码与文件的预览
- **命令行工具**: 提供 `uniclip` CLI，可在终端完成完整的设置、配对与同步流程
- **安全加密**: 使用 XChaCha20-Poly1305 AEAD，传输与本地存储全程加密
- **多设备管理**: 管理已配对设备、在线状态与每台设备的同步偏好

## 🚀 安装方法

### 从 Releases 下载

访问 [GitHub Releases](https://github.com/UniClipboard/UniClipboard/releases) 页面，下载适合您操作系统的安装包。

### 从源码构建

```bash
# 克隆仓库
git clone https://github.com/UniClipboard/UniClipboard.git
cd UniClipboard

# 安装依赖
bun install

# 开发模式启动
bun tauri dev

# 构建应用
bun tauri build
```

## 🎮 使用说明

### 第一台设备（新建空间）

1. 首次启动应用，选择 **新建空间**
2. 设置加密口令 — 用于保护空间内的所有数据
3. 设置完成。复制的内容将以加密形式存储在该空间中

### 添加更多设备（通过邀请码加入）

1. 在已有设备上打开 **设备** 页，**生成邀请码**（短期有效，几分钟内可用）
2. 在新设备上启动应用，选择 **加入已有空间**，输入邀请码与空间口令
3. 口令验证通过后，新设备完成加入并自动开始同步

> 已经完成设置、想切换到另一个空间？在 **设备** 页使用 **切换空间**（或 CLI 中的 `uniclip switch-space`）—— 本地的剪贴板历史会被重新加密并迁移到新空间。

### 主要页面

- **仪表盘** —— 剪贴板历史，支持全文搜索与详细预览
- **快捷面板** —— 通过键盘快捷键唤出的浮层，便于快速访问历史
- **设备** —— 管理已配对设备与在线状态、生成邀请码、切换空间
- **设置** —— 配置通用、同步、安全、网络、存储与搜索索引等选项

## 🔧 高级功能

### 网络配置

UniClipboard 在你的设备之间通过加密的端到端 P2P 通道直接同步：

- **同一网络**: 在同一 Wi-Fi 下的设备直接互联，延迟最低
- **不同网络**: 通过 NAT 穿透（UDP 打洞）让位于不同家庭/办公室网络的设备尽可能直连
- **中继回落**: 当无法直连时，流量自动回落到中继节点 —— 仍然是端到端加密，中继只能看到密文
- **网络变化下自恢复**: 切换 Wi-Fi、设备睡眠唤醒或短暂断网后，连接会自动恢复，无需重新配对

### 命令行工具

`uniclip` 命令行工具与 GUI 流程一致，并可在无桌面环境（如服务器）下使用：

```bash
uniclip init                    # 在本机创建一个新的加密空间
uniclip invite                  # 生成短期邀请码
uniclip join <code>             # 通过邀请码加入已有空间
uniclip members                 # 列出已配对设备及在线状态
uniclip send "hello"            # 把内容发送到其他设备
uniclip watch                   # 实时接收来自其他设备的剪贴板内容
uniclip switch-space            # 把本机切换到另一个空间
uniclip status / start / stop   # 守护进程生命周期
```

### 安全功能

- **端到端加密**: 数据在设备间传输时加密，且在本地存储阶段也保持加密
- **XChaCha20-Poly1305 加密**: 使用现代 AEAD 加密算法提供认证加密
  - 24 字节随机 nonce，有效降低 nonce 重用风险
  - 32 字节（256 位）加密密钥
  - 提供密文完整性和真实性验证
- **Argon2id 密钥派生**: 从用户密码安全派生加密密钥
  - 内存成本：128 MB
  - 迭代次数：3 次
  - 并行度：4 线程
  - 抗 GPU/ASIC 破解攻击
- **密钥管理**: 分层密钥架构保护数据安全
  - 主密钥（MasterKey）用于剪贴板内容加密
  - 密钥加密密钥（KEK）通过 Argon2id 从密码派生
  - KEK 安全存储于系统密钥环（macOS Keychain、Windows Credential Manager、Linux Secret Service）
  - 主密钥加密存储于 KeySlot 文件
- **空间隔离**: 每个空间拥有独立的主密钥；切换到另一个空间时，本地历史会用新空间的主密钥重新加密
- **设备授权**: 精确控制每台设备的访问权限

## 🤝 参与贡献

非常欢迎各种形式的贡献！如果您对改进 UniClipboard 感兴趣，请：

1. Fork 本仓库
2. 创建您的特性分支 (`git checkout -b feature/amazing-feature`)
3. 提交您的更改 (`git commit -m 'Add some amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 创建一个 Pull Request

## 📄 许可证

本项目采用 AGPL-3.0 许可证 - 详情请参阅 [LICENSE](./LICENSE) 文件。

## 🙏 鸣谢

- [Tauri](https://tauri.app) - 提供跨平台应用框架
- [React](https://react.dev) - 前端界面开发框架
- [Rust](https://www.rust-lang.org) - 安全高效的后端实现语言

---

💡 **有问题或建议?** [创建 Issue](https://github.com/UniClipboard/UniClipboard/issues/new) 或联系我们讨论!

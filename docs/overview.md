# Project Overview

**UniClipboard Desktop** is a privacy-first, cross-device clipboard synchronization tool. It combines a React/Tauri desktop UI with a modular Rust backend and daemon so devices can pair, sync clipboard content, and manage encrypted local history.

## Technology Stack

| Layer                | Technology                    | Purpose                              |
| -------------------- | ----------------------------- | ------------------------------------ |
| **Frontend**         | React 19 + TypeScript + Vite  | UI and user interaction              |
| **State Management** | Redux Toolkit + RTK Query     | Client state and API caching         |
| **UI Components**    | Tailwind CSS + Shadcn/ui      | Responsive, accessible components    |
| **Desktop Shell**    | Rust + Tauri 2                | GUI 壳适配（commands, tray, panel）  |
| **Daemon**           | Rust (axum + tokio)           | 后台常驻服务，承载全部业务逻辑       |
| **Database**         | SQLite + Diesel ORM           | Local clipboard history storage      |
| **Realtime / IPC**   | Daemon HTTP + WebSocket       | GUI/CLI 通过 127.0.0.1 与 daemon 通信 |
| **P2P Network**      | iroh (QUIC + Ed25519)         | NAT 穿越、设备发现、加密传输         |
| **Encryption**       | XChaCha20-Poly1305 + Argon2id | End-to-end content encryption        |

## What It Does

UniClipboard solves the problem of **clipboard fragmentation across devices**:

- **Automatic Sync**: Copy on one device, paste on another
- **Cross-Platform**: Works on macOS, Windows, and Linux
- **Pairing + Device Management**: Onboard a first device, join additional devices, and manage trust relationships
- **Desktop-Native UX**: Tray integration, quick panel with inline preview, and global shortcuts
- **Privacy First**: Clipboard content is encrypted before persistence/sync and decrypted only on user devices
- **History Management**: Searchable clipboard history with configurable limits

## System Architecture

UniClipboard 采用 **六边形架构（Ports & Adapters）**，运行时分为三个独立进程：daemon（业务核心）、GUI（纯客户端壳）、CLI（轻量命令行客户端）。

### 运行时拓扑

```
┌─────────────────────────────────────┐     ┌────────────────────┐
│         GUI (Tauri + React)         │     │   CLI (uniclip)    │
│  - Quick Panel / Tray / Settings    │     │  - copy/paste/list │
│  - 不打开 SQLite、不运行 iroh       │     │  - search/status   │
└──────────────────┬──────────────────┘     └─────────┬──────────┘
                   │ HTTP + WebSocket (127.0.0.1)      │
                   └──────────────────┬────────────────┘
                                      ▼
┌──────────────────────────────────────────────────────────────┐
│                    Daemon (uniclipd)                          │
│  uc-bootstrap 组装 → uc-application 编排 → uc-core 领域      │
│  uc-infra (SQLite/iroh/crypto) + uc-platform (OS adapters)   │
│  uc-webserver (axum HTTP/WS API)                             │
└──────────────────────────────────────────────────────────────┘
                   │ iroh QUIC (P2P encrypted)
                   ▼
            ┌──────────────┐
            │  Peer Devices │
            └──────────────┘
```

### 关键架构原则

1. **依赖反转**：用例仅依赖 Port trait，不依赖具体基础设施
2. **GUI/Daemon 严格分离**：GUI 进程不内嵌 SQLite/iroh/AppFacade；daemon 不依赖任何 GUI 框架
3. **唯一组合根**：`uc-bootstrap` 是唯一允许同时依赖 core + app + infra + platform 的 crate
4. **可测试性**：76+ async Port trait 通过 `Arc<dyn Port>` 注入，测试使用 mock/fake

## Crate Structure

```
src-tauri/crates/
# ── 领域核心层（零外部依赖）──
├── uc-core/              # 纯领域模型 + Port trait 定义
├── uc-observability/     # 双输出 tracing、profile 过滤、分析门控
├── uc-app-paths/         # 轻量目录布局权威（数据/缓存/临时目录解析）
# ── 应用编排层 ──
├── uc-application/       # 用例 / Facade / 状态机编排
# ── 基础设施 & 平台层 ──
├── uc-infra/             # Port 实现：Diesel repos, iroh P2P, 加密, 存储
├── uc-platform/          # OS 适配：剪贴板监听, Keychain, 自启动
# ── 组合根 ──
├── uc-bootstrap/         # 唯一允许依赖全部层的 crate（DI 装配）
# ── Daemon 子系统 ──
├── uc-daemon-contract/   # 纯 serde 传输契约（HTTP API 类型）
├── uc-daemon-process/    # 薄进程原语（PID, socket, spawn）
├── uc-daemon-local/      # 本地 daemon 元数据（auth token, 健康轮询）
├── uc-webserver/         # axum HTTP + WebSocket 服务端
├── uc-daemon/            # Daemon 运行时库 + uniclipd 二进制
├── uc-daemon-client/     # Daemon HTTP/WS 客户端（GUI + CLI 共用）
# ── GUI 桌面层 ──
├── uc-desktop/           # 桌面宿主逻辑（GUI 框架无关）
├── uc-tauri/             # Tauri 壳适配（commands, tray, panel）
# ── CLI ──
├── uc-cli/               # uniclip 命令行工具
├── uc-cli-macros/        # CLI proc-macro 辅助
# ── 测试/Spike ──
└── p2p-bench/            # P2P 吞吐量基准（不发布）
```

## How Clipboard Sync Works

### 1. Local Clipboard Change Detected

```
OS clipboard watcher
        ↓
Platform/daemon worker
        ↓
Capture + normalize representations
        ↓
Application use case persists event + emits updates
```

### 2. Content Materialization

The system transforms raw clipboard data into storable representations:

```
Raw clipboard content
        ↓
Select/derive representations (text, image, file, thumbnail)
        ↓
Encrypt with XChaCha20-Poly1305
        ↓
Store metadata in SQLite
        ↓
Store blobs/files on disk
```

### 3. Distribute and Reflect Updates

```
Clipboard event stored
        ↓
Daemon / network layer fans out updates to paired peers
        ↓
Frontend receives realtime status and clipboard refresh signals
        ↓
Remote-origin events avoid re-capture loops via origin tracking
```

## Current State

- 16 crate 的六边形模块化架构已完成，生产代码运行在此之上
- GUI 进程与 daemon 进程完全分离（ADR-008），GUI 是纯 HTTP/WS 客户端
- CLI (`uniclip`) 同样通过 daemon client 与后台通信，不直接链接 iroh/diesel
- 若文档与代码冲突，以代码为准并更新文档

## Development Setup

### Prerequisites

- **Bun** (package manager): `curl -fsSL https://bun.sh/install | bash`
- **Rust**: install via `rustup`
- **Node.js** (via nvm or system package manager)
- **Tauri CLI**: `cargo install tauri-cli`

### Quick Start

```bash
# Install dependencies
bun install

# Frontend-only dev server
bun run dev

# Full Tauri app with Rust backend
bun run tauri:dev

# Frontend tests
bun run test

# Rust tests
(cd src-tauri && cargo test --workspace)

# Build for production
bun run tauri build
```

### Directory Navigation

```
uniclipboard-desktop/
├── src/                      # Frontend (React + TypeScript)
│   ├── pages/               # Route pages (Dashboard, Devices, Settings)
│   ├── components/          # Reusable UI components
│   ├── store/               # Redux slices
│   └── api/                 # Tauri command invocations
│
├── src-tauri/               # Backend (Rust)
│   ├── crates/              # Modular architecture (see above)
│   ├── src/                 # Tauri GUI entrypoint and platform glue
│   └── tauri.conf.json      # Tauri configuration
│
├── docs/                    # Documentation (this file)
└── CLAUDE.md                # Instructions for Claude Code
```

## Key Design Decisions

### Why Hexagonal Architecture?

**Problem**: Traditional layered architecture creates tight coupling between business logic and infrastructure (database, network, OS APIs).

**Solution**: Hexagonal Architecture (Ports and Adapters) separates concerns:

- **Ports** (interfaces in uc-core): Define what the application needs
- **Adapters** (implementations in uc-infra/uc-platform): Provide external dependencies

**Benefits**:

- Test business logic without real database/network
- Swap implementations (e.g., PostgreSQL → SQLite) without changing use cases
- Clear separation of concerns enforced by Rust module system

### Why Tauri 2?

**Problem**: Electron is resource-heavy and has limited native access.

**Solution**: Tauri 2 uses Rust backend + Web frontend:

- **Smaller bundle size**: ~3MB vs ~200MB (Electron)
- **Better performance**: Native Rust code for heavy operations
- **System access**: Rust crates for clipboard, file system, networking

### Why iroh for P2P?

**Problem**: Building reliable P2P networking from scratch is complex.

**Solution**: iroh provides:

- QUIC-based NAT traversal (hole punching + relay fallback)
- Ed25519 身份认证（NodeId = public key）
- mDNS LAN 发现
- 内置 relay 用于无法直连时的加密中继

### Why XChaCha20-Poly1305 for Encryption?

**Requirements**:

- Authenticated encryption (detect tampering)
- Fast performance for real-time sync
- Cross-platform availability

**Solution**: XChaCha20-Poly1305:

- **Authenticated**: Detects tampering and preserves integrity
- **Nonce-friendly**: Large nonces simplify safe random generation for many encrypted payloads
- **Well-supported in Rust**: Matches the current backend implementation

## Security Architecture

### Encryption Flow

```
User Clipboard Content
        ↓
Generate Random nonce
        ↓
Derive Key from User Password (Argon2id)
        ↓
XChaCha20-Poly1305 Encrypt (Content + nonce + key)
        ↓
Store ciphertext + metadata
```

### Key Management

- **Password Storage**: System keyring (macOS Keychain, Windows Credential Manager, Linux Secret Service)
- **Salt**: Stored in `~/.uniclipboard/salt` (unique per installation)
- **Key Derivation**: Argon2id (memory-hard, resistant to GPU attacks)
- **No Plaintext**: Clipboard content never stored unencrypted

### Network Security

- **P2P 传输**：iroh QUIC 通道加密（Ed25519 身份验证）+ 应用层 AEAD 双重加密
- **前端实时更新**：daemon WebSocket 事件订阅（仅限 127.0.0.1）
- **设备认证**：iroh NodeId 指纹 + HMAC 挑战 - 应答配对协议
- **LAN-only 模式**：可配置禁用 relay，完全局域网内运行

## Performance Considerations

### Clipboard History Limits

- **Default**: 1000 entries per device
- **Configurable**: Via settings (trade-off: disk space vs history)
- **Pruning**: Automatic cleanup when limit exceeded (FIFO)

### Blob Storage

Large clipboard items (images, rich text) stored separately:

- **Inline**: Text content < 10KB stored in database
- **Blob**: Large content stored in `~/.uniclipboard/blobs/`
- **Reference**: Database stores blob hash (SHA-256)

### Network Optimization

- **Deduplication**: Identical content sent once per session
- **Compression**: Large blobs compressed before sync
- **Batching**: Multiple clipboard changes batched in single network call

## Testing Strategy

### Unit Tests

- **Domain models**: Test business rules in isolation
- **Use cases**: Test application logic with mock ports
- **Repository mappers**: Test entity ↔ domain conversion

### Integration Tests

- **Bootstrap wiring**: Verify dependency injection works
- **Database migrations**: Test schema changes
- **End-to-end**: Full clipboard sync flow (hardware tests)

### Test Commands

```bash
# Run all Rust tests
cd src-tauri && cargo test --workspace

# Run specific crate tests
cd src-tauri && cargo test -p uc-core
cd src-tauri && cargo test -p uc-app

# Run integration tests
cd src-tauri && cargo test --test '*_integration_test' -- --ignored

# Run with logging
cd src-tauri && RUST_LOG=debug cargo test --workspace
```

## Further Reading

- [Architecture Principles](architecture/principles.md) - Deep dive into Hexagonal Architecture
- [Bootstrap System](architecture/bootstrap.md) - How dependency injection works
- [Module Boundaries](architecture/module-boundaries.md) - What each module can/cannot do
- [Error Handling](guides/error-handling.md) - Error handling strategy
- [DeepWiki](https://deepwiki.com/UniClipboard/UniClipboard) - Interactive diagrams

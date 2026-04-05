# Project Overview

**UniClipboard Desktop** is a privacy-first, cross-device clipboard synchronization tool. It combines a React/Tauri desktop UI with a modular Rust backend and daemon so devices can pair, sync clipboard content, and manage encrypted local history.

## Technology Stack

| Layer                | Technology                    | Purpose                              |
| -------------------- | ----------------------------- | ------------------------------------ |
| **Frontend**         | React 18 + TypeScript + Vite  | UI and user interaction              |
| **State Management** | Redux Toolkit + RTK Query     | Client state and API caching         |
| **UI Components**    | Tailwind CSS + Shadcn/ui      | Responsive, accessible components    |
| **Backend**          | Rust + Tauri 2                | Native integration and system access |
| **Database**         | SQLite + Diesel ORM           | Local clipboard history storage      |
| **Realtime / IPC**   | Daemon WS/API bridge          | Background sync and frontend updates |
| **P2P Network**      | libp2p (Rust)                 | Device discovery, pairing, sync      |
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

UniClipboard is organized around **Hexagonal Architecture (Ports and Adapters)**, while still carrying some integration-heavy code in the Tauri entrypoint during the migration.

### High-Level Flow

```
┌──────────────────────────────────────────────────────────────┐
│                    User Interface / GUI                      │
│       (React UI, route gating, panels, Tauri commands)      │
└──────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────┐
│                   Application / Orchestration                │
│ (setup, pairing, clipboard flows, security, space access)   │
└──────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────┐
│                         Core Domain                          │
│    (clipboard, device, security, network, settings, IDs)    │
└──────────────────────────────────────────────────────────────┘
                              ↑
              ┌───────────────┼───────────────┐
              │               │               │
┌──────────────────────────┐ ┌──────────────────────────┐ ┌──────────────────────────┐
│   Infrastructure         │ │   Platform Adapters      │ │   Background Runtime     │
│  - Database (SQLite)     │ │  - Clipboard (OS API)    │ │  - uc-daemon             │
│  - File System / Blobs   │ │  - Network (libp2p)      │ │  - WS/API bridge         │
│  - Keyring / Crypto      │ │  - Notifications         │ │  - sync workers          │
│  - Settings              │ │  - App lifecycle hooks   │ │  - event fan-out         │
└──────────────────────────┘ └──────────────────────────┘ └──────────────────────────┘
```

### Key Architectural Principles

1. **Dependency Inversion**: Use cases depend on ports, not concrete infrastructure
2. **External Isolation**: OS, DB, crypto, and networking sit behind adapters
3. **Daemon-Aware Design**: The GUI and background runtime coordinate through explicit APIs/events
4. **Testability**: Core and application logic can be tested without real infrastructure

## Crate Structure

```
src-tauri/crates/
├── uc-core/              # Domain models, IDs, protocols, ports
├── uc-app/               # Use cases and orchestration
├── uc-infra/             # DB, file system, crypto, settings implementations
├── uc-platform/          # Clipboard, OS, network/runtime adapters
├── uc-tauri/             # Tauri commands, adapters, bootstrap glue
├── uc-bootstrap/         # Shared bootstrap context/builders
├── uc-daemon/            # Background daemon runtime and APIs
├── uc-daemon-client/     # GUI-side daemon client and realtime bridge
├── uc-observability/     # Logging/tracing helpers
├── uc-clipboard-probe/   # Clipboard probing helpers
└── uc-cli/               # CLI utilities
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

The project is in an active architecture migration. The important current reality is:

- The modular crate layout is real and already used in production code
- The GUI process and daemon process are both important runtime pieces
- Some documentation still describes older migration phases, removed directories, or obsolete implementation details
- Prefer describing boundaries and runtime responsibilities over quoting stale completion percentages

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

### Why libp2p for P2P?

**Problem**: Building reliable P2P networking from scratch is complex.

**Solution**: libp2p provides:

- NAT traversal (hole punching)
- Peer discovery (mDNS)
- Multiple transport protocols
- Battle-tested by IPFS, Polkadot, etc.

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

- **Pairing / peer transport**: libp2p-based networking for device discovery and pairing flows
- **Frontend realtime updates**: daemon WS/API bridge for GUI synchronization
- **Device Authentication**: Peer ID fingerprint verification

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

# External Integrations

**Analysis Date:** 2026-03-02

## APIs & External Services

**Update Management:**

- GitHub Releases API - Fetches update artifacts and manifests
  - SDK/Client: `tauri-plugin-updater` 2.9.0
  - Manifest endpoint: `https://uniclipboard.github.io/UniClipboard/{channel}.json`
  - Channels: `stable`, release channels with version placeholders
  - Implementation: `src-tauri/crates/uc-tauri/src/commands/updater.rs`
  - Public key signing verification included in `tauri.conf.json`

## Data Storage

**Databases:**

- SQLite 3.35+ (bundled via `libsqlite3-sys`)
  - Connection pool: r2d2 connection pooling via Diesel
  - Client/ORM: Diesel 2.3.5 with query builder and migrations
  - Location: Platform-specific app data directory
  - Schema: Migrations in `src-tauri/crates/uc-infra/src/db/migrations/`
  - Tables: clipboard_entries, clipboard_selections, clipboard_events, representations, thumbnails, blobs, devices, paired_devices

**File Storage:**

- Local filesystem only - No cloud storage integration
- Paths managed via `dirs` crate for platform-specific locations:
  - macOS: `~/Library/Application Support/com.uniclipboard/`
  - Linux: `~/.local/share/com.uniclipboard/`
  - Windows: `%LOCALAPPDATA%\com.uniclipboard\`
- Log files:
  - macOS: `~/Library/Logs/com.uniclipboard/`
  - Linux: `~/.local/share/com.uniclipboard/logs/`
  - Windows: `%LOCALAPPDATA%\com.uniclipboard\logs/`

**Caching:**

- None - Caching handled in-memory via Redux (frontend) and Tokio channels (backend)
- RTK Query provides automatic response caching on frontend

## Authentication & Identity

**Auth Provider:**

- Custom local-first approach with no external authentication
- Password-based encryption: Argon2id key derivation
- Implementation: `src-tauri/crates/uc-infra/src/security/encryption.rs`

**Key Storage:**

- JSON key slot store with KEK-wrapped master keys
  - Implementation: `src-tauri/crates/uc-infra/src/fs/key_slot_store.rs`
- Master key derivation: Argon2id from user passphrase
- System keyring access via `keyring` crate (3.6.3) with platform-native backends
  - macOS: Keychain (`apple-native`)
  - Windows: Credential Manager (`windows-native`)
  - Linux: Secret Service (`linux-native-sync-persistent`)

**Encryption:**

- Algorithm: XChaCha20-Poly1305 AEAD cipher
- Key derivation: Argon2 Argon2id configuration
- Nonce: 192-bit random nonce per encryption (XChaCha20 property)
- Applies to: Clipboard content transmission and storage

## Monitoring & Observability

**Error Tracking:**

- Sentry (0.46.1) - Error and crash monitoring
  - Frontend DSN: `VITE_SENTRY_DSN` environment variable
  - Backend DSN: `SENTRY_DSN` environment variable
  - Backend implementation: `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs`
  - Frontend implementation: `src/observability/sentry.ts`
  - Features:
    - 10% trace sampling (frontend)
    - 100% replay on error (frontend)
    - Tracing integration on backend
    - Sensitive data redaction before sending
    - Platform tagging (macOS, Windows, Linux)

**Logs:**

- Backend: Tracing-based structured logging
  - Output: stdout + file
  - File paths: Platform-specific (see File Storage above)
  - Format: `YYYY-MM-DD HH:MM:SS.mmm [LEVEL] [file.rs:line] [target] message`
  - Levels: debug (dev), info (prod)
  - Filters: libp2p_mdns, Tauri internal spans suppressed
  - Implementation: `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs`

- Frontend: Console (Tauri plugin-log)
  - Piped to browser DevTools console
  - Implementation: `src/main.tsx` (attachConsole from `@tauri-apps/plugin-log`)

- Legacy: log crate (being phased out)
  - Development: Webview console
  - Production: stdout

## CI/CD & Deployment

**Hosting:**

- No centralized backend server - Peer-to-peer architecture
- Update artifacts: GitHub Pages (`uniclipboard.github.io`)
- Distribution: GitHub Releases

**CI Pipeline:**

- GitHub Actions - Multi-platform builds (manual trigger)
  - Platforms: macOS (aarch64, x86_64), Ubuntu 22.04, Windows
  - Creates release artifacts with auto-update manifests
  - Uses `tauri-action` for cross-platform builds

**Deployment:**

- Self-contained desktop binaries (no dependencies)
- In-app updater via `tauri-plugin-updater`
- Manifest-based update checking with signature verification

## Environment Configuration

**Required env vars:**

- No required env vars for basic operation (all defaults provided)
- Optional for observability:
  - `SENTRY_DSN` - Backend error tracking
  - `VITE_SENTRY_DSN` - Frontend error tracking

**Secrets location:**

- Encryption keys: JSON key slot store with KEK-wrapped master keys
- No API keys or credentials in code
- Password: User-provided via UI at initialization

**Build-time configuration:**

- Environment variables injected via Vite:
  - `import.meta.env.VITE_SENTRY_DSN`
  - `import.meta.env.VITE_APP_VERSION`
  - `import.meta.env.MODE` (development/production)

## Webhooks & Callbacks

**Incoming:**

- None - No external webhooks received

**Outgoing:**

- None - All communication is direct P2P via libp2p
  - LAN communication: TCP + mDNS discovery
  - Remote communication: TCP via network address/port

**Internal Event System:**

- Tokio MPSC channels for inter-crate communication
- Tauri IPC for frontend-backend events
- Platform runtime event bus (see `uc-platform/runtime/`)

## Network Communication

**P2P Network:**

- libp2p 0.56 stack for peer-to-peer networking
  - Transports: TCP, QUIC
  - Security: Noise protocol encryption
  - Multiplexing: Yamux
  - Discovery: mDNS (Multicast DNS)
  - Protocols:
    - Identify - Peer information exchange
    - Request/Response - Clipboard sync messages
  - Configuration: `src-tauri/crates/uc-platform/src/network/` (in development)

**LAN Sync:**

- mDNS-based peer discovery on local network
- TCP/QUIC direct connections between devices
- WebSocket support (future/legacy compatibility)

**Remote Sync:**

- WebDAV support (documented, implementation in progress)
- Network-based P2P over public internet (requires peer address/port)

**Clipboard Sync Protocol:**

- Message format: JSON (via libp2p-request-response)
- Content encryption: XChaCha20-Poly1305
- Automatic discovery on LAN, manual pairing for remote

## Platform-Specific Integrations

**macOS:**

- System clipboard access via native APIs
- Keychain integration (via `keyring` crate `apple-native` backend)
- Transparent title bar and window effects
- Autostart via LaunchAgent (tauri-plugin-autostart)

**Windows:**

- Windows clipboard access
- Credential Manager key storage (via `keyring` crate `windows-native` backend)
- Window glass effect (hudWindow)
- Registry autostart configuration

**Linux:**

- X11/Wayland clipboard access
- Secret Service key storage (via `keyring` crate `linux-native-sync-persistent` backend)
- Standard window decorations

---

_Integration audit: 2026-03-02_

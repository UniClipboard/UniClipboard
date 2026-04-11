# RUNTIME.md — UniClipboard Desktop Runtime Context

> This file provides GSD agents with accurate runtime environment information, avoiding hallucinated paths or configurations.

---

## 1. Technology Stack

### Frontend Stack

| Technology      | Version |
| --------------- | ------- |
| React           | ^18.3.1 |
| TypeScript      | \~5.6.3 |
| Bun             | 1.3.4   |
| Vite            | ^6.4.1  |
| Tailwind CSS    | ^4.1.18 |
| Redux Toolkit   | ^2.11.2 |
| @tauri-apps/api | ^2.9.1  |

### Backend Stack

| Technology        | Version               |
| ----------------- | --------------------- |
| Rust              | 1.92.0 (edition 2021) |
| Tauri             | 2.x                   |
| Tokio             | 1.28+                 |
| Axum              | 0.7                   |
| tokio-tungstenite | 0.24                  |

### Key Tauri Plugins

- `tauri-plugin-log`: 2.7.1
- `tauri-plugin-stronghold`: \~2.3.1
- `tauri-plugin-global-shortcut`: ^2.3.1
- `tauri-plugin-notification`: ^2.3.3
- `tauri-plugin-updater`: ^2.9.0
- `tauri-plugin-autostart`: \~2.5.1

### Crypto/Security Libraries

- `jsonwebtoken`: 10
- `argon2`: 0.5.3
- `blake3`: 1.8.2
- `sha2`: 0.10

---

## 2. Project Structure

### Top-Level Directories

```
uniclipboard-desktop/
├── src/                      # Frontend (React/TypeScript/Tauri webview)
├── src-tauri/               # Rust backend
│   ├── crates/             # Rust workspace submodules
│   ├── Cargo.toml          # Workspace root config
│   ├── tauri.conf.json     # Tauri configuration
│   └── build.rs            # Build script
├── package.json             # Frontend dependencies
├── vite.config.ts          # Vite configuration
└── .env / .env.example     # Environment variables
```

### Rust Crates Architecture (Hexagonal)

```
src-tauri/crates/
├── uc-core/         # Core domain model (no external dependencies) — pure interface contracts
├── uc-app/          # Use case layer (depends on uc-core)
├── uc-infra/        # Infrastructure (SQLite, FS, Crypto adapters)
├── uc-platform/    # Platform adapters (macOS app dirs, clipboard)
├── uc-tauri/        # Tauri command/event bridge layer
├── uc-bootstrap/   # Composition root (only crate depending on all others)
├── uc-daemon/       # Headless HTTP API service (axum + WebSocket)
├── uc-daemon-client/# Daemon HTTP/WS client library
├── uc-observability/# Tracing infrastructure
└── uc-cli/         # Command-line tool
```

### Frontend Directory (`src/`)

```
src/
├── api/daemon/      # Daemon HTTP client (clipboard, encryption, pairing, settings, storage)
├── components/      # React components (clipboard, device, layout, setting, ui)
├── hooks/           # Custom Hooks (daemon, device, clipboard, setup)
├── lib/             # Utilities (daemon-auth, daemon-ws, tauri-command)
├── store/slices/    # Redux slices (clipboard, devices, fileTransfer, stats)
├── types/           # TypeScript type definitions
└── observability/   # Observability module
```

### Key Configuration Files

| File                  | Path                        |
| --------------------- | --------------------------- |
| Rust Workspace        | `src-tauri/Cargo.toml`      |
| Tauri Config          | `src-tauri/tauri.conf.json` |
| Tauri Capabilities    | `src-tauri/capabilities/`   |
| Frontend Dependencies | `package.json`              |
| Vite Config           | `vite.config.ts`            |
| Environment Variables | `.env` / `.env.example`     |

---

## 3. Development Environment

### Core Development Commands

```bash
# Frontend development
bun run dev              # Vite dev server (:1420)

# Tauri development (recommended)
bun run tauri dev        # Start GUI + daemon

# Dual-device development
bun run tauri:dev:dual   # Start both peerA and peerB

# Build
bun run build            # Frontend build
bun tauri build          # Full build

# Testing
bun test                 # Frontend tests (Vitest)
npx vitest run           # Same, explicit call
cd src-tauri && cargo test  # Rust tests
```

### Multi-Device Development Config

```bash
# peerA — active mode (full permissions)
UC_PROFILE=a UC_CLIPBOARD_MODE=full tauri dev

# peerB — passive mode (isolated data directory)
UC_PROFILE=b UC_CLIPBOARD_MODE=passive tauri dev
```

### Environment Variables

#### Frontend (Vite)

| Variable           | Description                    |
| ------------------ | ------------------------------ |
| `VITE_SENTRY_DSN`  | Sentry error monitoring        |
| `VITE_APP_VERSION` | Application version            |
| `VITE_SEQ_URL`     | Seq log service URL            |
| `UNICLIPBOARD_ENV` | Runtime environment identifier |

#### Backend / Daemon

| Variable                     | Description                              |
| ---------------------------- | ---------------------------------------- |
| `UC_LOG_PROFILE`             | Log level (dev/prod/debug_clipboard/cli) |
| `UC_SEQ_URL`                 | Seq log service URL                      |
| `UC_PROFILE`                 | Data directory isolation (a/b/cli)       |
| `UC_CLIPBOARD_MODE`          | Clipboard mode (full/passive)            |
| `UC_DISABLE_SINGLE_INSTANCE` | Disable single-instance lock             |

### Ports and Addresses

| Address                          | Purpose                     |
| -------------------------------- | --------------------------- |
| `http://localhost:1420`          | Vite frontend dev server    |
| `ws://host:1421`                 | Vite HMR WebSocket          |
| `http://localhost:5341`          | Seq log aggregation service |
| `127.0.0.1:<ephemeral>`          | Daemon HTTP API             |
| `ws://<daemon-host>:<ephemeral>` | Daemon WebSocket            |

---

## 4. Runtime Characteristics

### Process Architecture

```
┌─────────────────────────────────────┐
│     Tauri Main Process (Rust)        │
│  uc-tauri: Tray/shortcuts/notifs    │
│  - Spawns daemon subprocess          │
│  - Emits daemon://connection-info    │
├─────────────────────────────────────┤
│     Webview (React)                  │
│  - Listens for connection-info event │
│  - HTTP requests → Daemon           │
│  - WebSocket connection → Daemon    │
└─────────────────────────────────────┘
              ↕
┌─────────────────────────────────────┐
│  Daemon Subprocess (externalBin)    │
│  uc-daemon: axum HTTP + WS server   │
│  uc-app: Business logic             │
│  uc-infra: DB/FS/Crypto            │
│  uc-core: Domain model             │
└─────────────────────────────────────┘
```

### Communication Mechanisms

**Tauri IPC** (legacy, being phased out)

- Used only for native features: tray, shortcuts, auto-update
- `daemon://connection-info`: one-shot event carrying `{ baseUrl, wsUrl, token }`

**Daemon HTTP API** (primary path)

- Auth: POST `/auth/connect` → JWT token
- Frontend calls via `src/api/daemon/client.ts` singleton

**Daemon WebSocket** (real-time events)

- Frontend `src/lib/daemon-ws.ts` connects directly
- Auto-reconnect + auto-resubscribe all topics

### Data Storage

| Path                                          | Purpose               |
| --------------------------------------------- | --------------------- |
| `~/Library/Application Support/app.uniclipboard.desktop[-<profile>]/` | Main data directory   |
| `~/Library/Caches/app.uniclipboard.desktop[-<profile>]/`              | Cache directory       |
| `{app_data}/uniclipboard.db`                  | SQLite database       |
| `{app_data}/vault/`                           | Encrypted key storage |
| `{app_data}/settings.json`                    | User settings         |
| `{app_data}/logs/`                            | Log files             |

---

## 5. Current Milestone Status

**Active Milestone**: M003-fbgash

**Goal**: Replace Tauri invoke() with daemon HTTP + WebSocket as the primary frontend-backend path; reduce uc-tauri to a thin shell for native features only.

| Slice                                     | Status      |
| ----------------------------------------- | ----------- |
| S01: Frontend Daemon HTTP Client & Auth   | ✅ Complete |
| S02: Frontend Clipboard API Migration     | ✅ Complete |
| S03: Frontend WebSocket Direct Connection | ✅ Complete |
| S04: uc-tauri Command Cleanup             | ✅ Complete |
| S05: Integration Testing & Security Audit | ⬜ Pending  |

---

## 6. Critical Development Constraints

### Cargo Command Location

- **All Rust commands MUST be executed from** `src-tauri/` **directory**
- Example: `cd src-tauri && cargo build`
- Cargo.toml is NOT in the project root

### Hexagonal Architecture Rules

- `uc-core` must NOT depend on any other uc-\* crate
- All external capabilities MUST go through interfaces defined in `uc-core/ports/`
- `uc-infra` and `uc-platform` must only depend on `uc-core`

### WebSocket Authentication

- Auth token is passed via URL query parameter: `ws://host/ws?auth=Session%20TOKEN`
- Browsers do not allow custom headers on WebSocket upgrade requests

### Frontend Testing

- Use `npx vitest run`, NOT `bun test`
- `bun test` does not support vitest-specific APIs (vi.fn(), vi.mock, etc.)

---

## 7. Common Operations

### Start Development

```bash
bun run tauri dev
```

### Run Frontend Tests

```bash
npx vitest run src/hooks/__tests__
```

### Run Rust Tests

```bash
cd src-tauri && cargo test -p uc-core
```

### View Logs

```bash
tail -f ~/Library/Application\ Support/app.uniclipboard.desktop*/logs/*
```

### Clear Cache

```bash
rm -rf ~/Library/Caches/uniclipboard/
```

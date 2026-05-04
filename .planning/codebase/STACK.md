# Technology Stack

**Analysis Date:** 2026-03-02

## Languages

**Primary:**

- Rust (Edition 2021) - Backend application and cross-platform core logic
- TypeScript (~5.6.3) - Frontend React application
- JavaScript - Build scripts and configuration

**Secondary:**

- Objective-C (macOS platform-specific integration via `objc` and `objc2` crates)

## Runtime

**Environment:**

- Tauri 2.9.x - Cross-platform desktop framework using WebView + Rust backend
- Node.js (via Bun) - Development tooling and package management

**Package Manager:**

- Bun (primary) - Significantly faster than npm/yarn for install and dev
- npm (legacy lockfile exists: `package-lock.json`)
- Cargo - Rust package management and workspace build system

## Frameworks

**Core:**

- Tauri 2.9.1 - Desktop application framework with IPC bridge between frontend and backend
- React 18.3.1 - Frontend UI framework
- Vite 6.4.1 - Frontend build tool and dev server (port 1420)
- Tokio 1.28 (full features) - Async runtime for Rust backend

**State Management:**

- Redux Toolkit 2.11.2 - Frontend state management
- RTK Query (included with Redux Toolkit) - Data fetching and caching
- React Context - Supplementary state (Settings, Search, Shortcuts, Updates)

**UI Components:**

- Shadcn/ui (built on Radix UI primitives) - Accessible component library
- Radix UI (@radix-ui/\*) - Underlying accessible UI primitives
- Tailwind CSS 4.1.18 - Utility-first CSS framework
- Headless UI 2.2.9 - Additional unstyled component utilities
- Lucide React 0.562.0 - Icon library
- Framer Motion 12.23.26 - Animation library

**Testing:**

- Vitest 4.0.17 - Frontend unit test runner
- Jest DOM 6.9.1 - DOM matchers for testing
- React Testing Library 16.3.2 - React component testing utilities

**Linting/Formatting:**

- ESLint 9.39.2 - Frontend code linting (with React and TypeScript plugins)
- TypeScript ESLint 8.50.1 - TypeScript-specific linting rules
- Prettier 3.7.4 - Code formatter
- Husky 9.1.7 - Git hooks for pre-commit checks
- Lint-staged 16.2.7 - Run linters on staged files

**Build/Dev:**

- Tailwind CSS with Vite plugin (@tailwindcss/vite) - CSS compilation
- Vite React plugin (@vitejs/plugin-react) - React Fast Refresh support
- Cargo LLVM coverage (`cargo-llvm-cov`) - Rust test coverage reporting

## Key Dependencies

**Cryptography & Security:**

- XChaCha20-Poly1305 (`chacha20poly1305` 0.10.1) - AEAD encryption for clipboard content
- Argon2 (0.5.3) - Password hashing (Argon2id)
- SHA-2 (0.10) - Cryptographic hashing
- Blake3 (1.8.2) - Fast cryptographic hashing
- Zeroize (1.8.2) - Secure memory clearing for sensitive data

**Networking:**

- libp2p (0.56) - P2P networking with:
  - TCP transport
  - Noise encryption protocol
  - Yamux multiplexing
  - mDNS discovery
  - Identify protocol
  - Request/response protocol
  - QUIC transport
- libp2p-request-response (0.29) - JSON protocol support
- libp2p-stream (0.4.0-alpha) - Stream abstractions
- local-ip-address (0.6) - Local IP detection

**Database & Persistence:**

- Diesel 2.3.5 - ORM and query builder for SQLite
- SQLite (via `libsqlite3-sys` bundled) - Embedded database
- Diesel migrations 2.2.0 - Database schema versioning
- r2d2 (via Diesel) - Connection pooling

**Clipboard Access:**

- arboard 3.4 - Cross-platform clipboard library
- clipboard-rs 0.3 - Alternative clipboard implementation
- clipboard-win 5.4 - Windows-specific clipboard access

**Image Processing:**

- image 0.25 (with PNG, JPEG, WebP support) - Image encoding/decoding
- png 0.18 - PNG-specific utilities

**Logging & Error Tracking:**

- tracing (0.1) - Structured logging with spans and context
- tracing-subscriber (0.3) - Tracing layer configuration
- tracing-appender (0.2) - File-based log output
- tracing-log (0.2) - Bridge from `log` crate to `tracing`
- Sentry (0.46.1) - Error tracking and crash reporting with tracing integration
- log (0.4.29) - Legacy logging facade (being phased out)

**Serialization & Configuration:**

- serde (1) with derive - Serialization framework
- serde_json (1) - JSON support
- TOML (0.8) - TOML configuration parsing
- @ltd/j-toml (1.38.0) - Frontend TOML parsing

**Utilities & Infrastructure:**

- async-trait (0.1) - Async trait support
- anyhow (1.0) - Flexible error handling
- thiserror (2.0.17) - Typed error definitions
- tokio-util (0.7) - Tokio utilities
- futures (0.3.31) - Async utilities
- chrono (0.4) - Date/time handling
- uuid (1.10.0) - UUID generation (v4)
- rand (0.8-0.9.2) - Random number generation
- base64 (0.22) - Base64 encoding
- base32 (0.5) - Base32 encoding
- hex (0.4.3) - Hexadecimal encoding
- bytes (1.7) - Byte buffer utilities
- once_cell (1.19) - One-time initialization
- keyring (3.6.3) - System keyring access with platform-specific backends
- config (0.15.19) - Configuration management
- gethostname (1.1) - Device hostname retrieval
- dirs (6.0) - Platform-specific directory paths
- i18next (25.7.3) - Frontend internationalization
- react-i18next (16.5.0) - React i18n integration

**Tauri Plugins:**

- tauri-plugin-updater (2.9.0) - In-app update system
- tauri-plugin-autostart (2.5.1) - Auto-start on system boot
- tauri-plugin-single-instance (2) - Prevent multiple app instances
- tauri-plugin-opener (2.5.2) - Open URLs/files with system handlers
- tauri-plugin-log (2.7.1) - Frontend logging bridge to backend

**Frontend Utilities:**

- react-router-dom (7.11.0) - Client-side routing
- react-hotkeys-hook (5.2.1) - Keyboard shortcut handling
- react-icons (5.5.0) - Icon set library
- react-redux (9.2.0) - React bindings for Redux
- sonner (2.0.7) - Toast notifications
- class-variance-authority (0.7.1) - Type-safe component variants
- clsx (2.1.1) - Conditional classname utility
- tailwind-merge (3.4.0) - Merge Tailwind class conflicts

**Platform-Specific (macOS):**

- objc (0.2) - Objective-C runtime bindings
- objc2 (0.6) - Modern Objective-C bindings
- objc2-app-kit (0.3) - AppKit framework bindings
- objc2-foundation (0.3) - Foundation framework bindings

**Platform-Specific (Windows):**

- winapi (0.3) - Windows API bindings (user, base, min definitions)

## Configuration

**Environment:**

- `TAURI_DEV_HOST` - Dev server hostname for hot module reload
- `UNICLIPBOARD_ENV` - Application environment (development/production)
- `UC_DISABLE_SINGLE_INSTANCE` - Override single-instance enforcement (development only)
- `UC_PROFILE` - User profile selection (development multi-peer testing)
- `UC_CLIPBOARD_MODE` - Clipboard behavior (full/passive mode)
- `SENTRY_DSN` - Sentry error tracking endpoint (optional, backend)
- `VITE_SENTRY_DSN` - Sentry error tracking endpoint (optional, frontend)
- `VITE_APP_VERSION` - Application version for Sentry tagging
- `RUST_LOG` - Tracing level filter (debug/info/warn/error)

**Build:**

- `tsconfig.json` - TypeScript compiler configuration (target ES2020, strict mode, path aliases)
- `vite.config.ts` - Vite dev server (port 1420, HMR configuration, Tailwind integration)
- `eslint.config.js` - ESLint rules (TypeScript, React, import ordering)
- `.prettierrc` - Code formatter settings (semi: false, singleQuote: true, 100 char width)
- `src-tauri/Cargo.toml` - Rust workspace manifest with 5 member crates
- `src-tauri/tauri.conf.json` - Tauri app configuration (updater endpoints, window settings)

**Database:**

- Diesel migrations managed in `src-tauri/crates/uc-infra/src/db/migrations/`
- SQLite database stored in platform-specific application data directory

## Platform Requirements

**Development:**

- macOS, Linux, or Windows with Rust toolchain (1.70+)
- Node.js/Bun for frontend tooling
- Tauri 2.x CLI (`@tauri-apps/cli`)
- Cargo and Rust stable channel

**Production:**

- Deployment: Desktop application via self-contained binary
- Update mechanism: GitHub releases with `uniclipboard.github.io` manifest
- Supported platforms: macOS (Apple Silicon + x86_64), Linux, Windows
- No server infrastructure required (peer-to-peer and local-first)

**Release Optimization:**

- LTO (Link Time Optimization) enabled
- Panic mode set to `abort` for smaller binaries
- Debug symbols stripped
- Optimized for binary size (`opt-level = "z"`)

---

_Stack analysis: 2026-03-02_

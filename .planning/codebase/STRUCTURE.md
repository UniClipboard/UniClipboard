# Codebase Structure

**Analysis Date:** 2026-03-11

## Directory Layout

```
UniClipboard/
├── src/                        # Frontend (React + TypeScript)
│   ├── api/                    # Tauri command invoke wrappers
│   ├── components/             # Reusable UI components
│   │   ├── clipboard/          # Clipboard list, item, preview components
│   │   ├── device/             # Device list/card components
│   │   ├── layout/             # Header, sidebar, shell components
│   │   ├── setting/            # Settings form components
│   │   ├── ui/                 # Shadcn/ui primitives (Radix-based)
│   │   └── feedback/           # Toast, dialog, notification components
│   ├── contexts/               # React Context providers
│   ├── hooks/                  # Custom React hooks
│   ├── i18n/                   # Internationalization (locales/)
│   ├── layouts/                # Route layout wrappers
│   ├── lib/                    # Utilities (cn, protocol, tauri-command)
│   ├── observability/          # Frontend trace/observability helpers
│   ├── pages/                  # Route pages
│   ├── shortcuts/              # Keyboard shortcut definitions
│   ├── store/                  # Redux Toolkit store
│   │   └── slices/             # Redux slices per domain
│   ├── styles/                 # Global CSS, theme files
│   └── types/                  # Shared TypeScript type declarations
│
├── src-tauri/                  # Backend (Rust + Tauri)
│   ├── src/
│   │   ├── main.rs             # App entry point, Tauri builder, command registration
│   │   └── plugins/            # Platform-specific plugin wrappers
│   ├── crates/
│   │   ├── uc-core/            # Domain models + port trait definitions
│   │   ├── uc-app/             # Use case implementations
│   │   ├── uc-infra/           # Infrastructure implementations (DB, crypto, FS)
│   │   ├── uc-platform/        # Platform adapters (libp2p, clipboard watcher)
│   │   ├── uc-tauri/           # Composition root, Tauri commands, events
│   │   ├── uc-observability/   # Logging / Seq CLEF formatter
│   │   └── uc-clipboard-probe/ # Standalone clipboard capability detection binary
│   ├── capabilities/           # Tauri capability/permission definitions
│   ├── gen/                    # Generated Tauri files
│   ├── icons/                  # App icon assets
│   └── test_resources/         # Test fixture files
│
├── .planning/                  # Planning documents and phase tracking
│   ├── codebase/               # Codebase analysis documents (this directory)
│   └── phases/                 # Feature phase planning docs
├── docs/                       # Architecture and design documentation
├── scripts/                    # Build/utility scripts
├── workers/                    # Background worker scripts (if any)
├── assets/                     # Static assets
└── public/                     # Public web assets
```

## Directory Purposes

**`src/api/`:**

- Purpose: Typed wrappers around `invoke()` / Tauri command calls
- Contains: `clipboardItems.ts`, `p2p.ts`, `security.ts`, `setup.ts`, `lifecycle.ts`, `updater.ts`, `vault.ts`
- Key files: `src/api/clipboardItems.ts` — all clipboard CRUD commands and type definitions

**`src/components/clipboard/`:**

- Purpose: All clipboard history UI components
- Contains: `ClipboardContent.tsx`, `ClipboardItem.tsx`, `ClipboardPreview.tsx`, `ClipboardItemRow.tsx`, `ClipboardActionBar.tsx`, `DeleteConfirmDialog.tsx`

**`src/hooks/`:**

- Purpose: Custom hooks for Tauri event subscriptions and derived state
- Contains: `useClipboardEvents.ts` (main clipboard event listener + pagination), `useLifecycleStatus.ts`, `usePlatform.ts`, `useShortcut*.ts`

**`src/contexts/`:**

- Purpose: React Context providers for cross-component state
- Contains: `SettingContext.tsx` (app settings), `SearchContext.tsx`, `ShortcutContext.tsx`, `UpdateContext.tsx`

**`src/store/slices/`:**

- Purpose: Redux Toolkit slice definitions
- Contains: `clipboardSlice.ts`, `devicesSlice.ts`, `statsSlice.ts`
- Pattern: Each slice has async thunks calling `src/api/` functions, plus reducers for Tauri event-driven updates

**`src/lib/`:**

- Purpose: Shared utilities
- Contains: `tauri-command.ts` (`invokeWithTrace` wrapper), `protocol.ts` (generates `uc://` blob/thumbnail URLs), `cn.ts` (Tailwind class merge)

**`src-tauri/crates/uc-core/src/`:**

- Purpose: Zero-dependency domain kernel
- Key files: `ports/mod.rs` (all port trait re-exports), `clipboard/`, `device/`, `security/`, `ids/` (typed ID newtypes), `config/`

**`src-tauri/crates/uc-app/src/usecases/`:**

- Purpose: One file per business operation
- Key files: `delete_clipboard_entry.rs`, `list_clipboard_entries.rs`, `initialize_encryption.rs`, `clipboard/sync_inbound.rs`, `clipboard/sync_outbound.rs`, `pairing/`, `setup/`, `space_access/`

**`src-tauri/crates/uc-infra/src/`:**

- Purpose: Concrete port implementations
- Key subdirs: `db/repositories/` (Diesel SQLite repos), `db/schema.rs` (generated Diesel schema), `db/migrations/` (`src-tauri/crates/uc-infra/migrations/`), `security/` (encryption), `blob/`, `fs/key_slot_store.rs`

**`src-tauri/crates/uc-platform/src/`:**

- Purpose: OS and network platform adapters
- Key files: `adapters/libp2p_network.rs` (P2P transport), `clipboard/` (system clipboard), `runtime/runtime.rs` (`PlatformRuntime`), `ipc/command.rs`, `ipc/event.rs`

**`src-tauri/crates/uc-tauri/src/`:**

- Purpose: Tauri integration layer — the only place all crates are visible
- Key files: `bootstrap/wiring.rs` (DI composition root), `bootstrap/runtime.rs` (`AppRuntime`, `UseCases`), `commands/clipboard.rs`, `commands/encryption.rs`, `commands/pairing.rs`, `events/mod.rs`, `protocol.rs`

**`src-tauri/src/main.rs`:**

- Purpose: Application binary entry point
- Responsibilities: Tauri builder, plugin registration, `invoke_handler![]` command list, custom `uc://` URI scheme, platform-specific window config

## Key File Locations

**Entry Points:**

- `src-tauri/src/main.rs`: Rust binary entry, Tauri setup
- `src/main.tsx` (or `index.html`): Frontend Vite entry
- `src/pages/DashboardPage.tsx`: Primary clipboard history view

**Configuration:**

- `src-tauri/tauri.conf.json`: Tauri app configuration, window config, dev server port (1420)
- `src-tauri/Cargo.toml`: Rust workspace / crate dependencies
- `tsconfig.json`: TypeScript config with `@/*` path alias → `src/*`
- `vite.config.ts`: Vite build config
- `tailwind.config.js`: Tailwind theme config

**Core Logic:**

- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`: Dependency injection root
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs`: `AppRuntime` and `UseCases` accessor
- `src-tauri/crates/uc-core/src/ports/mod.rs`: All port trait re-exports
- `src-tauri/crates/uc-infra/src/db/schema.rs`: Database schema (Diesel generated)
- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs`: P2P network adapter

**Testing:**

- `src-tauri/crates/uc-infra/tests/`: Integration tests for infrastructure
- `src-tauri/crates/uc-app/tests/`: Application-level tests
- `src-tauri/crates/uc-core/tests/`: Domain model tests
- `src/**/__tests__/`: Frontend unit tests (alongside source files)

## Naming Conventions

**Rust Files:**

- `snake_case.rs` for all Rust source files
- Use case files named after the operation: `delete_clipboard_entry.rs`, `list_clipboard_entries.rs`
- Port trait files named after the port: `clipboard_entry_repository.rs`, `encryption.rs`
- Repository implementations: `clipboard_entry_repo.rs` (in `uc-infra/db/repositories/`)

**TypeScript Files:**

- `PascalCase.tsx` for React components
- `camelCase.ts` for hooks, utilities, API modules
- `kebab-case.ts` for context files (e.g., `search-context.ts`, `setting-context.ts`)

**Rust Crates:**

- All prefixed with `uc-` (UniClipboard): `uc-core`, `uc-app`, `uc-infra`, `uc-platform`, `uc-tauri`, `uc-observability`, `uc-clipboard-probe`

**Redux Slices:**

- `src/store/slices/clipboardSlice.ts` — PascalCase with `Slice` suffix

## Where to Add New Code

**New Tauri Command:**

1. Define port trait in `src-tauri/crates/uc-core/src/ports/` if new external contract needed
2. Implement port in `src-tauri/crates/uc-infra/src/` or `uc-platform/src/`
3. Create use case in `src-tauri/crates/uc-app/src/usecases/<domain>/your_use_case.rs`
4. Add accessor method to `UseCases` in `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs`
5. Add command function in `src-tauri/crates/uc-tauri/src/commands/<domain>.rs`
6. Register in `invoke_handler![]` in `src-tauri/src/main.rs`
7. Add TypeScript wrapper in `src/api/<domain>.ts`

**New Frontend Component:**

- Domain UI components: `src/components/<domain>/YourComponent.tsx`
- Generic UI primitives: `src/components/ui/` (Shadcn pattern)
- Page-level components: `src/pages/YourPage.tsx`

**New Redux Slice:**

- Add file: `src/store/slices/yourDomainSlice.ts`
- Register in root store: `src/store/` (check `src/store/hooks.ts` for pattern)

**New Hook:**

- `src/hooks/useYourFeature.ts`

**New Database Table:**

- Create Diesel migration in `src-tauri/crates/uc-infra/migrations/<timestamp>_description/`
- Update schema in `src-tauri/crates/uc-infra/src/db/schema.rs` (regenerate via `diesel migration run`)
- Add model in `src-tauri/crates/uc-infra/src/db/models/`
- Add repository in `src-tauri/crates/uc-infra/src/db/repositories/`
- Define port trait in `src-tauri/crates/uc-core/src/ports/`

**New Platform Adapter:**

- Implement port trait from `uc-core/src/ports/` in `src-tauri/crates/uc-platform/src/adapters/`
- Wire it in `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`

## Special Directories

**`.planning/`:**

- Purpose: GSD planning docs, phase tracking, roadmap
- Generated: No
- Committed: Yes

**`src-tauri/crates/uc-infra/migrations/`:**

- Purpose: Diesel SQL migration files for database schema evolution
- Generated: Partially (scaffold by Diesel CLI, SQL written manually)
- Committed: Yes

**`src-tauri/gen/`:**

- Purpose: Tauri-generated files (capability schemas, etc.)
- Generated: Yes (by Tauri tooling)
- Committed: Yes (required for build)

**`src-tauri/target/`:**

- Purpose: Rust build artifacts
- Generated: Yes
- Committed: No (gitignored)

**`src/components/ui/`:**

- Purpose: Shadcn/ui component library (Radix UI primitives + Tailwind styling)
- Generated: Partially (scaffolded by `shadcn` CLI, then modified)
- Committed: Yes

---

_Structure analysis: 2026-03-11_

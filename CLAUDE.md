# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

uniclipboard-desktop is a cross-platform clipboard synchronization tool built with Tauri 2, React, and Rust. It enables real-time clipboard sharing between devices on LAN (WebSocket) and remotely (WebDAV), with XChaCha20-Poly1305 encryption for security.

## Architecture Documentation

For detailed architecture design, interaction flows, and system overview, refer to the project's DeepWiki documentation:

- **URL**: https://deepwiki.com/UniClipboard/UniClipboard
- **Access**: Use `mcp-deepwiki` MCP server to query the documentation programmatically

This resource provides comprehensive diagrams, flow explanations, and design decisions that complement the code structure.

## Development Commands

### Core Development

```bash
# Install dependencies (uses Bun)
bun install

# Start development server (frontend on :1420, backend hot-reload)
bun tauri dev

# Build for production
bun tauri build

# Frontend-only development
bun run dev        # Start Vite dev server
bun run build      # Build frontend with TypeScript check
bun run preview    # Preview production build
```

### Testing

```bash
# Frontend tests (Vitest, configured with jsdom)
bun test                    # Run all tests
bun test -- --watch         # Watch mode
bun test -- src/lib         # Run tests in a specific directory

# Rust tests (MUST run from src-tauri/)
cd src-tauri && cargo test
cd src-tauri && cargo test -p uc-core           # Test a specific crate
cd src-tauri && cargo test test_name             # Run a single test

# Coverage
bun run test:coverage
open src-tauri/target/llvm-cov/html/index.html
```

Integration tests can use Cargo features: `integration_tests`, `network_tests`, `hardware_tests`.

### Linting & Formatting

```bash
bun run lint        # ESLint
bun run lint:fix    # ESLint with auto-fix
bun run format      # Prettier
```

### Multi-Device Development

```bash
bun run tauri:dev:peerA    # Launch peer A (full clipboard mode)
bun run tauri:dev:peerB    # Launch peer B (passive mode, no separate dev server)
bun run tauri:dev:dual     # Launch both peers concurrently
```

### Cross-Platform Building

Building is handled via GitHub Actions. Trigger manually from GitHub Actions tab with:

- **platform**: macos-aarch64, macos-x86_64, ubuntu-22.04, windows-latest, or all
- **version**: Version number (e.g., 1.0.0)

### Cargo Command Location

**CRITICAL**: All Rust-related commands (cargo build, cargo test, cargo check, etc.) MUST be executed from `src-tauri/`.

```bash
# ✅ CORRECT - Always run from src-tauri/
cd src-tauri && cargo build
cd src-tauri && cargo test
cd src-tauri && cargo check

# ❌ FORBIDDEN - Never run from project root
cargo build
cargo test
```

**Never run any Cargo command from the project root.**
**If Cargo.toml is not present in the current directory, stop immediately and do not retry.**

## Logging

### Overview

The application uses **`tracing`** crate as the primary logging framework with structured logging and span-based context tracking.

**Supported Features**:

- ✅ **Spans** - Structured context spans with parent-child relationships (e.g., `tracing::info_span!`)
- ✅ **Structured fields** - Field-based logging with typed values
- ✅ **Event logging** - `tracing::info!`, `tracing::error!`, etc.

See [docs/architecture/logging-architecture.md](docs/architecture/logging-architecture.md) for detailed architecture, span naming conventions, and configuration.

### Configuration

Logging is initialized in `src-tauri/src/main.rs` using `init_tracing_subscriber()` from the `uc-observability` crate. The `uc-tauri/src/bootstrap/tracing.rs` module delegates to `uc-observability` for dual-output tracing (pretty console + structured JSON/CLEF file).

### Environment Behavior

- **Development**: Debug level, `tracing::*` outputs to terminal, legacy `log::*` outputs to Webview console
- **Production**: Info level, `tracing::*` outputs to stdout, legacy `log::*` outputs to `uniclipboard.log` + stdout

### Log File Locations

- **macOS**: `~/Library/Logs/app.uniclipboard.desktop/uniclipboard.log`
- **Linux**: `~/.local/share/app.uniclipboard.desktop/logs/uniclipboard.log`
- **Windows**: `%LOCALAPPDATA%\app.uniclipboard.desktop\logs/uniclipboard.log`

### Using Logs in Code

```rust
use tracing::{info, error, warn, debug, trace, info_span, Instrument};

pub fn my_function() {
    info!("Something happened");
    error!("Something went wrong: {}", error);
    debug!("Detailed debugging info");
}

// For async operations with spans
pub async fn my_async_function() {
    let span = info_span!("usecase.example.execute", param = %value);
    async move {
        info!("Processing...");
    }.instrument(span).await
}
```

### Span Best Practices

**CRITICAL**: Understand the difference between **Spans** and **Events**:

- **Span** = An operation's time range (has a beginning and end)
- **Event** = Something that happens at a single moment in time

**Correct Pattern**: Use spans to represent each operation step, not just individual debug events:

```rust
// ❌ WRONG - Too many debug events, no span context
debug!("Deleting selection");
self.selection_repo.delete_selection(entry_id).await?;
debug!("Selection deleted");

// ✅ CORRECT - Use span to represent the operation
self.selection_repo
    .delete_selection(entry_id)
    .instrument(info_span!("delete_selection", entry_id = %entry_id))
    .await?;

// ✅ CORRECT - Span for async blocks with multiple steps
let entry = async {
    self.entry_repo
        .get_entry(entry_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Entry not found: {}", entry_id))
}
.instrument(info_span!("fetch_entry", entry_id = %entry_id))
.await?;
```

**Span Hierarchy Example**:

```
usecase.delete_clipboard_entry.execute        ← #[instrument] auto-created
├── fetch_entry                               ← Manual span
├── delete_selection                          ← Manual span
├── delete_entry                              ← Manual span
└── delete_event                              ← Manual span
```

**Key Benefits**:

- Spans automatically record operation start/end time
- Tokio Console and log aggregators show complete call hierarchy
- Reduces redundant log code
- Each operation's duration is automatically tracked
- Better for debugging async systems where multiple operations interleave

**When to Use Events vs Spans**:

- Use **events** (`info!`, `error!`, etc.) for single-moment occurrences (errors, state changes)
- Use **spans** (`info_span!` + `.instrument()`) for operations with duration
- Use `#[tracing::instrument]` on functions to auto-create spans with parameters as fields

**Sources**:

- [Tokio Tracing Guide](https://tokio.rs/tokio/topics/tracing)
- [tracing::instrument Tutorial](https://gist.github.com/oliverdaff/d1d5e5bc1baba088b768b89ff82dc3ec)

### Viewing Logs

**Development:**

- Terminal: Logs appear in the terminal where `bun tauri dev` is running
- Browser: Open DevTools (F12) → Console tab

**Production:**

- Check the log file at the platform-specific location above
- Run `tail -f ~/Library/Logs/app.uniclipboard.desktop/uniclipboard.log` (macOS) for live monitoring

### Log Filtering

The logging system filters out:

- `libp2p_mdns` errors below WARN level (harmless proxy software errors)
- Tauri internal event logs to avoid infinite loops
- `ipc::request` logs in production builds

See `src-tauri/crates/uc-observability/` for tracing configuration and log profiles (`Dev`, `Prod`, `DebugClipboard`).

## Architecture

### Backend (Rust with Tauri 2)

The backend follows **Hexagonal Architecture (Ports and Adapters)** with crate-based modularization:

```
src-tauri/crates/
├── uc-core/              # Core domain layer — models, ports (traits), business rules
│   ├── clipboard/        # Clipboard aggregate root
│   ├── device/           # Device aggregate root
│   ├── network/          # Network domain models & protocol
│   ├── security/         # Security domain models
│   ├── settings/         # Settings domain models
│   └── ports/            # Port definitions (29 trait files)
│       ├── clipboard/    # Clipboard-specific ports
│       ├── security/     # Security ports
│       ├── setup/        # Setup ports
│       └── space/        # Space access ports
├── uc-infra/             # Infrastructure implementations (adapters)
│   ├── db/               # Database layer (Diesel ORM + SQLite)
│   ├── blob/             # Blob storage
│   ├── network/          # Network infrastructure
│   ├── security/         # Encryption implementations
│   └── settings/         # Settings storage
├── uc-platform/          # Platform adapter layer
│   ├── adapters/         # Platform-specific adapters
│   ├── clipboard/        # Platform clipboard access
│   ├── runtime/          # Application runtime
│   ├── ipc/              # IPC event/command system
│   └── ports/            # Platform port definitions
├── uc-app/               # Application layer — use cases
│   └── usecases/         # clipboard/, pairing/, file_sync/, setup/, settings/, storage/, etc.
├── uc-tauri/             # Tauri integration layer
│   ├── commands/         # Tauri command handlers (56+ commands in 12 files)
│   ├── bootstrap/        # Runtime initialization
│   └── models/           # DTOs for frontend
├── uc-observability/     # Logging & tracing infrastructure
│   └── (dual-output tracing: console + JSON, CLEF format, Seq integration)
└── uc-clipboard-probe/   # Clipboard diagnostic utility
```

**Dependency flow**: `uc-tauri` → `uc-app` → `uc-core` ← `uc-infra` / `uc-platform`

**Key principles**:

- **Port/Adapter pattern**: All external dependencies accessed through trait ports in `uc-core/ports/`
- **Message-driven runtime**: Async event-based system with Tokio
- **Crate boundaries**: Enforced separation through Rust module system

**Entry point**: `src-tauri/src/main.rs` — bootstraps the Tauri app, registers all commands and plugins

### Frontend (React 18 + TypeScript + Vite)

```
src/
├── pages/            # Route pages (Dashboard, Devices, Settings)
├── components/       # Reusable UI components (Shadcn/ui based)
├── layouts/          # Layout wrappers
├── store/            # Redux Toolkit slices (state management)
├── api/              # Tauri command invocations
├── contexts/         # React Context (SettingsProvider)
├── hooks/            # Custom React hooks
├── lib/              # Utilities (cn, shadcn UI helpers)
├── quick-panel/      # Quick access panel with inline preview (separate Vite entry, runtime-created window)
├── i18n/             # Internationalization
├── shortcuts/        # Keyboard shortcut definitions and handling
├── observability/    # Frontend observability (Sentry, Seq, tracing)
├── types/            # TypeScript type definitions
└── styles/           # Global CSS and theme definitions
```

**Overlay panel**: The quick panel is a separate Vite entry point (`quick-panel.html`) created at runtime via Tauri window API. Preview content expands inside the same window.

**State management**: Redux Toolkit with RTK Query
**Routing**: React Router v7
**UI**: Tailwind CSS + Shadcn/ui components (Radix UI primitives)

## Key Technical Details

### Path Aliases

TypeScript path aliases configured: `@/*` maps to `src/*` ([tsconfig.json:24-27](tsconfig.json#L24-L27))

### Database Migrations

Diesel migrations in `src-tauri/crates/uc-infra/src/db/`. Run with `diesel migration run` (requires Diesel CLI setup).

### Security Implementation

- **Encryption**: XChaCha20-Poly1305 AEAD for clipboard content ([`src-tauri/crates/uc-infra/src/security/encryption.rs`](src-tauri/crates/uc-infra/src/security/encryption.rs))
  - Chosen for its large nonce (192-bit) reducing nonce reuse risks
  - Provides authenticated encryption with associated data (AEAD)
  - Suitable for cross-platform applications with software-only implementation
- **Password hashing**: Argon2 via Tauri Stronghold plugin
- **Key storage**: Key slot file system with KEK-wrapped master keys ([`src-tauri/crates/uc-infra/src/fs/key_slot_store.rs`](src-tauri/crates/uc-infra/src/fs/key_slot_store.rs))
- **Key derivation**: Argon2id for passphrase-to-key derivation

**Note**: The `aes-gcm` dependency in `Cargo.toml` is currently unused and can be removed in a future cleanup.

### Event System

- Frontend listens to clipboard changes via `listen_clipboard_new_content` Tauri command
- Backend publishes events through custom event bus
- WebSocket events for cross-device sync

### Platform-Specific Code

- macOS: Transparent title bar, `macos-private-api` enabled for the quick panel
- Windows/Unix: Standard window decorations
- Clipboard: Platform implementations in `uc-platform/src/clipboard/`

### Configuration

Settings managed through the `SettingsPort` trait (defined in `uc-core/src/ports/settings.rs`, implemented in `uc-infra/src/settings/`). Includes:

- General (silent_start, etc.)
- Network (webserver_port)
- Sync (websocket/webdav settings)
- Security (encryption password)
- Storage limits
- Keyboard shortcuts
- File sync settings

## Clipboard Capture Integration

### Automatic Capture Flow

The application automatically captures clipboard content when it changes:

1. **ClipboardWatcher** (Platform Layer) monitors system clipboard
2. Sends `PlatformEvent::ClipboardChanged { snapshot }` when change detected
3. **PlatformRuntime** receives event and calls `ClipboardChangeHandler` callback
4. **AppRuntime** implements the callback, invokes `CaptureClipboardUseCase`
5. **UseCase** persists event, representations, and creates `ClipboardEntry`

### Important: Callback Architecture

The integration uses a **callback pattern** maintaining proper layer separation:

- Platform Layer → depends on `ClipboardChangeHandler` trait (in uc-core/ports)
- App Layer → implements `ClipboardChangeHandler` trait
- Platform pushes changes upward via trait call
- No dependency from Platform to App (follows DIP)

### When Modifying

- **Platform Layer:** Never call App layer directly, use callback trait
- **App Layer:** Implement callback to handle events, can call multiple use cases
- **UseCase:** `execute_with_snapshot()` for automatic capture, `execute()` for manual

## Tauri Commands

All frontend-backend communication through Tauri commands defined in [commands/](src-tauri/crates/uc-tauri/src/commands/). Command files are organized by domain: `clipboard.rs`, `encryption.rs`, `pairing.rs`, `setup.rs`, `storage.rs`, `settings.rs`, `autostart.rs`, `lifecycle.rs`, `updater.rs`, `tray.rs`, `quick_panel.rs`, `preview_panel.rs`.

See [docs/architecture/commands-status.md](docs/architecture/commands-status.md) for detailed migration status.

### Architecture Pattern

Commands MUST follow the UseCases accessor pattern:

```rust
#[tauri::command]
pub async fn example_command(
    runtime: State<'_, AppRuntime>,
) -> Result<(), String> {
    let uc = runtime.usecases().example_use_case();
    uc.execute().await.map_err(|e| e.to_string())
}
```

When adding new commands:

1. Define command function in `src-tauri/crates/uc-tauri/src/commands/`
2. Create/refer to use case in `uc-app/src/usecases/`
3. Add accessor method to `UseCases` in `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs`
4. Register in `invoke_handler![]` in `src-tauri/src/main.rs`
5. Use `runtime.usecases().xxx()` - NEVER `runtime.deps.xxx`

## Daemon HTTP API (uc-daemon)

### File Organization

```
crates/uc-daemon/src/api/
├── settings.rs       # Handler logic (router + handler functions)
├── dto/
│   ├── mod.rs         # Sub-module exports
│   ├── error.rs       # ApiError + ApiErrorResponse
│   └── settings.rs    # All settings-related DTOs
├── server.rs         # DaemonApiState + router builder
├── routes.rs         # L1/L2+ router composition + helpers
├── openapi.rs        # OpenAPI doc definition (utoipa)
└── *.rs             # Other domain handlers (clipboard.rs, pairing.rs, etc.)
```

### router() Granularity

Each handler file may export a `pub fn router() -> Router<DaemonApiState>`. Follow these guidelines:

| Condition                                                                              | Action                                  |
| -------------------------------------------------------------------------------------- | --------------------------------------- |
| Domain is simple, < 500 lines total                                                    | Keep handlers + DTOs in one file        |
| Domain is complex (10+ DTOs, > 500 lines)                                              | Split into `dto/xxx.rs` for DTOs        |
| Sub-resources have distinct path prefixes (`/settings/general`, `/settings/retention`) | Split into sub-modules under `api/xxx/` |

**Rule**: A file either has `router()` (exports routes) or doesn't (internal module/DTO library). Never more than one `router()` per file.

### Runtime Access in Handlers

`DaemonApiState.runtime` may be `None` in some modes. Use the helper:

```rust
// For handlers returning Result<T, ApiError> (e.g. settings):
let runtime = state.runtime_or_error()?;

// For handlers returning impl IntoResponse:
let Some(runtime) = state.runtime.clone() else {
    return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
};
```

### Handler Response Patterns

```rust
// Pattern A: Result-returning handler (used by utoipa::path for OpenAPI)
async fn get_handler(State(state): State<DaemonApiState>) -> Result<Json<ResponseDto>, ApiError> {
    let runtime = state.runtime_or_error()?;
    let usecases = CoreUseCases::new(runtime.as_ref());
    let data = usecases.some_use_case().execute().await
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(ResponseDto { data, ts: chrono::Utc::now().timestamp_millis() }))
}

// Pattern B: impl IntoResponse handler (no utoipa::path annotation)
async fn list_handler(State(state): State<DaemonApiState>) -> impl IntoResponse {
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };
    let usecases = CoreUseCases::new(runtime.as_ref());
    // ...
    Json(json!({ "data": result, "ts": ts })).into_response()
}
```

### Adding a New API Endpoint

1. **DTO**: Add to `dto/xxx.rs` — Request/Response/Patch DTOs + `From<core::X>` impls
2. **Handler**: Add to `xxx.rs` — `router()` + handler functions + merge logic (if PATCH)
3. **Conversion**: `From<core::X> for XxxDto` for reads; `From<XxxDto> for core::X` for writes
4. **OpenAPI**: Register in `openapi.rs` `#[openapi(paths(...), components(schemas(...)))]`
5. **Router**: Add `.merge(crate::api::xxx::router())` in `routes.rs` `router_l2_plus()`
6. **Comment**: Add NOTE on handler for important behavior differences (e.g. "no OS side effects")

## Development Notes

- **Package manager**: Bun (not npm/yarn) - faster install/dev times
- **Dev server port**: 1420 (configured in [tauri.conf.json:8](src-tauri/tauri.conf.json#L8))
- **Release optimization**: Size-optimized Rust profile (LTO, panic=abort, strip symbols) ([Cargo.toml:87-92](src-tauri/Cargo.toml#L87-L92))
- **Single instance**: Enforced via `tauri-plugin-single-instance`
- **Autostart**: Managed via `tauri-plugin-autostart` (MacOS LaunchAgent on macOS)

## Development Style

### Problem-Solving Philosophy

**CRITICAL**: Don't treat symptoms in isolation. Always step back and analyze problems from a higher-level perspective before implementing fixes.

**Symptoms vs. Root Causes**:

```
❌ ANTI-PATTERN - Symptom-focused
"Component renders wrong" → Add useEffect hack → "State desync" → Add more hacks → Spaghetti code

✅ CORRECT - Root cause analysis
"Component renders wrong" → Trace data flow → Identify architectural gap → Design proper solution → Fix at the right layer
```

**High-Level Thinking Checklist**:

Before making changes, ask:

1. **Where does this problem originate?**
   - UI layer issue, or state management problem?
   - API contract mismatch, or business logic gap?
   - Infrastructure limitation, or architectural flaw?

2. **What's the systemic fix?**
   - Can this be solved by improving the abstraction?
   - Would a design pattern eliminate this class of bugs?
   - Is there a missing piece in the architecture?

3. **What are the trade-offs?**
   - Short-term hack vs. long-term maintainability
   - Local fix vs. systemic improvement
   - Quick workaround vs. proper solution

**Examples**:

```rust
// ❌ WRONG - Treating symptoms everywhere
async fn sync_clipboard() {
    match send_to_device().await {
        Err(_) => sleep(Duration::from_secs(1)).await, // Band-aid
        Ok(_) => {}
    }
}

// ✅ CORRECT - Fix the retry logic at the infrastructure layer
// infrastructure/sync/retry_policy.rs
pub struct RetryPolicy {
    max_attempts: u32,
    backoff_strategy: BackoffStrategy,
}

async fn sync_clipboard_with_retry(policy: &RetryPolicy) -> Result<()> {
    policy.execute(|| send_to_device()).await
}
```

```tsx
// ❌ WRONG - Local state patch
function DeviceList() {
  const [devices, setDevices] = useState([])
  useEffect(() => {
    fetchDevices().then(setDevices)
    setInterval(() => fetchDevices().then(setDevices), 5000) // Manual polling
  }, [])
}

// ✅ CORRECT - Leverage existing state management (Redux RTK Query)
function DeviceList() {
  const { data: devices } = useGetDevicesQuery() // Built-in caching, refetch, error handling
}
```

**Rationale**: High-level problem-solving prevents technical debt, reduces code complexity, and creates more maintainable solutions. Always identify the root cause and fix it at the appropriate abstraction layer.

### Rust Error Handling

**CRITICAL**: Never use `unwrap()` or `expect()` in production code. Always handle errors explicitly:

```rust
// ❌ FORBIDDEN
let value = some_option.unwrap();
let result = some_result.expect("failed");

// ✅ CORRECT - Use pattern matching
match some_option {
    Some(value) => { /* handle value */ },
    None => { /* handle error case */ },
}

// ✅ CORRECT - Use ? operator with proper error propagation
pub fn do_something() -> Result<(), MyError> {
    let value = some_option.ok_or(MyError::NotFound)?;
    // ...
}

// ✅ CORRECT - Use unwrap_or/unwrap_or_default for non-critical defaults
let value = some_option.unwrap_or_default();
let config = config_option.unwrap_or_else(|| Config::default());

// ✅ ACCEPTABLE in tests only
#[cfg(test)]
mod tests {
    #[test]
    fn test_something() {
        let value = some_option.unwrap(); // OK in tests
    }
}
```

**Rationale**: Explicit error handling prevents panics in production, provides better error messages, and makes failure modes visible to callers.

### Avoid Silent Failures in Event-Driven Code

**CRITICAL**: When handling events or commands in async/event-driven systems, never silently ignore errors. Always log errors and emit failure events when appropriate.

**Anti-Pattern**: Silent failures with `if let Ok(...)`:

```rust
// ❌ WRONG - Silent failure, caller never knows the operation failed
NetworkCommand::SendPairingRequest { peer_id, message } => {
    if let Ok(peer) = peer_id.parse::<PeerId>() {
        self.swarm.send_request(&peer, request);
        debug!("Sent pairing request to {}", peer_id);
    }
    // If parsing fails, execution silently continues - user has no feedback!
}
```

**Correct Pattern**: Explicit error handling with logging and event emission:

```rust
// ✅ CORRECT - Log error and emit event for frontend to handle
NetworkCommand::SendPairingRequest { peer_id, message } => {
    match peer_id.parse::<PeerId>() {
        Ok(peer) => {
            self.swarm.send_request(&peer, request);
            debug!("Sent pairing request to {}", peer_id);
        }
        Err(e) => {
            warn!("Invalid peer_id '{}': {}", peer_id, e);
            let _ = self
                .event_tx
                .send(NetworkEvent::Error(format!(
                    "Failed to send pairing request: invalid peer_id '{}': {}",
                    peer_id, e
                )))
                .await;
        }
    }
}
```

**Key Rules**:

1. **Use `match` instead of `if let`** - When the `Err` case represents a failure that users should know about
2. **Always log errors** - Use `warn!()` or `error!()` to ensure failures are visible in logs
3. **Emit error events** - Send `NetworkEvent::Error` or equivalent so the UI can display user-friendly error messages
4. **Handle missing resources** - When an expected resource (like a pending channel) is missing, log a warning

**When to use `if let` vs `match`**:

```rust
// ✅ OK - Using if let when the None/Err case is truly benign
if let Some(value) = optional_cache.get(&key) {
    // Use cached value
}

// ✅ OK - Using if let when fallback behavior is acceptable
if let Ok(config) = read_config() {
    apply_config(config);
} else {
    use_default_config(); // Explicit fallback
}

// ❌ WRONG - Using if let when failure should be reported
if let Ok(peer_id) = str.parse::<PeerId>() {
    send_request(peer_id);
}
// Error is swallowed!
```

### Tauri State Management

**CRITICAL**: All state accessed via `tauri::State<'_, T>` in commands MUST be registered with `.manage()` before the app starts.

**Common Error**: `state not managed for field 'X' on command 'Y'. You must call .manage() before using this command`

**Root Cause**: When a Tauri command uses `state: tauri::State<'_, MyType>` to access shared state, `MyType` must be registered in the Builder setup using `.manage()`.

**Correct Pattern**:

```rust
// ❌ WRONG - AppRuntimeHandle created internally, never managed
// main.rs
fn run_app(setting: Setting) {
    Builder::default()
        .setup(|app| {
            // AppRuntime creates its own channels internally
            let runtime = AppRuntime::new(...).await?;
            // No .manage() call - commands will fail!
            Ok(())
        })
}

// api/clipboard_items.rs
#[tauri::command]
pub async fn get_clipboard_items(
    state: tauri::State<'_, AppRuntimeHandle>, // ERROR: not managed!
) -> Result<Vec<Item>, String> {
    // ...
}

// ✅ CORRECT - Create channels before setup, manage the handle
// main.rs
fn run_app(setting: Setting) {
    // Create channels FIRST
    let (clipboard_cmd_tx, clipboard_cmd_rx) = mpsc::channel(100);
    let (p2p_cmd_tx, p2p_cmd_rx) = mpsc::channel(100);

    // Create handle with senders
    let handle = AppRuntimeHandle::new(clipboard_cmd_tx, p2p_cmd_tx, Arc::new(setting));

    Builder::default()
        .manage(handle)  // Register BEFORE setup
        .setup(move |app| {
            // Pass receivers to runtime
            AppRuntime::new_with_channels(..., clipboard_cmd_rx, p2p_cmd_rx).await
        })
}
```

**Key Rules**:

1. **Create channels before Builder** - Senders and receivers must be created outside `.setup()`
2. **Register with .manage()** - Any type accessed via `tauri::State` must be managed
3. **Clone senders, move receivers** - Senders can be cloned for the handle, receivers move to the runtime
4. **Use Arc for shared immutable data** - Config and other read-only data should use `Arc<T>`

**Rationale**: Tauri's state system requires explicit registration to ensure thread safety and proper lifetime management. Commands can only access state that was registered before the app started.

### Frontend Styling (Tailwind CSS)

**CRITICAL**: Avoid fixed pixel values (`w-[XXpx]`, `h-[XXpx]`) for cross-platform compatibility. Use Tailwind's built-in utilities or relative units (rem) instead:

```tsx
// ❌ FORBIDDEN - Fixed pixels don't scale across platforms/DPI
<div className="w-[200px] h-[60px]" />
<div className="min-w-[80px]" />
<div className="h-[1px]" />

// ✅ CORRECT - Use Tailwind utilities (rem-based)
<div className="w-52 h-15" />           // w-52 = 13rem, h-15 = 3.75rem
<div className="min-w-20" />            // min-w-20 = 5rem
<div className="h-px" />                // 1px height (special case)

// ✅ CORRECT - Use rem values directly when needed
<div className="w-[3.75rem]" />         // 60px = 3.75rem
<div className="h-[0.0625rem]" />       // 1px = 0.0625rem

// ✅ ACCEPTABLE - For truly fixed sizes (borders, shadows, etc.)
<div className="border shadow-lg" />
```

**Rationale**: Rem-based units scale with the root font size, providing better cross-platform consistency across different screen densities, DPI settings, and user accessibility preferences. Tailwind's default configuration uses `1rem = 16px`.

**Common Tailwind Width Reference**:

- `w-16` = 4rem (64px)
- `w-20` = 5rem (80px)
- `w-52` = 13rem (208px)
- `h-px` = 1px (special utility)

## UI/UX Guidelines

### Theme Support Best Practices

**ALWAYS test components in both light and dark themes** to ensure proper contrast and visibility.

**Container Components** (Dialog, Card, Popover, etc.):

- Use `bg-card` + `text-card-foreground` for containers with content
- Use `bg-background` only for page/base backgrounds
- Use `bg-muted` for disabled/readonly states with `text-foreground` (not `text-muted-foreground`)

**Common Pitfalls**:

```tsx
// ❌ WRONG - Background color on containers makes them blend in
<DialogContent className="bg-background" />

// ✅ CORRECT - Card color creates proper visual hierarchy
<DialogContent className="bg-card text-card-foreground" />

// ❌ WRONG - Muted text on readonly inputs is hard to read
<input className="bg-muted text-muted-foreground" readOnly />

// ✅ CORRECT - Muted background with foreground text
<input className="bg-muted/50 text-foreground" readOnly />
```

**Status Messages**:

- Add `border border-{color}/20` to banners for better visibility in light mode
- Use `font-medium` on text for better readability
- Ensure hover states use `/70` opacity (not `/60`) for visibility

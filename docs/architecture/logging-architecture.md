# Logging Architecture

## Overview

UniClipboard uses **`tracing`** crate as the primary logging framework with structured logging and span-based context tracking. The system produces **dual output** from a single tracing pipeline:

- **Console output**: Pretty human-readable format with ANSI colors (stdout)
- **JSON file output**: Structured flat JSON with daily-rotating files (7-day retention) for tooling and analysis

A **dual-track** coexistence is maintained during the transition from legacy `log` crate to `tracing`:

- `log::*` macros -> `tauri-plugin-log` -> Webview (dev) / stdout (prod)
- `tracing::*` macros -> `uc-observability` subscriber -> console + JSON file

**Current Status**: Phases 0-3 complete, actively using `tracing` across all architectural layers. Dual-output logging with profile system active. Spans, structured logs, and panics flow through **Sentry** (Issues + Logs + Performance) — the OTLP→Seq pipeline used previously was retired in commit `faa8eb8d` (backend) and issue #543 (frontend); see [Sentry Logs (Centralized Observability)](#sentry-logs-centralized-observability).

## Architecture

### Primary Logging Framework: `tracing`

The application uses `tracing` crate for structured, span-aware logging:

**Supported Features**:

- **Spans** - Structured context spans with parent-child relationships
- **Structured fields** - Field-based logging with typed values
- **Span hierarchy** - Cross-layer traceability
- **Instrumentation** - `.instrument()` for async operations
- **Event logging** - `tracing::info!`, `tracing::error!`, etc.

**Migration Status**:

| Phase   | Description                                             | Status       |
| ------- | ------------------------------------------------------- | ------------ |
| Phase 0 | Infrastructure setup (tracing dependencies, subscriber) | Complete     |
| Phase 1 | Command layer root spans                                | Complete     |
| Phase 2 | UseCase layer child spans                               | Complete     |
| Phase 3 | Infra/Platform layer debug spans                        | Complete     |
| Phase 4 | Remove `log` dependency (optional)                      | Not required |

### Dual-Track System

During the transition, both `log` and `tracing` coexist:

```rust
// Legacy code (still works via tauri-plugin-log)
log::info!("Application started");

// New code (preferred) - produces both console + JSON output
tracing::info!("Application started");
tracing::info_span!("command.clipboard.capture", device_id = %id);
```

**Note**: `tracing-log` bridge is NOT configured. The two systems operate independently:

- `log::` macros -> `tauri-plugin-log` -> Webview (dev) / stdout (prod)
- `tracing::` macros -> `uc-observability` subscriber -> console (pretty) + JSON file + Sentry (Logs + Performance, when DSN is configured)

### Module Organization

#### 1. Observability Crate

**Location**: `src-tauri/crates/uc-observability/`

```
uc-observability/
├── src/
│   ├── lib.rs           # Public API re-exports
│   ├── profile.rs       # LogProfile enum (Dev/Prod/DebugClipboard)
│   ├── format.rs        # FlatJsonFormat custom FormatEvent
│   ├── init.rs          # Layer builders + standalone init
│   ├── redact.rs        # Sink-agnostic field-name redaction blocklist
│   └── telemetry_gate.rs # Runtime gate mirrored from settings.general.telemetry_enabled
└── Cargo.toml
```

Provides:

- `LogProfile` - Profile-based filter selection via `UC_LOG_PROFILE`
- `build_console_layer()` - Pretty console layer with per-layer EnvFilter
- `build_json_layer()` - JSON file layer with FlatJsonFormat and daily rolling
- `redact_attributes()` - Shared blocklist applied by Sentry's `before_send_log`
- `telemetry_gate` - Runtime on/off switch synchronized with the user setting
- `init_tracing_subscriber()` - Standalone convenience init (no Sentry)

**Zero app-layer dependencies** - Sentry integration is kept in the caller.

#### 2. Bootstrap Configuration

**Location**: `src-tauri/crates/uc-tauri/src/bootstrap/`

```
bootstrap/
├── logging.rs       # tauri-plugin-log configuration (legacy, Webview + stdout)
└── tracing.rs       # Thin wrapper: uc-observability layers + Sentry
```

**Initialization Flow**:

```
main.rs
  ├─> init_tracing_subscriber()         // uc-tauri/bootstrap/tracing.rs
  │    ├─> LogProfile::from_env()       // Select profile
  │    ├─> sentry::init()               // Optional Sentry (if SENTRY_DSN set)
  │    │     - logs feature enabled (sentry 0.48+)
  │    │     - before_send_log applies redact + telemetry_gate
  │    │     - sentry-trace + baggage headers auto-installed
  │    ├─> build_console_layer()        // From uc-observability
  │    ├─> build_json_layer()           // From uc-observability
  │    ├─> sentry_tracing::layer()      // Routes ERROR→Issue+Log, WARN→Log, INFO→Breadcrumb
  │    └─> registry().with(...).try_init()  // Compose and register
  │
  └─> Builder::default()
       └─> .plugin(logging::get_builder().build())
            └─> Legacy log::* macros still work (Webview/stdout only)
```

#### 3. Layer-Based Tracing

Each architectural layer has specific span naming conventions:

**Clipboard Pipeline** (`uc-app/src/usecases/clipboard/`):

- Root span per clipboard operation
- Naming: `clipboard.{operation}`
- Example: `clipboard.flow` (root), `clipboard.normalize`, `clipboard.cache_representations`

**Command Layer** (`uc-tauri/src/commands/`):

- Root spans for Tauri commands
- Naming: `command.{module}.{action}`
- Example: `command.clipboard.get_entries`, `command.encryption.initialize`

**UseCase Layer** (`uc-app/src/usecases/`):

- Business logic spans
- Naming: `usecase.{usecase_name}.{method}`
- Example: `usecase.list_clipboard_entries.execute`

**Infrastructure Layer** (`uc-infra/src/`):

- Database and repository operations
- Naming: `infra.{component}.{operation}`
- Example: `infra.sqlite.insert_clipboard_event`, `infra.blob.materialize`

**Platform Layer** (`uc-platform/src/`):

- Platform-specific operations
- Naming: `platform.{module}.{operation}`
- Example: `platform.linux.read_clipboard`, `platform.encryption.set_master_key`

## Log Profiles

The `UC_LOG_PROFILE` environment variable selects a logging profile that controls filter verbosity for both console and JSON outputs.

### Profile Selection Precedence

1. **`RUST_LOG`** env var (overrides everything when set)
2. **`UC_LOG_PROFILE`** env var (`dev`, `prod`, `debug_clipboard`)
3. **Build-type default**: debug builds -> `dev`, release builds -> `prod`

### Available Profiles

| Profile           | Base Level | Console Behavior           | JSON Behavior             | Sentry Behavior                                       | Special Overrides                                                                                         |
| ----------------- | ---------- | -------------------------- | ------------------------- | ----------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| `dev`             | `debug`    | Pretty format, ANSI colors | Flat JSON, daily rotating | Enabled (if `SENTRY_DSN` set) — gated at runtime      | `uc_platform=debug`, `uc_infra=debug`                                                                     |
| `prod`            | `info`     | Pretty format, ANSI colors | Flat JSON, daily rotating | Enabled (if `SENTRY_DSN` set) — gated at runtime      | (none)                                                                                                    |
| `debug_clipboard` | `info`     | Pretty format, ANSI colors | Flat JSON, daily rotating | Enabled (if `SENTRY_DSN` set) — gated at runtime      | `uc_platform::adapters::clipboard=trace`, `uc_app::usecases::clipboard=debug`, `uc_core::clipboard=debug` |

All profiles include common noise filters:

- `libp2p_mdns=info`
- `libp2p_mdns::behaviour::iface=off`
- `tauri=warn`
- `wry=off`
- `ipc::request=off`
- `hyper_util=info`
- `hyper=info`
- `quinn=info`
- `quinn_proto=info`
- `quinn_udp=info`
- `Connection::poll=warn`
- `Pool::poll=warn`
- `Swarm::poll=warn`

### Usage Examples

```bash
# Use debug_clipboard profile for clipboard debugging
UC_LOG_PROFILE=debug_clipboard bun run tauri:dev

# Use prod profile in development for testing production behavior
UC_LOG_PROFILE=prod bun run tauri:dev

# Override profile with RUST_LOG (takes precedence)
RUST_LOG=uc_platform::clipboard=trace bun run tauri:dev

# Enable all debug logs
RUST_LOG=debug bun run tauri:dev
```

## Dual Output

The tracing subscriber produces two simultaneous outputs from the same pipeline:

### Console Output

- **Format**: Pretty human-readable with timestamps, file/line, target, ANSI colors
- **Destination**: stdout (terminal where app is running)
- **Example**:

```
2026-03-10 10:30:45.123 INFO [clipboard.rs:51] [command.clipboard.get_entries] Fetching entries
2026-03-10 10:30:45.456 ERROR [clipboard.rs:52] [platform.linux.read_clipboard] Failed to read clipboard: NotFound
```

### JSON File Output

- **Format**: Flat NDJSON (one JSON object per line)
- **Destination**: Daily-rotating file in platform log directory
- **File naming**: `uniclipboard-{gui,daemon,cli}.json.YYYY-MM-DD` (role prefix; see [JSON File Locations](#json-file-locations))
- **Rotation**: New file each day (UTC date boundary)

**JSON field layout**:

| Field       | Description                                               |
| ----------- | --------------------------------------------------------- |
| `timestamp` | ISO 8601 UTC timestamp (e.g., `2026-03-10T10:30:45.123Z`) |
| `level`     | Log level (`TRACE`, `DEBUG`, `INFO`, `WARN`, `ERROR`)     |
| `target`    | Rust module path of the log callsite                      |
| `message`   | The log message string                                    |
| `span`      | Name of the current (leaf) span                           |
| _(fields)_  | Span fields flattened to top level                        |
| _(fields)_  | Event fields at top level                                 |

**Field conflict resolution**: When a span field has the same key as an event field, the span field is prefixed with `parent_`. Event fields always keep their original key.

**Example JSON line**:

```json
{
  "timestamp": "2026-03-10T10:30:45.123Z",
  "level": "INFO",
  "target": "command.clipboard.get_entries",
  "message": "Fetching entries",
  "span": "command.clipboard.get_entries",
  "device_id": "abc-123",
  "limit": 50
}
```

### JSON File Locations

Each process role writes its own daily-rotating file — `uniclipboard-gui`,
`uniclipboard-daemon`, or `uniclipboard-cli` (e.g.
`uniclipboard-daemon.json.YYYY-MM-DD`) — so co-resident processes never share a
file. Logs follow each platform's logging convention and live **outside** the
data directory:

- **macOS**: `~/Library/Logs/app.uniclipboard.desktop[-<profile>]/`
- **Linux**: `~/.local/state/app.uniclipboard.desktop[-<profile>]/logs/` (XDG state dir)
- **Windows**: `%LOCALAPPDATA%\app.uniclipboard.desktop[-<profile>]\logs\`

`[-<profile>]` means the directory gains a suffix such as `-dev` when `UC_PROFILE` is set.

**Retention**: the last 7 daily files per role are kept; older files are pruned
automatically on each start, so the log directory cannot grow without bound.
Portable ("green") builds keep logs next to the executable under
`<exe>/data/logs/`. The single source of truth for the location is
`uc_app_paths::app_log_dir()`.

## Configuration

### Development Mode

When `debug_assertions` is true (debug builds):

**tracing (uc-observability)**:

- **Profile**: `dev` (or `UC_LOG_PROFILE` override)
- **Level**: `Debug`
- **Targets**: `uc_platform=debug`, `uc_infra=debug`
- **Console**: Pretty format to stdout
- **JSON**: Flat JSON to daily-rotating file
- **Sentry**: Enabled if `SENTRY_DSN` is set; further gated by the user's
  in-app `general.telemetry_enabled` setting at runtime

**tauri-plugin-log (legacy)**:

- **Level**: `Debug`
- **Target**: `Webview` (browser DevTools console)
- **Filters**: Tauri internals, wry noise

### Production Mode

When `debug_assertions` is false (release builds):

**tracing (uc-observability)**:

- **Profile**: `prod` (or `UC_LOG_PROFILE` override)
- **Level**: `Info`
- **Console**: Pretty format to stdout
- **JSON**: Flat JSON to daily-rotating file
- **Sentry**: Enabled if `SENTRY_DSN` is set; gated at runtime by the user's
  `general.telemetry_enabled` setting (default-off until the daemon hands the
  persisted preference back to the GUI). `INFO` events are sent as breadcrumbs
  only; `WARN` and `ERROR` become searchable Logs / Issues.

**tauri-plugin-log (legacy)**:

- **Level**: `Info`
- **Target**: `Stdout` only (file logging handled by tracing)
- **Filters**: Tauri internals, wry noise, `ipc::request`

### Environment Variables

| Variable           | Purpose                                                                                              | Default            |
| ------------------ | ---------------------------------------------------------------------------------------------------- | ------------------ |
| `UC_LOG_PROFILE`   | Select logging profile (`dev`, `prod`, `debug_clipboard`)                                            | Build-type default |
| `RUST_LOG`         | Override profile filters (standard tracing env)                                                      | Not set            |
| `SENTRY_DSN`       | Enable backend Sentry (Issues + Logs + Performance). Independent project from the frontend DSN.      | Not set (disabled) |
| `VITE_SENTRY_DSN`  | Enable frontend Sentry (browser SDK). Baked in at build time. Independent project from `SENTRY_DSN`. | Not set (disabled) |

**Note:** All `OTEL_*` and `UC_SEQ_URL` environment variables were removed
together with the OTLP→Seq pipeline (backend: commit `faa8eb8d`, frontend:
issue #543). They are no longer consulted; remove them from your environment
to avoid confusion. The legacy `VITE_OTEL_EXPORTER_OTLP_ENDPOINT` and
`OTEL_EXPORTER_OTLP_ENDPOINT` GitHub secrets can be deleted once the
production Seq instance is decommissioned.

### Color Coding

Console output color coding:

- ERROR: Red (bold)
- WARN: Yellow
- INFO: Green
- DEBUG: Blue
- TRACE: Cyan

## Usage Patterns

### Basic Logging

```rust
use tracing::{info, error, warn, debug, trace};

pub fn process_clipboard(content: String) {
    debug!("Processing clipboard content: {} bytes", content.len());

    match parse(&content) {
        Ok(data) => info!("Successfully parsed clipboard data"),
        Err(e) => error!("Failed to parse clipboard: {}", e),
    }
}
```

### Span Creation

```rust
use tracing::info_span;

// Create span with fields
let span = info_span!(
    "command.clipboard.capture",
    device_id = %device.id,
    limit = limit,
    offset = offset
);

// Use with async operation
async move {
    // ... operation logic
}.instrument(span).await
```

### Structured Fields

Add context to spans with typed fields:

```rust
use tracing::{info_span, debug_span};

// Command layer - user-facing spans
info_span!(
    "command.encryption.initialize",
    passphrase_hash = %hash,
    salt_length = salt.len()
)

// Infra layer - debug spans
debug_span!(
    "infra.sqlite.insert",
    table = "clipboard_entries",
    entry_id = %id
)
```

### Span Hierarchy

Spans automatically form parent-child relationships:

```
command.clipboard.get_entries{device_id=abc123}
└─ usecase.list_clipboard_entries.execute{limit=50, offset=0}
   ├─ infra.sqlite.fetch_entries{sql="SELECT..."}
   └─ event: returning 42 entries
```

**Clipboard pipeline hierarchy** (Phase 87 OTel model):

```
clipboard.flow{origin="local_capture"}         ← root span (one per clipboard operation)
├── clipboard.normalize
├── clipboard.persist_event
├── clipboard.cache_representations
├── clipboard.select_policy
├── clipboard.persist_entry
└── clipboard.spool_blobs
```

For cross-device sync, the inbound side continues the same trace via W3C traceparent:

```
clipboard.flow{origin="inbound_sync", ...}     ← same TraceId as sender
├── clipboard.inbound_decode
└── clipboard.inbound_apply
```

### Instrumentation Pattern

Standard pattern for async operations:

```rust
use tracing::{info_span, Instrument};
use tracing::debug_span;

// For async operations
pub async fn execute(&self, params: Params) -> Result<()> {
    let span = info_span!(
        "usecase.example.execute",
        param1 = %params.param1,
        param2 = params.param2
    );

    async move {
        // Business logic here
        self.inner_operation().await?;
        Ok(())
    }.instrument(span).await
}

// For debug-level operations (only in debug builds)
#[cfg(debug_assertions)]
fn debug_operation(&self) {
    let span = debug_span!("platform.debug.operation");
    span.in_scope(|| {
        // Debug logic here
    });
}
```

### Error Logging with Context

```rust
use tracing::error;

match risky_operation().await {
    Ok(result) => {
        tracing::info!("Operation succeeded");
    }
    Err(e) => {
        error!(
            error = %e,
            context = "failed to process clipboard",
            "Operation failed: {}", e
        );
    }
}
```

## Span Naming Conventions

### Standard Format

```
{layer}.{module}.{operation}
```

### Layer Prefixes

| Prefix       | Usage                        | Examples                                                           |
| ------------ | ---------------------------- | ------------------------------------------------------------------ |
| `clipboard.` | Clipboard pipeline spans     | `clipboard.flow`, `clipboard.normalize`, `clipboard.outbound_send` |
| `command.`   | Tauri command handlers       | `command.clipboard.get_entries`                                    |
| `usecase.`   | UseCase business logic       | `usecase.capture_clipboard.execute`                                |
| `infra.`     | Infrastructure (DB, storage) | `infra.sqlite.insert_blob`                                         |
| `platform.`  | Platform adapters            | `platform.macos.read_clipboard`                                    |

### Clipboard Pipeline Span Names

All clipboard pipeline stages use dotted OTel semconv form:

| Span Name                         | Stage Description                        |
| --------------------------------- | ---------------------------------------- |
| `clipboard.flow`                  | Root span — entire clipboard operation   |
| `clipboard.detect`                | Clipboard change detection               |
| `clipboard.normalize`             | Content normalization                    |
| `clipboard.persist_event`         | Persist clipboard event to storage       |
| `clipboard.cache_representations` | Build and cache content representations  |
| `clipboard.select_policy`         | Evaluate sync policy for this operation  |
| `clipboard.persist_entry`         | Persist final clipboard entry            |
| `clipboard.spool_blobs`           | Queue blobs for outbound transfer        |
| `clipboard.outbound_prepare`      | Prepare outbound sync message            |
| `clipboard.outbound_send`         | Send to peer devices                     |
| `clipboard.inbound_decode`        | Decode inbound clipboard message         |
| `clipboard.inbound_apply`         | Apply inbound content to local clipboard |

### Field Naming

- **Use snake_case** for field names
- **Use `%` formatting** for types implementing `Display`
- **Use `?` formatting** for types implementing `Debug`

```rust
// Display formatting (cleaner output)
device_id = %device.id

// Debug formatting (detailed output)
config = ?config.options

// Direct values
count = 42
```

## Filtering

### Noise Reduction

**libp2p_mdns**:

- Set to `info` to avoid spam from harmless mDNS errors
- `libp2p_mdns::behaviour::iface` set to `off`
- Caused by proxy software virtual network interfaces

**Tauri Internal Events** (tauri-plugin-log only):

- Filtered to prevent infinite loops with Webview target
- `tauri::*` modules
- `tracing::*` modules
- `tauri-` prefixed modules
- `wry::*` modules

**IPC Request Logs**:

- Development: Enabled for debugging
- Production: Filtered to reduce verbosity

## Viewing Logs

### Development

**Terminal (tracing output - console + JSON)**:

```bash
bun run tauri:dev
# tracing::* macros appear in terminal (pretty format)
# JSON file written to platform log directory simultaneously
```

**Browser DevTools (log output)**:

1. Open app in development mode
2. Press F12 or right-click -> Inspect
3. Go to Console tab
4. `log::*` macros appear here

### Production

**Terminal**:

```bash
# Run the application
./uniclipboard

# tracing::* output appears in terminal (pretty format)
# log::* output also appears in terminal (stdout)
```

**JSON log file**:

Replace `gui` with `daemon` or `cli` to inspect another role's file.

```bash
# macOS - view latest JSON log
cat ~/Library/Logs/app.uniclipboard.desktop/uniclipboard-gui.json.$(date +%Y-%m-%d) | jq .

# macOS - follow live
tail -f ~/Library/Logs/app.uniclipboard.desktop/uniclipboard-gui.json.$(date +%Y-%m-%d)

# Linux
tail -f ~/.local/state/app.uniclipboard.desktop/logs/uniclipboard-gui.json.$(date +%Y-%m-%d)

# Windows (PowerShell)
Get-Content "$env:LOCALAPPDATA\app.uniclipboard.desktop\logs\uniclipboard-gui.json.$(Get-Date -Format yyyy-MM-dd)" -Wait
```

**Filter JSON logs for errors**:

```bash
cat ~/Library/Logs/app.uniclipboard.desktop/uniclipboard-gui.json.$(date +%Y-%m-%d) | jq 'select(.level == "ERROR")'
```

**View last 100 lines**:

```bash
tail -n 100 ~/Library/Logs/app.uniclipboard.desktop/uniclipboard-gui.json.$(date +%Y-%m-%d)
```

## Testing

### Unit Tests

The tracing and observability modules include tests:

```bash
# Run uc-observability tests (profile, format, init)
cd src-tauri && cargo test --package uc-observability

# Run uc-tauri tracing bootstrap tests
cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing
```

### Manual Testing

1. **Development**: Run `bun run tauri:dev` and check:
   - Terminal for `tracing::*` console output (pretty)
   - JSON file created in platform log directory
   - Browser DevTools for `log::*` output
2. **Production**: Build and run, check:
   - JSON file exists and contains valid NDJSON entries
   - Terminal shows `tracing::*` console output
3. **Profile selection**: Verify `UC_LOG_PROFILE=debug_clipboard` shows clipboard trace logs

## Troubleshooting

### No logs appearing

**Check tracing initialization**:

1. Verify `main.rs` calls `init_tracing_subscriber()` before any logging
2. Check `tracing` dependency is present
3. Ensure you're using `tracing::info!` not `println!`

**Check log plugin**:

1. Verify `main.rs` has `.plugin(logging::get_builder().build())`
2. Check `log` crate dependency is present

### Logs not appearing in browser

1. Check Webview target is enabled in `logging.rs` for development mode
2. Open browser DevTools and check Console tab
3. Verify there are no JavaScript errors preventing log display

### JSON log file not created

1. Check app has write permissions to the log directory
2. Verify the directory exists: `ls ~/Library/Logs/app.uniclipboard.desktop/` (macOS)
3. Check `init_tracing_subscriber()` completed without error (look for "Tracing initialized" in console)
4. Ensure `UC_LOG_PROFILE` is a valid value (or unset for default)

### Profile not taking effect

1. Check if `RUST_LOG` is set -- it overrides `UC_LOG_PROFILE`
2. Verify `UC_LOG_PROFILE` value is exactly `dev`, `prod`, or `debug_clipboard`
3. Unrecognized values fall back to build-type default

### Span hierarchy not visible

1. Ensure spans are created with `info_span!` or `debug_span!`
2. Verify `.instrument(span)` is used for async operations
3. Check that parent spans are not closed before child operations complete

## Migration Guide

### Adding Tracing to New Code

**1. Import tracing**:

```rust
use tracing::{info_span, info, Instrument};
```

**2. Create span for operations**:

```rust
let span = info_span!(
    "layer.module.operation",
    field1 = %value1,
    field2 = value2
);
```

**3. Instrument async operations**:

```rust
async move {
    // operation
}.instrument(span).await
```

### Converting Legacy Code

**Before** (log crate):

```rust
use log::info;

pub async fn get_entries(&self) -> Result<Vec<Entry>> {
    info!("Fetching entries");
    // ...
}
```

**After** (tracing crate):

```rust
use tracing::{info_span, info, Instrument};

pub async fn get_entries(&self) -> Result<Vec<Entry>> {
    let span = info_span!("usecase.get_entries.execute");
    async move {
        info!("Fetching entries");
        // ...
    }.instrument(span).await
}
```

## Best Practices

### DO

- **Use spans for operations**: Every usecase/command should have a span
- **Add structured fields**: Include operation parameters as span fields
- **Follow naming conventions**: Use `{layer}.{module}.{operation}` format
- **Use appropriate log levels**: `error!`, `warn!`, `info!`, `debug!`, `trace!`
- **Instrument async operations**: Use `.instrument(span)` for async functions
- **Add context to errors**: Include error details and context in error logs

### DON'T

- **Don't use `log::*` in new code**: Prefer `tracing::*` macros
- **Don't create spans for trivial operations**: Spans should represent meaningful work
- **Don't mix formatting styles**: Be consistent with field formatting
- **Don't forget to close spans**: Spans end when their scope ends
- **Don't use `unwrap()` in spans**: Handle errors explicitly

## Performance Considerations

### Span Creation Overhead

- Spans are **cheap** to create but not free
- Use `debug_span!` for operations that should only be traced in debug builds
- Avoid creating spans in tight loops

### Field Formatting

- **`%` formatting** (Display): Faster, cleaner output
- **`?` formatting** (Debug): Slower, detailed output
- Use `%` for production-critical fields
- Use `?` for development-only fields

### Level Filtering

- Spans below the configured level are **not created** (zero overhead)
- Set appropriate levels for each layer
- Use environment-specific filtering in production

## Sentry Logs (Centralized Observability)

### Overview

[Sentry](https://sentry.io) is the single sink for production observability across the daemon, the GUI, and panics. Issues, structured Logs, and Performance Spans share one transport, so a single `trace_id` correlates a clipboard operation from the Rust pipeline through the Tauri command into the React UI.

Three tracks share the same DSN-keyed project:

- **Issues** — `tracing::error!` (backend) and `Sentry.captureException` (frontend) for actionable failures.
- **Logs** — `tracing::warn!`/`error!` (backend) and the pino → `Sentry.logger` bridge in `src/lib/logger.ts` (frontend) for searchable structured logs.
- **Performance** — every `#[tracing::instrument]` span (backend) and every `traceManager.startTrace()` span (frontend) becomes a Sentry span; cross-process correlation is preserved by Sentry's `sentry-trace` + `baggage` headers.

The previous OTLP→Seq pipeline was retired in commit `faa8eb8d` (backend) and issue #543 (frontend) because the upstream Seq instance hit disk-full and started returning 503s, which surfaced inside Sentry as a flood of `BatchLogProcessor.ExportError` issues. Routing logs directly to Sentry removed the second sink and the noise it generated.

### Privacy and Telemetry Gate

Both backend and frontend honor the in-app **Settings → General → Telemetry** toggle (`general.telemetry_enabled`):

- **Backend** — `uc_observability::telemetry_gate` is consulted by Sentry's `before_send`, `before_breadcrumb`, and `before_send_log` hooks. When the gate is off, all three return `None` and nothing leaves the process.
- **Frontend** — `setFrontendSentryEnabled` flips the same flag for the browser SDK. The `beforeSend` / `beforeBreadcrumb` / `beforeSendLog` hooks in `src/observability/sentry.ts` short-circuit to `null` while disabled. The default is **off** at startup; SettingContext flips it on once the daemon returns the persisted user preference, so any events captured before that point are dropped silently.

A shared field-name redaction blocklist (backend: `uc_observability::redact`, frontend: `src/observability/redaction.ts`) is applied to attributes regardless of the gate state, so secrets like `password`, `token`, `auth`, `api_key`, etc. never leave the process even if telemetry is enabled.

### Configuration

| Variable           | Purpose                                                                                              | Default            |
| ------------------ | ---------------------------------------------------------------------------------------------------- | ------------------ |
| `SENTRY_DSN`       | Backend DSN baked into the Rust binary at compile time via `option_env!` in `uc-bootstrap/tracing.rs`. | Not set (disabled) |
| `VITE_SENTRY_DSN`  | Frontend DSN baked into the Vite bundle at build time. Independent project from `SENTRY_DSN`.        | Not set (disabled) |
| `SENTRY_AUTH_TOKEN` | Used by CI to upload `.dSYM` / `.pdb` / DWARF debug symbols and source maps.                         | Not set            |
| `SENTRY_ORG`        | Sentry organization slug used by the symbol-upload CLI.                                              | Not set            |

The two DSNs **must point to different Sentry projects** so frontend rate
limits, sample rates, and quotas can be tuned independently from the backend.

### Backend → Sentry Mapping

The backend uses `sentry-tracing` 0.48+ with the `EventFilter` bitflags:

| `tracing` event | Sentry destination                                  |
| --------------- | --------------------------------------------------- |
| `error!`        | **Issue** (Event) **+** searchable **Log** entry    |
| `warn!`         | searchable **Log** entry                            |
| `info!`         | **Breadcrumb** (attached to next Issue, not stored) |
| `debug!`/`trace!` | dropped (console + JSON file only)                |

`INFO` is intentionally a breadcrumb-only level to keep the Sentry monthly logs quota safe. Spans created with `#[tracing::instrument]` become Performance spans and contribute to transaction sampling.

### Frontend → Sentry Mapping

The frontend uses `@sentry/react` 10.36+ with `enableLogs: true`. The pino logger in `src/lib/logger.ts` forwards `info`+ records to `Sentry.logger.{info,warn,error,fatal}`; `debug` and `trace` stay client-side. React render errors are captured by `<Sentry.ErrorBoundary>` in `main.tsx`. Browser routing is instrumented by `reactRouterV7BrowserTracingIntegration` for parameterized navigation timing.

### Querying Logs in Sentry

Sentry's **Explore → Logs** view supports querying by attribute. Common queries:

- All logs for a single trace: `trace_id:<id>`
- Filter by frontend module: `module:daemon-ws`
- Backend errors over the last hour: `level:error environment:production` with the time range set to `1h`
- Cross-process flow: open any frontend log and click the linked `trace_id` to see the corresponding backend spans in **Performance**.

### Distributed Tracing with sentry-trace + baggage

Cross-device clipboard synchronization shares a single `trace_id` between sender and receiver. Sentry's umbrella crate auto-installs the `sentry-trace` and `baggage` propagators, replacing the W3C `traceparent` header used in the OTLP era. The `ClipboardMessage` protocol field that carried `traceparent` is retained as `Option<String>` for backward-compatible deserialization from old peers.

```
trace_id: a1b2c3d4…                            (same on both devices)
│
├── clipboard.flow{origin="local_capture"}     (Sender peer, device A)
│   ├── clipboard.normalize
│   ├── clipboard.cache_representations
│   ├── clipboard.outbound_prepare
│   └── clipboard.outbound_send
│
└── clipboard.flow{origin="inbound_sync"}      (Receiver peer, device B)
    ├── clipboard.inbound_decode
    └── clipboard.inbound_apply
```

When a message arrives without correlation headers (older peer running a pre-migration build), the receiver creates a new local root span and emits a rate-limited `warn!` once per peer. Subsequent messages from the same legacy peer fall back to `debug!` to avoid log spam.

### Troubleshooting

**No events arriving in Sentry:**

1. Confirm the DSN is set at build time: `bun run build` should not log a `Sentry DSN missing` warning.
2. Confirm the user has telemetry enabled in **Settings → General**. The default is off.
3. Confirm the build environment matches the project: backend events go to the project keyed by `SENTRY_DSN`, frontend events to `VITE_SENTRY_DSN`. They must be different projects.
4. Backend only: `RUST_LOG=sentry=debug bun run tauri:dev` exposes the SDK's transport diagnostics.

**Symbols are missing in stack traces:**

The `Upload Sentry debug symbols` step in `.github/workflows/build.yml` requires `SENTRY_AUTH_TOKEN` and `SENTRY_ORG`. The step is skipped if either secret is absent, but the build still succeeds.

### Legacy Seq Saved Searches

The pre-migration Seq signal files have been moved to `docs/_archive/seq/signals/` for historical reference. They are not consulted by any tooling and may reference fields that no longer exist; do not import them into a fresh deployment.

## References

- [Tracing Crate Documentation](https://docs.rs/tracing/)
- [Tracing Subscriber Documentation](https://docs.rs/tracing-subscriber/)
- [Tauri Plugin Log Documentation](https://v2.tauri.app/plugin/logging/)
- [Sentry Rust SDK](https://docs.rs/sentry/)
- [`@sentry/react` SDK](https://docs.sentry.io/platforms/javascript/guides/react/)
- [Sentry Logs feature](https://docs.sentry.io/product/explore/logs/)
- [Sentry distributed tracing — sentry-trace + baggage](https://docs.sentry.io/concepts/key-terms/tracing/distributed-tracing/)
- Source:
  - `src-tauri/crates/uc-observability/` (profile, format, init, redact, telemetry_gate)
  - `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs` (Sentry + uc-observability composition)
  - `src-tauri/crates/uc-tauri/src/bootstrap/logging.rs` (legacy log plugin, Webview + stdout)
  - `src/observability/sentry.ts` (frontend Sentry init + redaction hooks)
  - `src/lib/logger.ts` (pino → Sentry.logger bridge)
- Archive:
  - `docs/_archive/seq/signals/` (legacy Seq saved searches — not used)
- Guides:
  - [Tracing Usage Guide](../guides/tracing.md)
  - [Coding Standards](../guides/coding-standards.md)

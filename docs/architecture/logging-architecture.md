# Logging Architecture

## Overview

UniClipboard uses **`tracing`** crate as the primary logging framework with structured logging and span-based context tracking. The system produces **dual output** from a single tracing pipeline:

- **Console output**: Pretty human-readable format with ANSI colors (stdout)
- **JSON file output**: Structured flat JSON with daily-rotating files for tooling and analysis

A **dual-track** coexistence is maintained during the transition from legacy `log` crate to `tracing`:

- `log::*` macros -> `tauri-plugin-log` -> Webview (dev) / stdout (prod)
- `tracing::*` macros -> `uc-observability` subscriber -> console + JSON file

**Current Status**: Phases 0-3 complete, actively using `tracing` across all architectural layers. Dual-output logging with profile system active. Phase 87: OTLP pipeline active — spans and logs exported via OTLP/HTTP-protobuf to Seq (dev/debug_clipboard profiles only).

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
- `tracing::` macros -> `uc-observability` subscriber -> console (pretty) + JSON file + OTLP (when enabled)

### Module Organization

#### 1. Observability Crate

**Location**: `src-tauri/crates/uc-observability/`

```
uc-observability/
├── src/
│   ├── lib.rs         # Public API re-exports
│   ├── profile.rs     # LogProfile enum (Dev/Prod/DebugClipboard)
│   ├── format.rs      # FlatJsonFormat custom FormatEvent
│   ├── init.rs        # Layer builders + standalone init
│   └── otlp/
│       ├── mod.rs     # init_otlp_provider() + public exports
│       ├── layer.rs   # OtlpConcreteLayer<S> type alias + build_otlp_layer()
│       └── propagator.rs  # inject_current_context() + extract_remote_context()
└── Cargo.toml
```

Provides:

- `LogProfile` - Profile-based filter selection via `UC_LOG_PROFILE`
- `build_console_layer()` - Pretty console layer with per-layer EnvFilter
- `build_json_layer()` - JSON file layer with FlatJsonFormat and daily rolling
- `init_otlp_provider()` - Async OTLP provider init (returns SdkTracerProvider + OtlpGuard)
- `build_otlp_layer()` - Create OTLP tracing-subscriber layer from provider
- `init_tracing_subscriber()` - Standalone convenience init (no Sentry)

**Zero app-layer dependencies** - Sentry integration is kept in the caller.

#### 2. Bootstrap Configuration

**Location**: `src-tauri/crates/uc-tauri/src/bootstrap/`

```
bootstrap/
├── logging.rs       # tauri-plugin-log configuration (legacy, Webview + stdout)
└── tracing.rs       # Thin wrapper: uc-observability layers + Sentry + OTLP
```

**Initialization Flow**:

```
main.rs
  ├─> init_tracing_subscriber()         // uc-tauri/bootstrap/tracing.rs
  │    ├─> LogProfile::from_env()       // Select profile
  │    ├─> sentry::init()               // Optional Sentry (if SENTRY_DSN set)
  │    ├─> build_console_layer()        // From uc-observability
  │    ├─> build_json_layer()           // From uc-observability
  │    ├─> init_otlp_provider().await   // Optional OTLP (if OTEL_EXPORTER_OTLP_ENDPOINT set + dev/debug_clipboard profile)
  │    ├─> build_otlp_layer()           // Create OTLP tracing layer
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

| Profile           | Base Level | Console Behavior           | JSON Behavior             | OTLP Behavior         | Special Overrides                                                                                         |
| ----------------- | ---------- | -------------------------- | ------------------------- | --------------------- | --------------------------------------------------------------------------------------------------------- |
| `dev`             | `debug`    | Pretty format, ANSI colors | Flat JSON, daily rotating | Enabled (if env set)  | `uc_platform=debug`, `uc_infra=debug`                                                                     |
| `prod`            | `info`     | Pretty format, ANSI colors | Flat JSON, daily rotating | **Disabled** (always) | (none)                                                                                                    |
| `debug_clipboard` | `info`     | Pretty format, ANSI colors | Flat JSON, daily rotating | Enabled (if env set)  | `uc_platform::adapters::clipboard=trace`, `uc_app::usecases::clipboard=debug`, `uc_core::clipboard=debug` |

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
- **File naming**: `uniclipboard.json.YYYY-MM-DD`
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

- **macOS**: `~/Library/Logs/app.uniclipboard.desktop/uniclipboard.json.YYYY-MM-DD`
- **Linux**: `~/.local/share/app.uniclipboard.desktop/logs/uniclipboard.json.YYYY-MM-DD`
- **Windows**: `%LOCALAPPDATA%\app.uniclipboard.desktop\logs\uniclipboard.json.YYYY-MM-DD`

## Configuration

### Development Mode

When `debug_assertions` is true (debug builds):

**tracing (uc-observability)**:

- **Profile**: `dev` (or `UC_LOG_PROFILE` override)
- **Level**: `Debug`
- **Targets**: `uc_platform=debug`, `uc_infra=debug`
- **Console**: Pretty format to stdout
- **JSON**: Flat JSON to daily-rotating file
- **OTLP**: Enabled if `OTEL_EXPORTER_OTLP_ENDPOINT` is set

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
- **OTLP**: **Always disabled** — production builds skip the OTLP layer entirely regardless of env vars

**tauri-plugin-log (legacy)**:

- **Level**: `Info`
- **Target**: `Stdout` only (file logging handled by tracing)
- **Filters**: Tauri internals, wry noise, `ipc::request`

### Environment Variables

| Variable                      | Purpose                                                                         | Default                |
| ----------------------------- | ------------------------------------------------------------------------------- | ---------------------- |
| `UC_LOG_PROFILE`              | Select logging profile (`dev`, `prod`, `debug_clipboard`)                       | Build-type default     |
| `RUST_LOG`                    | Override profile filters (standard tracing env)                                 | Not set                |
| `SENTRY_DSN`                  | Enable Sentry error reporting                                                   | Not set (disabled)     |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP base URL for traces+logs export (e.g. `http://localhost:5341/ingest/otlp`) | Not set (disabled)     |
| `OTEL_EXPORTER_OTLP_HEADERS`  | Optional headers (e.g. `X-Seq-ApiKey=your-key`)                                 | Not set                |
| `OTEL_SERVICE_NAME`           | Override service name in resource attributes                                    | `uniclipboard-desktop` |
| `OTEL_RESOURCE_ATTRIBUTES`    | Additional OTel resource attributes (key=value,key2=value2)                     | Not set                |

**Note:** `UC_SEQ_URL` (removed in Phase 87) is no longer consulted. If `UC_SEQ_URL` is still set in your environment, the application logs a `WARN` on startup pointing you to `OTEL_EXPORTER_OTLP_ENDPOINT`. Set `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp` instead.

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

```bash
# macOS - view latest JSON log
cat ~/Library/Logs/app.uniclipboard.desktop/uniclipboard.json.$(date +%Y-%m-%d) | jq .

# macOS - follow live
tail -f ~/Library/Logs/app.uniclipboard.desktop/uniclipboard.json.$(date +%Y-%m-%d)

# Linux
tail -f ~/.local/share/app.uniclipboard.desktop/logs/uniclipboard.json.$(date +%Y-%m-%d)

# Windows (PowerShell)
Get-Content "$env:LOCALAPPDATA\app.uniclipboard.desktop\logs\uniclipboard.json.$(Get-Date -Format yyyy-MM-dd)" -Wait
```

**Filter JSON logs for errors**:

```bash
cat ~/Library/Logs/app.uniclipboard.desktop/uniclipboard.json.$(date +%Y-%m-%d) | jq 'select(.level == "ERROR")'
```

**View last 100 lines**:

```bash
tail -n 100 ~/Library/Logs/app.uniclipboard.desktop/uniclipboard.json.$(date +%Y-%m-%d)
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

## OpenTelemetry OTLP Integration (Local Visualization)

### Overview

[Seq](https://datalust.co/seq) is a structured log and trace server that provides a rich web UI for searching, filtering, and visualizing OTel traces and log events. Starting with Phase 87, UniClipboard exports spans and log events via the **OTLP/HTTP-protobuf** protocol to a local Seq instance using the standard OpenTelemetry environment variables.

Key capabilities when using Seq with OTLP:

- **Distributed trace view** — all pipeline stages for one clipboard operation shown as a parent-child span tree under a single TraceId
- **Cross-device trace continuity** — sender and receiver spans share the same TraceId via W3C traceparent
- **Full-text search** across all span attributes and log fields
- **Filter by TraceId or SpanName** to see all spans of a single clipboard operation
- **Dashboard creation** for monitoring clipboard operations

### Quick Start

**1. Start a local Seq instance:**

```bash
docker compose -f docker-compose.seq.yml up -d
```

**2. Set the OTLP endpoint environment variable:**

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp
```

**3. (Optional) Set a Seq API key if your instance requires authentication:**

```bash
export OTEL_EXPORTER_OTLP_HEADERS="X-Seq-ApiKey=your-api-key"
```

**4. Start the application:**

```bash
bun run tauri:dev
```

Spans will begin exporting to Seq immediately. Open [http://localhost:5341](http://localhost:5341) to view them. Use the "Traces" section to see the clipboard pipeline trace tree.

### Configuration

| Variable                      | Purpose                                    | Required | Default                |
| ----------------------------- | ------------------------------------------ | -------- | ---------------------- |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP base URL (see critical note below)    | Yes      | Not set (OTLP off)     |
| `OTEL_EXPORTER_OTLP_HEADERS`  | Optional headers, e.g. `X-Seq-ApiKey=...`  | No       | Not needed             |
| `OTEL_SERVICE_NAME`           | Override `service.name` resource attribute | No       | `uniclipboard-desktop` |
| `OTEL_RESOURCE_ATTRIBUTES`    | Additional resource attributes (k=v,k2=v2) | No       | Not set                |

**CRITICAL — Base URL vs. full path (Pitfall #7):**

Seq's own documentation sometimes shows the full endpoint path such as `/ingest/otlp/v1/traces`. Do **not** include `/v1/traces` or `/v1/logs` in `OTEL_EXPORTER_OTLP_ENDPOINT`. The OpenTelemetry SDK automatically appends `/v1/traces` and `/v1/logs` to the base URL you provide. The correct value is:

```
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp
```

Setting it to `http://localhost:5341/ingest/otlp/v1/traces` will cause the SDK to POST to `/ingest/otlp/v1/traces/v1/traces`, which returns 404.

- When `OTEL_EXPORTER_OTLP_ENDPOINT` is **not set**, the OTLP layer is completely disabled with zero overhead.
- In **production builds** (`prod` profile), the OTLP layer is always skipped even if the env var is set.

### Resource Attributes

Every span and log exported via OTLP includes the following resource attributes:

| Attribute                     | Value                                           |
| ----------------------------- | ----------------------------------------------- |
| `service.name`                | `uniclipboard-desktop` (or `OTEL_SERVICE_NAME`) |
| `service.version`             | Crate version from `CARGO_PKG_VERSION`          |
| `service.instance.id`         | Device ID (`device_id` from global context)     |
| `deployment.environment.name` | `development` (dev build) or `production`       |
| `os.type`                     | `linux`, `macos`, `windows`                     |

### Querying Traces in Seq

Once spans are flowing, use Seq's filter bar or the Traces UI to query:

**View all clipboard pipeline spans:**

```
SpanName like 'clipboard.%'
```

**View all spans for a specific trace:**

```
TraceId = 'your-trace-id-here'
```

**Find root flow spans only:**

```
SpanName = 'clipboard.flow'
```

**Filter by span name and time range:**

```
SpanName = 'clipboard.normalize' and @t > Now() - 1h
```

**Tip:** Click on any span in the Traces view to see its full attribute set. Click the TraceId to see the complete trace tree.

Ready-to-import Seq signal files are available in `docs/seq/signals/`. See the [Seq Signals section](#seq-signals) below.

### OTLP Data Model

Spans exported via OTLP carry the following key attributes:

| OTel Field            | Description                                            |
| --------------------- | ------------------------------------------------------ |
| `TraceId`             | W3C trace identifier — same across all spans in a flow |
| `SpanId`              | Unique span identifier                                 |
| `ParentSpanId`        | Links child spans to their parent (e.g. stage → flow)  |
| `SpanName`            | Dotted span name (e.g. `clipboard.normalize`)          |
| `service.name`        | Resource attribute: `uniclipboard-desktop`             |
| `service.instance.id` | Resource attribute: device identifier                  |

### Architecture

The OTLP integration uses a non-blocking pipeline to avoid impacting application performance:

```
tracing event / span
  -> tracing-opentelemetry bridge
  -> OpenTelemetry SDK (BatchSpanProcessor)
  -> opentelemetry-otlp exporter
  -> HTTP POST to /ingest/otlp/v1/traces (OTLP/HTTP-protobuf)
```

- **`tracing-opentelemetry` bridge** translates tracing spans into OTel spans
- **BatchSpanProcessor** batches spans for efficient export (SDK default: 512 spans or 5s, whichever comes first)
- **OtlpGuard** ensures remaining spans are flushed on application shutdown (same guard pattern as Phase 19 WorkerGuard)
- Log events from `tracing::info!` etc. are exported to `/ingest/otlp/v1/logs` simultaneously

### Troubleshooting

**Spans not appearing in Seq:**

1. Verify `OTEL_EXPORTER_OTLP_ENDPOINT` is set: `echo $OTEL_EXPORTER_OTLP_ENDPOINT`
2. Verify Seq is running: `docker compose -f docker-compose.seq.yml ps`
3. Verify Seq is reachable: `curl -s http://localhost:5341/api` (should return JSON)
4. Check the application terminal for "Tracing initialized with OTLP" log line
5. Ensure the base URL does **not** include `/v1/traces` — the SDK appends that automatically
6. Check that `UC_LOG_PROFILE` is `dev` or `debug_clipboard` — OTLP is disabled for `prod` profile

**Seq container not starting:**

1. Ensure Docker is running
2. Check port 5341 is not already in use: `lsof -i :5341`
3. Check container logs: `docker compose -f docker-compose.seq.yml logs seq`

**Old `UC_SEQ_URL` env var:**

If you see a startup `WARN: UC_SEQ_URL is set but is no longer used`, remove `UC_SEQ_URL` from your shell and set `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp` instead.

**Stopping Seq:**

```bash
docker compose -f docker-compose.seq.yml down        # Stop and remove container (data persists)
docker compose -f docker-compose.seq.yml down -v      # Stop and remove container + data volume
```

## Distributed Tracing with W3C Trace Context

UniClipboard provides end-to-end observability for cross-device clipboard synchronization using W3C traceparent propagation. A clipboard operation that originates on one device and is received by another shares a **single TraceId** across both peers — Seq's built-in trace view shows the complete multi-device journey as one trace tree.

### How It Works

1. **Sender side**: When the clipboard pipeline creates a `clipboard.flow` root span and prepares to sync, `inject_current_context()` extracts the W3C `traceparent` header from the current span context.
2. **Protocol field**: The `traceparent` string is written into `ClipboardMessage.traceparent: Option<String>` (backward-compatible via `serde(default, skip_serializing_if)`).
3. **Receiver side**: When the inbound sync handler receives the message, `extract_remote_context(message.traceparent.as_deref())` reconstructs the remote span context. The inbound `clipboard.flow` span calls `set_parent()` with this context, linking it to the sender's trace.
4. **Result**: Seq shows both the sender's capture pipeline and the receiver's apply pipeline under the same TraceId.

### Backward Compatibility — Legacy Peer Fallback

When receiving messages from older peer devices that do not send `traceparent`, the receiver creates a new local root span without a parent. A rate-limited `warn!` is emitted once per peer:

```
WARN clipboard.flow: Inbound message has no traceparent (sender may be running a pre-Phase-87 version); creating local root span
```

Subsequent messages from the same legacy peer are handled silently at `debug!` level to avoid log spam.

### Protocol Field Details

```rust
// ClipboardMessage (uc-core/src/network/protocol/clipboard.rs)
pub struct ClipboardMessage {
    // ... other fields ...

    /// W3C traceparent for distributed trace propagation (Phase 87+).
    /// serde(default) + skip_serializing_if ensures backward compatibility with older peers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,

    /// Deprecated: replaced by W3C traceparent in Phase 87. Scheduled for removal.
    #[deprecated(note = "Phase 87: replaced by W3C traceparent. Do not read or write. Field kept for backward compat deserialization only.")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_flow_id: Option<String>,
}
```

### Cross-Device Trace in Seq

With W3C traceparent active, a cross-device clipboard sync produces:

```
TraceId: a1b2c3d4...                           (same on both devices)
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

In Seq's Traces view, both sides appear in a single trace tree linked by TraceId. To find cross-device traces:

```
SpanName = 'clipboard.flow'
```

Then click any root span to open the full trace view.

## Seq Signals

Pre-configured Seq signal files are provided for common observability patterns. Files are located in `docs/seq/signals/` and can be imported into Seq as saved searches.

| Signal File              | Purpose                                   | Key Filter                    |
| ------------------------ | ----------------------------------------- | ----------------------------- |
| `flow-timeline.json`     | View all stages of one clipboard trace    | `SpanName like 'clipboard.%'` |
| `cross-device-flow.json` | View root flows, click to drill into tree | `SpanName = 'clipboard.flow'` |

**Usage:**

1. Start Seq: `docker compose -f docker-compose.seq.yml up -d`
2. Set `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp`
3. Run the application and trigger clipboard sync between devices
4. In Seq UI, navigate to Signals (or Saved Searches) and import the JSON files
5. Use the trace view (TraceId link) to see the complete multi-span tree

### LAN Access Configuration

For testing cross-device tracing on a local network, the `docker-compose.seq.yml` already binds Seq to all network interfaces. Set `OTEL_EXPORTER_OTLP_ENDPOINT=http://<your-local-ip>:5341/ingest/otlp` on each device to send traces to the centralized Seq instance.

## References

- [Tracing Crate Documentation](https://docs.rs/tracing/)
- [Tracing Subscriber Documentation](https://docs.rs/tracing-subscriber/)
- [OpenTelemetry Rust SDK](https://docs.rs/opentelemetry/)
- [opentelemetry-otlp crate](https://docs.rs/opentelemetry-otlp/)
- [tracing-opentelemetry bridge](https://docs.rs/tracing-opentelemetry/)
- [Tauri Plugin Log Documentation](https://v2.tauri.app/plugin/logging/)
- [Seq Documentation](https://docs.datalust.co/docs)
- [W3C Trace Context Specification](https://www.w3.org/TR/trace-context/)
- [OTel Semantic Conventions — Resource](https://opentelemetry.io/docs/specs/semconv/resource/)
- Source:
  - `src-tauri/crates/uc-observability/` (profile, format, init, otlp/)
  - `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs` (Sentry + OTLP + uc-observability composition)
  - `src-tauri/crates/uc-tauri/src/bootstrap/logging.rs` (legacy log plugin, Webview + stdout)
  - `docker-compose.seq.yml` (local Seq instance with OTLP ingestion)
  - `docs/seq/signals/` (ready-to-import Seq saved searches)
- Guides:
  - [Tracing Usage Guide](../guides/tracing.md)
  - [Coding Standards](../guides/coding-standards.md)

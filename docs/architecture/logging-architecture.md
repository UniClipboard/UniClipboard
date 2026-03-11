# Logging Architecture

## Overview

UniClipboard uses **`tracing`** crate as the primary logging framework with structured logging and span-based context tracking. The system produces **dual output** from a single tracing pipeline:

- **Console output**: Pretty human-readable format with ANSI colors (stdout)
- **JSON file output**: Structured flat JSON with daily-rotating files for tooling and analysis

A **dual-track** coexistence is maintained during the transition from legacy `log` crate to `tracing`:

- `log::*` macros -> `tauri-plugin-log` -> Webview (dev) / stdout (prod)
- `tracing::*` macros -> `uc-observability` subscriber -> console + JSON file

**Current Status**: Phases 0-3 complete, actively using `tracing` across all architectural layers. Dual-output logging with profile system active.

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
- `tracing::` macros -> `uc-observability` subscriber -> console (pretty) + JSON file

### Module Organization

#### 1. Observability Crate

**Location**: `src-tauri/crates/uc-observability/`

```
uc-observability/
├── src/
│   ├── lib.rs         # Public API re-exports
│   ├── profile.rs     # LogProfile enum (Dev/Prod/DebugClipboard)
│   ├── format.rs      # FlatJsonFormat custom FormatEvent
│   └── init.rs        # Layer builders + standalone init
└── Cargo.toml
```

Provides:

- `LogProfile` - Profile-based filter selection via `UC_LOG_PROFILE`
- `build_console_layer()` - Pretty console layer with per-layer EnvFilter
- `build_json_layer()` - JSON file layer with FlatJsonFormat and daily rolling
- `init_tracing_subscriber()` - Standalone convenience init (no Sentry)

**Zero app-layer dependencies** - Sentry integration is kept in the caller.

#### 2. Bootstrap Configuration

**Location**: `src-tauri/crates/uc-tauri/src/bootstrap/`

```
bootstrap/
├── logging.rs       # tauri-plugin-log configuration (legacy, Webview + stdout)
└── tracing.rs       # Thin wrapper: uc-observability layers + Sentry layer
```

**Initialization Flow**:

```
main.rs
  ├─> init_tracing_subscriber()         // uc-tauri/bootstrap/tracing.rs
  │    ├─> LogProfile::from_env()       // Select profile
  │    ├─> sentry::init()               // Optional Sentry (if SENTRY_DSN set)
  │    ├─> build_console_layer()        // From uc-observability
  │    ├─> build_json_layer()           // From uc-observability
  │    └─> registry().with(...).try_init()  // Compose and register
  │
  └─> Builder::default()
       └─> .plugin(logging::get_builder().build())
            └─> Legacy log::* macros still work (Webview/stdout only)
```

#### 3. Layer-Based Tracing

Each architectural layer has specific span naming conventions:

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

| Profile           | Base Level | Console Behavior           | JSON Behavior             | Special Overrides                                                                                         |
| ----------------- | ---------- | -------------------------- | ------------------------- | --------------------------------------------------------------------------------------------------------- |
| `dev`             | `debug`    | Pretty format, ANSI colors | Flat JSON, daily rotating | `uc_platform=debug`, `uc_infra=debug`                                                                     |
| `prod`            | `info`     | Pretty format, ANSI colors | Flat JSON, daily rotating | (none)                                                                                                    |
| `debug_clipboard` | `info`     | Pretty format, ANSI colors | Flat JSON, daily rotating | `uc_platform::adapters::clipboard=trace`, `uc_app::usecases::clipboard=debug`, `uc_core::clipboard=debug` |

All profiles include common noise filters:

- `libp2p_mdns=info`
- `libp2p_mdns::behaviour::iface=off`
- `tauri=warn`
- `wry=off`
- `ipc::request=off`

### Usage Examples

```bash
# Use debug_clipboard profile for clipboard debugging
UC_LOG_PROFILE=debug_clipboard bun tauri dev

# Use prod profile in development for testing production behavior
UC_LOG_PROFILE=prod bun tauri dev

# Override profile with RUST_LOG (takes precedence)
RUST_LOG=uc_platform::clipboard=trace bun tauri dev

# Enable all debug logs
RUST_LOG=debug bun tauri dev
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

- **macOS**: `~/Library/Logs/com.uniclipboard/uniclipboard.json.YYYY-MM-DD`
- **Linux**: `~/.local/share/com.uniclipboard/logs/uniclipboard.json.YYYY-MM-DD`
- **Windows**: `%LOCALAPPDATA%\com.uniclipboard\logs\uniclipboard.json.YYYY-MM-DD`

## Configuration

### Development Mode

When `debug_assertions` is true (debug builds):

**tracing (uc-observability)**:

- **Profile**: `dev` (or `UC_LOG_PROFILE` override)
- **Level**: `Debug`
- **Targets**: `uc_platform=debug`, `uc_infra=debug`
- **Console**: Pretty format to stdout
- **JSON**: Flat JSON to daily-rotating file

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

**tauri-plugin-log (legacy)**:

- **Level**: `Info`
- **Target**: `Stdout` only (file logging handled by tracing)
- **Filters**: Tauri internals, wry noise, `ipc::request`

### Environment Variables

| Variable         | Purpose                                                   | Default            |
| ---------------- | --------------------------------------------------------- | ------------------ |
| `UC_LOG_PROFILE` | Select logging profile (`dev`, `prod`, `debug_clipboard`) | Build-type default |
| `RUST_LOG`       | Override profile filters (standard tracing env)           | Not set            |
| `SENTRY_DSN`     | Enable Sentry error reporting                             | Not set (disabled) |

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

| Prefix      | Usage                        | Examples                            |
| ----------- | ---------------------------- | ----------------------------------- |
| `command.`  | Tauri command handlers       | `command.clipboard.get_entries`     |
| `usecase.`  | UseCase business logic       | `usecase.capture_clipboard.execute` |
| `infra.`    | Infrastructure (DB, storage) | `infra.sqlite.insert_blob`          |
| `platform.` | Platform adapters            | `platform.macos.read_clipboard`     |

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
bun tauri dev
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
cat ~/Library/Logs/com.uniclipboard/uniclipboard.json.$(date +%Y-%m-%d) | jq .

# macOS - follow live
tail -f ~/Library/Logs/com.uniclipboard/uniclipboard.json.$(date +%Y-%m-%d)

# Linux
tail -f ~/.local/share/com.uniclipboard/logs/uniclipboard.json.$(date +%Y-%m-%d)

# Windows (PowerShell)
Get-Content "$env:LOCALAPPDATA\com.uniclipboard\logs\uniclipboard.json.$(Get-Date -Format yyyy-MM-dd)" -Wait
```

**Filter JSON logs for errors**:

```bash
cat ~/Library/Logs/com.uniclipboard/uniclipboard.json.$(date +%Y-%m-%d) | jq 'select(.level == "ERROR")'
```

**View last 100 lines**:

```bash
tail -n 100 ~/Library/Logs/com.uniclipboard/uniclipboard.json.$(date +%Y-%m-%d)
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

1. **Development**: Run `bun tauri dev` and check:
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
2. Verify the directory exists: `ls ~/Library/Logs/com.uniclipboard/` (macOS)
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

## Seq Integration (Local Visualization)

### Overview

[Seq](https://datalust.co/seq) is a structured log server that provides a rich web UI for searching, filtering, and visualizing structured log events. UniClipboard can stream tracing events to a local Seq instance in real time using the [CLEF](https://clef-json.org/) (Compact Log Event Format) ingestion protocol.

Key capabilities when using Seq:

- **Full-text search** across all log fields
- **Filter by flow_id** to see all stages of a single clipboard operation in time order
- **Filter by stage** to see all events at a particular pipeline stage
- **Time-ordered views** showing event sequences with microsecond precision
- **Dashboard creation** for monitoring clipboard operations

### Quick Start

**1. Start a local Seq instance:**

```bash
docker compose -f docker-compose.seq.yml up -d
```

**2. Set the Seq URL environment variable:**

```bash
export UC_SEQ_URL=http://localhost:5341
```

**3. Start the application:**

```bash
bun tauri dev
```

Events will begin streaming to Seq immediately. Open [http://localhost:5341](http://localhost:5341) to view them.

### Configuration

| Variable         | Purpose                        | Required | Default    |
| ---------------- | ------------------------------ | -------- | ---------- |
| `UC_SEQ_URL`     | Seq server URL for CLEF ingest | Yes      | Not set    |
| `UC_SEQ_API_KEY` | API key for Seq authentication | No       | Not needed |

- When `UC_SEQ_URL` is **not set**, the Seq layer is completely disabled with zero overhead.
- When `UC_SEQ_URL` is set, events are formatted as CLEF JSON and sent to `{UC_SEQ_URL}/ingest/clef` via HTTP POST.
- `UC_SEQ_API_KEY` is only needed if your Seq instance requires authentication (not needed for local development).

### Querying Flows in Seq

Once events are flowing, use Seq's filter bar to query specific clipboard flows:

**Find all events for a specific flow:**

```
Has(flow_id)
```

**Filter by a specific flow ID:**

```
flow_id = 'your-flow-id-here'
```

**Filter by flow and stage:**

```
flow_id = 'your-flow-id-here' and stage = 'normalize'
```

**Find all events at a specific stage:**

```
stage = 'persist_event'
```

**See all clipboard capture flows:**

```
Has(flow_id) and stage = 'detect'
```

**Tip:** Click on any `flow_id` value in the Seq UI event detail panel, then select "Find" to automatically filter to that flow.

### Architecture

The Seq integration uses a non-blocking pipeline to avoid impacting application performance:

```
tracing event
  -> SeqLayer (formats as CLEF JSON string)
  -> mpsc channel (1024 buffer)
  -> background sender_loop (batches by count=100 or time=2s)
  -> HTTP POST to /ingest/clef
```

- **SeqLayer** implements the `tracing_subscriber::Layer` trait directly
- Events are formatted using **CLEFFormat** which produces Seq-compatible CLEF JSON
- An mpsc channel decouples the hot tracing path from network I/O
- The **background sender** batches events (up to 100 or every 2 seconds) and POSTs them to Seq
- **SeqGuard** ensures remaining events are flushed on application shutdown

### CLEF Format

Events are sent as newline-delimited CLEF JSON. Each line contains:

```json
{
  "@t": "2026-03-11T10:30:45.123456Z",
  "@l": "Information",
  "@m": "Clipboard content captured",
  "flow_id": "01958a3b-...",
  "stage": "detect",
  "device_id": "abc-123",
  "span": "usecase.capture_clipboard.execute"
}
```

| Field      | Description                                                        |
| ---------- | ------------------------------------------------------------------ |
| `@t`       | Timestamp in ISO 8601 UTC with microsecond precision               |
| `@l`       | Seq log level (Verbose, Debug, Information, Warning, Error, Fatal) |
| `@m`       | Log message                                                        |
| `flow_id`  | Clipboard operation correlation ID (UUID v7)                       |
| `stage`    | Pipeline stage name (detect, normalize, etc.)                      |
| _(fields)_ | All span fields flattened to top level                             |

### Troubleshooting

**Events not appearing in Seq:**

1. Verify `UC_SEQ_URL` is set: `echo $UC_SEQ_URL`
2. Verify Seq is running: `docker compose -f docker-compose.seq.yml ps`
3. Verify Seq is reachable: `curl -s http://localhost:5341/api` (should return JSON)
4. Check the application terminal for "Tracing initialized with dual output (console + JSON + Seq)" log line
5. If the log says just "(console + JSON)" without "+ Seq", the environment variable was not set before app startup

**Seq container not starting:**

1. Ensure Docker is running
2. Check port 5341 is not already in use: `lsof -i :5341`
3. Check container logs: `docker compose -f docker-compose.seq.yml logs seq`

**Events appearing but missing flow_id:**

1. Ensure you are triggering a clipboard capture (copy something)
2. Not all events have `flow_id` -- only clipboard pipeline events carry it
3. Use `Has(flow_id)` in Seq to filter to only flow-correlated events

**Stopping Seq:**

```bash
docker compose -f docker-compose.seq.yml down        # Stop and remove container (data persists)
docker compose -f docker-compose.seq.yml down -v      # Stop and remove container + data volume
```

## References

- [Tracing Crate Documentation](https://docs.rs/tracing/)
- [Tracing Subscriber Documentation](https://docs.rs/tracing-subscriber/)
- [Tauri Plugin Log Documentation](https://v2.tauri.app/plugin/logging/)
- [Seq Documentation](https://docs.datalust.co/docs)
- [CLEF Format Specification](https://clef-json.org/)
- Source:
  - `src-tauri/crates/uc-observability/` (profile, format, init, seq, clef_format)
  - `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs` (Sentry + Seq + uc-observability composition)
  - `src-tauri/crates/uc-tauri/src/bootstrap/logging.rs` (legacy log plugin, Webview + stdout)
  - `docker-compose.seq.yml` (local Seq instance)
- Guides:
  - [Tracing Usage Guide](../guides/tracing.md)
  - [Coding Standards](../guides/coding-standards.md)

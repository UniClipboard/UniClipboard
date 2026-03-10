# Phase 19: Dual Output Logging Foundation - Research

**Researched:** 2026-03-10
**Domain:** Rust tracing ecosystem / dual-output structured logging
**Confidence:** HIGH

## Summary

Phase 19 transforms the existing single-format tracing subscriber into a dual-output system: pretty console for developers and structured JSON file for tooling. The current codebase already uses `tracing-subscriber` 0.3.22 with `fmt` and `env-filter` features, plus `tracing-appender` 0.2.4 for non-blocking file writing. The refactoring is straightforward because `tracing-subscriber`'s `Registry` + `Layer` composition model is designed for exactly this pattern -- adding a second `fmt::Layer` with `.json()` formatting alongside the existing pretty layer.

The main complexity lies in the JSON event shape requirement: CONTEXT.md specifies flat top-level structure with parent span fields expanded and `parent_` prefix for conflict resolution. The built-in JSON formatter nests span fields inside `span` and `spans` objects rather than flattening them into the root. This requires a **custom `FormatEvent` implementation** to walk the span hierarchy, extract fields, and merge them flat into the JSON output with conflict-prefixed keys.

**Primary recommendation:** Add the `json` feature to `tracing-subscriber`, create a custom `FormatEvent` that flattens span fields, introduce a `LogProfile` enum (`dev`/`prod`/`debug_clipboard`) selected by `UC_LOG_PROFILE` env var, and wire two layers (pretty console + JSON file) with per-layer `EnvFilter`s based on the active profile.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Profile selection via `UC_LOG_PROFILE` environment variable
- Debug build defaults to `dev`, release build defaults to `prod`
- `debug_clipboard` raises verbosity only for clipboard-related targets, not globally
- All profiles use dual-output by default (pretty console + JSON file)
- Pretty console format stays as current human-readable style
- JSON file rotation: separate file per day
- Default JSON file location: platform-standard app log directory
- JSON field layout: flat top-level structure (query-friendly)
- Business field naming: `snake_case`
- Required JSON top-level groups: base log fields (timestamp, level, target, message), active span name/fields, parent span field expansion
- Field conflict rule: event field keeps original key, parent value uses `parent_` prefix
- Documentation home: `docs/architecture/logging-architecture.md`

### Claude's Discretion

- Internal implementation details of the custom FormatEvent
- Exact module organization within bootstrap/
- Test strategy and coverage approach

### Deferred Ideas (OUT OF SCOPE)

- Runtime profile hot-switching (OBS-01)
- Representation-level observability fields beyond current phase needs
- Seq/OTel and remote backend shipping
  </user_constraints>

<phase_requirements>

## Phase Requirements

| ID     | Description                                                                    | Research Support                                                                                                |
| ------ | ------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| LOG-01 | Dual output (pretty console + JSON file) from single tracing subscriber        | Registry + two fmt::Layer composition; one pretty, one JSON with custom FormatEvent                             |
| LOG-02 | JSON includes current span context and parent span fields                      | Custom FormatEvent walks span hierarchy via LookupSpan, extracts FormattedFields, flattens with parent\_ prefix |
| LOG-03 | Three log profiles (dev/prod/debug_clipboard) with defined filters and outputs | LogProfile enum + per-profile EnvFilter directives; per-layer filtering via .with_filter()                      |
| LOG-04 | Profile selection via config (env var) and documented                          | UC_LOG_PROFILE env var with cfg!(debug_assertions) fallback; update logging-architecture.md                     |

</phase_requirements>

## Standard Stack

### Core

| Library            | Version       | Purpose                                  | Why Standard                               |
| ------------------ | ------------- | ---------------------------------------- | ------------------------------------------ |
| tracing            | 0.1           | Instrumentation API                      | Already in use; ecosystem standard         |
| tracing-subscriber | 0.3.22        | Subscriber/Layer composition             | Already in use; needs `json` feature added |
| tracing-appender   | 0.2.4         | Non-blocking file appender with rolling  | Already in use; has `rolling::daily()`     |
| serde_json         | (already dep) | JSON serialization in custom FormatEvent | Needed for manual JSON construction        |

### Supporting

| Library        | Version       | Purpose                  | When to Use                                 |
| -------------- | ------------- | ------------------------ | ------------------------------------------- |
| chrono         | (already dep) | Timestamp formatting     | Already used by fmt layer's ChronoUtc timer |
| sentry-tracing | 0.46.1        | Sentry integration layer | Already in use; remains unchanged           |

### Alternatives Considered

| Instead of          | Could Use                                  | Tradeoff                                                                                                          |
| ------------------- | ------------------------------------------ | ----------------------------------------------------------------------------------------------------------------- |
| Custom FormatEvent  | `json-subscriber` crate                    | json-subscriber 0.2.7 has build failures; adds external dep for something achievable in ~100 lines of custom code |
| Custom FormatEvent  | Wait for upstream flatten_spans (PR #2705) | Open since 2023, not merged; cannot depend on it                                                                  |
| Per-layer EnvFilter | `Targets` filter                           | Targets is lighter but cannot filter on span context; EnvFilter is already in use                                 |

**Installation:**

```bash
# In src-tauri/crates/uc-tauri/Cargo.toml, update features:
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "chrono", "json"] }
# serde_json is already a transitive dependency; add explicit dep if not present
```

## Architecture Patterns

### Recommended Module Structure

```
src-tauri/crates/uc-tauri/src/bootstrap/
├── tracing.rs            # Refactored: init_tracing_subscriber() with profile + dual layers
├── tracing_json.rs       # NEW: Custom FormatEvent for flat JSON with span fields
├── tracing_profiles.rs   # NEW: LogProfile enum, filter builders per profile
└── logging.rs            # UNCHANGED: Legacy tauri-plugin-log (untouched this phase)
```

### Pattern 1: Registry + Multi-Layer Composition

**What:** Use `tracing_subscriber::registry()` as the base, then `.with()` multiple layers each having their own filter.
**When to use:** When different outputs need different formats and/or different filter levels.
**Example:**

```rust
// Source: tracing-subscriber docs - per-layer filtering
use tracing_subscriber::{fmt, prelude::*, EnvFilter, registry};

let console_filter = EnvFilter::new("debug,libp2p_mdns=warn");
let json_filter = EnvFilter::new("info,libp2p_mdns=warn");

let console_layer = fmt::layer()
    .with_timer(fmt::time::ChronoUtc::new("%Y-%m-%d %H:%M:%S%.3f".to_string()))
    .with_ansi(true)
    .with_writer(std::io::stdout)
    .with_filter(console_filter);

let json_layer = fmt::layer()
    .json()
    .flatten_event(true)
    .with_current_span(true)
    .with_span_list(true)
    .with_timer(fmt::time::ChronoUtc::new("%Y-%m-%dT%H:%M:%S%.6fZ".to_string()))
    .with_writer(non_blocking_file_writer)
    .with_filter(json_filter);

registry()
    .with(sentry_layer)
    .with(console_layer)
    .with(json_layer)
    .try_init()?;
```

### Pattern 2: Custom FormatEvent for Flat Span Fields

**What:** Implement `FormatEvent` trait to produce JSON where parent span fields are merged flat into the root object.
**When to use:** When the built-in JSON formatter's nested `spans` array doesn't meet query-friendliness requirements.
**Example:**

```rust
// Source: tokio-rs/tracing#2670 workaround pattern
use tracing_subscriber::fmt::{self, format::Writer, FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;
use tracing::Subscriber;
use serde::ser::{SerializeMap, Serializer as _};
use std::collections::HashMap;

pub struct FlatJsonFormat {
    timer: fmt::time::ChronoUtc,
}

impl<S, N> FormatEvent<S, N> for FlatJsonFormat
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        // 1. Collect span fields from root to leaf
        // 2. Collect event fields
        // 3. Detect conflicts: if event key == span key, prefix span's with "parent_"
        // 4. Serialize flat JSON: { timestamp, level, target, message, ...span_fields, ...event_fields }
        // ...
        Ok(())
    }
}
```

### Pattern 3: Profile-Based Filter Selection

**What:** An enum representing log profiles that resolves to different `EnvFilter` directives.
**When to use:** When different deployment contexts need different verbosity.
**Example:**

```rust
pub enum LogProfile {
    Dev,
    Prod,
    DebugClipboard,
}

impl LogProfile {
    pub fn from_env() -> Self {
        match std::env::var("UC_LOG_PROFILE").as_deref() {
            Ok("prod") => Self::Prod,
            Ok("debug_clipboard") => Self::DebugClipboard,
            Ok("dev") => Self::Dev,
            _ if cfg!(debug_assertions) => Self::Dev,
            _ => Self::Prod,
        }
    }

    pub fn console_filter(&self) -> EnvFilter {
        let base = match self {
            Self::Dev => "debug",
            Self::Prod => "info",
            Self::DebugClipboard => "info",
        };
        let mut filter = EnvFilter::new(base);
        // Common noise filters
        filter = filter
            .add_directive("libp2p_mdns=info".parse().unwrap())
            .add_directive("libp2p_mdns::behaviour::iface=off".parse().unwrap())
            .add_directive("tauri=warn".parse().unwrap())
            .add_directive("wry=off".parse().unwrap())
            .add_directive("ipc::request=off".parse().unwrap());

        if matches!(self, Self::Dev) {
            filter = filter
                .add_directive("uc_platform=debug".parse().unwrap())
                .add_directive("uc_infra=debug".parse().unwrap());
        }
        if matches!(self, Self::DebugClipboard) {
            // Targeted clipboard verbosity
            filter = filter
                .add_directive("uc_platform::adapters::clipboard=trace".parse().unwrap())
                .add_directive("uc_app::usecases::clipboard=debug".parse().unwrap())
                .add_directive("uc_core::clipboard=debug".parse().unwrap());
        }
        filter
    }

    pub fn json_filter(&self) -> EnvFilter {
        // JSON file always captures at info+ regardless of profile
        // debug_clipboard raises clipboard targets in JSON too
        // ...
    }
}
```

### Pattern 4: Daily Rolling JSON File Writer

**What:** Use `tracing_appender::rolling::daily()` for automatic date-based file rotation.
**When to use:** For the JSON file output destination.
**Example:**

```rust
use tracing_appender::rolling;
use tracing_appender::non_blocking;

let daily_appender = rolling::daily(&paths.logs_dir, "uniclipboard.json");
let (non_blocking_writer, guard) = non_blocking(daily_appender);
// Store guard to keep writer alive for app lifetime
```

**Note:** `rolling::daily()` appends `.YYYY-MM-DD` to the file name prefix. So `uniclipboard.json` produces `uniclipboard.json.2026-03-10`. Two guards now needed (console file guard if any, JSON file guard).

### Anti-Patterns to Avoid

- **Single global EnvFilter on Registry:** Don't place one EnvFilter on the registry level when layers need different verbosity. Use per-layer `.with_filter()` instead.
- **Dropping WorkerGuard early:** The `NonBlocking` writer stops flushing if its `WorkerGuard` is dropped. Both guards (existing log file + new JSON file) must be stored in `OnceLock` or similar static storage.
- **Modifying logging.rs:** The legacy `tauri-plugin-log` setup in `logging.rs` must remain untouched. It handles `log::*` macros independently. Touching it risks breaking the Webview console output.

## Don't Hand-Roll

| Problem                  | Don't Build                   | Use Instead                                                   | Why                                               |
| ------------------------ | ----------------------------- | ------------------------------------------------------------- | ------------------------------------------------- |
| Non-blocking file I/O    | Custom async file writer      | `tracing_appender::non_blocking()`                            | Handles backpressure, buffering, flush-on-drop    |
| Daily file rotation      | Custom date-checking rotation | `tracing_appender::rolling::daily()`                          | Battle-tested, handles midnight rollover          |
| Env-based filter parsing | Custom directive parser       | `EnvFilter::try_from_default_env()`                           | Supports `RUST_LOG` override, directive syntax    |
| Span hierarchy traversal | Manual parent tracking        | `ctx.event_scope()` / `ctx.lookup_current()` via `LookupSpan` | Handles concurrent spans, async context correctly |

**Key insight:** The tracing ecosystem provides all the building blocks. The only custom code needed is the `FormatEvent` implementation for flat JSON with conflict-prefixed parent fields -- roughly 80-120 lines.

## Common Pitfalls

### Pitfall 1: EnvFilter as Global vs Per-Layer

**What goes wrong:** Placing a single `EnvFilter` on the registry filters events before they reach any layer. This means JSON file and console get the same filter.
**Why it happens:** The current code does `registry().with(env_filter).with(stdout_layer)` -- filter is global.
**How to avoid:** Move to per-layer filtering: `registry().with(console_layer.with_filter(console_filter)).with(json_layer.with_filter(json_filter))`.
**Warning signs:** JSON file missing debug events that should be captured, or console showing events meant only for JSON.

### Pitfall 2: WorkerGuard Lifetime

**What goes wrong:** JSON log file stops receiving events after guard is dropped.
**Why it happens:** `tracing_appender::non_blocking()` returns a guard that must live for the app's lifetime.
**How to avoid:** Store both guards in `OnceLock<(WorkerGuard, WorkerGuard)>` or separate statics.
**Warning signs:** JSON file stops writing after initialization completes.

### Pitfall 3: RUST_LOG Override Interaction

**What goes wrong:** `RUST_LOG` env var overrides profile-based filters unexpectedly.
**Why it happens:** `EnvFilter::try_from_default_env()` reads `RUST_LOG` and ignores programmatic directives.
**How to avoid:** Check `RUST_LOG` first; if set, use it as override. If not set, build filter from profile. Document that `RUST_LOG` takes precedence over `UC_LOG_PROFILE`.
**Warning signs:** Profile selection appears to have no effect when `RUST_LOG` is set.

### Pitfall 4: Sentry Layer Interaction

**What goes wrong:** Sentry layer receives events filtered by the wrong filter or gets duplicate events.
**Why it happens:** If Sentry layer is outside the per-layer filter scope, it sees all events.
**How to avoid:** Keep Sentry layer with its own filter or at global level with appropriate filtering. Current code adds it as `Option<Layer>` which works fine.
**Warning signs:** Sentry dashboard shows excessive events or misses errors.

### Pitfall 5: Span Field Extraction from FormattedFields

**What goes wrong:** Custom FormatEvent fails to read span fields because they are stored as pre-formatted strings.
**Why it happens:** `tracing-subscriber` stores span fields in extensions as `FormattedFields<N>` which is a formatted string, not structured data.
**How to avoid:** Use a custom `MakeVisitor` / field visitor that stores fields as `serde_json::Value` in span extensions, or parse the formatted fields. Alternatively, use `JsonFields` as the field formatter (provided by the `json` feature) which stores fields as JSON.
**Warning signs:** Span fields appear as raw strings in JSON output rather than structured values.

### Pitfall 6: Two Log File Systems Coexisting

**What goes wrong:** Confusion about which file has which format. Both tracing.rs and logging.rs write files in production.
**Why it happens:** Legacy `tauri-plugin-log` writes `uniclipboard.log` (plain text from `log::*` macros). New JSON layer writes `uniclipboard.json.YYYY-MM-DD`.
**How to avoid:** Document clearly: `uniclipboard.log` = legacy log macros, `uniclipboard.json.*` = structured tracing output. Keep both until `log::*` usage is fully removed.
**Warning signs:** Developers looking for JSON in the wrong file.

## Code Examples

### Full Dual-Layer Initialization (Verified Pattern)

```rust
// Source: tracing-subscriber 0.3 docs + project-specific adaptation
use tracing_subscriber::{fmt, prelude::*, registry, EnvFilter};
use tracing_appender::{non_blocking, rolling};

pub fn init_tracing_subscriber() -> anyhow::Result<()> {
    let profile = LogProfile::from_env();

    // Console layer: pretty format (preserves current behavior)
    let console_layer = fmt::layer()
        .with_timer(fmt::time::ChronoUtc::new("%Y-%m-%d %H:%M:%S%.3f".to_string()))
        .with_level(true)
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .with_ansi(cfg!(not(test)))
        .with_writer(std::io::stdout)
        .with_filter(profile.console_filter());

    // JSON layer: structured output to daily-rotating file
    let (json_writer, json_guard) = build_json_writer()?;
    let json_layer = fmt::layer()
        .event_format(FlatJsonFormat::new())  // Custom formatter
        .with_writer(json_writer)
        .with_filter(profile.json_filter());

    // Store guards
    store_guards(json_guard)?;

    // Sentry layer (unchanged)
    let sentry_layer = build_sentry_layer();

    registry()
        .with(sentry_layer)
        .with(console_layer)
        .with(json_layer)
        .try_init()?;

    tracing::info!(
        profile = %profile,
        "Tracing initialized with dual output"
    );

    Ok(())
}
```

### Custom FormatEvent Skeleton

```rust
// Source: tokio-rs/tracing#2670 pattern adapted for project requirements
use serde::ser::{SerializeMap, Serializer};
use std::collections::BTreeMap;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::registry::LookupSpan;

pub struct FlatJsonFormat {
    timer: fmt::time::ChronoUtc,
}

impl<S, N> FormatEvent<S, N> for FlatJsonFormat
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let mut buf = Vec::new();
        let mut ser = serde_json::Serializer::new(&mut buf);
        let mut map = ser.serialize_map(None).map_err(|_| std::fmt::Error)?;

        // 1. Base fields
        // timestamp, level, target, message
        map.serialize_entry("timestamp", &self.format_timestamp())...;
        map.serialize_entry("level", &event.metadata().level().as_str())...;
        map.serialize_entry("target", &event.metadata().target())...;

        // 2. Collect parent span fields (root to leaf)
        let mut span_fields = BTreeMap::new();
        if let Some(scope) = ctx.event_scope() {
            // Current span name
            if let Some(leaf) = scope.clone().next() {
                map.serialize_entry("span", &leaf.name())...;
            }
            for span in scope {
                // Extract fields from each span's extensions
                // Store in span_fields map
            }
        }

        // 3. Collect event fields
        let mut event_fields = BTreeMap::new();
        let mut visitor = JsonVisitor(&mut event_fields);
        event.record(&mut visitor);

        // 4. Merge with conflict resolution
        for (key, value) in &span_fields {
            if event_fields.contains_key(key) {
                map.serialize_entry(&format!("parent_{}", key), value)...;
            } else {
                map.serialize_entry(key, value)...;
            }
        }
        for (key, value) in &event_fields {
            map.serialize_entry(key, value)...;
        }

        map.end().map_err(|_| std::fmt::Error)?;
        writeln!(writer, "{}", String::from_utf8_lossy(&buf))
    }
}
```

### Daily Rolling JSON Writer

```rust
// Source: tracing-appender 0.2 docs
fn build_json_writer() -> anyhow::Result<(NonBlocking, WorkerGuard)> {
    let app_dirs = DirsAppDirsAdapter::new().get_app_dirs()?;
    let paths = AppPaths::from_app_dirs(&app_dirs);
    fs::create_dir_all(&paths.logs_dir)?;

    let daily_appender = tracing_appender::rolling::daily(&paths.logs_dir, "uniclipboard.json");
    let (non_blocking, guard) = tracing_appender::non_blocking(daily_appender);
    Ok((non_blocking, guard))
}
```

## State of the Art

| Old Approach            | Current Approach                         | When Changed             | Impact                                          |
| ----------------------- | ---------------------------------------- | ------------------------ | ----------------------------------------------- |
| Single global EnvFilter | Per-layer filtering via `.with_filter()` | tracing-subscriber 0.3.x | Each output can have independent verbosity      |
| `fmt().json()` only     | Custom `FormatEvent` for flat JSON       | N/A (always available)   | Full control over JSON shape                    |
| `rolling::never()`      | `rolling::daily()`                       | tracing-appender 0.2.x   | Automatic daily rotation without external tools |
| Single output (stdout)  | Multi-layer (console + file + sentry)    | tracing-subscriber 0.3.x | Each layer independently formatted and filtered |

**Deprecated/outdated:**

- `tracing-subscriber` 0.2.x per-layer filtering was not available; 0.3.x introduced `Filter` trait
- The `chrono` feature on tracing-subscriber is deprecated in favor of `time` crate, but project already uses it and migration is out of scope

## Open Questions

1. **Exact span field storage format**
   - What we know: `tracing-subscriber` stores span fields in `FormattedFields<N>` in span extensions. When using `JsonFields` formatter, fields are stored as JSON strings.
   - What's unclear: Whether `JsonFields` stores parseable JSON or pre-formatted strings.
   - Recommendation: Use `JsonFields` as the `N` type parameter for the JSON layer. Verify in implementation that `FormattedFields<JsonFields>` contains parseable JSON. If not, implement a custom field storage extension.

2. **Guard storage for two non-blocking writers**
   - What we know: Current code stores one `WorkerGuard` in `OnceLock<WorkerGuard>`.
   - What's unclear: Best pattern for storing two guards.
   - Recommendation: Change to `OnceLock<Vec<WorkerGuard>>` or use a struct with named fields. The existing console file writer may stay as `rolling::never` or switch to stdout-only (current code already uses stdout).

3. **debug_clipboard target paths**
   - What we know: Profile should increase clipboard-related verbosity.
   - What's unclear: Exact Rust module paths for clipboard targets.
   - Recommendation: Audit `uc_platform`, `uc_app`, `uc_core`, `uc_infra` crates during implementation to identify all clipboard-related module paths. Start with `uc_platform::adapters::clipboard`, `uc_app::usecases::clipboard`, `uc_core::clipboard`.

## Validation Architecture

### Test Framework

| Property           | Value                                                                 |
| ------------------ | --------------------------------------------------------------------- |
| Framework          | cargo test (built-in)                                                 |
| Config file        | `src-tauri/Cargo.toml` (workspace)                                    |
| Quick run command  | `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing` |
| Full suite command | `cd src-tauri && cargo test`                                          |

### Phase Requirements -> Test Map

| Req ID | Behavior                                                       | Test Type | Automated Command                                                                                               | File Exists?                                       |
| ------ | -------------------------------------------------------------- | --------- | --------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| LOG-01 | Dual output layers are composed correctly                      | unit      | `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing -x`                                        | Partially (existing tests cover filter directives) |
| LOG-02 | JSON events contain flattened span fields with parent\_ prefix | unit      | `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing_json -x`                                   | No -- Wave 0                                       |
| LOG-03 | Three profiles produce correct filter directives               | unit      | `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing_profiles -x`                               | No -- Wave 0                                       |
| LOG-04 | UC_LOG_PROFILE env var selects correct profile                 | unit      | `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing_profiles::tests::test_profile_from_env -x` | No -- Wave 0                                       |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test --package uc-tauri -- bootstrap::tracing -x`
- **Per wave merge:** `cd src-tauri && cargo test --package uc-tauri`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-tauri/src/bootstrap/tracing_json.rs` -- unit tests for FlatJsonFormat with mock spans
- [ ] `src-tauri/crates/uc-tauri/src/bootstrap/tracing_profiles.rs` -- unit tests for LogProfile enum and filter construction
- [ ] Update `tracing-subscriber` features in Cargo.toml to include `json`

## Sources

### Primary (HIGH confidence)

- [tracing-subscriber 0.3 fmt::Layer docs](https://docs.rs/tracing-subscriber/0.3.22/tracing_subscriber/fmt/struct.Layer.html) -- Layer composition, per-layer filtering, json() method
- [tracing-subscriber 0.3 format::Json docs](https://docs.rs/tracing-subscriber/0.3.22/tracing_subscriber/fmt/format/struct.Json.html) -- flatten_event, with_current_span, with_span_list
- [tracing-appender 0.2 rolling::daily docs](https://docs.rs/tracing-appender/0.2.4/tracing_appender/rolling/fn.daily.html) -- Daily rolling file appender API
- [tracing-subscriber EnvFilter docs](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) -- Per-layer filtering, directive parsing
- Project source: `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs` -- Current subscriber initialization

### Secondary (MEDIUM confidence)

- [tokio-rs/tracing#2670](https://github.com/tokio-rs/tracing/issues/2670) -- Custom FormatEvent pattern for flat span fields; issue open, PR #2705 not merged
- [tracing-subscriber fmt module source](https://github.com/tokio-rs/tracing/blob/master/tracing-subscriber/src/fmt/mod.rs) -- Layer composition internals

### Tertiary (LOW confidence)

- [json-subscriber 0.2.7](https://docs.rs/json-subscriber) -- Alternative crate; build failures on latest release, not recommended

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH -- all libraries already in use, only `json` feature addition needed
- Architecture: HIGH -- Registry + multi-Layer is the documented pattern; per-layer filtering is stable API
- Custom FormatEvent: MEDIUM -- pattern is proven (issue #2670) but exact FormattedFields parsing needs implementation verification
- Pitfalls: HIGH -- based on direct analysis of current codebase and tracing-subscriber docs

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable ecosystem, no breaking changes expected)

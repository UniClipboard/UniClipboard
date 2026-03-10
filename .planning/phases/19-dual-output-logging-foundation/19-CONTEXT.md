# Phase 19: Dual Output Logging Foundation - Context

**Gathered:** 2026-03-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Refactor the existing single-format tracing subscriber into a dual-output system (pretty console + structured JSON file) with selectable log profiles (dev/prod/debug_clipboard). Developers can run the app and simultaneously see human-readable console logs and machine-readable JSON logs. Profile selection is via environment variable. This phase does NOT include flow_id/stage instrumentation (Phase 20), sync observability (Phase 21), or Seq integration (Phase 22).

</domain>

<decisions>
## Implementation Decisions

### Profile Selection Mechanism

- Profile selected via `UC_LOG_PROFILE` environment variable with values: `dev`, `prod`, `debug_clipboard`
- Debug build defaults to `dev`, release build defaults to `prod`
- `RUST_LOG` overrides everything — if RUST_LOG is set, UC_LOG_PROFILE is ignored
- Document precedence: RUST_LOG > UC_LOG_PROFILE > build-type default

### JSON Output Format

- Span fields flattened to JSON top-level (not nested in `spans` array) — requires custom `FormatEvent` implementation
- Field conflict resolution: event field keeps original key, parent span value gets `parent_` prefix
- Business field naming: `snake_case` (consistent with Rust code style)
- Required JSON top-level groups: base log fields (timestamp, level, target, message), active span name/fields, parent span field expansion

### Log Profile Definitions

- All profiles emit dual output (pretty console + JSON file) — no profile disables either output
- Console always uses pretty human-readable format (never JSON on console)
- **dev**: debug level for console and JSON, uc_platform/uc_infra=debug (preserves current behavior)
- **prod**: info level for console and JSON
- **debug_clipboard**: baseline info, only clipboard-related targets raised to debug/trace (not global raise)
- JSON file filter level follows console filter level (symmetric per profile)

### JSON File Management

- Daily rotation using `tracing_appender::rolling::daily()`
- File naming: `uniclipboard.json.YYYY-MM-DD` (appender default)
- Location: platform-standard app log directory (same as existing `uniclipboard.log`), using AppPaths.logs_dir
- No retention policy in this phase

### Crate Organization

- Create new `uc-observability` crate in `src-tauri/crates/`
- Contains: tracing initialization, LogProfile enum, FlatJsonFormat, profile filter builders
- Pre-reserves module structure for future Seq integration (Phase 22)
- Public API: single `init_tracing_subscriber(logs_dir: &Path, profile: LogProfile)` function + `LogProfile` enum
- Internal implementation (FlatJsonFormat etc.) is private
- No dependency on application-layer types (accepts `&Path` parameter, caller provides logs_dir)

### Legacy Coexistence

- Remove file output from `logging.rs` (legacy tauri-plugin-log) — structured JSON file replaces it
- Keep Webview console output in `logging.rs` — `log::*` macros continue outputting to browser DevTools for frontend debugging
- Result: `uniclipboard.log` file is removed; `uniclipboard.json.YYYY-MM-DD` replaces it for structured output

### Documentation

- Update existing `docs/architecture/logging-architecture.md` with profile system, dual output, and JSON format documentation

### Claude's Discretion

- Internal implementation details of the custom FormatEvent (span field extraction, JSON serialization)
- Exact module organization within uc-observability crate
- Test strategy and coverage approach
- Guard storage pattern for non-blocking writers
- Exact `debug_clipboard` target paths (audit during implementation)

</decisions>

<specifics>
## Specific Ideas

- User wants observability to be a standalone crate (`uc-observability`) for reuse and clean separation, with Seq integration pre-reserved
- The init function should accept path parameters rather than resolving paths internally — keeps the crate independent of app-layer types
- Legacy `uniclipboard.log` file output is actively removed (not just ignored) since JSON file replaces it

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `tracing.rs` (`uc-tauri/bootstrap/`): Current `init_tracing_subscriber()` with registry + EnvFilter + fmt::layer pattern — serves as migration baseline
- `build_filter_directives()`: Existing dev/prod filter logic to inform profile filter construction
- `build_file_writer()`: NonBlocking + WorkerGuard pattern with OnceLock storage — reuse for JSON writer
- `AppPaths.logs_dir`: Platform-resolved log directory path

### Established Patterns

- `registry().with(env_filter).with(sentry_layer).with(fmt_layer)` composition — must preserve Sentry layer integration
- `OnceLock<WorkerGuard>` for guard lifetime management
- `tracing-appender::non_blocking()` for async file I/O
- `fmt::time::ChronoUtc` for timestamp formatting

### Integration Points

- `main.rs` calls `init_tracing_subscriber()` — will change to call `uc_observability::init_tracing_subscriber(logs_dir, profile)`
- `logging.rs` remains for Webview console (its file output removed)
- Sentry layer construction stays in tracing initialization
- `Cargo.toml` workspace: add `uc-observability` as new crate member

</code_context>

<deferred>
## Deferred Ideas

- Runtime profile hot-switching without restart (OBS-01) — future milestone
- Representation-level observability fields (OBS-02) — future milestone
- Seq/OTel and remote backend shipping — Phase 22 / future milestone

</deferred>

---

_Phase: 19-dual-output-logging-foundation_
_Context gathered: 2026-03-10_

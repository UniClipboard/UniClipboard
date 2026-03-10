---
phase: 19-dual-output-logging-foundation
plan: 01
subsystem: infra
tags: [tracing, logging, json, observability, dual-output]

# Dependency graph
requires: []
provides:
  - uc-observability crate with dual-output tracing initialization
  - LogProfile enum (Dev/Prod/DebugClipboard) with per-profile EnvFilter construction
  - FlatJsonFormat custom FormatEvent for flat JSON with parent_ conflict prefix
  - init_tracing_subscriber(logs_dir, profile) public API
affects: [19-02, phase-20, phase-22]

# Tech tracking
tech-stack:
  added: [uc-observability crate, tracing-subscriber json feature, JsonFields]
  patterns: [per-layer EnvFilter, custom FormatEvent, OnceLock WorkerGuard, daily rolling appender]

key-files:
  created:
    - src-tauri/crates/uc-observability/Cargo.toml
    - src-tauri/crates/uc-observability/src/lib.rs
    - src-tauri/crates/uc-observability/src/profile.rs
    - src-tauri/crates/uc-observability/src/format.rs
    - src-tauri/crates/uc-observability/src/init.rs
  modified:
    - src-tauri/Cargo.toml

key-decisions:
  - 'Used JsonFields as field formatter for JSON layer so span fields are stored as parseable JSON in extensions'
  - "FlatJsonFormat uses chrono::Utc::now() directly for timestamp rather than tracing-subscriber's timer trait"
  - 'Sentry integration left to caller -- uc-observability has zero app-layer dependencies'

patterns-established:
  - 'Per-layer EnvFilter: each output layer gets its own filter via .with_filter() instead of global registry filter'
  - 'FlatJsonFormat: custom FormatEvent that walks span hierarchy, flattens fields, and prefixes conflicts with parent_'
  - 'LogProfile: env-var-driven profile selection with build-type defaults and RUST_LOG override'

requirements-completed: [LOG-01, LOG-02, LOG-03]

# Metrics
duration: 4min
completed: 2026-03-10
---

# Phase 19 Plan 01: Create uc-observability Crate Summary

**Dual-output tracing crate with LogProfile-driven filtering, FlatJsonFormat for flat span-field JSON, and daily-rolling file writer**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-10T13:41:06Z
- **Completed:** 2026-03-10T13:45:35Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

- Created standalone uc-observability crate with zero app-layer dependencies
- LogProfile enum with three profiles (Dev/Prod/DebugClipboard) resolving from UC_LOG_PROFILE env var with build-type defaults
- FlatJsonFormat custom FormatEvent producing valid NDJSON with span fields at top level and parent\_ prefix on conflicts
- init_tracing_subscriber composing console (pretty) + JSON (flat) layers on a single registry with per-layer EnvFilter
- 23 unit tests covering profile selection, filter directives, JSON output format, span flattening, and conflict resolution

## Task Commits

Each task was committed atomically:

1. **Task 1: Create uc-observability crate with LogProfile and FlatJsonFormat** - `b4477e42` (feat)
2. **Task 2: Implement init_tracing_subscriber with dual-output composition** - `9e69c1e3` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-observability/Cargo.toml` - Crate manifest with tracing, serde_json, chrono dependencies
- `src-tauri/crates/uc-observability/src/lib.rs` - Public API re-exports (LogProfile, init_tracing_subscriber)
- `src-tauri/crates/uc-observability/src/profile.rs` - LogProfile enum with from_env(), console_filter(), json_filter()
- `src-tauri/crates/uc-observability/src/format.rs` - FlatJsonFormat custom FormatEvent with JsonVisitor
- `src-tauri/crates/uc-observability/src/init.rs` - Dual-layer subscriber init with daily rolling JSON appender
- `src-tauri/Cargo.toml` - Added uc-observability to workspace members

## Decisions Made

- Used `JsonFields` as the `N` type parameter for the JSON layer so that `FormattedFields<N>` contains parseable JSON strings, enabling FlatJsonFormat to extract structured span data
- FlatJsonFormat timestamps use `chrono::Utc::now().to_rfc3339_opts()` directly rather than the tracing-subscriber timer trait, giving full control over ISO 8601 format
- Sentry integration is explicitly excluded from uc-observability -- the crate has no app-layer dependencies, and Sentry layer construction remains in the caller

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- uc-observability crate is ready for integration into main app (Plan 19-02)
- Plan 19-02 will wire init_tracing_subscriber into main.rs, replacing the existing tracing.rs setup
- The crate's public API (LogProfile + init_tracing_subscriber) is stable and tested

## Self-Check: PASSED

All files verified present. All commit hashes verified in git log.

---

_Phase: 19-dual-output-logging-foundation_
_Completed: 2026-03-10_

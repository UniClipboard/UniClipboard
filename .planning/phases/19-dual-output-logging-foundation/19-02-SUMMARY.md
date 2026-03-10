---
phase: 19-dual-output-logging-foundation
plan: 02
subsystem: infra
tags: [tracing, logging, json, observability, dual-output, sentry, profiles]

# Dependency graph
requires:
  - phase: 19-01
    provides: uc-observability crate with LogProfile, FlatJsonFormat, init_tracing_subscriber
provides:
  - Working app with dual-output logging (console + JSON) via uc-observability integration
  - Sentry layer composed with uc-observability layers in uc-tauri
  - Legacy logging cleaned up (no more uniclipboard.log file output)
  - Updated architecture documentation with profile system
affects: [phase-20, phase-21, phase-22]

# Tech tracking
tech-stack:
  added: [uc-observability integration into uc-tauri]
  patterns: [layer builder composition, generic Layer<S> return types, WorkerGuard re-export]

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-observability/src/init.rs
    - src-tauri/crates/uc-observability/src/lib.rs
    - src-tauri/crates/uc-tauri/Cargo.toml
    - src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs
    - src-tauri/crates/uc-tauri/src/bootstrap/logging.rs
    - docs/architecture/logging-architecture.md

key-decisions:
  - 'Used generic impl Layer<S> return types for build_console_layer/build_json_layer to enable caller composition without Box<dyn> issues'
  - 'Re-exported WorkerGuard from uc-observability to avoid adding tracing-appender as direct dependency of consumers'
  - 'Kept tracing-subscriber as uc-tauri dependency (with registry feature) for Sentry layer composition via try_init()'

patterns-established:
  - 'Layer builder composition: uc-observability exposes generic layer builders, callers compose with their own layers (Sentry) before try_init()'
  - 'WorkerGuard lifetime: stored in OnceLock statics at both uc-observability (standalone) and uc-tauri (app) levels'

requirements-completed: [LOG-01, LOG-02, LOG-03, LOG-04]

# Metrics
duration: 9min
completed: 2026-03-10
---

# Phase 19 Plan 02: Integrate uc-observability with Sentry Composition and Architecture Docs Summary

**Dual-output tracing wired into app via generic layer builders, Sentry layer composed locally, legacy file logging removed, architecture docs updated with profile system**

## Performance

- **Duration:** 9 min
- **Started:** 2026-03-10T13:48:48Z
- **Completed:** 2026-03-10T13:58:00Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments

- Exposed build_console_layer() and build_json_layer() as generic public API from uc-observability for caller composition
- Rewrote uc-tauri/bootstrap/tracing.rs as thin wrapper composing uc-observability layers + Sentry layer
- Removed LogDir target from logging.rs (JSON file via tracing replaces uniclipboard.log)
- Updated logging-architecture.md with profile system, dual output, JSON format, and troubleshooting

## Task Commits

Each task was committed atomically:

1. **Task 1: Integrate uc-observability into app and handle Sentry + legacy cleanup** - `6653e435` (feat)
2. **Task 2: Update logging architecture documentation for profiles and dual output** - `95aeb536` (docs)

## Files Created/Modified

- `src-tauri/crates/uc-observability/src/init.rs` - Added build_console_layer() and build_json_layer() generic public functions, refactored init_tracing_subscriber to use them
- `src-tauri/crates/uc-observability/src/lib.rs` - Re-exported builder functions and WorkerGuard
- `src-tauri/crates/uc-tauri/Cargo.toml` - Added uc-observability dependency, removed tracing-appender
- `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs` - Thin wrapper using uc-observability builders + Sentry
- `src-tauri/crates/uc-tauri/src/bootstrap/logging.rs` - Removed LogDir target, updated doc comments
- `src-tauri/Cargo.lock` - Updated lockfile
- `docs/architecture/logging-architecture.md` - Complete rewrite with profiles, dual output, JSON format

## Decisions Made

- Used generic `impl Layer<S>` return types instead of `Box<dyn Layer<Registry>>` for the builder functions. Box approach failed because `Layered<Box<dyn Layer<R>>, R>` creates a different subscriber type than `R`, breaking subsequent `.with()` calls. Generic approach lets the compiler infer correct layered types.
- Re-exported `WorkerGuard` from uc-observability so consumers don't need a direct tracing-appender dependency.
- Kept `tracing-subscriber` (with `registry` and `fmt` features) in uc-tauri because test code in commands/mod.rs uses `tracing_subscriber::fmt::MakeWriter`.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- Initial implementation attempted `Box<dyn Layer<Registry>>` return types for builder functions, but this caused compilation errors due to type system constraints in tracing-subscriber's `Layered` composition. Resolved by switching to generic `impl Layer<S>` return types.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Dual-output logging is fully operational: console (pretty) + JSON (flat, daily-rotating)
- Profile system active via UC_LOG_PROFILE env var
- Phase 20 can add flow_id/stage span fields that will automatically appear in both outputs
- Phase 22 can consume the JSON output for Seq integration

---

_Phase: 19-dual-output-logging-foundation_
_Completed: 2026-03-10_

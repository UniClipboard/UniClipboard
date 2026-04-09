---
phase: 87-otlp-seq-otlp
plan: "05"
subsystem: infra
tags: [observability, otlp, seq, tracing, rust]

requires:
  - phase: 87-04
    provides: OTLP pipeline fully wired and green; all callers migrated to OTLP spans

provides:
  - Legacy Seq/CLEF pipeline code permanently removed from uc-observability
  - Only OTLP pipeline remains as the telemetry exporter
  - span_fields module demoted to pub(crate) — internal helper only

affects: [uc-observability, uc-tauri, uc-app]

tech-stack:
  added: []
  patterns:
    - 'Pure deletion plan: remove all legacy Seq/CLEF code once callers are migrated'

key-files:
  created: []
  modified:
    - src-tauri/crates/uc-observability/src/lib.rs
    - src-tauri/crates/uc-observability/src/span_fields.rs

key-decisions:
  - '87-05: span_fields.rs retained as pub(crate) internal helper — format.rs still uses collect_span_fields for FlatJsonFormat'
  - '87-05: clef_format.rs deleted entirely; CLEF formatting no longer needed now that OTLP is the sole exporter'
  - '87-05: seq/ module fully deleted (layer.rs, mod.rs, sender.rs — ~900 lines removed)'

requirements-completed:
  - REQ-87-01

duration: 3min
completed: "2026-04-04"
---

# Phase 87 Plan 05: Legacy Seq/CLEF Pipeline Deletion Summary

**Hard-deleted ~1082 lines of legacy Seq/CLEF telemetry code (seq/, clef_format.rs) from uc-observability; OTLP is now the sole exporter path**

## Performance

- **Duration:** 3 min
- **Started:** 2026-04-04T09:14:00Z
- **Completed:** 2026-04-04T09:17:00Z
- **Tasks:** 1
- **Files modified:** 6 (4 deleted, 2 modified)

## Accomplishments

- Deleted `seq/` module (3 files: layer.rs, mod.rs, sender.rs — entire Seq HTTP ingestion implementation)
- Deleted `clef_format.rs` (CLEF JSON formatter for Seq)
- Cleaned `lib.rs`: removed `pub mod seq`, `pub use seq::*`, `pub mod clef_format`, `pub use clef_format::CLEFFormat`
- Demoted `span_fields` from `pub mod` to `pub(crate) mod` since it is still needed internally by `format.rs`
- Updated lib.rs doc header to declare OTLP as the sole telemetry exporter
- All 37 unit tests still pass; clippy -D warnings clean

## Task Commits

1. **Task 1: Delete seq module, clef_format.rs, clean lib.rs** - `eeae6c6c` (feat)

**Plan metadata:** (pending docs commit)

## Files Created/Modified

- `src-tauri/crates/uc-observability/src/lib.rs` — Removed seq/clef exports, updated module declarations, new header comment
- `src-tauri/crates/uc-observability/src/span_fields.rs` — Updated docstring (removed CLEFFormat reference)
- `src-tauri/crates/uc-observability/src/seq/mod.rs` — DELETED
- `src-tauri/crates/uc-observability/src/seq/layer.rs` — DELETED
- `src-tauri/crates/uc-observability/src/seq/sender.rs` — DELETED
- `src-tauri/crates/uc-observability/src/clef_format.rs` — DELETED

## Decisions Made

- `span_fields.rs` retained as `pub(crate)` because `format.rs` (FlatJsonFormat, used by the JSON file layer) still calls `collect_span_fields`. Deleting it would break the JSON log file output. The function is purely internal — no public API exposure.
- `chrono` dependency kept in Cargo.toml — still used by `format.rs` for timestamps.
- `reqwest` dependency kept — still used by the OTLP pipeline.
- Full workspace `cargo build` is impossible in this CI environment (missing `pkg-config`, `glib`, `openssl` system packages). Verified `cargo check` + `cargo test -p uc-observability` + `cargo check -p uc-app` all pass.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] span_fields.rs not deleted — still required by format.rs**

- **Found during:** Task 1 (pre-deletion grep analysis)
- **Issue:** Plan instructed deleting `span_fields.rs`, but `format.rs` imports `crate::span_fields::collect_span_fields` for the FlatJsonFormat JSON file layer. Deletion would break compilation.
- **Fix:** Kept `span_fields.rs` but demoted it to `pub(crate) mod span_fields` in lib.rs (no longer a public export). Removed `pub mod span_fields` and any re-exports from the public API.
- **Files modified:** src-tauri/crates/uc-observability/src/lib.rs, src-tauri/crates/uc-observability/src/span_fields.rs
- **Verification:** `cargo check -p uc-observability` passes; `cargo test -p uc-observability` 37/37 pass; `cargo clippy -D warnings` clean
- **Committed in:** eeae6c6c (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 - correctness bug)
**Impact on plan:** Necessary correction — span_fields is a shared internal helper, not seq-specific. Net result is identical: no public Seq/CLEF API exposed. All 37 tests pass.

## Issues Encountered

- Full workspace `cargo build` fails due to missing system libraries in CI environment (`pkg-config`, `glib-2.0`, `openssl`). This is a pre-existing environment constraint unrelated to this plan's changes. Verified the relevant crates (uc-observability, uc-app, uc-core) compile clean.

## Next Phase Readiness

- Plan 87-06 (documentation/docker-compose) can proceed independently
- uc-observability public API is now clean: only OTLP-related exports remain
- No legacy Seq public types remain in any crate

---

_Phase: 87-otlp-seq-otlp_
_Completed: 2026-04-04_

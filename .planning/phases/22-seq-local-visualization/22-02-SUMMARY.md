---
phase: 22-seq-local-visualization
plan: 02
subsystem: observability
tags: [seq, tracing, docker-compose, bootstrap, documentation]

# Dependency graph
requires:
  - phase: 22-seq-local-visualization
    provides: CLEFFormat, SeqLayer, build_seq_layer, SeqGuard from Plan 01
  - phase: 19-observability-foundation
    provides: LogProfile, build_console_layer/build_json_layer, OnceLock guard pattern
provides:
  - Seq layer composed in global tracing subscriber alongside console, JSON, and Sentry layers
  - SEQ_GUARD static OnceLock preventing early drop
  - docker-compose.seq.yml for one-command local Seq startup
  - Seq integration documentation (setup, querying, troubleshooting)
  - hyper/hyper_util noise filters suppressing connection pool debug spam
affects: []

# Tech tracking
tech-stack:
  added: [datalust/seq:2025.2 (docker)]
  patterns:
    [
      OnceLock guard pattern for optional tracing layers,
      env-driven optional Seq composition (zero overhead when UC_SEQ_URL unset),
    ]

key-files:
  created:
    - docker-compose.seq.yml
  modified:
    - src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs
    - src-tauri/crates/uc-observability/src/profile.rs
    - docs/architecture/logging-architecture.md

key-decisions:
  - 'Seq layer uses Option<Layer> pattern for zero-overhead when disabled'
  - 'hyper=info and hyper_util=info added to NOISE_FILTERS to suppress Seq HTTP client debug noise'

patterns-established:
  - 'OnceLock static guard storage for optional tracing layers (SEQ_GUARD alongside JSON_GUARD, SENTRY_GUARD)'

requirements-completed: [SEQ-02, SEQ-05, SEQ-06]

# Metrics
duration: 5min
completed: 2026-03-11
---

# Phase 22 Plan 02: Seq Bootstrap Integration Summary

**Seq layer wired into global subscriber with docker-compose for local dev, hyper noise filters, and full integration documentation**

## Performance

- **Duration:** 5 min (continuation after checkpoint approval)
- **Started:** 2026-03-11T06:35:13Z (initial), resumed 2026-03-11T14:15:21Z
- **Completed:** 2026-03-11T14:15:21Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- Seq layer composed alongside console, JSON, and Sentry layers in global tracing subscriber
- SeqGuard stored in OnceLock static following established JSON_GUARD/SENTRY_GUARD pattern
- docker-compose.seq.yml provides one-command local Seq startup
- Documentation covers Seq setup, configuration, flow querying, CLEF format, and troubleshooting
- Human-verified: events stream to Seq with queryable flow_id and stage fields
- Suppressed hyper connection pool debug noise with NOISE_FILTERS additions

## Task Commits

Each task was committed atomically:

1. **Task 1: Wire Seq layer into bootstrap and create docker-compose + docs** - `8ee67f62` (feat)
2. **Task 2: Verify Seq end-to-end flow visualization** - checkpoint:human-verify (approved)
   - **Noise filter fix** - `1e9abbb8` (fix) - hyper/hyper_util noise suppression

## Files Created/Modified

- `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs` - Seq layer composition with SEQ_GUARD OnceLock
- `docker-compose.seq.yml` - Local Seq instance (datalust/seq:2025.2) with volume persistence
- `docs/architecture/logging-architecture.md` - Seq integration section with setup, querying, troubleshooting
- `src-tauri/crates/uc-observability/src/profile.rs` - Added hyper=info and hyper_util=info noise filters

## Decisions Made

- Seq layer uses Option<Layer> pattern allowing zero overhead when UC_SEQ_URL is unset
- Added hyper=info and hyper_util=info to NOISE_FILTERS to suppress connection pool debug logs that were polluting Seq output

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Suppressed hyper connection pool debug noise**

- **Found during:** Task 2 (human verification)
- **Issue:** hyper and hyper_util crates emitting verbose connection pool debug logs that cluttered Seq output
- **Fix:** Added `hyper=info` and `hyper_util=info` to NOISE_FILTERS in profile.rs with corresponding test assertions
- **Files modified:** src-tauri/crates/uc-observability/src/profile.rs
- **Verification:** Noise filters applied, test assertions pass
- **Committed in:** 1e9abbb8

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Noise filter fix improves signal-to-noise ratio in Seq. No scope creep.

## Issues Encountered

None beyond the auto-fixed deviation above.

## User Setup Required

None - Seq is optional and configuration-driven via UC_SEQ_URL environment variable.

## Next Phase Readiness

- Phase 22 (Seq Local Visualization) is complete
- Developers can start Seq with `docker compose -f docker-compose.seq.yml up -d`
- Set `UC_SEQ_URL=http://localhost:5341` to enable Seq, unset to disable (zero overhead)
- Flow visualization is queryable via `flow_id` and `stage` fields in Seq UI

---

_Phase: 22-seq-local-visualization_
_Completed: 2026-03-11_

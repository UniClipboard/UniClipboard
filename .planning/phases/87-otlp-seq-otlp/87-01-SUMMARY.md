---
phase: 87-otlp-seq-otlp
plan: 01
subsystem: testing
tags: [opentelemetry, otlp, tracing, scaffold, wave0, tdd]

# Dependency graph
requires: []
provides:
  - Wave 0 failing test scaffolds for OTLP pipeline (REQ-87-01/03/04/06/14/15)
  - Stub uc_observability::otlp module with init_otlp_pipeline, OtlpGuard, build_resource, propagator types
  - Feature gates __wave0_scaffold_87 (uc-observability) and __wave0_scaffold_87_traceparent (uc-core) for gated compilation
  - dev-dep: opentelemetry-stdout 0.31, serial_test 3 added to uc-observability
  - prod-dep: opentelemetry 0.31, opentelemetry_sdk 0.31, tracing-opentelemetry 0.32 added to uc-observability
affects:
  - 87-02-PLAN (implements real uc_observability::otlp to flip otlp_pipeline.rs and propagation.rs green)
  - 87-03-PLAN (adds ClipboardMessage.traceparent to flip clipboard_message_traceparent.rs green)

# Tech tracking
tech-stack:
  added:
    - opentelemetry 0.31 (uc-observability dep)
    - opentelemetry_sdk 0.31 with rt-tokio (uc-observability dep)
    - tracing-opentelemetry 0.32 (uc-observability dep)
    - opentelemetry-stdout 0.31 (uc-observability dev-dep)
    - serial_test 3 (uc-observability dev-dep)
  patterns:
    - Wave 0 test scaffold pattern: feature-gated test files that compile but fail until implementation lands
    - Stub module pattern: otlp.rs stub provides type surface so tests can reference future API
    - serde(default) + skip_serializing_if backward-compat pattern (mirrors Phase 21 origin_flow_id precedent)

key-files:
  created:
    - src-tauri/crates/uc-observability/src/otlp.rs
    - src-tauri/crates/uc-observability/tests/otlp_pipeline.rs
    - src-tauri/crates/uc-observability/tests/propagation.rs
    - src-tauri/crates/uc-core/tests/clipboard_message_traceparent.rs
  modified:
    - src-tauri/crates/uc-observability/Cargo.toml
    - src-tauri/crates/uc-observability/src/lib.rs
    - src-tauri/crates/uc-core/Cargo.toml

key-decisions:
  - 'Simplified init_otlp_pipeline to non-generic signature (no <S> type param) using Box<dyn Layer<Registry>> for stub to avoid type-inference errors in tests'
  - 'Added opentelemetry + opentelemetry_sdk + tracing-opentelemetry to [dependencies] (not dev-only) because stub otlp.rs uses their types in public API surface'
  - 'Resource::iter() yields (&Key, &Value) pairs not KeyValue — tests use .map(|(k,v)| (k.clone(), v.clone())) pattern'
  - 'traceparent roundtrip test and root_flow_has_child_stage_spans marked #[ignore] with TODO Plan 02/03 — function stubs exist and compile'

patterns-established:
  - 'Wave0 scaffold: #![cfg(feature = "__wave0_scaffold_XX")] gate makes test files compile-invisible in default build'
  - 'Stub module: create src/otlp.rs with placeholder types/functions returning Ok(None)/None so future test imports resolve'

requirements-completed:
  - REQ-87-01
  - REQ-87-03
  - REQ-87-04
  - REQ-87-06
  - REQ-87-14
  - REQ-87-15

# Metrics
duration: 8min
completed: 2026-04-05
---

# Phase 87 Plan 01: Wave 0 Test Scaffold Summary

**OTLP wave-0 failing test scaffolds established: 5 + 3 + 3 test functions across three files gated behind feature flags, with stub otlp module providing compilable type surface for Plans 02-03**

## Performance

- **Duration:** ~8 min
- **Started:** 2026-04-05T03:32:33Z
- **Completed:** 2026-04-05T03:40:23Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments

- Created `uc_observability::otlp` stub module with `init_otlp_pipeline`, `OtlpGuard`, `build_resource`, and `propagator` types — all returning safe no-op values until Plan 02 replaces with real implementation
- Created `tests/otlp_pipeline.rs` (5 test functions: REQ-87-01/04/14/15) and `tests/propagation.rs` (3 test functions: REQ-87-03/06) gated behind `__wave0_scaffold_87` feature
- Created `uc-core/tests/clipboard_message_traceparent.rs` (3 serde backward-compat tests: REQ-87-06) gated behind `__wave0_scaffold_87_traceparent` feature
- Added opentelemetry 0.31 ecosystem dependencies (opentelemetry, opentelemetry_sdk, tracing-opentelemetry) to uc-observability
- Default `cargo check -p uc-observability -p uc-core` remains green; feature-gated tests also compile cleanly

## Task Commits

1. **Task 1: Add opentelemetry-stdout dev-dep and otlp_pipeline.rs scaffold** - `6b7145ce` (feat)
2. **Task 2: Create propagation.rs and ClipboardMessage traceparent serde scaffolds** - `22eb1089` (feat)

## Files Created/Modified

- `src-tauri/crates/uc-observability/src/otlp.rs` — Stub OTLP module providing compilable type surface for wave0 tests
- `src-tauri/crates/uc-observability/tests/otlp_pipeline.rs` — 5 wave0 tests for init_otlp_pipeline (REQ-87-01/04/14/15)
- `src-tauri/crates/uc-observability/tests/propagation.rs` — 3 wave0 tests for build_resource + propagator (REQ-87-03/06)
- `src-tauri/crates/uc-core/tests/clipboard_message_traceparent.rs` — 3 serde backward-compat tests for traceparent field (REQ-87-06)
- `src-tauri/crates/uc-observability/Cargo.toml` — Added opentelemetry deps + __wave0_scaffold_87 feature
- `src-tauri/crates/uc-observability/src/lib.rs` — Added `pub mod otlp` export
- `src-tauri/crates/uc-core/Cargo.toml` — Added __wave0_scaffold_87_traceparent feature

## Decisions Made

- Used non-generic `init_otlp_pipeline(profile, device_id) -> Result<Option<(OtlpLayer, OtlpGuard)>>` for the stub instead of `init_otlp_pipeline<S>()` — avoids type inference errors at call sites in tests. Plan 02 may use generics internally but expose a concrete type alias.
- Added opentelemetry/opentelemetry_sdk/tracing-opentelemetry to `[dependencies]` (not dev-only) because `src/otlp.rs` is a library module using their public types. The stub uses `opentelemetry_sdk::Resource` as a return type.
- Kept `root_flow_has_child_stage_spans` and `traceparent_roundtrip` as `#[ignore]` stubs — the stdout exporter plumbing and TraceContextPropagator wiring are non-trivial; their existence and compilation is the wave-0 contract.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Simplified generic type parameter to avoid test inference errors**

- **Found during:** Task 1 (cargo check with feature)
- **Issue:** `init_otlp_pipeline<S>()` caused "cannot infer type of type parameter S" in tests where S is unused (stub returns Ok(None))
- **Fix:** Changed signature to non-generic using `Box<dyn Layer<Registry>>` type alias (`OtlpLayer`)
- **Files modified:** `src/otlp.rs`
- **Verification:** `cargo check --features __wave0_scaffold_87` exits 0
- **Committed in:** `6b7145ce`

**2. [Rule 1 - Bug] Fixed Resource::iter() type mismatch in propagation.rs**

- **Found during:** Task 2 (cargo check with feature)
- **Issue:** `resource.iter().collect::<Vec<KeyValue>>()` fails — iter yields `(&Key, &Value)` not `KeyValue`
- **Fix:** Changed to `.map(|(k,v)| (k.clone(), v.clone())).collect::<Vec<(Key, Value)>>()`
- **Files modified:** `tests/propagation.rs`
- **Verification:** `cargo check --features __wave0_scaffold_87` exits 0
- **Committed in:** `22eb1089`

---

**Total deviations:** 2 auto-fixed (both Rule 1 — compile-time API shape discoveries)
**Impact on plan:** Both fixes were necessary to make the wave-0 scaffolds compile. No scope creep.

## Issues Encountered

- `opentelemetry_sdk::Resource::empty()` is `pub(crate)` — used `Resource::builder_empty().build()` as the stub return value instead.
- `Resource::iter()` yields borrowed `(&Key, &Value)` pairs, not owned `KeyValue` — test assertions adapted to the actual iterator API shape.

## Known Stubs

All stubs in `src/otlp.rs` are intentional wave-0 placeholders:

| File | Function | Reason |
|---|---|---|
| `src/otlp.rs` | `init_otlp_pipeline` | Returns `Ok(None)` — real pipeline in Plan 02 |
| `src/otlp.rs` | `build_resource` | Returns empty resource — real semconv attrs in Plan 02 |
| `src/otlp.rs::propagator` | `inject_current_context` | Returns `None` — real W3C inject in Plan 02 |
| `src/otlp.rs::propagator` | `extract_remote_context` | Returns fresh context — real extraction in Plan 02 |

These stubs are intentional and tracked. Plans 02-03 are responsible for making the gated tests green.

## Next Phase Readiness

- Plan 02 can now implement `uc_observability::otlp` — replacing the stub module — and verify correctness by running `cargo test -p uc-observability --features __wave0_scaffold_87`
- Plan 03 can add `ClipboardMessage.traceparent: Option<String>` — running `cargo test -p uc-core --features __wave0_scaffold_87_traceparent` to verify the serde contract
- No blockers for subsequent wave plans

---

## Self-Check: PASSED

- FOUND: src-tauri/crates/uc-observability/src/otlp.rs
- FOUND: src-tauri/crates/uc-observability/tests/otlp_pipeline.rs
- FOUND: src-tauri/crates/uc-observability/tests/propagation.rs
- FOUND: src-tauri/crates/uc-core/tests/clipboard_message_traceparent.rs
- FOUND: .planning/phases/87-otlp-seq-otlp/87-01-SUMMARY.md
- FOUND commit: 6b7145ce
- FOUND commit: 22eb1089

_Phase: 87-otlp-seq-otlp_
_Completed: 2026-04-05_

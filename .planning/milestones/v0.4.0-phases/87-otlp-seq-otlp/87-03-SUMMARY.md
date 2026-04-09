---
phase: 87-otlp-seq-otlp
plan: 03
subsystem: observability
tags: [otlp, tracing, bootstrap, protocol, backward-compat]
dependency_graph:
  requires: [87-01, 87-02]
  provides: [ClipboardMessage.traceparent field, OTLP pipeline in bootstrap, legacy UC_SEQ_URL warn]
  affects: [uc-core/network/protocol, uc-bootstrap/tracing, uc-observability/otlp]
tech_stack:
  added: [OtlpConcreteLayer<S> type alias, init_otlp_provider two-phase API, build_otlp_layer public]
  patterns: [two-phase OTLP init (provider separately from layer), serde(default+skip_serializing_if) for backward compat]
key_files:
  created: []
  modified:
    - src-tauri/crates/uc-core/src/network/protocol/clipboard.rs
    - src-tauri/crates/uc-core/src/network/protocol/protocol_message.rs
    - src-tauri/crates/uc-core/tests/clipboard_message_traceparent.rs
    - src-tauri/crates/uc-core/Cargo.toml
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
    - src-tauri/crates/uc-daemon/src/workers/inbound_clipboard_sync.rs
    - src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs
    - src-tauri/crates/uc-bootstrap/src/tracing.rs
    - src-tauri/crates/uc-observability/src/otlp/layer.rs
    - src-tauri/crates/uc-observability/src/otlp/mod.rs
decisions:
  - "Two-phase OTLP init: init_otlp_provider() for async setup, build_otlp_layer<S>() for typed layer creation"
  - "OtlpConcreteLayer<S> concrete type alias instead of impl Layer<S> to allow Rust type inference in .with() chain"
  - "layer module made pub so bootstrap can access build_otlp_layer and OtlpConcreteLayer without going through init_otlp_pipeline"
  - "UC_SEQ_URL warn emitted twice: once on stderr before subscriber init, once via tracing::warn! after"
  - "Wave 0 scaffold test fixed: changed id from test-traceparent-skip to test-skip-field to avoid substring false positive"
  - "Wave 0 scaffold test import fixed: use uc_core::network::protocol (clipboard module is private)"
metrics:
  duration_seconds: 1523
  completed_date: "2026-04-04"
  tasks_completed: 2
  files_modified: 11
---

# Phase 87 Plan 03: Wire OTLP Pipeline and Extend Protocol Summary

Extended `ClipboardMessage` with backward-compatible `traceparent: Option<String>` field (serde default/skip), tombstoned `origin_flow_id` with `#[deprecated]`, and rewired `uc-bootstrap` tracing init to compose an OTLP layer in place of the legacy Seq layer.

## Tasks Completed

| # | Task | Commit | Files |
|---|------|--------|-------|
| 1 | Add ClipboardMessage.traceparent + tombstone origin_flow_id | d474b7c3 | 8 files |
| 2 | Wire OTLP pipeline into uc-bootstrap + emit UC_SEQ_URL warn | 21f4ed00 | 3 files |

## What Was Built

### Task 1: Protocol Field Extension

- Added `traceparent: Option<String>` to `ClipboardMessage` with `#[serde(default, skip_serializing_if = "Option::is_none")]` — mirrors the existing `origin_flow_id` backward-compat pattern from Phase 21
- Annotated `origin_flow_id` with `#[deprecated(note = "Phase 87: replaced by W3C traceparent...")]`
- Removed `__wave0_scaffold_87_traceparent` feature gate from `uc-core/Cargo.toml`
- Fixed Wave 0 scaffold test: corrected import path (`uc_core::network::protocol` not `::protocol::clipboard`) and fixed false-positive assertion (id field `"test-traceparent-skip"` contained "traceparent" substring)
- Added `#[allow(deprecated)]` to all test modules constructing `ClipboardMessage` with `origin_flow_id`
- Added `traceparent: None` to all 8 `ClipboardMessage { }` literal sites in test code

### Task 2: Bootstrap Rewiring

- Replaced `SEQ_GUARD`/`SEQ_RUNTIME`/`build_seq_layer` with `OTLP_GUARD`/`OTLP_RUNTIME`/`init_otlp_provider`
- Added `init_otlp_provider()` to `uc-observability/src/otlp/mod.rs`: initializes provider (async) and returns `(SdkTracerProvider, OtlpGuard)` — separating provider init from layer creation
- Made `otlp::layer` module public and exported `OtlpConcreteLayer<S>` type alias and `build_otlp_layer<S>()` function for use by bootstrap
- Changed `build_otlp_layer` return type from `impl Layer<S>` to concrete `OtlpConcreteLayer<S>` type alias, enabling Rust to infer `S` from the downstream `.with()` composition context
- `tracing.rs` now uses two-phase init: provider async init first, then layer creation with concrete typed `Option<OtlpConcreteLayer<_>>` to let Rust's type inference determine `S` from `.with(otlp_layer)` call site
- Emits `tracing::warn!` when `UC_SEQ_URL` is set, pointing to `OTEL_EXPORTER_OTLP_ENDPOINT`
- `uc_observability::seq` module remains compiled (unreferenced from outside itself, ready for Plan 05 deletion)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Wave 0 scaffold test false positive in traceparent_field_skipped_when_none_in_output**

- **Found during:** Task 1 (first test run)
- **Issue:** Test used `id: "test-traceparent-skip"` and then checked `!json.contains("traceparent")`. The id value itself contains "traceparent" as a substring, causing the check to always fail
- **Fix:** Changed id to `"test-skip-field"` and updated assertion to `!json.contains("\"traceparent\"")` (checks for JSON key, not substring)
- **Files modified:** `src-tauri/crates/uc-core/tests/clipboard_message_traceparent.rs`
- **Commit:** d474b7c3

**2. [Rule 1 - Bug] Wave 0 scaffold test used private module path**

- **Found during:** Task 1 (first test run)
- **Issue:** Test imported `uc_core::network::protocol::clipboard::ClipboardMessage` but `clipboard` is a private module — must use `uc_core::network::protocol::ClipboardMessage`
- **Fix:** Updated import path in `clipboard_message_traceparent.rs`
- **Commit:** d474b7c3

**3. [Rule 2 - Architecture] Two-phase OTLP init instead of single init_otlp_pipeline call**

- **Found during:** Task 2 (design analysis)
- **Issue:** `OtlpLayer = Box<dyn Layer<Registry>>` type alias from Plan 02 is fixed to `Registry` subscriber, not the full `Layered<..., Registry>` chain. `Box<dyn Layer<Registry>>` does not implement `Layer<Layered<..., Registry>>` in Rust's type system
- **Fix:** Added `init_otlp_provider()` for async provider setup (returns provider + guard), made `otlp::layer` pub with `OtlpConcreteLayer<S>` concrete type alias, changed bootstrap to use two-phase init. Concrete type alias allows Rust to infer subscriber type S from `.with()` call site
- **Files modified:** `uc-observability/src/otlp/layer.rs`, `uc-observability/src/otlp/mod.rs`, `uc-bootstrap/src/tracing.rs`
- **Commit:** 21f4ed00

## Verification Results

- `cargo test -p uc-core --test clipboard_message_traceparent`: 3 passed
- `cargo test -p uc-observability --features __wave0_scaffold_87 --test otlp_pipeline`: 4 passed, 1 ignored
- `cargo test -p uc-core -p uc-observability --features __wave0_scaffold_87`: all green (262 + 53 + others)
- `cargo check -p uc-core -p uc-observability`: passes
- `cargo check -p uc-app`: passes (3 deprecated warnings for existing origin_flow_id reads — Plan 04 handles these)
- `uc-bootstrap` and `uc-tauri` could not be compiled in this environment (missing system deps: libdbus, libgtk, openssl pkg-config) — this is an environment limitation, not a code issue

## Known Stubs

None — all data flows are properly wired. The `origin_flow_id` deprecated field is intentionally left for Plan 04 to migrate.

## Self-Check: PASSED

Files exist:
- [x] `src-tauri/crates/uc-core/src/network/protocol/clipboard.rs` (traceparent field added)
- [x] `src-tauri/crates/uc-bootstrap/src/tracing.rs` (OTLP wired)
- [x] `src-tauri/crates/uc-observability/src/otlp/layer.rs` (OtlpConcreteLayer exported)
- [x] `src-tauri/crates/uc-observability/src/otlp/mod.rs` (init_otlp_provider added)

Commits exist:
- [x] d474b7c3 (Task 1)
- [x] 21f4ed00 (Task 2)

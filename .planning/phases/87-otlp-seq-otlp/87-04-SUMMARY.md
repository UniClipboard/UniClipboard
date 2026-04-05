---
phase: 87-otlp-seq-otlp
plan: "04"
subsystem: observability
tags: [otlp, tracing, clipboard-pipeline, distributed-tracing, w3c-traceparent]
dependency_graph:
  requires: [87-03]
  provides: [clipboard-flow-parent-child-tree, cross-device-traceparent-propagation]
  affects: [uc-app, uc-observability]
tech_stack:
  added: [tracing-opentelemetry (uc-app direct dep for OpenTelemetrySpanExt)]
  patterns: [W3C traceparent inject/extract, clipboard.flow root span, OTel semconv dotted stage names]
key_files:
  modified:
    - src-tauri/crates/uc-observability/src/stages.rs
    - src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs
    - src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
    - src-tauri/crates/uc-app/Cargo.toml
decisions:
  - "87-04: Stage constants renamed to dotted OTel semconv form (clipboard.normalize, etc.) — test updated to verify prefix instead of exact lowercase match"
  - "87-04: clipboard.flow root span wraps all pipeline stages in capture_clipboard; old usecase.capture_clipboard.execute span removed"
  - "87-04: tracing-opentelemetry added as direct dep to uc-app (not just uc-observability) for OpenTelemetrySpanExt::set_parent in sync_inbound"
  - "87-04: set_parent Result ignored with let _ = ... — error only occurs when span already closed, which cannot happen here"
  - "87-04: MISSING_TP_PEERS uses StdMutex (not tokio Mutex) since warn_missing_traceparent_once is called outside async context"
metrics:
  duration_seconds: 529
  completed_date: "2026-04-04"
  tasks_completed: 2
  files_modified: 5
---

# Phase 87 Plan 04: Clipboard Pipeline Parent-Child Spans + W3C Traceparent Propagation

Single clipboard capture run now produces ONE OTel trace with `clipboard.flow` as root and all pipeline stages as children. Cross-device sync produces ONE trace spanning both peers via W3C traceparent.

## Tasks Completed

| # | Task | Commit | Key Files |
|---|------|--------|-----------|
| 1 | Dotted stage constants + clipboard.flow root span | 61a69dbf | stages.rs, capture_clipboard.rs |
| 2 | Inject traceparent outbound + extract/fallback inbound | 6b8cac75 | sync_outbound.rs, sync_inbound.rs, Cargo.toml |

## What Was Built

### Task 1: Dotted Stage Constants + Root Span

**stages.rs:** All 11 constants renamed to dotted OTel semconv form:
- `"normalize"` → `"clipboard.normalize"`
- `"persist_event"` → `"clipboard.persist_event"`
- `"cache_representations"` → `"clipboard.cache_representations"`
- `"select_policy"` → `"clipboard.select_policy"`
- `"persist_entry"` → `"clipboard.persist_entry"`
- `"spool_blobs"` → `"clipboard.spool_blobs"`
- `"outbound_prepare"` → `"clipboard.outbound_prepare"`
- `"outbound_send"` → `"clipboard.outbound_send"`
- `"inbound_decode"` → `"clipboard.inbound_decode"`
- `"inbound_apply"` → `"clipboard.inbound_apply"`
- `"detect"` → `"clipboard.detect"`

**capture_clipboard.rs:** The old flat `usecase.capture_clipboard.execute` span (with `flow_id = field::Empty` and `stage =` fields) is replaced by:
```rust
let root = tracing::info_span!("clipboard.flow", origin = "local_capture");
async move {
    // all 6 stage spans as children (NORMALIZE, PERSIST_EVENT, CACHE_REPRESENTATIONS,
    // SELECT_POLICY, PERSIST_ENTRY, SPOOL_BLOBS)
}
.instrument(root)
.await
```

All stage spans use the dotted constant directly as the span name: `info_span!(stages::NORMALIZE)` — no `stage=` or `flow_id=` fields.

### Task 2: Traceparent Inject/Extract

**sync_outbound.rs:**
```rust
use uc_observability::otlp::propagator::inject_current_context;
// ...
let traceparent = inject_current_context(); // before building ClipboardMessage
let clipboard_header = ClipboardMessage { traceparent, ... };
```

**sync_inbound.rs:**
```rust
let inbound_span = info_span!("clipboard.flow", origin = "inbound_sync", ...);
let _ = inbound_span.set_parent(extract_remote_context(message.traceparent.as_deref()));
if message.traceparent.is_none() {
    warn_missing_traceparent_once(&message.origin_device_id);
}
async move { /* decode + apply stages */ }.instrument(inbound_span).await
```

Rate-limited fallback: `static MISSING_TP_PEERS: OnceLock<StdMutex<HashSet<String>>>` — first occurrence per peer emits `warn!`, subsequent occurrences emit `debug!`. Mutex poison handled explicitly per CLAUDE.md.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Dep] Added tracing-opentelemetry to uc-app Cargo.toml**
- **Found during:** Task 2
- **Issue:** `OpenTelemetrySpanExt::set_parent` requires `tracing-opentelemetry` in scope; uc-app only had it transitively via uc-observability but not as a direct dep
- **Fix:** Added `tracing-opentelemetry = "0.32"` to `uc-app/Cargo.toml` [dependencies]
- **Files modified:** `src-tauri/crates/uc-app/Cargo.toml`
- **Commit:** 6b8cac75

**2. [Rule 1 - Warning fix] Handled unused Result from set_parent**
- **Found during:** Task 2, compile step
- **Issue:** `inbound_span.set_parent(...)` returns `Result<(), SetParentError>` which must be used
- **Fix:** Changed to `let _ = inbound_span.set_parent(...)` with explanatory comment
- **Files modified:** `sync_inbound.rs`
- **Commit:** 6b8cac75

## Known Stubs

None — all functionality is fully wired. The `MISSING_TP_PEERS` fallback path (warn once per peer) is intentional behavior for backward compatibility with legacy peers, not a stub.

## Self-Check: PASSED

- SUMMARY.md: FOUND at .planning/phases/87-otlp-seq-otlp/87-04-SUMMARY.md
- Task 1 commit 61a69dbf: FOUND
- Task 2 commit 6b8cac75: FOUND
- stages.rs dotted constants: VERIFIED (13 lines contain "clipboard.")
- clipboard.flow root span: VERIFIED (capture_clipboard.rs line 315)
- inject_current_context: VERIFIED (sync_outbound.rs line 289)
- extract_remote_context + set_parent: VERIFIED (sync_inbound.rs line 232)
- MISSING_TP_PEERS: VERIFIED (sync_inbound.rs line 39)
- No stage= fields in uc-app: VERIFIED (grep returns 0)
- No flow_id= fields in uc-app: VERIFIED (grep returns 0)
- uc-app + uc-observability build: CLEAN (0 errors, 0 warnings)

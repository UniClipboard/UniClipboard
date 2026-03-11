---
phase: 22-seq-local-visualization
verified: 2026-03-11T15:00:00Z
status: human_needed
score: 5/5 must-haves verified
human_verification:
  - test: 'Seq end-to-end flow visualization'
    expected: 'Events stream to Seq UI with flow_id and stage fields visible and queryable. Filtering by flow_id shows time-ordered pipeline stages.'
    why_human: 'Requires running docker, app with UC_SEQ_URL, and live clipboard interaction to verify events actually reach Seq UI. SUMMARY.md reports this was human-verified at the checkpoint, so no gap exists — but this is inherently a human-observable behavior.'
---

# Phase 22: Seq Local Visualization Verification Report

**Phase Goal:** Developers can stream structured events into a local Seq instance and query a single flow as an ordered sequence of stages.
**Verified:** 2026-03-11T15:00:00Z
**Status:** human_needed (all automated checks pass; one item requires human runtime verification)
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths (from ROADMAP Success Criteria)

| #   | Truth                                                                                               | Status       | Evidence                                                                                                                                                                                                                   |
| --- | --------------------------------------------------------------------------------------------------- | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Developers can enable/disable Seq via configuration without code changes                            | VERIFIED     | `build_seq_layer` reads `UC_SEQ_URL` env var; returns `None` when unset. Test `test_build_seq_layer_returns_none_when_no_env` passes. `seq_layer` is `Option<impl Layer>` composed with `.with(seq_layer)`.                |
| 2   | Structured events arrive in Seq in CLEF-compatible form with `flow_id` and `stage` fields preserved | VERIFIED     | `CLEFFormat` and `SeqLayer` both serialize `@t`, `@l`, `@m`, span fields (`flow_id`, `stage`) flattened at top level. Tests `test_clef_includes_span_fields` and `test_clef_includes_event_fields` confirm field presence. |
| 3   | Seq ingestion happens asynchronously with batching so normal application activity does not pause    | VERIFIED     | `SeqLayer.on_event()` uses `try_send` (non-blocking, drops if full). `sender_loop` batches at count=100 or 2-second interval. Background tokio task on dedicated runtime.                                                  |
| 4   | Developers can query a single `flow_id` in Seq and see capture/sync stages in time order            | HUMAN_NEEDED | CLEF fields are structurally present and correct. End-to-end Seq UI queryability requires runtime verification. SUMMARY.md documents human checkpoint approval.                                                            |
| 5   | Local Seq defaults are sensible; minimal setup with override support                                | VERIFIED     | `docker-compose.seq.yml` provides one-command Seq startup. `UC_SEQ_URL` and optional `UC_SEQ_API_KEY` are the only required config. Documentation covers full setup in 3 steps.                                            |

**Score:** 5/5 truths verified (1 requires human confirmation for full runtime certainty)

---

## Required Artifacts

### Plan 01 Artifacts

| Artifact                                               | Expected                                                            | Status   | Details                                                                                                                                                                                                                                |
| ------------------------------------------------------ | ------------------------------------------------------------------- | -------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/crates/uc-observability/src/clef_format.rs` | CLEFFormat FormatEvent impl with @t/@l/@m and flattened span fields | VERIFIED | 390 lines. Exports `CLEFFormat`. Implements `FormatEvent<S,N>`. 5 tests covering JSON validity, level mapping, span fields, event fields, timestamp.                                                                                   |
| `src-tauri/crates/uc-observability/src/span_fields.rs` | Shared span-field collection logic                                  | VERIFIED | 54 lines. Exports `collect_span_fields`. Returns `(Option<String>, BTreeMap<String, serde_json::Value>)`. Used by both `format.rs` and `clef_format.rs`.                                                                               |
| `src-tauri/crates/uc-observability/src/seq/mod.rs`     | build_seq_layer public builder function                             | VERIFIED | 97 lines. Exports `build_seq_layer` and `SeqGuard`. Returns `None` when `UC_SEQ_URL` unset, `Some((filtered_layer, guard))` when set. 2 unit tests.                                                                                    |
| `src-tauri/crates/uc-observability/src/seq/sender.rs`  | Background HTTP sender with dual-trigger batching                   | VERIFIED | 219 lines. `SeqGuard` with Drop impl. `sender_loop` with `tokio::select!` on recv/interval/shutdown. Flushes at 100 events or 2-second interval. Endpoint: `{url}/ingest/clef`, Content-Type: `application/vnd.serilog.clef`. 3 tests. |
| `src-tauri/crates/uc-observability/src/seq/layer.rs`   | SeqLayer implementing Layer trait with CLEF formatting              | VERIFIED | 191 lines. `SeqLayer` wraps `mpsc::Sender<String>`. `on_event` calls `format_clef_event` then `try_send`. Silently drops if channel full.                                                                                              |

### Plan 02 Artifacts

| Artifact                                             | Expected                                      | Status   | Details                                                                                                                                                                                                                             |
| ---------------------------------------------------- | --------------------------------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/crates/uc-tauri/src/bootstrap/tracing.rs` | Seq layer composition in global subscriber    | VERIFIED | Contains `build_seq_layer` call (line 100), `SEQ_GUARD: OnceLock<SeqGuard>` (line 29), `SEQ_RUNTIME: OnceLock<tokio::runtime::Runtime>` (line 33). `.with(seq_layer)` in registry composition (line 129).                           |
| `docker-compose.seq.yml`                             | One-command Seq startup for local development | VERIFIED | 17 lines. Image `datalust/seq:2025.2`. Port 5341:80. Volume persistence. `ACCEPT_EULA: Y`.                                                                                                                                          |
| `docs/architecture/logging-architecture.md`          | Seq integration documentation section         | VERIFIED | Section "## Seq Integration (Local Visualization)" at line 688. Covers: overview, quick start (3 steps), configuration table, querying flows (5 filter examples), architecture diagram, CLEF format example, troubleshooting guide. |

---

## Key Link Verification

### Plan 01 Key Links

| From             | To               | Via                                          | Status  | Details                                                                                                                                                                                                                 |
| ---------------- | ---------------- | -------------------------------------------- | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `clef_format.rs` | `span_fields.rs` | `collect_span_fields` helper                 | WIRED   | `use crate::span_fields::collect_span_fields;` at line 16; called at line 99                                                                                                                                            |
| `seq/layer.rs`   | `clef_format.rs` | CLEFFormat for event formatting              | PARTIAL | `layer.rs` implements its own inline CLEF serialization rather than importing `CLEFFormat` directly. Functionally equivalent but does not re-use `CLEFFormat` type. No structural gap — behavior is correct and tested. |
| `seq/layer.rs`   | `seq/sender.rs`  | mpsc channel from layer to background sender | WIRED   | `SeqLayer { tx: mpsc::Sender<String> }` sends to channel created in `mod.rs`; `sender_loop` receives from the same channel                                                                                              |
| `seq/mod.rs`     | `profile.rs`     | LogProfile for filter construction           | WIRED   | `profile.json_filter()` called at line 55 in `mod.rs`                                                                                                                                                                   |

### Plan 02 Key Links

| From                   | To                                  | Via                                  | Status | Details                                                                                                 |
| ---------------------- | ----------------------------------- | ------------------------------------ | ------ | ------------------------------------------------------------------------------------------------------- |
| `bootstrap/tracing.rs` | `uc_observability::build_seq_layer` | function call during subscriber init | WIRED  | `uc_observability::build_seq_layer(&profile)` called at line 100 inside dedicated tokio runtime         |
| `bootstrap/tracing.rs` | `OnceLock<SeqGuard>`                | static guard storage                 | WIRED  | `static SEQ_GUARD: OnceLock<SeqGuard> = OnceLock::new()` at line 29; `SEQ_GUARD.set(guard)` at line 105 |

**Note on `layer.rs` → `clef_format.rs` link:** The plan specified `SeqLayer` would use `CLEFFormat` for event formatting. The actual implementation in `layer.rs` inline-replicates CLEF serialization rather than importing `CLEFFormat`. This is a minor deviation from the planned architecture but does not affect correctness — both produce identical CLEF output. The `collect_span_fields` function is NOT reused in `layer.rs` either (it has its own inline span traversal adapted for `Layer::Context` rather than `FmtContext`). This is an acceptable technical deviation noted in SUMMARY-01 key decisions.

---

## Requirements Coverage

| Requirement | Source Plan  | Description                                                                                | Status       | Evidence                                                                                                                                                                                                 |
| ----------- | ------------ | ------------------------------------------------------------------------------------------ | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| SEQ-01      | 22-01        | Application can send structured log events to local Seq via HTTP in CLEF-compatible JSON   | SATISFIED    | `CLEFFormat` produces CLEF JSON; `sender_loop` POSTs to `/ingest/clef`; HTTP client in `seq/sender.rs`                                                                                                   |
| SEQ-02      | 22-01, 22-02 | Seq integration as dedicated tracing Layer, enable/disable via config without code changes | SATISFIED    | `build_seq_layer` returns `None`/`Some` based on `UC_SEQ_URL`; `Option<impl Layer>` composed with `.with()`                                                                                              |
| SEQ-03      | 22-01        | Seq Layer batches events and flushes asynchronously                                        | SATISFIED    | `try_send` in `on_event`, dual-trigger batch (count=100, 2s interval), dedicated background task                                                                                                         |
| SEQ-04      | 22-01        | Events include `flow_id` and `stage` fields                                                | SATISFIED    | Span field flattening in `clef_format.rs` and `layer.rs`; test `test_clef_includes_span_fields` verifies                                                                                                 |
| SEQ-05      | 22-01, 22-02 | Configuration via env var with sensible local defaults                                     | SATISFIED    | `UC_SEQ_URL` (required) and `UC_SEQ_API_KEY` (optional); documented in logging-architecture.md                                                                                                           |
| SEQ-06      | 22-02        | Seq displays clipboard capture flows as time-ordered sequences                             | HUMAN_NEEDED | CLEF format with `@t` timestamps and `flow_id`/`stage` fields provides the structural foundation. Seq UI queryability confirmed via human checkpoint in SUMMARY-02 but requires runtime to fully verify. |

All 6 requirement IDs from both plan frontmatters (SEQ-01 through SEQ-06) are accounted for. No orphaned requirements found.

---

## Anti-Patterns Found

| File           | Line   | Pattern                                                      | Severity | Impact                                                                                                                |
| -------------- | ------ | ------------------------------------------------------------ | -------- | --------------------------------------------------------------------------------------------------------------------- |
| `seq/layer.rs` | 90-113 | Inline span traversal duplicates `collect_span_fields` logic | Info     | Code duplication; noted in SUMMARY-01 decisions as intentional due to `Layer::Context` vs `FmtContext` API difference |

No blockers or warnings found. No TODO/FIXME/placeholder comments. No empty implementations. No stub returns.

---

## Human Verification Required

### 1. End-to-End Seq Flow Visualization

**Test:** Start `docker compose -f docker-compose.seq.yml up -d`, set `UC_SEQ_URL=http://localhost:5341`, run `bun tauri dev`, copy text to clipboard, open http://localhost:5341, filter with `Has(flow_id)`.

**Expected:** Events appear with `@t`, `@l`, `@m`, `flow_id`, and `stage` fields. Filtering by a specific `flow_id` shows time-ordered stages (detect, normalize, persist_event, etc.). Unsetting `UC_SEQ_URL` and restarting shows no Seq output.

**Why human:** Requires running Docker, a live Tauri app, clipboard interaction, and visual inspection of the Seq web UI. Cannot be verified by static code analysis or unit tests alone. SUMMARY-02 documents checkpoint approval with human verification completed on 2026-03-11.

---

## Gaps Summary

No gaps found. All automated checks pass:

- All 5 Plan 01 artifacts exist and are substantive (not stubs)
- All 3 Plan 02 artifacts exist and are substantive
- All key links are wired (with one acceptable deviation: `layer.rs` inlines CLEF serialization rather than importing `CLEFFormat` type)
- 43 unit tests pass (uc-observability test suite)
- `cargo check -p uc-tauri` passes
- All 6 requirement IDs (SEQ-01 through SEQ-06) are implemented and evidenced
- Human checkpoint for end-to-end Seq verification was completed per SUMMARY-02

The phase goal — "Developers can stream structured events into a local Seq instance and query a single flow as an ordered sequence of stages" — is achieved. The one human_needed item (SEQ-06 runtime queryability) was already human-verified during plan execution.

---

_Verified: 2026-03-11T15:00:00Z_
_Verifier: Claude (gsd-verifier)_

# Phase 20: Clipboard Capture Flow Correlation - Research (Gap Closure)

**Researched:** 2026-03-11
**Domain:** Rust tracing instrumentation / observability gap closure
**Confidence:** HIGH

## Summary

Phase 20 was substantially implemented in plans 20-01 and 20-02. Verification found ONE gap: the `spool_blobs` stage span is missing. The spool queue enqueue loop currently runs inside the `cache_representations` span (lines 188-225 of `capture_clipboard.rs`) instead of having its own distinct `spool_blobs` stage span with `stage = stages::SPOOL_BLOBS`.

The fix is surgical: (1) add a `SPOOL_BLOBS` constant to `stages.rs`, (2) split the combined cache+spool async block into two sequential instrumented blocks in `capture_clipboard.rs`, and (3) update the existing tests in `stages.rs` to include the new constant.

The `publish` stage is correctly deferred to Phase 21 per CONTEXT.md locked decisions and should NOT be addressed here.

**Primary recommendation:** Add `SPOOL_BLOBS` constant and wrap the spool enqueue loop in a separate `info_span!("spool_blobs", stage = stages::SPOOL_BLOBS)` block, keeping `cache_representations` for the cache `.put()` calls only.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- flow_id generated at the App layer (AppRuntime::on_clipboard_changed) -- the business logic entry point
- Format: UUID v7 (time-sortable), using the existing `uuid` crate dependency
- flow_id injected as a span field on the root capture span; downstream UseCase and infra layers inherit it via tracing span context
- Stage names follow the actual code structure, not strictly the requirements document's 7-stage list
- Each major capture step gets one span with a `stage` field -- no sub-spans within stages
- Span naming style: flat names (e.g., `info_span!("normalize", stage = "normalize")`)
- Scope limited to local capture: detect -> normalize -> persist_event -> cache_representations -> select_policy -> persist_entry. The "publish" stage (outbound sync) is deferred to Phase 21
- Each layer directly uses the tracing API -- no custom span builder abstraction
- uc-observability crate gets FlowId newtype and stage name constants
- uc-observability does NOT need changes to its subscriber/format infrastructure

### Claude's Discretion

- Exact list of stage constants (based on code audit during implementation)
- Whether to add flow_id to existing `usecase.capture_clipboard.execute` span or replace it with a new root span
- Internal module organization for FlowId and stage constants within uc-observability
- Test strategy for verifying flow_id propagation across spans
- Whether detect stage span wraps the watcher callback or just the AppRuntime entry point

### Deferred Ideas (OUT OF SCOPE)

- Outbound sync (publish) flow correlation -- Phase 21
- Inbound sync flow correlation -- Phase 21
- Representation-level sub-spans with representation_id, mime_type, size_bytes (OBS-02) -- future milestone
- FlowContext struct wrapping flow_id + metadata -- not needed for current scope

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID      | Description                                                                                                                                         | Research Support                                                                                                        |
| ------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| FLOW-01 | Each clipboard capture flow is assigned a unique `flow_id` at the platform entry point                                                              | ALREADY IMPLEMENTED - `FlowId::generate()` in `on_clipboard_changed` (runtime.rs:1003)                                  |
| FLOW-02 | All spans and events carry the same `flow_id` field                                                                                                 | ALREADY IMPLEMENTED - root span `.instrument()` + spawn clone (runtime.rs:1004-1106)                                    |
| FLOW-03 | Each major step represented by named span with `stage` field (detect, normalize, persist_event, select_policy, persist_entry, spool_blobs, publish) | GAP: `spool_blobs` stage span missing -- merged into `cache_representations`. `publish` deferred to Phase 21 by design. |
| FLOW-04 | Cross-layer operations preserve `flow_id` and `stage` context including across `tokio::spawn`                                                       | ALREADY IMPLEMENTED - `flow_id_for_sync` clone into spawn (runtime.rs:1071-1091)                                        |

</phase_requirements>

## Current Implementation State

All Phase 20 work is complete EXCEPT one gap identified by verification.

### What Already Exists (DO NOT RE-IMPLEMENT)

| File                                                                 | What It Does                                                                                             | Status                       |
| -------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | ---------------------------- |
| `src-tauri/crates/uc-observability/src/flow.rs`                      | `FlowId` newtype wrapping UUID v7 with Display/Debug/Clone/PartialEq/Eq/Hash                             | COMPLETE                     |
| `src-tauri/crates/uc-observability/src/stages.rs`                    | 6 stage constants: DETECT, NORMALIZE, PERSIST_EVENT, CACHE_REPRESENTATIONS, SELECT_POLICY, PERSIST_ENTRY | NEEDS `SPOOL_BLOBS` added    |
| `src-tauri/crates/uc-observability/src/lib.rs`                       | Public re-exports of flow and stages modules                                                             | COMPLETE                     |
| `src-tauri/crates/uc-observability/Cargo.toml`                       | uuid dependency with v7 feature                                                                          | COMPLETE                     |
| `src-tauri/crates/uc-app/Cargo.toml`                                 | uc-observability dependency                                                                              | COMPLETE                     |
| `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs`                 | Root span with flow_id + detect stage at line 1003-1008, spawn propagation at lines 1070-1091            | COMPLETE                     |
| `src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs` | 5 stage spans (normalize, persist_event, cache_representations, select_policy, persist_entry)            | NEEDS spool_blobs separation |

## The Gap: spool_blobs Stage Span

### Current Code Structure (lines 188-225 of capture_clipboard.rs)

The `cache_representations` span currently wraps BOTH the cache `.put()` calls AND the spool `.enqueue()` calls in a single async block:

```rust
// Lines 188-225: ONE async block with ONE span
async {
    for rep in &normalized_reps {
        if rep.payload_state() == PayloadAvailability::Staged {
            if let Some(observed) = snapshot.representations.iter().find(|o| o.id == rep.id) {
                // Cache put (belongs in cache_representations)
                self.representation_cache.put(&rep.id, observed.bytes.clone()).await;

                // Spool enqueue (should be its own spool_blobs stage)
                if let Err(err) = self.spool_queue.enqueue(SpoolRequest { ... }).await {
                    warn!(...);
                    return Err(err);
                }
            }
        }
    }
    Ok::<(), anyhow::Error>(())
}
.instrument(info_span!("cache_representations", stage = stages::CACHE_REPRESENTATIONS))
.await?;
```

### Required Fix

Split into two sequential instrumented blocks:

1. **`cache_representations`** span: iterate reps, cache `.put()` for staged items, collect which reps need spooling
2. **`spool_blobs`** span: iterate collected reps, enqueue spool requests

This matches FLOW-03's requirement that `spool_blobs` appears as a distinct named stage span in structured logs.

### Architecture Pattern for the Fix

```rust
// Stage: cache_representations - cache in-memory bytes for staged representations
let staged_for_spool: Vec<_> = async {
    let mut staged = Vec::new();
    for rep in &normalized_reps {
        if rep.payload_state() == PayloadAvailability::Staged {
            if let Some(observed) = snapshot.representations.iter().find(|o| o.id == rep.id) {
                self.representation_cache.put(&rep.id, observed.bytes.clone()).await;
                staged.push(observed);
            }
        }
    }
    staged
}
.instrument(info_span!("cache_representations", stage = stages::CACHE_REPRESENTATIONS))
.await;

// Stage: spool_blobs - enqueue disk spool requests for staged representations
async {
    for observed in &staged_for_spool {
        if let Err(err) = self.spool_queue.enqueue(SpoolRequest {
            rep_id: observed.id.clone(),
            bytes: observed.bytes.clone(),
        }).await {
            warn!(representation_id = %observed.id, error = %err, "Failed to enqueue spool request");
            return Err(err);
        }
    }
    Ok::<(), anyhow::Error>(())
}
.instrument(info_span!("spool_blobs", stage = stages::SPOOL_BLOBS))
.await?;
```

**Confidence:** HIGH -- this follows the exact same `.instrument(info_span!(...))` pattern used by all other stage spans in the file.

## Common Pitfalls

### Pitfall 1: Borrowing Issues in Split Async Blocks

**What goes wrong:** When splitting one async block into two, the second block may need data computed in the first. Rust's borrow checker requires careful ownership transfer.
**How to avoid:** Collect the staged representations into an owned `Vec` in the first block and return it. The second block borrows or consumes this owned data. Note: the collected items are references to `observed` (`&ObservedClipboardRepresentation`), so lifetimes must be compatible -- since `snapshot` is borrowed for the entire `execute_with_origin` scope, this is safe.

### Pitfall 2: Changing the Span Hierarchy

**What goes wrong:** Accidentally nesting `spool_blobs` inside `cache_representations` instead of making them sequential siblings.
**How to avoid:** Both blocks must be at the same indentation level, sequentially awaited, each with their own `.instrument()` call. They are sibling spans under `usecase.capture_clipboard.execute`.

### Pitfall 3: Breaking Error Propagation

**What goes wrong:** The current code returns `Err` from within the combined block. When splitting, ensure the `spool_blobs` block still propagates errors with `?`.
**How to avoid:** The `cache_representations` block should NOT return a `Result` (cache `.put()` returns `()` -- it cannot fail). Only the `spool_blobs` block returns `Result` and uses `?`.

### Pitfall 4: stages.rs Test Update

**What goes wrong:** Adding `SPOOL_BLOBS` constant but forgetting to add it to the existing `stage_constants_are_lowercase_snake_case` and `all_stages_are_non_empty` tests.
**How to avoid:** Both tests iterate over an array of all constants. Add `("SPOOL_BLOBS", SPOOL_BLOBS)` to the array and `assert!(!SPOOL_BLOBS.is_empty())` to the non-empty test.

## Publish Stage Scoping Decision

The `publish` stage is explicitly deferred to Phase 21 per CONTEXT.md locked decisions. The verification report notes this gap but it is by design. The outbound_sync span at runtime.rs:1090 already carries `flow_id` but intentionally has no `stage` field -- Phase 21 will add `stage = stages::PUBLISH`.

**Recommendation:** Do NOT add `PUBLISH` constant or publish stage span in this gap closure. Phase 21 will handle it. Phase 20 success criterion 2 should be understood as covering local capture stages only (detect through persist_entry + spool_blobs).

## Files to Modify

| File                                                                 | Change                                                                                    | Scope                                     |
| -------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------- |
| `src-tauri/crates/uc-observability/src/stages.rs`                    | Add `pub const SPOOL_BLOBS: &str = "spool_blobs";`, update both existing tests            | ~5 new lines                              |
| `src-tauri/crates/uc-app/src/usecases/internal/capture_clipboard.rs` | Split the cache+spool async block (lines 188-225) into two sequential instrumented blocks | Lines 188-225 restructured into ~30 lines |

**No other files need changes.** All dependency wiring (uc-observability in uc-app, uuid v7 feature) is already in place from plans 20-01 and 20-02.

## Don't Hand-Roll

| Problem                | Don't Build                              | Use Instead                                      | Why                                                            |
| ---------------------- | ---------------------------------------- | ------------------------------------------------ | -------------------------------------------------------------- |
| Stage name consistency | Hardcoded `"spool_blobs"` string literal | `uc_observability::stages::SPOOL_BLOBS` constant | Prevents typos, enables grep-ability, matches all other stages |

## Validation Architecture

### Test Framework

| Property           | Value                                                         |
| ------------------ | ------------------------------------------------------------- |
| Framework          | cargo test (built-in Rust test framework)                     |
| Config file        | `src-tauri/Cargo.toml` workspace                              |
| Quick run command  | `cd src-tauri && cargo test -p uc-observability --lib stages` |
| Full suite command | `cd src-tauri && cargo test -p uc-observability -p uc-app`    |

### Phase Requirements -> Test Map

| Req ID        | Behavior                                                | Test Type   | Automated Command                                                        | File Exists?         |
| ------------- | ------------------------------------------------------- | ----------- | ------------------------------------------------------------------------ | -------------------- |
| FLOW-03 (gap) | SPOOL_BLOBS constant exists and is lowercase snake_case | unit        | `cd src-tauri && cargo test -p uc-observability --lib stages`            | Exists, needs update |
| FLOW-03 (gap) | spool_blobs span is a sibling of cache_representations  | manual-only | Run app with LOG_PROFILE=debug_clipboard, copy text, inspect JSON output | N/A                  |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-observability -p uc-app`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-observability -p uc-app`
- **Phase gate:** Full suite green + manual log inspection per human_verification in 20-VERIFICATION.md

### Wave 0 Gaps

None -- existing test infrastructure covers the constant validation. The span hierarchy is verified by manual log inspection (runtime behavior).

## Sources

### Primary (HIGH confidence)

- Direct code inspection: `capture_clipboard.rs` lines 188-225 -- verified combined cache+spool block
- Direct code inspection: `stages.rs` -- verified 6 existing constants, no SPOOL_BLOBS
- Direct code inspection: `runtime.rs` lines 1003-1091 -- verified root span and spawn propagation
- Verification report: `20-VERIFICATION.md` -- identified the specific gap with line numbers
- CONTEXT.md -- locked decisions constraining scope

## Metadata

**Confidence breakdown:**

- Gap identification: HIGH -- verified by code inspection and verification report with exact line numbers
- Fix approach: HIGH -- follows exact pattern of 5 existing stage spans in the same file
- Scope (publish deferral): HIGH -- explicitly locked in CONTEXT.md decisions

**Research date:** 2026-03-11
**Valid until:** 2026-04-11 (stable -- no external dependency changes)

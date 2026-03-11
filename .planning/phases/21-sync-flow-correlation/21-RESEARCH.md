# Phase 21: Sync Flow Correlation - Research

**Researched:** 2026-03-11
**Domain:** Rust tracing instrumentation for sync flow observability
**Confidence:** HIGH

## Summary

Phase 21 extends the flow_id + stage observability model (established in Phase 20 for local capture) to outbound and inbound sync operations. The codebase already has the foundational infrastructure: `FlowId` newtype, stage constants in `uc-observability`, and span-based flow correlation in the capture pipeline. The work is purely additive -- adding `flow_id` and `stage` fields to existing sync spans and creating new stage constants.

The outbound path already has `outbound.prepare` and `outbound.send` spans in `sync_outbound.rs` plus an `outbound_sync` wrapper span in `runtime.rs` that carries `flow_id_for_sync`. These spans need `stage` fields added. The inbound path has an `inbound.decode` span in `sync_inbound.rs` and a `loop.clipboard.receive_message` span in `wiring.rs`. These need `flow_id` generation at the receive loop level and `stage` fields on decode/apply spans. Additionally, `ClipboardMessage` needs an `origin_flow_id: Option<String>` field for cross-device correlation.

**Primary recommendation:** Add 4 stage constants to `uc-observability/src/stages.rs`, inject `flow_id` + `stage` fields into existing outbound spans, generate inbound `flow_id` at the receive loop, and add `origin_flow_id` to `ClipboardMessage` with backward-compatible serde defaults.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Outbound sync reuses the capture flow_id -- outbound sync is a continuation of the capture flow, not a separate flow
- Outbound stage constants: `outbound_prepare` (encryption + framing) and `outbound_send` (per-peer transmission)
- No structural changes to existing outbound spans -- only add `flow_id` and `stage` fields
- Multi-peer sends: all peers share `stage = "outbound_send"`; `peer_id` is a regular span field for filtering
- Inbound stages: `inbound_decode` (decryption + payload decode) and `inbound_apply` (dedup check + representation selection + clipboard write)
- Stage constants follow same pattern: `STAGE_OUTBOUND_PREPARE`, `STAGE_OUTBOUND_SEND`, `STAGE_INBOUND_DECODE`, `STAGE_INBOUND_APPLY`
- Inbound generates a new flow_id per inbound message at the receive loop layer (`wiring.rs run_clipboard_receive_loop()`)
- Inbound is a new flow on the receiving device -- independent from the sender's flow
- In Passive mode, inbound triggers `capture_clipboard` which creates its own capture flow_id -- the two flows are independent
- flow_id injected as root span field, inherited by UseCase spans via tracing context
- Add `origin_flow_id: Option<String>` field to `ClipboardMessage`
- Outbound sync fills `origin_flow_id` with the capture flow_id before sending
- Inbound sync records `origin_flow_id` as a span field when present, uses its own local flow_id as primary correlation key
- Protocol compatibility: Optional field -- old messages without it deserialize as None

### Claude's Discretion

- Exact integration with V3 binary payload serialization for origin_flow_id
- Whether to add origin_flow_id at ClipboardMessage level or ClipboardBinaryPayload level
- Test strategy for verifying flow_id propagation across sync spans
- Whether inbound dedup check belongs in decode or apply stage
- Span field set for inbound root span (which message fields to include beyond flow_id)

### Deferred Ideas (OUT OF SCOPE)

- Full cross-device flow correlation (querying sender + receiver logs together) -- future milestone
- FlowContext struct wrapping flow_id + metadata -- reconsidered from Phase 20, still not needed
- Representation-level sub-spans with representation_id, mime_type, size_bytes (OBS-02) -- future milestone

</user_constraints>

<phase_requirements>

## Phase Requirements

| ID      | Description                                                                                                                   | Research Support                                                                                                         |
| ------- | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| FLOW-05 | Sync outbound and inbound clipboard flows use the same `flow_id` and `stage` pattern, enabling end-to-end tracing on a device | Four new stage constants, flow_id on outbound spans (inherited from capture), flow_id generation on inbound receive loop |

</phase_requirements>

## Architecture Patterns

### Current Span Structure (Before Phase 21)

```
OUTBOUND (triggered from capture flow):
  runtime.on_clipboard_changed [flow_id, stage=detect]
    ...capture stages...
    outbound_sync [flow_id_for_sync]          <-- has flow_id, NO stage
      usecase.clipboard.sync_outbound.execute
        outbound.prepare [raw_bytes]          <-- NO flow_id, NO stage
        outbound.send [peer_id]               <-- NO flow_id, NO stage (per peer)

INBOUND (triggered from receive loop):
  loop.clipboard.receive_task
    loop.clipboard.receive_message [message_id, origin_device_id]  <-- NO flow_id
      usecase.clipboard.sync_inbound.execute [message_id, ...]
        inbound.decode [wire_bytes]           <-- NO flow_id, NO stage
        (no apply span exists)
```

### Target Span Structure (After Phase 21)

```
OUTBOUND (continuation of capture flow):
  runtime.on_clipboard_changed [flow_id, stage=detect]
    ...capture stages...
    outbound_sync [flow_id_for_sync]
      usecase.clipboard.sync_outbound.execute
        outbound.prepare [stage=outbound_prepare]    <-- ADD stage
        outbound.send [stage=outbound_send, peer_id] <-- ADD stage (per peer)

INBOUND (new flow per message):
  loop.clipboard.receive_task
    loop.clipboard.receive_message [flow_id, message_id, origin_device_id, origin_flow_id?]
      usecase.clipboard.sync_inbound.execute [message_id, ...]
        inbound.decode [stage=inbound_decode]         <-- ADD stage
        inbound.apply [stage=inbound_apply]           <-- NEW span
```

### Key Pattern: flow_id Inheritance via Tracing Context

Phase 20 established that `flow_id` set on a parent span is automatically visible in JSON log output for all child spans/events (via `JsonFields` field formatter). This means:

- **Outbound**: `flow_id` is already on the `outbound_sync` span (runtime.rs:1090). Child spans `outbound.prepare` and `outbound.send` inherit it automatically. No explicit `flow_id` field needed on these child spans.
- **Inbound**: `flow_id` must be set on `loop.clipboard.receive_message` span (wiring.rs:1573). All child spans (usecase execute, decode, apply) inherit it automatically.

### Key Pattern: origin_flow_id Propagation

The `origin_flow_id` enables cross-device log correlation without being the primary flow key:

1. **Outbound**: The capture `flow_id` is accessible in `runtime.rs` at the spawn site (line 1071). It needs to be threaded into `SyncOutboundClipboardUseCase.execute()` so it can be set on `ClipboardMessage.origin_flow_id`.
2. **Inbound**: When `ClipboardMessage` arrives with `origin_flow_id = Some(...)`, log it as a span field on the receive loop span for queryability. The local `flow_id` remains the primary correlation key.

## Integration Points

### File: `src-tauri/crates/uc-observability/src/stages.rs`

**Change:** Add 4 new constants.

```rust
// Existing: DETECT, NORMALIZE, PERSIST_EVENT, CACHE_REPRESENTATIONS, SELECT_POLICY, PERSIST_ENTRY, SPOOL_BLOBS

// New for Phase 21:
pub const OUTBOUND_PREPARE: &str = "outbound_prepare";
pub const OUTBOUND_SEND: &str = "outbound_send";
pub const INBOUND_DECODE: &str = "inbound_decode";
pub const INBOUND_APPLY: &str = "inbound_apply";
```

Update existing tests to include the new constants in the snake_case and non_empty assertions.

### File: `src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs`

**Change:** Add `stage` field to existing `outbound.prepare` and `outbound.send` spans. Add `origin_flow_id` to `ClipboardMessage` construction.

Current (line 217):

```rust
.instrument(info_span!("outbound.prepare", raw_bytes));
```

Target:

```rust
.instrument(info_span!("outbound.prepare", raw_bytes, stage = uc_observability::stages::OUTBOUND_PREPARE));
```

Current (line 274):

```rust
.instrument(info_span!("outbound.send", peer_id = %first_peer.peer_id))
```

Target:

```rust
.instrument(info_span!("outbound.send", peer_id = %first_peer.peer_id, stage = uc_observability::stages::OUTBOUND_SEND))
```

**origin_flow_id threading:** The `execute()` method signature needs an optional `origin_flow_id: Option<String>` parameter (or the flow_id can be extracted from the current tracing span context). The simpler approach: add the parameter explicitly since `runtime.rs` already has the `flow_id` available.

Current ClipboardMessage construction (line 169-177):

```rust
let clipboard_header = ClipboardMessage {
    id: message_id,
    content_hash,
    encrypted_content: vec![],
    timestamp: Utc::now(),
    origin_device_id,
    origin_device_name,
    payload_version: ClipboardPayloadVersion::V3,
};
```

Target: Add `origin_flow_id` field.

### File: `src-tauri/crates/uc-core/src/network/protocol/clipboard.rs`

**Change:** Add `origin_flow_id: Option<String>` to `ClipboardMessage` struct with `#[serde(default)]` for backward compatibility.

```rust
pub struct ClipboardMessage {
    pub id: String,
    pub content_hash: String,
    #[serde_as(as = "Base64")]
    pub encrypted_content: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub origin_device_id: String,
    pub origin_device_name: String,
    pub payload_version: ClipboardPayloadVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_flow_id: Option<String>,  // NEW
}
```

The `#[serde(default)]` ensures old messages without the field deserialize as `None`. The `skip_serializing_if` keeps wire format compact when not set.

### File: `src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs`

**Change:** Add `stage` field to existing `inbound.decode` span. Create new `inbound.apply` span wrapping the representation selection + clipboard write logic.

Current decode span (line 286-289):

```rust
.instrument(info_span!(
    "inbound.decode",
    wire_bytes = message.encrypted_content.len(),
))
```

Target:

```rust
.instrument(info_span!(
    "inbound.decode",
    wire_bytes = message.encrypted_content.len(),
    stage = uc_observability::stages::INBOUND_DECODE,
))
```

For `inbound.apply`: wrap everything from `select_highest_priority_repr_index` through clipboard write / capture execute in a new span:

```rust
async {
    // representation selection + OS clipboard write or passive capture
}
.instrument(info_span!("inbound.apply", stage = uc_observability::stages::INBOUND_APPLY))
.await
```

**Dedup placement decision (Claude's discretion):** The dedup check (recent_ids) is a fast pre-filter before any expensive work. It should remain BEFORE the decode span (where it currently sits at line 219-236), not inside decode or apply. This keeps decode and apply focused on their named responsibilities.

### File: `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs`

**Change:** Generate `FlowId` at receive loop and add it to the `loop.clipboard.receive_message` span. Also add `origin_flow_id` as a span field when present.

Current (line 1570-1581):

```rust
while let Some((message, pre_decoded)) = clipboard_rx.recv().await {
    let message_id = message.id.clone();
    let origin_device_id = message.origin_device_id.clone();
    let span = info_span!(
        "loop.clipboard.receive_message",
        message_id = %message_id,
        origin_device_id = %origin_device_id
    );
    let result = async { usecase.execute_with_outcome(message, pre_decoded).await }
        .instrument(span)
        .await;
```

Target:

```rust
while let Some((message, pre_decoded)) = clipboard_rx.recv().await {
    let flow_id = uc_observability::FlowId::generate();
    let message_id = message.id.clone();
    let origin_device_id = message.origin_device_id.clone();
    let origin_flow_id = message.origin_flow_id.clone().unwrap_or_default();
    let span = info_span!(
        "loop.clipboard.receive_message",
        %flow_id,
        message_id = %message_id,
        origin_device_id = %origin_device_id,
        origin_flow_id = %origin_flow_id,
    );
    let result = async { usecase.execute_with_outcome(message, pre_decoded).await }
        .instrument(span)
        .await;
```

### File: `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs`

**Change:** Pass `origin_flow_id` to `SyncOutboundClipboardUseCase.execute()`. The `flow_id` is already available at line 1003.

Current (line 1074-1075):

```rust
outbound_sync_uc.execute(outbound_snapshot, origin)
```

Target: Thread the flow_id string into the execute call so it can populate `ClipboardMessage.origin_flow_id`.

### File: `src-tauri/crates/uc-tauri/Cargo.toml`

**Check:** `uc-observability` is already a dependency (added in Phase 20). No change needed.

### File: `src-tauri/crates/uc-app/Cargo.toml`

**Check:** `uc-observability` is already a dependency (added in Phase 20). No change needed.

## Don't Hand-Roll

| Problem                          | Don't Build                  | Use Instead                               | Why                                                       |
| -------------------------------- | ---------------------------- | ----------------------------------------- | --------------------------------------------------------- |
| flow_id propagation across spans | Manual parameter threading   | Tracing span context inheritance          | Parent span fields visible to all children via JsonFields |
| UUID generation                  | Custom ID scheme             | `FlowId::generate()` (UUID v7)            | Already established in Phase 20                           |
| Backward-compatible serde        | Custom deserialization logic | `#[serde(default, skip_serializing_if)]`  | Standard serde pattern for optional fields                |
| Stage naming                     | Inline string literals       | Constants from `uc_observability::stages` | Single source of truth, compile-time verified             |

## Common Pitfalls

### Pitfall 1: Outbound flow_id Already Propagates via Span Context

**What goes wrong:** Adding explicit `flow_id` fields to `outbound.prepare` and `outbound.send` spans creates duplicate fields in JSON output.
**Why it happens:** The `outbound_sync` parent span at runtime.rs:1090 already carries `flow_id_for_sync`. Child spans inherit this.
**How to avoid:** Only add `stage` fields to outbound child spans. Do NOT add `flow_id` -- it is already inherited.
**Warning signs:** `flow_id` appearing twice in JSON log output for outbound spans.

### Pitfall 2: SyncOutbound Uses `executor::block_on` (Not Async)

**What goes wrong:** Assuming `SyncOutboundClipboardUseCase.execute()` is async and can receive `flow_id` via span context from an async parent.
**Why it happens:** The `execute` method at line 59 uses `executor::block_on(self.execute_async(...))`. It IS called from within a `tokio::task::spawn_blocking` (runtime.rs:1074), which is already inside the `outbound_sync` span.
**How to avoid:** The span context from `outbound_sync` propagates into `spawn_blocking` correctly. The `execute_async` internal method and its child spans will see the parent's `flow_id`. For `origin_flow_id`, pass it as a parameter to `execute()` since it needs to be set on the `ClipboardMessage` struct field, not just on a span.

### Pitfall 3: ClipboardMessage Serde Backward Compatibility

**What goes wrong:** Adding `origin_flow_id` as a required field breaks deserialization of messages from older peers.
**Why it happens:** Messages from peers running older versions won't include `origin_flow_id` in their JSON.
**How to avoid:** Use `#[serde(default)]` on the field. This makes it deserialize as `None` when absent.
**Warning signs:** Deserialization errors on inbound messages after upgrade.

### Pitfall 4: Empty String vs None for origin_flow_id Span Field

**What goes wrong:** Using `unwrap_or_default()` on `origin_flow_id` produces an empty string `""` in span fields, which looks like a bug in log output.
**Why it happens:** Tracing always records the field value, even if empty.
**How to avoid:** Use `tracing::field::Empty` for the field when `origin_flow_id` is None, or format as a meaningful default like `"<none>"`. Alternatively, use conditional field recording. The simplest approach is `unwrap_or_default()` since empty string is clearly "not set" when filtering logs.

### Pitfall 5: Test Compilation for ClipboardMessage Changes

**What goes wrong:** Adding `origin_flow_id` to `ClipboardMessage` breaks all existing test code that constructs `ClipboardMessage` literals.
**Why it happens:** Rust struct literal syntax requires all fields.
**How to avoid:** Audit all test files that construct `ClipboardMessage`. Add `origin_flow_id: None` to each. Key locations: `sync_outbound.rs` tests (line 567-575 parse_framed), `sync_inbound.rs` tests, and any `uc-core` protocol tests.

## Code Examples

### Stage Constants Addition

```rust
// src-tauri/crates/uc-observability/src/stages.rs
// Source: Established pattern from Phase 20

pub const OUTBOUND_PREPARE: &str = "outbound_prepare";
pub const OUTBOUND_SEND: &str = "outbound_send";
pub const INBOUND_DECODE: &str = "inbound_decode";
pub const INBOUND_APPLY: &str = "inbound_apply";
```

### Inbound flow_id Generation at Receive Loop

```rust
// src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs
// Source: Phase 20 pattern from runtime.rs:1003

use uc_observability::FlowId;

while let Some((message, pre_decoded)) = clipboard_rx.recv().await {
    let flow_id = FlowId::generate();
    let message_id = message.id.clone();
    let origin_device_id = message.origin_device_id.clone();
    let origin_flow_id_display = message.origin_flow_id.as_deref().unwrap_or("");
    let span = info_span!(
        "loop.clipboard.receive_message",
        %flow_id,
        message_id = %message_id,
        origin_device_id = %origin_device_id,
        origin_flow_id = origin_flow_id_display,
    );
    // All child spans (usecase, decode, apply) inherit flow_id automatically
    let result = async { usecase.execute_with_outcome(message, pre_decoded).await }
        .instrument(span)
        .await;
```

### Backward-Compatible ClipboardMessage Field

```rust
// src-tauri/crates/uc-core/src/network/protocol/clipboard.rs
// Source: Standard serde pattern

#[serde(default, skip_serializing_if = "Option::is_none")]
pub origin_flow_id: Option<String>,
```

### Inbound Apply Span Wrapping

```rust
// src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs
// Source: Phase 20 pattern for stage spans

let outcome = async {
    let selected_idx = match select_highest_priority_repr_index(&v3_payload.representations) {
        Some(i) => i,
        None => { /* ... */ }
    };
    // ... representation selection, OS write or passive capture ...
}
.instrument(info_span!("inbound.apply", stage = uc_observability::stages::INBOUND_APPLY))
.await;
```

## Validation Architecture

### Test Framework

| Property           | Value                                            |
| ------------------ | ------------------------------------------------ |
| Framework          | Rust built-in `#[test]` + tokio::test            |
| Config file        | `src-tauri/Cargo.toml` (workspace test config)   |
| Quick run command  | `cd src-tauri && cargo test -p uc-observability` |
| Full suite command | `cd src-tauri && cargo test`                     |

### Phase Requirements -> Test Map

| Req ID  | Behavior                                   | Test Type | Automated Command                                          | File Exists?    |
| ------- | ------------------------------------------ | --------- | ---------------------------------------------------------- | --------------- |
| FLOW-05 | Stage constants are snake_case             | unit      | `cd src-tauri && cargo test -p uc-observability stages -x` | Extend existing |
| FLOW-05 | ClipboardMessage serde with origin_flow_id | unit      | `cd src-tauri && cargo test -p uc-core protocol -x`        | Extend existing |
| FLOW-05 | Outbound spans carry stage fields          | manual    | Verify via log inspection during `bun tauri dev`           | N/A             |
| FLOW-05 | Inbound spans carry flow_id + stage        | manual    | Verify via log inspection during `bun tauri dev`           | N/A             |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-observability && cargo test -p uc-core`
- **Per wave merge:** `cd src-tauri && cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

None -- existing test infrastructure covers all phase requirements. The stage constants test in `stages.rs` already tests the pattern; it just needs the new constants added. The `ClipboardMessage` serde tests in `uc-core` already exist; they need the new field covered.

## Open Questions

1. **origin_flow_id: ClipboardMessage vs ClipboardBinaryPayload level?**
   - What we know: CONTEXT.md says add to `ClipboardMessage`. `ClipboardMessage` is the JSON header, `ClipboardBinaryPayload` is the encrypted binary body.
   - What's unclear: Placing it in JSON header means it's visible without decryption (could be a privacy concern, but it's just a UUID). Placing in binary payload means it requires decryption to read.
   - Recommendation: **ClipboardMessage (JSON header)** -- it's metadata about the flow, not payload content. Being visible without decryption enables network-level correlation tooling in the future. Also simpler since ClipboardBinaryPayload has a custom binary format that would need versioning.

2. **How to thread flow_id string into SyncOutboundClipboardUseCase.execute()?**
   - What we know: The `flow_id` is available in `runtime.rs` at line 1071. The `execute()` method currently takes `(snapshot, origin)`.
   - What's unclear: Whether to add an `origin_flow_id: Option<String>` parameter or use a separate method.
   - Recommendation: Add `origin_flow_id: Option<String>` parameter to `execute()` and `execute_async()`. Callers that don't have a flow_id (e.g., `execute_current_snapshot`) can pass `None`. Update the one call site in `runtime.rs` to pass `Some(flow_id.to_string())`.

## Sources

### Primary (HIGH confidence)

- Direct code audit of `sync_outbound.rs`, `sync_inbound.rs`, `wiring.rs`, `runtime.rs`, `stages.rs`, `flow.rs`
- Phase 20 RESEARCH.md and CONTEXT.md for established patterns
- Phase 21 CONTEXT.md for locked decisions

### Secondary (MEDIUM confidence)

- serde documentation for `#[serde(default)]` and `#[serde(skip_serializing_if)]` behavior

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - no new dependencies, reuses existing uc-observability crate
- Architecture: HIGH - direct code audit of all integration points, patterns established in Phase 20
- Pitfalls: HIGH - identified from actual code structure (block_on, serde, test breakage)

**Research date:** 2026-03-11
**Valid until:** 2026-04-11 (stable codebase, no external dependencies involved)

# Phase 23: Distributed Tracing with Trace View Visualization - Research

**Researched:** 2026-03-11
**Domain:** Cross-device log correlation via Seq, CLEF field injection, Seq signal expressions
**Confidence:** HIGH

## Summary

Phase 23 extends the existing single-device flow observability (Phases 20-21) and Seq integration (Phase 22) to enable cross-device tracing. The core work is: (1) injecting `device_id` as a static field into every CLEF event sent to Seq, (2) ensuring `origin_flow_id` is properly populated on inbound messages so receiver flows link back to sender flows, (3) updating the docker-compose Seq setup for LAN-accessible shared instances, and (4) shipping Seq saved search/signal JSON configs for cross-device flow queries.

The codebase is well-prepared. `origin_flow_id` is already on the `ClipboardMessage` wire format (with `serde(default)` backward compat). The outbound sync use case already passes `Some(flow_id_str)` when sending. The inbound wiring loop already reads `origin_flow_id` from the message and attaches it to the span. The `SeqLayer` already formats CLEF JSON with flattened span fields. The main gaps are: (a) `device_id` is not yet injected into CLEF events, (b) `docker-compose.seq.yml` binds to localhost only, and (c) no Seq signal/search configs exist yet.

**Primary recommendation:** Add `device_id` as an `Option<String>` parameter to `build_seq_layer()`, inject it as a static field in `SeqLayer::on_event()`, and resolve device_id early in `init_tracing_subscriber()` by reading `device_id.txt` directly from the config directory (bypassing the `DeviceIdentityPort` which is not yet constructed at tracing init time).

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Linked traces: sender and receiver keep separate flow_ids, connected by origin_flow_id
- Inbound flow retains its own local flow_id as primary correlation key
- origin_flow_id from the sender is recorded as a queryable field on the receiver's flow
- In Seq, a clickable cross-reference (saved search URL template) lets developers jump from receiver flow to sender flow via origin_flow_id
- All devices send logs to a shared Seq instance (not per-device), enabling true cross-device queries
- device_id is attached to every Seq event (not just flow-correlated events)
- Injected at the Seq layer level as a static field during layer initialization -- read once from app config/state, added to every CLEF event by SeqLayer
- Uses the existing device_id from the app's device identity system (same as ClipboardMessage.origin_device_id)
- Saved searches + signal expressions in Seq (no custom dashboard)
- Two primary signals: flow timeline (by flow_id + device_id grouping) and cross-device flow (by origin_flow_id)
- Signal/search definitions shipped as JSON config files in docs/seq/signals/
- origin_flow_id stays on ClipboardMessage header only -- no duplication into V3 binary payload
- Multi-hop scenarios (A -> B -> C) are explicitly out of scope
- When older peer sends message without origin_flow_id: log a warning, graceful degradation
- Update docker-compose.seq.yml: bind to 0.0.0.0, set SEQ_FIRSTRUN_ADMINPASSWORD
- Documentation: extend docs/architecture/logging-architecture.md

### Claude's Discretion

- Exact Seq signal expression syntax and JSON export format
- SEQ_FIRSTRUN_ADMINPASSWORD default value choice
- device_id field name in CLEF output (e.g., `device_id` vs `DeviceId`)
- How to read device_id at Seq layer init (from AppRuntime state or config)
- Seq saved search URL template format for clickable cross-references
- Test strategy for cross-device correlation

### Deferred Ideas (OUT OF SCOPE)

- Multi-hop trace chain (A -> B -> C)
- Console clickable Seq URL
- Full OpenTelemetry distributed tracing with W3C trace context headers
- Seq dashboard with visual waterfall/timeline panels

</user_constraints>

## Standard Stack

### Core

| Library            | Version    | Purpose                       | Why Standard                                       |
| ------------------ | ---------- | ----------------------------- | -------------------------------------------------- |
| tracing            | (existing) | Structured logging framework  | Already in use, span fields become CLEF properties |
| tracing-subscriber | (existing) | Layer composition             | SeqLayer already implements `Layer<S>` trait       |
| serde_json         | (existing) | CLEF JSON serialization       | Used in SeqLayer::format_clef_event                |
| reqwest            | (existing) | HTTP client for Seq ingestion | Used by sender_loop in seq/sender.rs               |
| chrono             | (existing) | ISO 8601 timestamps           | CLEF @t field formatting                           |
| uuid               | (existing) | Device ID and Flow ID         | UUID v4 for device_id, v7 for flow_id              |

### Supporting

| Library      | Version | Purpose                | When to Use                                     |
| ------------ | ------- | ---------------------- | ----------------------------------------------- |
| datalust/seq | 2025.2  | Log aggregation server | Docker image, already in docker-compose.seq.yml |

No new dependencies are required for this phase.

## Architecture Patterns

### Recommended Change Structure

```
src-tauri/crates/
├── uc-observability/src/
│   ├── seq/
│   │   ├── layer.rs          # ADD: device_id as static field in on_event()
│   │   └── mod.rs            # MODIFY: build_seq_layer() accepts device_id param
│   └── lib.rs                # UPDATE: re-export signature change
├── uc-tauri/src/bootstrap/
│   └── tracing.rs            # MODIFY: resolve device_id early, pass to build_seq_layer()
docker-compose.seq.yml         # MODIFY: bind 0.0.0.0, add password
docs/
├── architecture/
│   └── logging-architecture.md  # EXTEND: cross-device tracing section
└── seq/signals/               # NEW: Seq signal/search JSON configs
    ├── flow-timeline.json
    └── cross-device-flow.json
```

### Pattern 1: Static Field Injection in SeqLayer

**What:** Add device_id as a constructor parameter to `SeqLayer`, inject it into every CLEF JSON event as a top-level field before span/event fields.

**When to use:** When a field must appear on every event regardless of span context.

**Example:**

```rust
// In seq/layer.rs
pub(crate) struct SeqLayer {
    tx: mpsc::Sender<String>,
    device_id: Option<String>,  // NEW: static field
}

impl SeqLayer {
    pub(crate) fn new(tx: mpsc::Sender<String>, device_id: Option<String>) -> Self {
        Self { tx, device_id }
    }
}

// In format_clef_event (or on_event), after @m and before span fields:
if let Some(ref device_id) = self.device_id {
    map.serialize_entry("device_id", device_id).ok()?;
}
```

**Why Option<String>:** Graceful degradation if device_id.txt doesn't exist yet (first launch). The field simply won't appear rather than crashing.

### Pattern 2: Early Device ID Resolution

**What:** Read device_id directly from `{config_dir}/device_id.txt` in `init_tracing_subscriber()`, before the full `LocalDeviceIdentity` is constructed in `create_platform_layer()`.

**Why this pattern exists:** Tracing subscriber is initialized at line 435 of `main.rs`, long before Tauri Builder `.setup()` runs and `create_platform_layer()` constructs `LocalDeviceIdentity`. We cannot use `DeviceIdentityPort` because it doesn't exist yet.

**Example:**

```rust
// In tracing.rs init_tracing_subscriber()
fn resolve_device_id_for_seq(config_dir: &Path) -> Option<String> {
    let path = config_dir.join("device_id.txt");
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// Then pass to build_seq_layer:
let device_id = resolve_device_id_for_seq(&paths.config_dir);
let layer_result = rt.block_on(async {
    uc_observability::build_seq_layer(&profile, device_id.as_deref())
});
```

**Key insight:** This reads the same `device_id.txt` file that `LocalDeviceIdentity::load_or_create` uses. On first launch, the file may not exist yet, so device_id will be `None` -- the first run's logs won't have device_id in Seq, which is acceptable. From the second launch onward, the file exists and device_id is always present.

### Pattern 3: Graceful Degradation for Missing origin_flow_id

**What:** On inbound message processing, log a warning when `origin_flow_id` is None (older peer) but continue processing normally.

**Where:** Already partially implemented in `wiring.rs:1574-1580` where `origin_flow_id_display` defaults to `""`. The warning log should be added.

**Example:**

```rust
// In the inbound clipboard receive loop (wiring.rs)
if message.origin_flow_id.is_none() {
    warn!(
        message_id = %message.id,
        origin_device_id = %message.origin_device_id,
        "Inbound message has no origin_flow_id (sender may be an older version)"
    );
}
```

### Anti-Patterns to Avoid

- **Don't inject device_id via a root span:** A root span with device_id would require careful lifetime management and wouldn't work with the current dual-layer architecture where SeqLayer formats independently. Static field injection at the layer level is simpler and more reliable.
- **Don't use UC_DEVICE_ID env var for device_id:** The device_id comes from `device_id.txt` -- adding a second source of truth creates confusion. Reading the file directly is the correct approach.
- **Don't make device_id a span field on each flow:** This would require modifying every flow entry point. Layer-level injection ensures 100% coverage with zero changes to business code.

## Don't Hand-Roll

| Problem                     | Don't Build                       | Use Instead                                               | Why                                                                 |
| --------------------------- | --------------------------------- | --------------------------------------------------------- | ------------------------------------------------------------------- |
| Cross-device trace linking  | Custom trace propagation protocol | origin_flow_id field on ClipboardMessage (already exists) | Wire format already supports it, just needs activation              |
| Seq signal expressions      | Manual query construction         | Seq's built-in signal expression syntax                   | Seq signals support grouping, filtering, and URL templates natively |
| Device-scoped log filtering | Custom log routing per device     | Single shared Seq instance + device_id field filter       | Seq handles multi-tenant filtering efficiently                      |
| CLEF format                 | Custom JSON serialization         | Existing SeqLayer::format_clef_event                      | Adding one field to existing serialization logic                    |

## Common Pitfalls

### Pitfall 1: Tracing Init vs Device Identity Lifecycle

**What goes wrong:** Attempting to use `DeviceIdentityPort` or `AppRuntime` to get device_id during tracing initialization will fail because these are constructed later in `create_platform_layer()`.

**Why it happens:** `init_tracing_subscriber()` is called at main.rs:435, before Tauri Builder setup.

**How to avoid:** Read `device_id.txt` directly from the filesystem in `init_tracing_subscriber()`. This file is the same backing store used by `LocalDeviceIdentity`.

**Warning signs:** Compilation error about borrowing AppRuntime before it exists, or runtime panic about uninitialized state.

### Pitfall 2: format_clef_event Is a Free Function

**What goes wrong:** Attempting to access `self.device_id` inside `format_clef_event()` won't work because it's a free function, not a method on `SeqLayer`.

**Why it happens:** The current `on_event()` delegates to `format_clef_event(event, &ctx)` which doesn't have access to SeqLayer fields.

**How to avoid:** Either (a) pass `device_id` as an additional parameter to `format_clef_event()`, or (b) move the formatting logic into `on_event()` directly, or (c) make `format_clef_event` a method on SeqLayer. Option (a) is simplest and least invasive.

### Pitfall 3: Seq Signal Export Format Is Not Standardized

**What goes wrong:** Seq signals can be exported/imported via the Seq API, but the exact JSON format varies by Seq version and may not be well-documented.

**Why it happens:** Seq's signal feature is relatively new and the export format is an internal API detail.

**How to avoid:** Document the signal expressions as human-readable text alongside JSON exports. Provide step-by-step instructions for manual creation in Seq UI as a fallback. Test exports against the specific Seq 2025.2 image used in docker-compose.

### Pitfall 4: First-Launch device_id Absence

**What goes wrong:** On first app launch, `device_id.txt` doesn't exist yet (it's created later by `LocalDeviceIdentity::load_or_create`). Seq events from the first session won't have device_id.

**Why it happens:** Tracing init happens before device identity creation.

**How to avoid:** Accept this as a known limitation. Use `Option<String>` for device_id in SeqLayer. Document that first-launch logs may lack device_id. This is acceptable because: (a) first launch is typically a setup/config session, (b) subsequent launches will always have device_id.

### Pitfall 5: Docker Compose Bind Address Security

**What goes wrong:** Binding Seq to 0.0.0.0 exposes it to the entire network, not just LAN.

**Why it happens:** Required for cross-device access in development.

**How to avoid:** Set `SEQ_FIRSTRUN_ADMINPASSWORD` so Seq is not open. Document that this is a development-only configuration. Consider adding a comment in docker-compose.seq.yml warning about production use.

## Code Examples

### Existing: origin_flow_id Already on the Wire

```rust
// Source: uc-core/src/network/protocol/clipboard.rs:54
pub struct ClipboardMessage {
    pub id: String,
    pub content_hash: String,
    pub encrypted_content: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub origin_device_id: String,
    pub origin_device_name: String,
    pub payload_version: ClipboardPayloadVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_flow_id: Option<String>,  // Already exists with backward compat
}
```

### Existing: Outbound Sync Passes origin_flow_id

```rust
// Source: uc-tauri/src/bootstrap/runtime.rs:1070-1076
let outbound_sync_uc = self.usecases().sync_outbound_clipboard();
let flow_id_for_sync = flow_id.clone();
let flow_id_str = flow_id_for_sync.to_string();
// ...
outbound_sync_uc.execute(outbound_snapshot, origin, Some(flow_id_str))
```

### Existing: Inbound Receive Loop Reads origin_flow_id

```rust
// Source: uc-tauri/src/bootstrap/wiring.rs:1570-1584
while let Some((message, pre_decoded)) = clipboard_rx.recv().await {
    let flow_id = uc_observability::FlowId::generate();
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
}
```

### New: SeqLayer with Static device_id Field

```rust
// Target: uc-observability/src/seq/layer.rs
pub(crate) struct SeqLayer {
    tx: mpsc::Sender<String>,
    device_id: Option<String>,
}

impl SeqLayer {
    pub(crate) fn new(tx: mpsc::Sender<String>, device_id: Option<String>) -> Self {
        Self { tx, device_id }
    }
}

// In format_clef_event (modified to accept device_id):
fn format_clef_event<S>(
    event: &tracing::Event<'_>,
    ctx: &tracing_subscriber::layer::Context<'_, S>,
    device_id: Option<&str>,
) -> Option<String>
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    // ... existing @t, @l, @m, target serialization ...

    // Inject device_id as static field (before span fields)
    if let Some(did) = device_id {
        map.serialize_entry("device_id", did).ok()?;
    }

    // ... rest of span/event field serialization ...
}
```

### New: build_seq_layer with device_id Parameter

```rust
// Target: uc-observability/src/seq/mod.rs
pub fn build_seq_layer<S>(
    profile: &LogProfile,
    device_id: Option<&str>,
) -> Option<(impl Layer<S> + Send + Sync, SeqGuard)>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    // ... existing URL/API key resolution ...
    let seq_layer = layer::SeqLayer::new(tx, device_id.map(String::from));
    // ... rest unchanged ...
}
```

### New: Seq Signal Expression for Flow Timeline

```
-- Seq signal expression for flow timeline
-- Shows all stages for a given flow_id ordered by timestamp, grouped by device_id
select flow_id, stage, device_id, @Timestamp
from stream
where flow_id = '{flow_id}'
order by @Timestamp
```

### New: Seq Signal Expression for Cross-Device Flow

```
-- Find both sender and receiver flows linked by origin_flow_id
select flow_id, origin_flow_id, device_id, stage, @Timestamp, @Message
from stream
where flow_id = '{value}' or origin_flow_id = '{value}'
order by @Timestamp
```

### New: Seq Clickable Cross-Reference URL Template

```
-- URL format for jumping from receiver flow to sender's flow in Seq
-- When viewing a receiver flow, the origin_flow_id field links to:
http://{seq_host}:5341/#/events?filter=flow_id%3D'{origin_flow_id}'
```

## State of the Art

| Old Approach                | Current Approach                   | When Changed | Impact                                       |
| --------------------------- | ---------------------------------- | ------------ | -------------------------------------------- |
| No cross-device correlation | origin_flow_id on ClipboardMessage | Phase 21     | Wire format ready, not yet visualized        |
| No Seq integration          | SeqLayer with CLEF + batched HTTP  | Phase 22     | Infrastructure ready for device_id extension |
| Seq bound to localhost      | Shared Seq on LAN                  | This phase   | Enables multi-device log aggregation         |

## Open Questions

1. **AppPaths.config_dir availability in tracing.rs**
   - What we know: `init_tracing_subscriber()` currently resolves `AppPaths` from `DirsAppDirsAdapter` for the logs directory. The config_dir should be available from the same `AppPaths`.
   - What's unclear: Whether `AppPaths` exposes a `config_dir` field or only `logs_dir`. Need to verify the `AppPaths::from_app_dirs()` implementation.
   - Recommendation: Check `AppPaths` struct. If config_dir isn't exposed, add it or resolve it from the same `DirsAppDirsAdapter` that already works.

2. **Seq Signal Export/Import API**
   - What we know: Seq has a REST API for managing signals and saved searches. The 2025.2 version supports signals.
   - What's unclear: The exact JSON schema for signal export/import. Whether signals can be imported via API or only through UI.
   - Recommendation: Provide signal expressions as documented text + step-by-step Seq UI instructions. If the JSON export format is discoverable during implementation, ship JSON files as a bonus.

3. **Verify origin_flow_id End-to-End Population**
   - What we know: Outbound sync passes `Some(flow_id_str)` at runtime.rs:1076. Inbound receive reads it at wiring.rs:1574. The field is on ClipboardMessage with serde(default).
   - What's unclear: Whether the origin_flow_id survives the full network transport path (libp2p serialization/deserialization). The CONTEXT.md says to "verify that Phase 21's origin_flow_id population on outbound sync is actually implemented and working."
   - Recommendation: Add an integration verification step. This is a good candidate for manual testing with two devices + Seq, or a unit test that round-trips a ClipboardMessage through serialization.

## Validation Architecture

> nyquist_validation not explicitly set to false in config.json, including this section.

### Test Framework

| Property           | Value                                                  |
| ------------------ | ------------------------------------------------------ |
| Framework          | Rust built-in test + cargo test                        |
| Config file        | src-tauri/Cargo.toml                                   |
| Quick run command  | `cd src-tauri && cargo test -p uc-observability --lib` |
| Full suite command | `cd src-tauri && cargo test`                           |

### Phase Requirements -> Test Map

Phase 23 has no formal requirement IDs (TBD in REQUIREMENTS.md). Tests map to implementation decisions:

| Decision                    | Behavior                               | Test Type | Automated Command                                                     | File Exists?                          |
| --------------------------- | -------------------------------------- | --------- | --------------------------------------------------------------------- | ------------------------------------- |
| device_id injection         | CLEF events include device_id field    | unit      | `cd src-tauri && cargo test -p uc-observability seq::layer::tests -x` | Partially (layer.rs has no tests yet) |
| device_id absent gracefully | CLEF events valid without device_id    | unit      | same as above                                                         | Wave 0                                |
| build_seq_layer signature   | Accepts device_id parameter            | unit      | `cd src-tauri && cargo test -p uc-observability seq::tests -x`        | Existing tests need update            |
| origin_flow_id warning      | Warning logged for None origin_flow_id | unit      | `cd src-tauri && cargo test -p uc-tauri -x`                           | Wave 0                                |
| CLEF device_id field name   | Field name is "device_id"              | unit      | `cd src-tauri && cargo test -p uc-observability clef -x`              | Wave 0                                |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-observability --lib`
- **Per wave merge:** `cd src-tauri && cargo test`
- **Phase gate:** Full suite green before /gsd:verify-work

### Wave 0 Gaps

- [ ] `uc-observability/src/seq/layer.rs` tests -- SeqLayer has no unit tests; need tests for device_id injection
- [ ] Update existing `seq/mod.rs` tests for new `build_seq_layer` signature (device_id parameter)
- [ ] Manual verification: docker-compose Seq instance accessible from LAN, signal expressions work

## Sources

### Primary (HIGH confidence)

- Direct codebase analysis of `uc-observability/src/seq/layer.rs`, `seq/mod.rs`, `clef_format.rs`
- Direct codebase analysis of `uc-tauri/src/bootstrap/tracing.rs` and `wiring.rs`
- Direct codebase analysis of `uc-core/src/network/protocol/clipboard.rs` (ClipboardMessage with origin_flow_id)
- Direct codebase analysis of `uc-infra/src/device/mod.rs` and `storage.rs` (device_id.txt persistence)
- Phase 22 CONTEXT.md and STATE.md for architectural decisions

### Secondary (MEDIUM confidence)

- Seq CLEF format specification (standard, well-documented by Datalust)
- Seq signal expressions syntax (based on Seq documentation)

### Tertiary (LOW confidence)

- Seq signal JSON export/import format -- needs verification against Seq 2025.2 API

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - no new dependencies, all existing libraries
- Architecture: HIGH - direct codebase analysis, clear integration points
- Pitfalls: HIGH - identified from actual code structure (tracing init lifecycle, free function pattern)
- Seq signals: MEDIUM - syntax is documented but JSON export format needs verification

**Research date:** 2026-03-11
**Valid until:** 2026-04-11 (stable domain, no external dependencies changing)

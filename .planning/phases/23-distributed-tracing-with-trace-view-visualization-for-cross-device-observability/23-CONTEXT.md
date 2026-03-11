# Phase 23: Distributed Tracing with Trace View Visualization - Context

**Gathered:** 2026-03-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Enable cross-device flow observability so developers can follow a clipboard copy-paste flow from the originating device through sync to the receiving device, visualized as connected traces in a shared Seq instance. Builds on Phase 20-22's single-device flow correlation and Seq integration. This phase does NOT include OTel integration, multi-hop relay scenarios, or runtime profile switching.

</domain>

<decisions>
## Implementation Decisions

### Cross-Device Correlation Model

- Linked traces: sender and receiver keep separate flow_ids, connected by origin_flow_id
- Inbound flow retains its own local flow_id as primary correlation key
- origin_flow_id from the sender is recorded as a queryable field on the receiver's flow
- In Seq, a clickable cross-reference (saved search URL template) lets developers jump from receiver flow to sender flow via origin_flow_id
- All devices send logs to a shared Seq instance (not per-device), enabling true cross-device queries

### Device Identity in Seq

- device_id is attached to every Seq event (not just flow-correlated events)
- Injected at the Seq layer level as a static field during layer initialization — read once from app config/state, added to every CLEF event by SeqLayer
- Uses the existing device_id from the app's device identity system (same as ClipboardMessage.origin_device_id)
- Enables device-based filtering and grouping in Seq queries

### Trace View Visualization

- Saved searches + signal expressions in Seq (no custom dashboard)
- Two primary signals:
  1. **Flow timeline** — shows all stages for a given flow_id ordered by timestamp, with device_id as grouping field
  2. **Cross-device flow** — finds both sender and receiver flows linked by origin_flow_id
- Signal/search definitions shipped as JSON config files in the repo (e.g., `docs/seq/signals/`)
- Developer imports them into their Seq instance — version-controlled and reproducible

### Propagation Completeness

- origin_flow_id stays on ClipboardMessage header only — no duplication into V3 binary payload
- Multi-hop scenarios (A → B → C) are explicitly out of scope — no trace_chain field
- Verify that Phase 21's origin_flow_id population on outbound sync is actually implemented and working; complete it if not
- When an older peer sends a message without origin_flow_id: log a warning so developers know the sender is an older version; inbound flow still works normally with graceful degradation

### Developer Setup & Workflow

- Update existing docker-compose.seq.yml: bind to 0.0.0.0 (LAN-accessible) and set SEQ_FIRSTRUN_ADMINPASSWORD with a default dev password
- Developer points UC_SEQ_URL on each device to the shared Seq instance
- Entry point: copy on Device A, paste on Device B, then open Seq and search by flow_id from console output
- Documentation: extend existing docs/architecture/logging-architecture.md with cross-device tracing section

### Claude's Discretion

- Exact Seq signal expression syntax and JSON export format
- SEQ_FIRSTRUN_ADMINPASSWORD default value choice
- device_id field name in CLEF output (e.g., `device_id` vs `DeviceId`)
- How to read device_id at Seq layer init (from AppRuntime state or config)
- Seq saved search URL template format for clickable cross-references
- Test strategy for cross-device correlation

</decisions>

<specifics>
## Specific Ideas

- Phase 21 pre-reserved origin_flow_id on ClipboardMessage with serde(default) backward compat — this phase activates its full potential
- Phase 22's SeqLayer already formats CLEF events — device_id injection is a natural extension of the existing CLEF field set
- Seq's signal expressions support `@Properties.flow_id` style queries — the flow timeline signal can use this for grouping
- The clickable cross-reference can use Seq's URL format: `/#/events?filter=origin_flow_id%3D'{value}'`

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `SeqLayer` + `CLEFFormat` (uc-observability/src/seq/, clef_format.rs): Existing Seq integration — extend with device_id field injection
- `FlowId` newtype (uc-observability/src/flow.rs): UUID v7 generation with Display impl
- Stage constants (uc-observability/src/stages.rs): All 11 stages defined (detect through inbound_apply)
- `origin_flow_id: Option<String>` on ClipboardMessage (uc-core/src/network/protocol/clipboard.rs:54): Already on the wire with backward compat
- `build_seq_layer()` (uc-observability/src/seq/): Returns Option<(Layer, SeqGuard)> — device_id can be passed as a new parameter
- `docker-compose.seq.yml`: Existing Seq Docker setup to update

### Established Patterns

- CLEF field flattening: span fields become top-level JSON properties in Seq events
- `UC_SEQ_URL` env var controls Seq activation (Phase 22 pattern)
- `info_span!("name", flow_id = %flow_id, stage = STAGE_CONST)` for flow-correlated spans
- Repo-shipped config: docker-compose.seq.yml precedent for developer tooling config

### Integration Points

- `uc-observability/src/clef_format.rs`: Add device_id as a static CLEF field on every event
- `uc-observability/src/seq/`: Extend build_seq_layer to accept device_id parameter
- `uc-tauri/src/bootstrap/tracing.rs`: Pass device_id when composing Seq layer
- `docker-compose.seq.yml`: Update bind address and add SEQ_FIRSTRUN_ADMINPASSWORD
- `docs/architecture/logging-architecture.md`: Add cross-device tracing section
- `docs/seq/signals/`: New directory for Seq signal/search JSON exports

</code_context>

<deferred>
## Deferred Ideas

- Multi-hop trace chain (A → B → C) — would need trace_chain field on protocol, separate phase
- Console clickable Seq URL (log a direct link to Seq trace on flow completion) — nice-to-have, future enhancement
- Full OpenTelemetry distributed tracing with W3C trace context headers — future milestone
- Seq dashboard with visual waterfall/timeline panels — future enhancement if saved searches prove insufficient

</deferred>

---

_Phase: 23-distributed-tracing-with-trace-view-visualization-for-cross-device-observability_
_Context gathered: 2026-03-11_

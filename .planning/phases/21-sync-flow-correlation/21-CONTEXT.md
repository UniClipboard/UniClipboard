# Phase 21: Sync Flow Correlation - Context

**Gathered:** 2026-03-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Extend the flow_id + stage observability model (established in Phase 20 for local capture) to outbound and inbound sync operations. Developers can filter logs by a single flow_id and follow an outbound sync from prepare through send, or an inbound sync from decode through apply. This phase covers requirement FLOW-05. Seq integration (SEQ-01 through SEQ-06) belongs to Phase 22.

</domain>

<decisions>
## Implementation Decisions

### Outbound Sync Stages

- Reuse existing span structure: add `stage` field to current `outbound.prepare` and `outbound.send` spans
- Stage constants: `outbound_prepare` (covers encryption + framing) and `outbound_send` (covers per-peer transmission)
- flow_id: reuse the capture flow_id — outbound sync is a continuation of the capture flow, not a separate flow
- Multi-peer sends: all peers share the same `stage = "outbound_send"` value; `peer_id` is a regular span field for filtering
- No structural changes to existing spans — only add `flow_id` and `stage` fields

### Inbound Sync Stages

- Two stages: `inbound_decode` (covers decryption + payload decode) and `inbound_apply` (covers dedup check + representation selection + clipboard write)
- Stage constants follow same pattern as outbound: `STAGE_INBOUND_DECODE`, `STAGE_INBOUND_APPLY` in uc-observability
- Aligns with existing `inbound.decode` span — add stage field to it, create new span for apply

### Inbound flow_id Strategy

- Generate a new flow_id per inbound message at the receive loop layer (wiring.rs `run_clipboard_receive_loop()`)
- Inbound is a new flow on the receiving device — independent from the sender's flow
- In Passive mode, inbound triggers `capture_clipboard` which creates its own capture flow_id — the two flows are independent, correlated only by time proximity
- flow_id injected as root span field, inherited by UseCase spans via tracing context

### Cross-Device Correlation (Pre-reserved)

- Add `origin_flow_id: Option<String>` field to `ClipboardMessage`
- Outbound sync fills this with the capture flow_id before sending
- Inbound sync records `origin_flow_id` as a span field when present (for log querying), but uses its own local flow_id as the primary correlation key
- Protocol compatibility: Optional field — old messages without it deserialize as None, inbound handling degrades gracefully
- This enables future cross-device flow tracing without requiring protocol changes later

### Claude's Discretion

- Exact integration with V3 binary payload serialization for origin_flow_id
- Whether to add origin_flow_id at ClipboardMessage level or ClipboardBinaryPayload level
- Test strategy for verifying flow_id propagation across sync spans
- Whether inbound dedup check belongs in decode or apply stage
- Span field set for inbound root span (which message fields to include beyond flow_id)

</decisions>

<specifics>
## Specific Ideas

- Phase 20 already has outbound_sync span carrying flow_id but no stage field — Phase 21 adds the stage fields to complete the picture
- Stage constant naming follows `outbound_*` / `inbound_*` prefix pattern for clear directionality, matching existing span names (outbound.prepare, outbound.send, inbound.decode)
- The receive loop in wiring.rs already has a `loop.clipboard.receive_message` span — this is the natural injection point for inbound flow_id

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `SyncOutboundClipboardUseCase` (sync_outbound.rs): Has `outbound.prepare` and `outbound.send` spans — add stage field
- `SyncInboundClipboardUseCase` (sync_inbound.rs): Has `inbound.decode` span — add stage field, create `inbound_apply` span
- `run_clipboard_receive_loop()` (wiring.rs:1565): Message receive loop — inject flow_id generation here
- `FlowId` newtype (uc-observability/src/flow.rs): UUID v7 generation, Display impl — reuse for sync flows
- Stage constants (uc-observability/src/stages.rs): Existing capture stages — extend with sync stages

### Established Patterns

- `info_span!("name", stage = STAGE_CONST, flow_id = %flow_id)` for stage spans (Phase 20 pattern)
- `.instrument(span).await` for async span propagation
- Explicit flow_id passing into `tokio::spawn` closures (runtime.rs pattern from Phase 20)
- `FlowId::generate()` factory for UUID v7 creation

### Integration Points

- `uc-observability/src/stages.rs`: Add STAGE_OUTBOUND_PREPARE, STAGE_OUTBOUND_SEND, STAGE_INBOUND_DECODE, STAGE_INBOUND_APPLY constants
- `sync_outbound.rs`: Add flow_id + stage fields to existing outbound.prepare and outbound.send spans
- `sync_inbound.rs`: Add stage field to inbound.decode span, create inbound_apply span
- `wiring.rs`: Generate flow_id in receive loop, pass to UseCase
- `ClipboardMessage` struct: Add origin_flow_id: Option<String> field

</code_context>

<deferred>
## Deferred Ideas

- Full cross-device flow correlation (querying sender + receiver logs together) — future milestone
- FlowContext struct wrapping flow_id + metadata — reconsidered from Phase 20, still not needed for current scope
- Representation-level sub-spans with representation_id, mime_type, size_bytes (OBS-02) — future milestone

</deferred>

---

_Phase: 21-sync-flow-correlation_
_Context gathered: 2026-03-11_

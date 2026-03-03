# Phase 3: True Inbound Streaming - Context

**Gathered:** 2026-03-03
**Status:** Ready for planning

<domain>
## Phase Boundary

Eliminate the `read_to_end` bottleneck in `libp2p_network.rs` — separate the outer `ProtocolMessage` JSON envelope from the V2 binary payload so `ChunkedDecoder::decode_from` can operate at the stream level. Peak memory drops from ~2× payload size to ~1× chunk size.

No new user-visible behavior. No UI changes. No new UTL REQ-IDs. This is a tech debt resolution.

</domain>

<decisions>
## Implementation Decisions

### Streaming requirement

- Inbound V2 clipboard messages MUST NOT buffer the full payload before starting decoding
- `ChunkedDecoder::decode_from` must be called while the stream is still open
- Peak memory target: ~1× chunk size (not ~2× payload size as currently)

### Error handling (carried from Phase 2)

- All failures are silent — warn-level log, stream discarded, no user notification
- Same behavior as Phase 2: discard on any decode/validation error

### V1 backward compatibility stance

- Phase 2 established: V1 devices silently ignore V2 messages
- Phase 3 changes the sender wire format for V2 — V1 receivers may see garbled data for V2 clipboard messages; this is acceptable since V1 receivers cannot process V2 payloads anyway
- V1 messages (non-clipboard or V1 clipboard) must continue to work unaffected

### Claude's Discretion

- Wire framing design: how to byte-delimit the JSON envelope from the raw binary payload (e.g., length-prefix before JSON, new wrapper, version byte, etc.)
- Whether `ClipboardMessage.encrypted_content` stays in the struct (set to empty for V2) or a new V2 envelope type is introduced
- Whether `payload_size_bytes` or another field is added to the JSON envelope for V2
- Scope of interface changes: transport-only vs. `ClipboardMessage` struct changes
- Whether outbound also changes in this phase (sender must produce the new framing for receivers to benefit)
- Exact async streaming adapter to bridge `AsyncRead` stream to `ChunkedDecoder`'s sync `Read` trait (or whether ChunkedDecoder gets an async variant)

</decisions>

<specifics>
## Specific Ideas

- Phase 2 explicitly deferred "Option A (transport streaming)" to avoid `ClipboardTransportPort` interface changes — Phase 3 is the phase to revisit this
- `ChunkedDecoder::decode_from<R: Read>` is already designed for streaming (`read_exact` based, no `read_to_end`) — the integration point is ready
- The V2 base64 bloat in JSON is a known issue: base64 adds ~33% overhead on top of the binary, plus JSON deserialization creates a second copy → ~2-3× actual payload size in memory

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `ChunkedDecoder::decode_from<R: Read>`: Already streaming, just needs to receive the raw stream bytes instead of a `Cursor<Vec<u8>>`
- `BUSINESS_PAYLOAD_MAX_BYTES` (300MB): Still relevant as a size guard — but can be enforced at stream level rather than after full read
- `ProtocolMessage::from_bytes(bytes: &[u8])`: Current JSON deserializer — will need a streaming-compatible variant or a two-step parse for V2

### Established Patterns

- `libp2p_network.rs` lines 889–924: Current receive loop — `stream.take(...).read_to_end(...)` then `handle_business_payload(...)`. Phase 3 modifies this path for V2 messages.
- `ClipboardMessage` struct (`uc-core/src/network/protocol/clipboard.rs`): Contains `encrypted_content: Vec<u8>` (base64 in JSON) and `payload_version: ClipboardPayloadVersion`
- `handle_business_payload`: Dispatches on `ProtocolMessage` variant — V2 clipboard path must change to use streaming

### Integration Points

- `libp2p_network.rs` inbound stream handler: Primary change site
- `ProtocolMessage`/`ClipboardMessage` serialization: May need a V2-specific envelope struct or field modification
- `sync_inbound` use case: Receives a `ClipboardMessage` — downstream interface should remain stable if streaming is contained in the transport layer

</code_context>

<deferred>
## Deferred Ideas

- Outbound streaming (sender-side): Phase 2 deferred "Option A" (transport streaming for sender). Phase 3 focuses on inbound; if sender changes are needed for wire format compatibility, they are minimal and scoped to producing the new framing
- Parallel chunk download: Architecture supports it; not in scope
- Resumable transfers: Architecture supports it; not in scope

</deferred>

---

_Phase: 03-true-inbound-streaming_
_Context gathered: 2026-03-03_

# Phase 2: Unified Transfer Layer - Context

**Gathered:** 2026-03-03
**Status:** Ready for planning

<domain>
## Phase Boundary

Build a unified chunked data transfer layer that is content-type agnostic (text/image/file). The layer automatically chunks large payloads, encrypts each chunk independently (chunk-level AEAD), transfers over the existing libp2p transport, and the receiver validates, reassembles, and writes to the system clipboard. This is infrastructure — no new UI except the content appearing in clipboard history when transfer completes.

</domain>

<decisions>
## Implementation Decisions

### Transfer UI behavior

- UI is completely silent during transfer — no placeholder cards, no loading states
- Content appears in clipboard history list only after full assembly and validation
- Transfer failures are silent — no error cards, no notifications, only error-level logs
- Checksum/tag validation failures are also silently discarded with error logging

### Transfer failure handling

- Sender: silent abandon on disconnect, no retry, no notification to user
- Receiver: immediately marks transfer as failed on connection drop (no timeout grace period)
- Partial transfers are discarded, no partial data retained
- All failures logged at error level for debugging

### Multi-representation strategy

- Transmit ALL clipboard representations in a single transfer (e.g., PNG + HTML + text packed together)
- All representations bundled into one payload, chunked and sent as an atomic unit
- Receiver writes only the highest-priority representation to clipboard:
  - Priority order: image > rich text (HTML) > plain text
- Single-representation write is a temporary limitation — future phases may support multi-representation clipboard write

### Protocol compatibility

- Old devices receiving new format messages silently ignore them (unknown payload version → discard)
- Reuse existing `ProtocolMessage::Clipboard(ClipboardMessage)` variant — add payload version field inside to distinguish V1 (text-only) from V2 (unified chunked)
- libp2p protocol ID stays at `/uc-business/1.0.0` — no protocol ID bump
- Version differentiation happens entirely at payload layer

### Chunk-level AEAD encryption (user-specified design)

- Streaming chunking — NOT whole-payload-in-memory-first. Read source in chunks, encrypt per chunk, send per chunk
- Memory usage ≈ chunk_size × 2 (read buffer + encrypt buffer), regardless of total payload size
- Each chunk independently encrypted with XChaCha20-Poly1305:
  - `nonce = H(file_id + chunk_index)` — deterministic per chunk
  - `AAD = file_id + chunk_index` — prevents replay and reordering
  - Each chunk has its own authentication tag
- Chunks are independently verifiable, supporting parallel processing and future resume capability

### Blob storage format (user-specified design)

- Logical chunking, physically contiguous — single file on disk
- File layout: `[chunk_0][chunk_1][chunk_2]...[chunk_n]`
- Each chunk struct: `{ u32 chunk_index, u32 length, [ciphertext], [tag] }`
- Index table (header or tail) for O(1) random access:

  ```
  Header:
    chunk_size
    total_chunks
    index_table_offset
    index_table_len
    header_tag (AEAD)

  IndexTable:
    entry[i] = { offset, len }  // i = chunk_index
  ```

- Read flow: read header → locate index entry → seek to offset → read chunk → decrypt with nonce(file_id, i)

### Claude's Discretion

- Chunk size selection (e.g., 64KB, 256KB, 1MB) — optimize based on LAN transfer characteristics
- Hash function choice for nonce derivation H()
- Exact header/index table binary format and endianness
- Internal buffer management and async pipeline design
- Timeout values for stream operations
- Compression strategy (if any)

</decisions>

<specifics>
## Specific Ideas

- User explicitly designed the chunk-level AEAD scheme with deterministic nonces and per-chunk AAD — this is a locked architectural decision, not a suggestion
- User emphasized: must NOT load entire payload into memory before chunking — streaming approach is mandatory
- Index table with O(1) seek is required for the blob format
- The design intentionally supports future capabilities: parallel download, resumable transfer, anti-replay — even though Phase 2 only implements sequential streaming

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `ProtocolMessage` enum (`uc-core/src/network/protocol/protocol_message.rs`): Add payload version field to `ClipboardMessage`, keep existing variants
- `ClipboardMessage` struct (`uc-core/src/network/protocol/clipboard.rs`): Extend with version field, `encrypted_content: Vec<u8>` becomes chunked payload
- `EncryptedBlob` (`uc-infra/src/security/encryption.rs`): Current whole-blob encryption — needs chunk-level counterpart
- `SystemClipboardSnapshot.representations: Vec<ObservedClipboardRepresentation>`: Already supports multiple MIME types — can be used as multi-representation source
- `SyncOutboundClipboardUseCase` / `SyncInboundClipboardUseCase`: Main entry points for send/receive — need chunking layer integration
- Echo prevention mechanisms (origin filtering, content hash dedup, recent ID tracking): Must continue working with chunked protocol

### Established Patterns

- Hexagonal architecture: ports in `uc-core`, adapters in `uc-platform`/`uc-infra` — new chunking layer should follow same pattern
- JSON serialization for protocol messages — V2 payload may need binary serialization for efficiency
- XChaCha20-Poly1305 via `uc-infra/src/security/encryption.rs` — reuse cipher but change to per-chunk encryption
- AAD pattern (`uc-core/src/security/aad.rs`): Extend for chunk-level AAD (file_id + chunk_index)

### Integration Points

- `ClipboardTransportPort::send_clipboard()` / `subscribe_clipboard()`: Transport interface — chunking layer sits between use case and transport
- libp2p adapter (`uc-platform/src/adapters/libp2p_network.rs`): Stream handler needs to support chunked reads/writes instead of single payload
- `BUSINESS_PAYLOAD_MAX_BYTES` (100MB): May need adjustment or removal since chunking handles large payloads
- Background receiver loop (`uc-tauri/src/bootstrap/wiring.rs`): Needs chunked reassembly before passing to inbound use case

</code_context>

<deferred>
## Deferred Ideas

- Multi-representation clipboard write on receiver side — requires platform-level clipboard write support for multiple MIME types simultaneously
- Transfer progress UI (progress bars, percentage) — could be added in a future UX phase if needed
- Parallel chunk download — architecture supports it but Phase 2 implements sequential streaming only
- Resumable/interrupted transfer resume — architecture supports it via chunk-level AEAD but not implemented in Phase 2

</deferred>

---

_Phase: 02-unified-transfer-layer_
_Context gathered: 2026-03-03_

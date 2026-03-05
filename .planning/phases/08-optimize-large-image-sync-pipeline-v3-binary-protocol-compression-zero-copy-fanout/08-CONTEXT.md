# Phase 8: Optimize large image sync pipeline (V3 binary protocol, compression, zero-copy fanout) - Context

**Gathered:** 2026-03-05
**Status:** Ready for planning

<domain>
## Phase Boundary

Significantly reduce end-to-end latency for large image clipboard sync between devices. Replace the V2 JSON+base64 encoding with a V3 binary protocol, add zstd compression before encryption, eliminate per-peer memory copies via Arc<[u8]>, and parallelize encode preparation with business path setup. Full-representation sync semantics are preserved (all reps sent), but only the highest-priority rep is persisted on the receiver (with TODO for future multi-rep storage).

This phase does NOT implement multi-rep persistence on the receiver, does NOT change the dashboard display strategy, and does NOT maintain backward compatibility with V1/V2 protocols (direct upgrade, all devices must upgrade together).

</domain>

<decisions>
## Implementation Decisions

### Compression layer placement

- Compression lives INSIDE TransferPayloadEncryptorPort/DecryptorPort implementation (chunked_transfer.rs)
- sync_outbound and sync_inbound are unaware of compression — port signature unchanged
- Pipeline: plaintext -> zstd compress (inside port) -> chunked encrypt -> wire bytes
- Decode pipeline reverses: wire bytes -> chunked decrypt -> zstd decompress -> plaintext
- Compression flag stored in V3 wire frame header (compression_algo + uncompressed_len fields)
- Hard-coded threshold: skip compression for payloads <= 8KB
- Hard-coded zstd level 3 (consistent with Phase 4 blob at-rest choice)
- No configurable compression parameters — YAGNI, can add later if needed

### V3 wire format

- Completely new wire format with magic "UC3\0" (not an extension of UC2)
- Decoder identifies format by magic bytes — UC2 and UC3 are independent
- V3 wire header layout (37 bytes total):
  - [4B] magic: 0x55 0x43 0x33 0x00 ("UC3\0")
  - [1B] compression_algo (0=none, 1=zstd)
  - [4B] uncompressed_len (u32 LE)
  - [16B] transfer_id (UUID v4 raw bytes)
  - [4B] total_chunks (u32 LE)
  - [4B] chunk_size_hint (u32 LE)
  - [4B] total_plaintext_len (u32 LE) — this is compressed size when compression active
  - Then chunked AEAD same as V2: per chunk [4B ciphertext_len] [NB ciphertext]

### V3 payload binary codec

- Located in uc-core/network/protocol/ (alongside existing V2 types)
- Replaces JSON+base64 with length-prefixed binary encoding
- No external serialization dependency (pure std::io Read/Write)
- Payload layout (before compression):
  - [8B] ts_ms (i64 LE)
  - [2B] rep_count (u16 LE)
  - For each rep:
    - [2B] format_id_len (u16 LE)
    - [NB] format_id (UTF-8)
    - [1B] has_mime (0/1)
    - [2B] mime_len (u16 LE, if has_mime)
    - [NB] mime (UTF-8, if has_mime)
    - [4B] data_len (u32 LE)
    - [NB] data (raw bytes)

### Port interface changes

- ClipboardTransportPort::send_clipboard: Vec<u8> -> Arc<[u8]>
- ClipboardTransportPort::broadcast_clipboard: Vec<u8> -> Arc<[u8]> (unified for consistency)
- All trait implementors updated: libp2p adapter, all test mocks
- TransferPayloadEncryptorPort/DecryptorPort signatures unchanged (compression is internal)
- This is a breaking change — direct upgrade, no backward compatibility shim

### V1/V2 legacy code removal

- DELETE all V1 and V2 encoding AND decoding paths
- Remove: ClipboardMultiRepPayloadV2, ClipboardTextPayloadV1, WireRepresentation (V2 serde version)
- Remove: UC2 ChunkedEncoder/ChunkedDecoder (replaced by UC3 versions)
- Remove: V1/V2 dispatch logic in sync_inbound
- Remove: ClipboardPayloadVersion::V1 and V2 enum variants
- Clean break — if a device sends V1/V2, the receiver will reject with an error

### Parallelization strategy

- Parallel: V3 encode+compress+encrypt runs concurrently with first peer's ensure_business_path (tokio::join!)
- After both complete: send to first peer, then serial loop for remaining peers (ensure -> send)
- NOT full parallel fanout to all peers — avoids concurrent libp2p pressure
- Arc<[u8]> enables zero-copy for the serial fanout loop

### Inbound multi-rep handling

- V3 inbound decodes all reps from binary payload
- Only highest-priority rep is persisted (existing select_highest_priority_repr_index logic)
- debug! log outputs each rep's mime/format_id/size for development visibility
- info! level only shows summary count
- TODO comments at two locations in sync_inbound.rs:
  1. Near select_highest_priority_repr_index: "Multi-rep storage not yet supported — persisting highest-priority only"
  2. At actual write: "TODO: batch-write all reps when RepresentationRepository supports it"

### Observability

- Use tracing spans (not manual Instant::now()) — consistent with project convention
- Focus on critical path only, not fine-grained network layer metrics
- Outbound spans: one span for "outbound.prepare" (encode+compress+encrypt with raw_bytes/compressed_bytes/encrypted_bytes fields), one span for "outbound.send" per peer
- Inbound spans: one span for "inbound.decode" (decrypt+decompress+decode with wire_bytes/decompressed_bytes fields)
- NOT adding spans to libp2p adapter internals (queue_wait_ms, open_ms etc.) — too invasive for this phase

### Testing strategy

- Unit tests: V3 payload binary codec round-trip, compression on/off paths, large payload boundary (tens of MB)
- Test data: randomly generated bytes (vec![0u8; 10_000_000]) to simulate large images, no real image fixtures
- Performance comparison: manual log comparison using tracing span output, no criterion benchmarks
- Integration tests: single-peer large image sync end-to-end, encryption session not-ready regression

### Claude's Discretion

- Exact V3 payload struct naming (ClipboardPayloadV3, ClipboardBinaryPayload, etc.)
- Chunk nonce derivation update for V3 (whether to change AAD prefix from "uc:chunk-nonce:v1|" to "v3|")
- Error variant additions to ChunkedTransferError for V3-specific failures
- Whether to keep WireRepresentation struct name or rename for V3 binary context
- Exact TODO comment wording

</decisions>

<specifics>
## Specific Ideas

- Network compression is independent from blob at-rest compression (Phase 4) — different paths, different AAD, different granularity (single blob vs bundled payload)
- Follow the same binary framing philosophy as UCBL blob format: fixed-size headers, length-prefixed variable data, raw bytes without encoding overhead
- The "compress -> encrypt -> chunk" pipeline order is critical: compression operates on plaintext (best ratio), encryption provides per-chunk AEAD, chunking enables streaming
- Performance targets from proposal: end-to-end latency reduction >= 40%, wire bytes reduction >= 60%

</specifics>

<code_context>

## Existing Code Insights

### Reusable Assets

- `ChunkedEncoder`/`ChunkedDecoder` (`uc-infra/src/clipboard/chunked_transfer.rs`): V2 chunked AEAD — will be replaced by V3 version but same architectural pattern (streaming encode/decode, CHUNK_SIZE constant, derive_chunk_nonce)
- `TransferPayloadEncryptorAdapter`/`TransferPayloadDecryptorAdapter`: Port adapters wrapping chunked transfer — modify internals, keep adapter pattern
- `ClipboardMultiRepPayloadV2` (`uc-core/src/network/protocol/clipboard_payload_v2.rs`): Reference for V3 payload structure (same fields: ts_ms, representations with mime/format_id/bytes)
- `aad::for_chunk_transfer()` (`uc-core/src/security/aad.rs`): Chunk AAD generation — reuse or update for V3
- `SyncOutboundClipboardUseCase` (`uc-app/src/usecases/clipboard/sync_outbound.rs`): Main modification target — 265 lines production + 735 lines tests, well-tested fanout logic
- `SyncInboundClipboardUseCase` (`uc-app/src/usecases/clipboard/sync_inbound.rs`): Large file (~700+ lines) with V1/V2 dispatch — will be simplified by removing V1/V2

### Established Patterns

- Hexagonal architecture: ports in uc-core, implementations in uc-infra
- Binary wire format: 4-byte LE length prefix for variable-length segments (used in UC2 chunked transfer)
- AEAD per-chunk: XChaCha20-Poly1305 with derived nonce from transfer_id + chunk_index
- Port adapter pattern: thin adapter structs implementing port traits, delegating to internal implementations
- Two-segment wire framing: [4B json_len][json_header][trailing_binary] for ProtocolMessage

### Integration Points

- `ProtocolMessage::frame_to_bytes(Some(&encrypted_content))`: Framing for two-segment wire — outbound calls this after encryption
- `ClipboardPayloadVersion` enum in ProtocolMessage JSON header: Will have only V3 after cleanup
- libp2p adapter `BusinessCommand::SendClipboard`: Needs Arc<[u8]> field change
- `select_highest_priority_repr_index()` in sync_inbound: Existing priority selection logic, reuse for V3

</code_context>

<deferred>
## Deferred Ideas

- **Multi-rep persistence**: Receiver stores all reps, not just highest-priority — requires RepresentationRepository batch-write support (noted with TODO)
- **Configurable compression**: Allow runtime configuration of compression level, threshold, and algorithm — YAGNI for now
- **Streaming V3 encode**: True streaming encoder that writes directly to libp2p stream without buffering full compressed payload — would further reduce memory but adds complexity
- **Network layer observability**: Fine-grained spans in libp2p adapter (queue_wait, connection open/close) — deferred to avoid excessive instrumentation

</deferred>

---

_Phase: 08-optimize-large-image-sync-pipeline-v3-binary-protocol-compression-zero-copy-fanout_
_Context gathered: 2026-03-05_

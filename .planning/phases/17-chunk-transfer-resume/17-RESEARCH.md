# Phase 17: Chunked Transfer and Resume - Research

**Researched:** 2026-03-08
**Domain:** Network streaming I/O, resume protocol, transfer progress events
**Confidence:** HIGH

## Summary

The current clipboard transfer pipeline encrypts the full payload into memory (via `TransferPayloadEncryptorAdapter` into a `Vec<u8>`), frames it with a JSON header, and sends the entire blob over a single libp2p stream using `stream.write_all(data)`. On the receive side, the full stream is read into memory via `read_to_end(&mut buf)` inside a `spawn_blocking` task, then decrypted. This works but loads the entire payload into memory on both sides simultaneously.

The V3 wire format already supports chunked AEAD encryption (256KB chunks with per-chunk nonces), but this chunking is only used at the cryptographic layer -- the network I/O layer treats the result as a single monolithic blob. The goal of this phase is to bridge the gap: send/receive chunk-by-chunk over the network, track transfer state for resume capability, and emit progress events for the frontend.

**Primary recommendation:** Introduce a streaming transport layer that writes V3 header + encrypted chunks individually over the libp2p stream, with a lightweight resume handshake protocol using the existing `transfer_id` from the V3 header. Add a `TransferProgressPort` to uc-core for progress callbacks, with a Tauri event adapter in uc-tauri.

## Current Implementation Details

### Encryption Layer (uc-infra)

| Component                         | File                                         | Role                                                    |
| --------------------------------- | -------------------------------------------- | ------------------------------------------------------- |
| `ChunkedEncoder`                  | `uc-infra/src/clipboard/chunked_transfer.rs` | Writes V3 header + per-chunk ciphertext to `impl Write` |
| `ChunkedDecoder`                  | same                                         | Reads V3 header + per-chunk ciphertext from `impl Read` |
| `TransferPayloadEncryptorAdapter` | same                                         | Port adapter: encrypts entire payload into `Vec<u8>`    |
| `TransferPayloadDecryptorAdapter` | same                                         | Port adapter: decrypts entire `&[u8]` into `Vec<u8>`    |

**V3 Wire Format (37-byte header):**

```
[4 bytes]  magic: "UC3\0"
[1 byte]   compression_algo (0=none, 1=zstd)
[4 bytes]  uncompressed_len (u32 LE)
[16 bytes] transfer_id (UUID v4 raw bytes)  <-- KEY for resume
[4 bytes]  total_chunks (u32 LE)
[4 bytes]  chunk_size_hint (u32 LE) = 256KB
[4 bytes]  total_plaintext_len (u32 LE)
Per chunk:
  [4 bytes]  chunk_ciphertext_len (u32 LE)
  [N bytes]  ciphertext (plaintext + 16-byte Poly1305 tag)
```

- CHUNK_SIZE = 256KB
- Max decompressed size: 128 MiB (`RECEIVE_PLAINTEXT_CAP`)
- Compression: zstd level 3 when payload > 8KB and compressed < original
- Each chunk has a unique nonce derived from `transfer_id + chunk_index`

### Transport Layer (uc-platform)

**Outbound path:**

1. `SyncOutboundClipboardUseCase` calls `TransferPayloadEncryptorAdapter.encrypt()` -- produces entire encrypted blob in `Vec<u8>`
2. `ProtocolMessage::Clipboard(header).frame_to_bytes(Some(&encrypted))` -- prepends 4-byte JSON length + JSON header, appends encrypted blob
3. Result is `Arc<[u8]>` sent via `ClipboardTransportPort.send_clipboard()`
4. `execute_business_stream()` opens libp2p stream, calls `stream.write_all(data)`, then `stream.close()`

**Inbound path:**

1. Inbound stream listener reads 4-byte JSON length, then JSON header
2. For V3 Clipboard messages: `spawn_blocking` with `SyncIoBridge` wrapping the async reader
3. Inside blocking task: `sync_reader.read_to_end(&mut buf)` reads entire remaining stream
4. `transfer_decryptor.decrypt(&encrypted, &master_key)` decrypts the blob
5. Decrypted plaintext passed to `SyncInboundClipboardUseCase.execute()`

**Key constants:**

- `BUSINESS_PAYLOAD_MAX_BYTES`: 300 MiB
- `BUSINESS_READ_TIMEOUT`: 120s
- `BUSINESS_STREAM_WRITE_TIMEOUT`: 120s

### Port Definitions (uc-core)

| Port                           | Signature                                              | Current Usage               |
| ------------------------------ | ------------------------------------------------------ | --------------------------- |
| `TransferPayloadEncryptorPort` | `fn encrypt(&self, key, plaintext) -> Result<Vec<u8>>` | Whole-payload in-memory     |
| `TransferPayloadDecryptorPort` | `fn decrypt(&self, encrypted, key) -> Result<Vec<u8>>` | Whole-payload in-memory     |
| `ClipboardTransportPort`       | `async fn send_clipboard(&self, peer_id, Arc<[u8]>)`   | Sends entire framed payload |

### Event System

- `NetworkEvent` enum in `uc-core/src/network/events.rs` -- no transfer progress variant exists
- Frontend events emitted via `tauri::AppHandle.emit()` in `uc-tauri/src/events/`
- Pattern: define event struct, create `forward_*_event()` function, call `app.emit("channel://event", payload)`

## Architecture Patterns

### Recommended Approach: Streaming Chunked I/O

The key insight is that `ChunkedEncoder` already writes chunk-by-chunk to `impl Write`, and `ChunkedDecoder` already reads chunk-by-chunk from `impl Read`. The current bottleneck is that the _port adapters_ buffer everything into `Vec<u8>` before handing it to the network layer.

**Approach: New streaming port + adapter pair that writes/reads directly to/from the libp2p stream.**

### Layer Separation

```
uc-core (ports):
  - TransferPayloadEncryptorPort    (existing, keep for backward compat)
  - TransferPayloadDecryptorPort    (existing, keep for backward compat)
  - NEW: StreamingTransferPort      (chunk-by-chunk network I/O)
  - NEW: TransferProgressPort       (progress callbacks)
  - NEW: TransferStatePort          (resume state persistence)

uc-infra (adapters):
  - ChunkedEncoder / ChunkedDecoder (existing, reuse directly)
  - NEW: TransferStateStore         (simple file/memory store for transfer resume state)

uc-platform (network):
  - Modify execute_business_stream to support chunked writes
  - Modify inbound stream handler to support chunked reads
  - Implement TransferProgressPort adapter

uc-app (use cases):
  - Modify SyncOutboundClipboardUseCase to use streaming path
  - Modify SyncInboundClipboardUseCase to accept streamed data
```

### Pattern 1: Streaming Outbound (Chunk-by-Chunk Write)

**What:** Instead of encrypting the entire payload into memory, encrypt and write each chunk directly to the network stream.

**Implementation strategy:**

```rust
// In uc-platform: modify the outbound business stream
async fn send_clipboard_chunked(
    stream: &mut libp2p::Stream,
    header_bytes: &[u8],       // JSON header frame
    master_key: &MasterKey,
    transfer_id: &[u8; 16],
    plaintext: &[u8],          // possibly compressed
    compression_algo: u8,
    uncompressed_len: u32,
    progress_tx: Option<&dyn TransferProgressPort>,
) -> Result<()> {
    // 1. Write JSON header frame
    stream.write_all(header_bytes).await?;

    // 2. Write V3 header (37 bytes)
    // 3. For each chunk:
    //    a. Encrypt chunk
    //    b. Write 4-byte ciphertext length + ciphertext
    //    c. Emit progress event
}
```

**Key decision:** The `ChunkedEncoder::encode_to()` takes `impl Write` (synchronous). For async streaming, two options:

1. **Use `SyncIoBridge` in `spawn_blocking`** -- wrap the async stream writer. This is the same pattern already used for inbound reads. Simpler.
2. **Rewrite as async** -- create an async version of encode/decode. More work, but avoids blocking thread pool.

**Recommendation:** Use option 1 (SyncIoBridge) for consistency with existing patterns and lower risk. The blocking task only holds the stream for the duration of each chunk write, and `spawn_blocking` is already used on the receive side.

### Pattern 2: Streaming Inbound (Chunk-by-Chunk Read)

**What:** Read and decrypt each chunk individually instead of buffering entire stream.

**Current code already does this internally** in `ChunkedDecoder::decode_from()` -- it reads chunk by chunk. The problem is it accumulates into `Vec<u8>`. For true streaming, the decoder would need to yield chunks one at a time.

**However**, the inbound use case ultimately needs all the plaintext assembled to decode the `ClipboardBinaryPayload`. So true streaming to the use case layer has limited benefit for clipboard payloads. The real win is:

1. **Progress reporting** during the receive
2. **Resume capability** -- knowing which chunks were successfully received

**Recommendation:** Keep `ChunkedDecoder` accumulating plaintext, but add progress callbacks during chunk reads. For resume, track `transfer_id + last_successful_chunk_index`.

### Pattern 3: Resume Protocol

**What:** Allow interrupted transfers to resume from the last successful chunk.

**V3 wire format advantage:** The header contains `transfer_id` (UUID) and `total_chunks`. Each chunk has a deterministic nonce derived from `transfer_id + chunk_index`. This means any chunk can be independently encrypted/decrypted if we know the transfer_id and chunk_index.

**Resume handshake (new sub-protocol on business stream):**

```
Receiver                         Sender
   |                                |
   |  <-- V3 header + chunks 0..N  |  (transfer interrupted at chunk N)
   |                                |
   |  ... connection restored ...   |
   |                                |
   |  <-- Resume request:           |
   |       transfer_id, start_chunk |
   |       (sent as new stream)     |
   |                                |
   |  --> Resume header:            |
   |       transfer_id, total_chunks|
   |       chunks N+1..end          |
   |                                |
```

**Implementation choices:**

Option A: **New ProtocolMessage variant** -- Add `ProtocolMessage::TransferResume { transfer_id, start_chunk }` to the JSON header. Sender re-opens stream, sends resume header, then remaining chunks. Receiver must persist partial state.

Option B: **Simpler retry-from-scratch with dedup** -- On failure, retry the entire transfer. Use `transfer_id` dedup to avoid double-processing. No resume protocol needed, but wastes bandwidth.

**Recommendation:** Start with **Option A** for large transfers (>1MB), with **Option B** as fallback for small transfers. The V3 format already has all the primitives needed (transfer_id, total_chunks, deterministic nonces).

**Resume state storage:** A simple in-memory `HashMap<[u8; 16], TransferResumeState>` with TTL eviction (e.g., 5 minutes). No need for persistent storage -- if the app restarts, retry from scratch.

### Pattern 4: Transfer Progress Events

**What:** Emit progress events from the transfer layer to the frontend.

**New event variant in NetworkEvent:**

```rust
NetworkEvent::TransferProgress {
    transfer_id: String,
    peer_id: String,
    direction: TransferDirection,  // Sending / Receiving
    chunks_completed: u32,
    total_chunks: u32,
    bytes_transferred: u64,
    total_bytes: u64,
}
```

**Frontend event:** Emit via `app.emit("transfer://progress", payload)` using existing Tauri event pattern.

**Throttling:** Don't emit for every chunk (256KB chunks on fast LAN = hundreds of events/second). Throttle to ~10 events/second or emit every N chunks.

### Anti-Patterns to Avoid

- **Don't break existing port contracts**: The existing `TransferPayloadEncryptorPort` / `DecryptorPort` must continue to work for backward compatibility and testing. Add new streaming variants alongside.
- **Don't hold the libp2p swarm event loop**: All stream I/O must happen in spawned tasks, not in the swarm event loop. Current code already does this correctly.
- **Don't make the resume protocol mandatory**: Simple/small transfers should still work with the current fire-and-forget approach. Resume adds complexity only justified for large payloads.
- **Don't store resume state persistently**: In-memory with TTL is sufficient. App restart means retry from scratch.

## Don't Hand-Roll

| Problem              | Don't Build          | Use Instead                               | Why                                                  |
| -------------------- | -------------------- | ----------------------------------------- | ---------------------------------------------------- |
| Async-to-sync bridge | Custom adapter       | `tokio_util::io::SyncIoBridge`            | Already used in codebase, battle-tested              |
| Stream framing       | Manual byte counting | Existing V3 header format                 | Already has transfer_id, total_chunks, chunk sizes   |
| Progress throttling  | Custom timer         | `tokio::time::Interval` or simple counter | Standard pattern, low complexity                     |
| Nonce derivation     | New scheme           | Existing `derive_chunk_nonce()`           | Already deterministic from transfer_id + chunk_index |

## Common Pitfalls

### Pitfall 1: Stream lifetime with spawn_blocking

**What goes wrong:** The libp2p `Stream` is `!Send` in some configurations, preventing it from being moved into `spawn_blocking`.
**Why it happens:** libp2p streams may have thread-affinity constraints.
**How to avoid:** Use `SyncIoBridge` which wraps an `AsyncRead`/`AsyncWrite` and is `Send`. This is the same pattern already working in the inbound path.
**Warning signs:** Compiler errors about `Send` bounds on task futures.

### Pitfall 2: Partial write on stream close

**What goes wrong:** If the sender calls `stream.close()` before all chunks are flushed, the receiver gets a truncated stream.
**Why it happens:** `close()` on a yamux stream sends a FIN. If there are buffered writes not yet flushed, they may be lost.
**How to avoid:** Always `flush()` before `close()`. Or use `write_all()` which ensures all bytes are sent before returning.
**Warning signs:** `TruncatedChunk` errors on the receiver side intermittently.

### Pitfall 3: Memory regression during resume

**What goes wrong:** Resume state accumulates received chunks in memory, defeating the purpose of streaming.
**Why it happens:** Storing decrypted chunks in a `Vec<Vec<u8>>` for resume = same memory as before.
**How to avoid:** Store only the encrypted bytes for resume (or better, just track chunk index and re-request). On resume, the receiver only needs to tell the sender "start from chunk N".
**Warning signs:** Memory usage spikes during large transfers despite "streaming" implementation.

### Pitfall 4: Progress event flooding

**What goes wrong:** Emitting a Tauri event for every 256KB chunk on a fast LAN creates thousands of events per second, lagging the frontend.
**Why it happens:** A 100MB file = ~400 chunks. On gigabit LAN, all 400 complete in < 1 second.
**How to avoid:** Throttle progress events to max 10/second or emit every Nth chunk. Always emit at 0% and 100%.
**Warning signs:** Frontend becomes unresponsive during transfers.

### Pitfall 5: Backward compatibility

**What goes wrong:** Older peers that expect the full payload in a single write cannot handle chunked streaming.
**Why it happens:** The receiver currently calls `read_to_end()` which works regardless of how the sender writes (chunked or not).
**How to avoid:** `read_to_end()` is agnostic to write patterns -- it reads until EOF. So the sender can switch to chunked writes without breaking receivers. The critical part is that the total bytes on the wire remain identical.
**Warning signs:** None expected -- this is a safe change.

## Code Examples

### Chunked Outbound Write (in uc-platform)

```rust
// Conceptual: write chunks directly to async stream
async fn write_chunked_payload(
    stream: &mut impl AsyncWrite + Unpin,
    framed_header: &[u8],  // 4-byte len + JSON
    encrypted_payload: &[u8],  // Already encrypted V3 wire format
    progress_callback: impl Fn(u64, u64),
) -> Result<()> {
    // Write the JSON header frame first
    stream.write_all(framed_header).await?;

    // Write the V3 encrypted payload in chunks
    // Note: the V3 format is self-describing (header + chunk lengths),
    // so we can write it in any chunk size without protocol changes.
    let total = encrypted_payload.len() as u64;
    let mut written = 0u64;
    for chunk in encrypted_payload.chunks(256 * 1024) {
        stream.write_all(chunk).await?;
        written += chunk.len() as u64;
        progress_callback(written, total);
    }
    stream.flush().await?;
    Ok(())
}
```

### Progress Port Definition (in uc-core)

```rust
// uc-core/src/ports/transfer_progress.rs
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub enum TransferDirection {
    Sending,
    Receiving,
}

#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub transfer_id: String,
    pub peer_id: String,
    pub direction: TransferDirection,
    pub chunks_completed: u32,
    pub total_chunks: u32,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
}

#[async_trait]
pub trait TransferProgressPort: Send + Sync {
    async fn report_progress(&self, progress: TransferProgress);
}
```

## Risk Areas and Mitigation

### Risk 1: libp2p stream behavior with chunked writes (MEDIUM)

The current code writes the entire payload in one `write_all()` call. Switching to multiple smaller writes should work identically from the receiver's perspective (TCP/yamux handles framing), but needs integration testing.
**Mitigation:** Test with varying payload sizes (1KB, 1MB, 50MB) across two local libp2p peers.

### Risk 2: Resume protocol complexity (HIGH)

Adding a resume handshake introduces a new protocol state machine. Both sender and receiver must agree on transfer_id, chunk boundaries, and handle edge cases (sender no longer has the payload cached, transfer_id expired).
**Mitigation:** Phase the work -- implement chunked I/O and progress first (lower risk), add resume in a subsequent sub-phase. Keep resume optional and gracefully fall back to full retry.

### Risk 3: Encryption port signature change (MEDIUM)

The current `TransferPayloadEncryptorPort.encrypt()` returns `Vec<u8>` (entire payload). Streaming would ideally take a `Write` target. Changing the port signature affects all implementations and tests.
**Mitigation:** Add a new streaming-aware port (`StreamingTransferEncryptorPort`) rather than modifying the existing one. The existing port remains for testing and small payloads.

## Validation Architecture

### Test Framework

| Property           | Value                                                           |
| ------------------ | --------------------------------------------------------------- |
| Framework          | cargo test (built-in Rust test framework)                       |
| Config file        | `src-tauri/Cargo.toml` (workspace)                              |
| Quick run command  | `cd src-tauri && cargo test -p uc-infra --lib chunked_transfer` |
| Full suite command | `cd src-tauri && cargo test`                                    |

### Phase Requirements -> Test Map

| Req ID | Behavior                                                   | Test Type   | Automated Command                                                         | File Exists?                                                  |
| ------ | ---------------------------------------------------------- | ----------- | ------------------------------------------------------------------------- | ------------------------------------------------------------- |
| CT-01  | Chunked write produces identical bytes as monolithic write | unit        | `cd src-tauri && cargo test -p uc-infra --lib chunked_transfer -x`        | Partially (encoder tests exist, streaming write tests needed) |
| CT-02  | Receiver decodes chunked-written stream correctly          | integration | `cd src-tauri && cargo test -p uc-platform --lib -- business_stream -x`   | Partially (echo tests exist)                                  |
| CT-03  | Progress events emitted with correct counts                | unit        | `cd src-tauri && cargo test -p uc-platform --lib -- transfer_progress -x` | Wave 0                                                        |
| CT-04  | Resume handshake recovers from mid-transfer interruption   | integration | `cd src-tauri && cargo test -p uc-platform --lib -- transfer_resume -x`   | Wave 0                                                        |
| CT-05  | Frontend receives progress events                          | integration | Manual (Tauri event system)                                               | Manual-only                                                   |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-infra --lib chunked_transfer && cargo test -p uc-platform --lib -- business_stream`
- **Per wave merge:** `cd src-tauri && cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-platform/src/adapters/transfer_progress_tests.rs` -- covers CT-03
- [ ] `src-tauri/crates/uc-platform/src/adapters/transfer_resume_tests.rs` -- covers CT-04
- [ ] Progress port definition in uc-core -- needed before test scaffolding

## Open Questions

1. **Resume scope: sender-side caching**
   - What we know: Receiver can track which chunks it received. Sender needs to re-send from a specific chunk.
   - What's unclear: How long should the sender cache the plaintext/encrypted payload for resume? The current flow discards it after send.
   - Recommendation: Cache the `Arc<[u8]>` encrypted payload in a TTL map (5 min) keyed by transfer_id. For resume, re-open stream and write from the requested chunk offset. If cache expired, fall back to full retry.

2. **Streaming encryption vs pre-encryption**
   - What we know: True streaming (encrypt+write per chunk) saves peak memory. Pre-encryption (current) is simpler.
   - What's unclear: Is the memory saving worth the added complexity for clipboard payloads (max 128MB)?
   - Recommendation: Start with pre-encryption + chunked network writes (simplest change, still solves progress and reduces peak network buffer). True streaming encryption can be added later if memory is still a concern.

3. **Protocol version negotiation**
   - What we know: The V3 wire format is self-describing. Chunked writes produce identical bytes on the wire.
   - What's unclear: Does the resume handshake need a new protocol version or can it be a new `ProtocolMessage` variant?
   - Recommendation: New `ProtocolMessage::TransferResume` variant -- no protocol version bump needed since the wire format bytes are unchanged.

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs` -- V3 wire format, encoder/decoder implementation
- `src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` -- Network stream handling, business command execution
- `src-tauri/crates/uc-core/src/ports/security/transfer_crypto.rs` -- Transfer crypto port definitions
- `src-tauri/crates/uc-core/src/ports/clipboard_transport.rs` -- Clipboard transport port
- `src-tauri/crates/uc-core/src/network/events.rs` -- NetworkEvent enum
- `src-tauri/crates/uc-tauri/src/events/mod.rs` -- Frontend event emission pattern

### Secondary (MEDIUM confidence)

- libp2p-stream 0.4.0-alpha API -- stream.write_all/read behavior (verified via codebase usage patterns)
- tokio_util::io::SyncIoBridge -- already used in codebase for inbound path

## Metadata

**Confidence breakdown:**

- Current implementation analysis: HIGH -- read all relevant source files
- Chunked I/O approach: HIGH -- V3 format already supports this, network layer change is minimal
- Resume protocol: MEDIUM -- design is sound but implementation details need validation during planning
- Progress events: HIGH -- follows existing event emission patterns exactly
- Test infrastructure: HIGH -- existing test patterns well established

**Research date:** 2026-03-08
**Valid until:** 2026-04-08 (stable domain, no external dependency changes expected)

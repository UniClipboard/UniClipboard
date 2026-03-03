# Phase 3: True Inbound Streaming - Research

**Researched:** 2026-03-03
**Domain:** libp2p stream-level parsing, sync/async IO bridging, wire format redesign
**Confidence:** HIGH

## Summary

Phase 3 eliminates the `read_to_end` bottleneck in the inbound clipboard path. Currently, the entire `ProtocolMessage` JSON envelope (including the base64-encoded V2 binary payload inside `encrypted_content`) is read into memory before any parsing or decoding occurs. For a 100MB image, this means ~133MB of base64 in the JSON + ~100MB decoded binary = ~233MB peak memory. The fix requires separating the V2 binary payload from the JSON envelope at the wire level so `ChunkedDecoder::decode_from` can operate directly on the stream.

The core challenge is that `ProtocolMessage` is a JSON-serialized enum wrapping `ClipboardMessage`, which embeds the V2 payload as a base64-encoded field. Streaming requires moving the V2 binary payload _outside_ the JSON envelope — sent sequentially after a smaller JSON header on the same libp2p stream. This is a wire format change for V2 clipboard messages only; V1 messages and non-clipboard messages (DeviceAnnounce, Heartbeat, Pairing) remain unchanged.

The second challenge is bridging `futures::AsyncRead` (libp2p's stream type) to `std::io::Read` (what `ChunkedDecoder::decode_from` accepts). The standard solution is `tokio_util::io::SyncIoBridge` inside `tokio::task::spawn_blocking`, which is already available in the project's dependency tree.

**Primary recommendation:** Introduce a two-segment wire format for V2 clipboard messages: `[4-byte JSON length][JSON header][V2 binary payload]`. The JSON header contains all `ClipboardMessage` metadata fields but with `encrypted_content` empty. The V2 binary payload follows immediately after, read directly by `ChunkedDecoder::decode_from` via `SyncIoBridge` + `spawn_blocking`. Outbound must also change to produce this new framing.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Streaming requirement

- Inbound V2 clipboard messages MUST NOT buffer the full payload before starting decoding
- `ChunkedDecoder::decode_from` must be called while the stream is still open
- Peak memory target: ~1x chunk size (not ~2x payload size as currently)

#### Error handling (carried from Phase 2)

- All failures are silent -- warn-level log, stream discarded, no user notification
- Same behavior as Phase 2: discard on any decode/validation error

#### V1 backward compatibility stance

- Phase 2 established: V1 devices silently ignore V2 messages
- Phase 3 changes the sender wire format for V2 -- V1 receivers may see garbled data for V2 clipboard messages; this is acceptable since V1 receivers cannot process V2 payloads anyway
- V1 messages (non-clipboard or V1 clipboard) must continue to work unaffected

### Claude's Discretion

- Wire framing design: how to byte-delimit the JSON envelope from the raw binary payload (e.g., length-prefix before JSON, new wrapper, version byte, etc.)
- Whether `ClipboardMessage.encrypted_content` stays in the struct (set to empty for V2) or a new V2 envelope type is introduced
- Whether `payload_size_bytes` or another field is added to the JSON envelope for V2
- Scope of interface changes: transport-only vs. `ClipboardMessage` struct changes
- Whether outbound also changes in this phase (sender must produce the new framing for receivers to benefit)
- Exact async streaming adapter to bridge `AsyncRead` stream to `ChunkedDecoder`'s sync `Read` trait (or whether ChunkedDecoder gets an async variant)

### Deferred Ideas (OUT OF SCOPE)

- Outbound streaming (sender-side): Phase 2 deferred "Option A" (transport streaming for sender). Phase 3 focuses on inbound; if sender changes are needed for wire format compatibility, they are minimal and scoped to producing the new framing
- Parallel chunk download: Architecture supports it; not in scope
- Resumable transfers: Architecture supports it; not in scope
  </user_constraints>

## Standard Stack

### Core

| Library            | Version     | Purpose                                                               | Why Standard                                                                                            |
| ------------------ | ----------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `tokio-util`       | 0.7         | `SyncIoBridge` + `compat` layer                                       | Already in uc-platform Cargo.toml; bridges `futures::AsyncRead` to `std::io::Read` for `ChunkedDecoder` |
| `libp2p-stream`    | 0.4.0-alpha | Provides `Stream` type implementing `futures::AsyncRead + AsyncWrite` | Already used; stream stays open for reading during decode                                               |
| `serde_json`       | 1           | Parse the smaller JSON header envelope                                | Already used everywhere; no change needed                                                               |
| `chacha20poly1305` | 0.10.1      | AEAD decryption in `ChunkedDecoder`                                   | Already in uc-infra; no change                                                                          |

### Supporting

| Library                                         | Version   | Purpose                                                        | When to Use                                                                      |
| ----------------------------------------------- | --------- | -------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `tokio::task::spawn_blocking`                   | 1 (tokio) | Run sync `ChunkedDecoder::decode_from` on blocking thread pool | When bridging the libp2p async stream to the sync `Read` trait                   |
| `tokio_util::compat::FuturesAsyncReadCompatExt` | 0.7       | Convert `futures::AsyncRead` to `tokio::io::AsyncRead`         | Before wrapping with `SyncIoBridge`; already used in `pairing_stream/service.rs` |

### Alternatives Considered

| Instead of                                                                 | Could Use                                                                              | Tradeoff                                                                                                                                                                                 |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `SyncIoBridge` + `spawn_blocking`                                          | Write an async variant of `ChunkedDecoder` using `tokio::io::AsyncReadExt::read_exact` | Duplicates decode logic; introduces `tokio` dependency in `uc-infra`; more code to maintain. Not recommended -- `SyncIoBridge` is battle-tested and `ChunkedDecoder` is already correct. |
| Length-prefix wire format                                                  | Delimiter-based framing (e.g., newline after JSON)                                     | Fragile; JSON can contain any byte if not careful; length-prefix is unambiguous and standard in binary protocols                                                                         |
| Keeping `ClipboardMessage` struct as-is (empty `encrypted_content` for V2) | New `V2ClipboardEnvelope` struct                                                       | New type adds complexity across the codebase (ports, channels, match arms). Reusing `ClipboardMessage` with `encrypted_content = vec![]` for V2-streaming is simpler and non-breaking.   |

## Architecture Patterns

### Wire Format Change: Two-Segment Framing

The key architectural change is moving from a single JSON blob to a two-segment wire format for V2 clipboard messages:

**Current wire format (V1 and V2 today):**

```
[entire ProtocolMessage JSON serialized as bytes, including base64-encoded encrypted_content]
```

**New wire format for V2 clipboard messages:**

```text
[4 bytes]  json_header_len (u32 LE)
[N bytes]  JSON header: ProtocolMessage::Clipboard(ClipboardMessage { encrypted_content: [], ... })
[M bytes]  V2 binary payload (raw UC2 chunked format, NOT base64)
```

**Non-clipboard messages and V1 clipboard messages:**

```text
[4 bytes]  json_header_len (u32 LE)
[N bytes]  JSON body (ProtocolMessage as before, complete)
           (no trailing bytes -- json_header_len == total remaining stream bytes)
```

**Rationale:**

- The 4-byte length prefix lets the receiver read exactly the JSON portion, then hand the remainder of the stream to `ChunkedDecoder::decode_from`.
- V2 `encrypted_content` is set to empty `Vec<u8>` in the JSON header (base64: `""` or omitted). The actual V2 payload follows raw (no base64) after the JSON.
- This eliminates the ~33% base64 overhead AND the `read_to_end` buffering.

### Inbound Processing Pipeline (New)

```
libp2p stream (futures::AsyncRead)
  |
  +--> read 4 bytes: json_header_len
  +--> read json_header_len bytes: JSON header
  +--> serde_json::from_slice -> ProtocolMessage
  |
  +--> match on ProtocolMessage variant:
       |
       +-- DeviceAnnounce/Heartbeat/Pairing: handle immediately (no trailing data)
       +-- Clipboard(msg) where msg.payload_version == V1: handle immediately (encrypted_content in JSON)
       +-- Clipboard(msg) where msg.payload_version == V2:
              |
              +--> stream.compat() -> tokio::io::AsyncRead
              +--> spawn_blocking {
                     SyncIoBridge::new(async_reader)
                     ChunkedDecoder::decode_from(sync_reader, &master_key)
                   }
              +--> pass plaintext + msg metadata to sync_inbound use case
```

### Outbound Changes (Required)

The outbound side must produce the new two-segment framing for V2 messages. This affects `execute_business_stream` in `libp2p_network.rs`:

```
Current: serialize entire ProtocolMessage (with base64 encrypted_content) -> write_all(bytes)
New:     serialize ProtocolMessage header (encrypted_content=[]) -> len-prefix + write_all(json_header)
         then write_all(v2_binary_payload) directly (raw, no base64)
```

This means `ClipboardTransportPort::send_clipboard` must change signature or the caller must provide both the header bytes and the raw V2 payload separately. Options:

1. **Transport-level split**: `send_clipboard` receives `(json_header: Vec<u8>, raw_payload: Option<Vec<u8>>)`. The transport writes len-prefix + header + raw payload.
2. **Pre-framed bytes**: The use case pre-frames the two-segment wire bytes into a single `Vec<u8>` and sends as today. This avoids interface changes but forces the use case to know about the wire format.

**Recommendation:** Option 2 (pre-framed bytes) is simpler and avoids changing `ClipboardTransportPort` trait, which affects all adapters. The outbound use case already builds the full `Vec<u8>` before sending -- it just changes _how_ the bytes are assembled.

### Scope Containment

```
uc-core:     ClipboardMessage struct UNCHANGED (encrypted_content stays, set to empty for V2 streaming)
             ClipboardTransportPort trait UNCHANGED
uc-infra:    ChunkedDecoder UNCHANGED (already accepts std::io::Read)
uc-app:      sync_outbound changes how it frames outbound bytes (two-segment instead of JSON-with-base64)
             sync_inbound receives ClipboardMessage with empty encrypted_content for V2 --
               BUT the plaintext bytes are now passed separately (new parameter or wrapper)
uc-platform: libp2p_network.rs inbound handler restructured to read two-segment format
             libp2p_network.rs outbound handler unchanged (just writes pre-framed bytes)
```

### Anti-Patterns to Avoid

- **Buffering the stream into Vec<u8> "just to parse JSON first":** This defeats the purpose. The JSON header must be read with bounded size, then the stream is handed off for streaming decode.
- **Adding async ChunkedDecoder duplicate:** Duplicating the decode logic for async adds maintenance burden. The sync `Read` interface is correct; use `SyncIoBridge` to adapt.
- **Changing `ClipboardMessage` struct incompatibly:** Keep `encrypted_content: Vec<u8>` in the struct. For V2 streaming inbound, it will be empty. For V1, it still carries the payload. This preserves all existing code paths.
- **Blocking the tokio async runtime:** `ChunkedDecoder::decode_from` does blocking `read_exact` calls. This MUST run inside `spawn_blocking`, never on the async executor.

## Don't Hand-Roll

| Problem                                          | Don't Build                                      | Use Instead                                                                                         | Why                                                                                                            |
| ------------------------------------------------ | ------------------------------------------------ | --------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| Bridging `futures::AsyncRead` to `std::io::Read` | Custom adapter with manual polling               | `tokio_util::compat::FuturesAsyncReadCompatExt` + `tokio_util::io::SyncIoBridge` + `spawn_blocking` | Handles waker registration, blocking semantics, runtime handle capture correctly. Already used in the project. |
| Length-prefix framing                            | Custom state machine for parsing framed messages | Direct `read_exact` for 4 bytes + `read_exact` for N bytes                                          | Simple enough to do with read_exact; no framing library needed for this case                                   |
| Async stream reading of fixed-size headers       | Custom future implementations                    | `tokio::io::AsyncReadExt::read_exact` (via compat layer)                                            | Battle-tested; handles partial reads, cancellation, EOF correctly                                              |

**Key insight:** The existing `ChunkedDecoder::decode_from<R: Read>` is already correctly designed for streaming. The problem is not in the decoder -- it's in the transport layer reading everything into memory before the decoder gets a chance to run. The fix is plumbing, not algorithm.

## Common Pitfalls

### Pitfall 1: Forgetting to close/consume the stream after V2 decode

**What goes wrong:** If `ChunkedDecoder::decode_from` reads all the V2 payload but the stream is not properly closed, the libp2p connection may leak or the peer may hang waiting for the close handshake.
**Why it happens:** The current code calls `limited.into_inner().close().await` after `read_to_end`. With streaming, the close must happen after `spawn_blocking` returns.
**How to avoid:** After `spawn_blocking` completes, call `stream.close().await` on the original libp2p stream. Ensure this happens even on decode errors.
**Warning signs:** Integration tests hang or time out; "stream close timed out" warnings in logs.

### Pitfall 2: Panicking inside spawn_blocking when SyncIoBridge is used incorrectly

**What goes wrong:** `SyncIoBridge::new(reader)` captures `Handle::current()`. If called outside a tokio runtime context, it panics.
**Why it happens:** `SyncIoBridge` is created in the async context before being moved into `spawn_blocking`. This is the correct pattern. Creating it inside `spawn_blocking` also works because `spawn_blocking` threads have access to the runtime handle.
**How to avoid:** Create `SyncIoBridge` inside the `spawn_blocking` closure, or before it in the async context. Both work. Verify with tests.
**Warning signs:** Panic: "Cannot start a runtime from within a runtime" or "no current runtime".

### Pitfall 3: V2 JSON header size guard missing

**What goes wrong:** A malicious or buggy peer sends a `json_header_len` of 4GB, causing the receiver to allocate 4GB for the header alone.
**Why it happens:** The 4-byte length prefix allows up to ~4GB values.
**How to avoid:** Cap `json_header_len` at a reasonable maximum (e.g., 64KB). V2 clipboard JSON headers without `encrypted_content` are tiny (a few hundred bytes). If exceeded, log a warning and discard the stream.
**Warning signs:** OOM on small payloads; unexpectedly large allocations before decode starts.

### Pitfall 4: Outbound framing inconsistency between V1 and V2

**What goes wrong:** If the outbound code writes V1 messages with the new length-prefix framing but old receivers expect raw JSON, V1 interop breaks.
**Why it happens:** Applying the new framing format to ALL messages instead of only V2 clipboard messages.
**How to avoid:** ALL messages get the new framing (length-prefix + JSON). This is forward-compatible: the receiver always reads a 4-byte length prefix first. Old V1 receivers that do `read_to_end` + `serde_json::from_slice` will see the 4-byte prefix as part of the JSON and fail deserialization -- but per the locked decision, V1 receivers already cannot process V2 payloads, so this is acceptable. However, V1 _senders_ sending to V3 receivers need consideration.
**Warning signs:** V1 DeviceAnnounce/Heartbeat messages from V3 senders fail on V1 receivers.

**Important design decision needed:** Whether ALL outbound messages use the new length-prefix format (simpler sender code but breaks V1 interop for non-clipboard messages) or only V2 clipboard messages use it (more complex but V1 non-clipboard messages still work). See Open Questions.

### Pitfall 5: `spawn_blocking` task outliving the stream

**What goes wrong:** If the async task that owns the libp2p stream is cancelled (e.g., timeout), but the `spawn_blocking` task is still reading from `SyncIoBridge`, the bridge will return `io::Error` on the next `read_exact`, and the blocking task will exit with an error. This is the correct behavior but needs explicit handling.
**Why it happens:** Tokio's `spawn_blocking` tasks are not automatically cancelled when the parent async task is dropped.
**How to avoid:** The `SyncIoBridge` will return `io::Error(BrokenPipe)` or similar when the async reader is dropped. `ChunkedDecoder` already returns `Err` on any read failure. Handle the error from `spawn_blocking` result.
**Warning signs:** "TruncatedHeader" or "TruncatedChunk" errors after timeout.

## Code Examples

### Example 1: Two-Segment Wire Write (Outbound)

```rust
// In sync_outbound.rs or libp2p_network.rs outbound path
fn frame_v2_outbound(
    clipboard_message: &ClipboardMessage,
    v2_binary_payload: &[u8],
) -> Vec<u8> {
    // Create a header-only ClipboardMessage with empty encrypted_content
    let header_msg = ClipboardMessage {
        encrypted_content: vec![], // V2 payload is NOT in the JSON
        ..clipboard_message.clone()
    };
    let json_header = ProtocolMessage::Clipboard(header_msg)
        .to_bytes()
        .expect("serialize header");

    let mut framed = Vec::with_capacity(4 + json_header.len() + v2_binary_payload.len());
    framed.extend_from_slice(&(json_header.len() as u32).to_le_bytes());
    framed.extend_from_slice(&json_header);
    framed.extend_from_slice(v2_binary_payload); // Raw binary, no base64
    framed
}
```

### Example 2: Two-Segment Inbound Read (Transport Layer)

```rust
// In libp2p_network.rs spawn_business_stream_handler
use tokio::io::AsyncReadExt;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tokio_util::io::SyncIoBridge;

async fn handle_inbound_stream(
    stream: libp2p::Stream,
    clipboard_tx: mpsc::Sender<(ClipboardMessage, Vec<u8>)>, // msg + plaintext
    // ... other params
) {
    let mut stream = stream.compat(); // futures::AsyncRead -> tokio::io::AsyncRead

    // Step 1: Read 4-byte JSON header length
    let mut len_buf = [0u8; 4];
    if stream.read_exact(&mut len_buf).await.is_err() {
        warn!("failed to read json header length");
        return;
    }
    let json_len = u32::from_le_bytes(len_buf) as usize;

    // Guard: cap JSON header size
    const MAX_JSON_HEADER_SIZE: usize = 64 * 1024; // 64 KB
    if json_len > MAX_JSON_HEADER_SIZE {
        warn!(json_len, "json header too large, discarding stream");
        return;
    }

    // Step 2: Read JSON header
    let mut json_buf = vec![0u8; json_len];
    if stream.read_exact(&mut json_buf).await.is_err() {
        warn!("failed to read json header");
        return;
    }

    let message = match ProtocolMessage::from_bytes(&json_buf) {
        Ok(m) => m,
        Err(e) => { warn!("invalid protocol message: {e}"); return; }
    };

    match message {
        ProtocolMessage::Clipboard(msg) if msg.payload_version == ClipboardPayloadVersion::V2 => {
            // Step 3: V2 streaming decode via spawn_blocking
            let result = tokio::task::spawn_blocking(move || {
                let sync_reader = SyncIoBridge::new(stream);
                ChunkedDecoder::decode_from(sync_reader, &master_key)
            }).await;
            // handle result...
        }
        other => {
            // V1 clipboard, DeviceAnnounce, Heartbeat, Pairing -- handle as before
            handle_non_streaming_message(other).await;
        }
    }
}
```

### Example 3: SyncIoBridge Usage Pattern (from project precedent)

```rust
// Already used in pairing_stream/service.rs:
use tokio_util::compat::FuturesAsyncReadCompatExt;
let stream = stream.compat(); // libp2p::Stream -> tokio Compat<Stream>
// compat() gives tokio::io::AsyncRead, which SyncIoBridge can wrap
```

## State of the Art

| Old Approach                                        | Current Approach                                                                              | When Changed         | Impact                                                                        |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------- | -------------------- | ----------------------------------------------------------------------------- |
| `stream.read_to_end()` + `serde_json::from_slice()` | Two-segment wire read: length-prefixed JSON header, then stream remainder to `ChunkedDecoder` | Phase 3 (this phase) | Peak memory drops from ~2-3x payload to ~1x chunk size (256KB) for V2 inbound |
| Base64-encoded `encrypted_content` in JSON          | Raw binary V2 payload after JSON header (no base64)                                           | Phase 3 (this phase) | Eliminates ~33% base64 overhead on wire AND in memory                         |
| `ProtocolMessage::to_bytes()` single JSON blob      | Length-prefixed JSON header + raw trailing payload                                            | Phase 3 (this phase) | Enables stream-level parsing without buffering entire message                 |

**Deprecated/outdated after this phase:**

- The pattern of embedding V2 binary payloads as base64 inside `ClipboardMessage.encrypted_content` JSON field (Phase 2 approach) is replaced by raw binary after JSON header.

## Open Questions

1. **Should ALL outbound messages use length-prefix framing, or only V2 clipboard?**
   - What we know: Applying length-prefix to ALL messages is simpler (one code path) but breaks V1 receivers for non-clipboard messages (DeviceAnnounce, Heartbeat). Applying only to V2 clipboard messages is more complex (two outbound code paths) but preserves V1 non-clipboard interop.
   - What's unclear: Whether V1 non-clipboard interop matters. If all devices are expected to upgrade together, universal framing is fine. If mixed-version networks must work for non-clipboard messages, selective framing is needed.
   - Recommendation: Apply length-prefix framing to ALL outbound messages. V1 receivers will fail to parse the 4-byte prefix, but per the locked decision, V1 interop for V2 clipboard messages is already broken. If a mixed-version network needs DeviceAnnounce to work, a version negotiation protocol would be needed anyway (out of scope for this phase). Alternatively, use a magic byte to distinguish: if first 4 bytes of stream match a known JSON start byte (`{` = 0x7B), fall back to read_to_end + JSON parse (V1 path). If first 4 bytes look like a u32 length, use the new path. This is fragile and not recommended.
   - **Recommended approach:** ALL messages get length-prefix framing. Accept V1 non-clipboard breakage. This simplifies implementation significantly.

2. **How should `sync_inbound` use case receive V2 plaintext without `encrypted_content`?**
   - What we know: Currently `sync_inbound` receives a `ClipboardMessage` and reads `encrypted_content` from it. With streaming, `encrypted_content` is empty and the plaintext comes from the transport layer after `ChunkedDecoder` runs.
   - What's unclear: The cleanest API boundary. Options: (a) transport passes `(ClipboardMessage, Vec<u8>)` tuple where Vec is the decoded plaintext; (b) transport fills `encrypted_content` with decoded plaintext before passing (misleading field name but zero interface change); (c) new channel type.
   - Recommendation: Option (a) -- pass a `(ClipboardMessage, Vec<u8>)` tuple. This keeps the interface honest. The `clipboard_tx` channel type changes from `mpsc::Sender<ClipboardMessage>` to `mpsc::Sender<(ClipboardMessage, Option<Vec<u8>>)>` where `Some(bytes)` means V2 pre-decoded plaintext and `None` means use `encrypted_content` from the message (V1 path). Alternatively, the transport layer can decode V2 and fill the `encrypted_content` field with the decoded plaintext, changing `payload_version` to a new sentinel -- but this is hacky.

3. **Should the size guard for V2 payload use stream-level take() or rely on ChunkedDecoder's header?**
   - What we know: `ChunkedDecoder` reads `total_plaintext_len` from the V2 header (first 32 bytes). The current `BUSINESS_PAYLOAD_MAX_BYTES` (300MB) guard uses `stream.take()`.
   - What's unclear: Whether `stream.take()` can still be used when the stream is split between JSON header and V2 payload.
   - Recommendation: Apply `stream.take(BUSINESS_PAYLOAD_MAX_BYTES)` to the entire stream before reading anything. The 4-byte length prefix + JSON header + V2 payload all count toward the limit. This preserves the existing size guard behavior.

## Sources

### Primary (HIGH confidence)

- **Project source code** (direct inspection): `libp2p_network.rs`, `chunked_transfer.rs`, `sync_inbound.rs`, `sync_outbound.rs`, `clipboard.rs`, `protocol_message.rs` -- all read and analyzed
- **Phase 2 Summary** (`02-03-SUMMARY.md`): Documents "Option B" decision and deferred inbound streaming
- **Phase 3 Context** (`03-CONTEXT.md`): User decisions and discretion areas
- **tokio-util SyncIoBridge docs** (https://docs.rs/tokio-util/latest/tokio_util/io/struct.SyncIoBridge.html): Confirmed `Read`, `Write` trait implementations, `spawn_blocking` requirement
- **libp2p Stream type docs** (https://libp2p.github.io/rust-libp2p/libp2p/struct.Stream.html): Confirmed `AsyncRead + AsyncWrite` implementation

### Secondary (MEDIUM confidence)

- **tokio bridging guide** (https://tokio.rs/tokio/topics/bridging): `spawn_blocking` patterns for sync/async bridging
- **Project precedent**: `pairing_stream/service.rs` already uses `FuturesAsyncReadCompatExt::compat()` to convert libp2p streams to tokio-compatible async readers

### Tertiary (LOW confidence)

- None -- all findings verified against project source code and official documentation

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH -- all libraries already in project dependencies, patterns already used in codebase
- Architecture: HIGH -- wire format change is straightforward; two-segment framing is a well-understood pattern in binary protocols
- Pitfalls: HIGH -- identified from direct analysis of current code paths and async/sync bridging gotchas
- Open Questions: MEDIUM -- question 1 (universal vs selective framing) and question 2 (API boundary for plaintext passing) require design decisions during planning

**Research date:** 2026-03-03
**Valid until:** 2026-04-03 (stable domain, no fast-moving dependencies)

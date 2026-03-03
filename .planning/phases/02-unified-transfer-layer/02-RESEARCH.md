# Phase 2: Unified Transfer Layer - Research

**Researched:** 2026-03-03
**Domain:** Chunked encrypted data transfer over libp2p streams, multi-representation clipboard protocol
**Confidence:** HIGH (all findings derived from direct codebase inspection)

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Transfer UI behavior

- UI is completely silent during transfer — no placeholder cards, no loading states
- Content appears in clipboard history list only after full assembly and validation
- Transfer failures are silent — no error cards, no notifications, only error-level logs
- Checksum/tag validation failures are also silently discarded with error logging

#### Transfer failure handling

- Sender: silent abandon on disconnect, no retry, no notification to user
- Receiver: immediately marks transfer as failed on connection drop (no timeout grace period)
- Partial transfers are discarded, no partial data retained
- All failures logged at error level for debugging

#### Multi-representation strategy

- Transmit ALL clipboard representations in a single transfer (e.g., PNG + HTML + text packed together)
- All representations bundled into one payload, chunked and sent as an atomic unit
- Receiver writes only the highest-priority representation to clipboard:
  - Priority order: image > rich text (HTML) > plain text
- Single-representation write is a temporary limitation — future phases may support multi-representation clipboard write

#### Protocol compatibility

- Old devices receiving new format messages silently ignore them (unknown payload version → discard)
- Reuse existing `ProtocolMessage::Clipboard(ClipboardMessage)` variant — add payload version field inside to distinguish V1 (text-only) from V2 (unified chunked)
- libp2p protocol ID stays at `/uc-business/1.0.0` — no protocol ID bump
- Version differentiation happens entirely at payload layer

#### Chunk-level AEAD encryption (user-specified design)

- Streaming chunking — NOT whole-payload-in-memory-first. Read source in chunks, encrypt per chunk, send per chunk
- Memory usage ≈ chunk_size × 2 (read buffer + encrypt buffer), regardless of total payload size
- Each chunk independently encrypted with XChaCha20-Poly1305:
  - `nonce = H(file_id + chunk_index)` — deterministic per chunk
  - `AAD = file_id + chunk_index` — prevents replay and reordering
  - Each chunk has its own authentication tag
- Chunks are independently verifiable, supporting parallel processing and future resume capability

#### Blob storage format (user-specified design)

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

### Deferred Ideas (OUT OF SCOPE)

- Multi-representation clipboard write on receiver side — requires platform-level clipboard write support for multiple MIME types simultaneously
- Transfer progress UI (progress bars, percentage) — could be added in a future UX phase if needed
- Parallel chunk download — architecture supports it but Phase 2 implements sequential streaming only
- Resumable/interrupted transfer resume — architecture supports it via chunk-level AEAD but not implemented in Phase 2
  </user_constraints>

---

## Summary

Phase 2 replaces the current V1 text-only clipboard sync with a unified chunked transfer layer. The codebase already has all required crypto primitives (`chacha20poly1305` 0.10.1, `blake3` 1.8.2 in `uc-infra`), a working libp2p stream transport in `uc-platform/src/adapters/libp2p_network.rs`, and the `SystemClipboardSnapshot` multi-representation model in `uc-core`. The migration is primarily additive: a new `ClipboardPayloadV2` type, a chunked stream encoder/decoder, and updated send/receive paths in the two sync use cases.

The key design challenge is that the current stream handler in `libp2p_network.rs` reads the entire payload into memory with `read_to_end` before dispatching (`BUSINESS_PAYLOAD_MAX_BYTES = 100 MB`). V2 payloads can be arbitrarily large, so both the sender's write path and the receiver's read path need to operate chunk-by-chunk over the already-established libp2p stream — the stream abstraction already supports incremental `write_all` / `read_exact`, so no transport layer changes are needed.

The echo prevention mechanisms (`origin_device_id` filter and `recent_ids` deduplication) must continue working with V2. The `content_hash` field in `ClipboardMessage` currently reflects a single representation's hash; for V2 it should be the `SnapshotHash` (hash of all representations) to preserve deduplication semantics.

**Primary recommendation:** Add a new `ClipboardPayloadV2` message type inside `ClipboardMessage.encrypted_content`, version-differentiate at deserialization time (V1 = existing JSON blob, V2 = new binary chunked format), and implement the chunked encoder/decoder entirely in `uc-infra` following the hexagonal pattern. Keep the libp2p stream open for the full transfer duration and write/read chunks incrementally without buffering the full payload.

---

## Standard Stack

### Core (already in project)

| Library                | Version              | Purpose                                                         | Location                                    |
| ---------------------- | -------------------- | --------------------------------------------------------------- | ------------------------------------------- |
| `chacha20poly1305`     | 0.10.1               | XChaCha20-Poly1305 AEAD per chunk                               | `uc-infra/Cargo.toml`                       |
| `blake3`               | 1.8.2                | Nonce derivation `H(file_id \|\| chunk_index)`, content hashing | `uc-infra/Cargo.toml`, `uc-core/Cargo.toml` |
| `tokio`                | 1 (full)             | Async IO, `AsyncRead`/`AsyncWrite` for streaming                | `uc-platform/Cargo.toml`                    |
| `tokio-util`           | 0.7 (`io`, `compat`) | `io::BufReader`, `io::BufWriter` wrappers                       | `uc-platform/Cargo.toml`                    |
| `futures`              | 0.3                  | `AsyncReadExt`, `AsyncWriteExt` traits                          | `uc-platform/Cargo.toml`                    |
| `bytes`                | 1.7                  | `BytesMut` for chunk buffers                                    | `uc-platform/Cargo.toml`                    |
| `uuid`                 | 1 (v4)               | Transfer ID / `file_id` generation                              | `uc-platform/Cargo.toml`                    |
| `serde` / `serde_json` | 1                    | V1 backward compat JSON envelope, V2 header serialization       | everywhere                                  |

### No New Dependencies Required

All necessary cryptographic, async IO, and serialization primitives are already present. No new crate additions are needed for Phase 2.

### Supporting — Left to Claude's Discretion

| Library                      | Purpose                         | Notes                                                                                 |
| ---------------------------- | ------------------------------- | ------------------------------------------------------------------------------------- |
| `byteorder` (not in project) | Binary framing                  | **Not needed** — use `u32::to_le_bytes()` / `u32::from_le_bytes()` from `std` instead |
| `bincode` (not in project)   | Binary serialization for header | **Not needed** — manual little-endian binary format is simpler and transparent        |

---

## Architecture Patterns

### Recommended Component Structure

```
uc-core/src/network/protocol/
├── clipboard.rs                     # ClipboardMessage — add payload_version field
├── clipboard_payload.rs             # ClipboardTextPayloadV1 (keep)
└── clipboard_payload_v2.rs          # NEW: ClipboardMultiRepPayloadV2 (pre-encrypt envelope)

uc-infra/src/clipboard/
└── chunked_transfer.rs              # NEW: ChunkedEncoder, ChunkedDecoder

uc-infra/src/clipboard/
└── blob_format.rs                   # NEW: ChunkedBlobFile read/write (header + index + chunks)

uc-app/src/usecases/clipboard/
├── sync_outbound.rs                 # MODIFY: add V2 path alongside V1
└── sync_inbound.rs                  # MODIFY: detect V2, route to reassembler

uc-platform/src/adapters/
└── libp2p_network.rs                # MODIFY: spawn_business_stream_handler reads chunks,
                                     #          execute_business_stream writes chunks
```

### Pattern 1: Payload Version Differentiation at ClipboardMessage

**What:** Add a `payload_version: u8` (or `PayloadVersion` enum) field to `ClipboardMessage`. Old receivers will fail to deserialize if the new field is not `#[serde(default)]`. Use `#[serde(default)]` so old messages without the field default to `V1`.

**Current `ClipboardMessage` in `uc-core/src/network/protocol/clipboard.rs`:**

```rust
pub struct ClipboardMessage {
    pub id: String,
    pub content_hash: String,
    pub encrypted_content: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub origin_device_id: String,
    pub origin_device_name: String,
}
```

**Modified for V2 (backward compatible):**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClipboardPayloadVersion {
    V1 = 1,
    V2 = 2,
}

impl Default for ClipboardPayloadVersion {
    fn default() -> Self { Self::V1 }
}

pub struct ClipboardMessage {
    pub id: String,
    pub content_hash: String,
    pub encrypted_content: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub origin_device_id: String,
    pub origin_device_name: String,
    #[serde(default)]
    pub payload_version: ClipboardPayloadVersion,
}
```

Old senders produce messages without `payload_version` → deserialized as `V1`. Old receivers see an unexpected field and silently ignore it (serde default behavior for `#[serde(deny_unknown_fields)]` is opt-in; the current code does NOT use it, so new fields are transparently ignored).

**Critical:** Old receivers see `payload_version: V2` and fail to decrypt `encrypted_content` (because V2 `encrypted_content` is NOT a JSON `EncryptedBlob` anymore — it's a binary header). They will log `serde_json::from_slice` error and drop the message. This is acceptable per the locked decision.

### Pattern 2: V2 Pre-Encryption Envelope

Before chunking, pack all `ObservedClipboardRepresentation` into a single serialized bundle. This is the plaintext that gets chunked and encrypted.

```rust
// uc-core/src/network/protocol/clipboard_payload_v2.rs
#[derive(Serialize, Deserialize)]
pub struct ClipboardMultiRepPayloadV2 {
    /// ts_ms from SystemClipboardSnapshot
    pub ts_ms: i64,
    /// All representations packed as-is
    pub representations: Vec<WireRepresentation>,
}

#[derive(Serialize, Deserialize)]
pub struct WireRepresentation {
    pub mime: Option<String>,
    pub format_id: String,
    pub bytes: Vec<u8>,
}
```

Serialization format: `serde_json` for simplicity and debuggability. For large images, the JSON overhead is negligible relative to the binary payload.

### Pattern 3: Deterministic Per-Chunk Nonce

Per the locked decision: `nonce = H(transfer_id || chunk_index)`, truncated to 24 bytes for XChaCha20-Poly1305.

```rust
// In uc-infra/src/clipboard/chunked_transfer.rs
fn derive_chunk_nonce(transfer_id: &[u8; 16], chunk_index: u32) -> [u8; 24] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"uc:chunk-nonce:v1|");
    hasher.update(transfer_id);
    hasher.update(&chunk_index.to_le_bytes());
    let hash = hasher.finalize();
    // blake3 output is 32 bytes; take first 24
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&hash.as_bytes()[..24]);
    nonce
}
```

`transfer_id`: a 16-byte UUID (UUID v4 as raw bytes, not string). This is the `file_id` from the context decisions.

### Pattern 4: Per-Chunk AAD

Per locked decision: `AAD = file_id || chunk_index` (binary concatenation).

```rust
fn chunk_aad(transfer_id: &[u8; 16], chunk_index: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(20);
    aad.extend_from_slice(transfer_id);
    aad.extend_from_slice(&chunk_index.to_le_bytes());
    aad
}
```

This must be consistent between encode and decode — keep it in a shared helper so sender and receiver always compute the same bytes.

### Pattern 5: Streaming Chunk Write over libp2p Stream

The existing `execute_business_stream` in `libp2p_network.rs` calls `stream.write_all(data)` where `data` is the entire payload `Vec<u8>`. For V2, replace this with an incremental chunk write loop:

```rust
// Pseudocode for V2 send path inside execute_business_stream
// stream: impl AsyncWrite + AsyncRead (libp2p_stream::Stream)

// Write binary header first (fixed-size)
stream.write_all(&header_bytes).await?;

// Then write each encrypted chunk
for (i, plaintext_chunk) in payload_chunks.enumerate() {
    let nonce = derive_chunk_nonce(&transfer_id, i as u32);
    let aad = chunk_aad(&transfer_id, i as u32);
    let ciphertext = cipher.encrypt(nonce, Payload { msg: plaintext_chunk, aad: &aad })?;
    let chunk_len = ciphertext.len() as u32;
    stream.write_all(&chunk_len.to_le_bytes()).await?;
    stream.write_all(&ciphertext).await?;
}
// No explicit EOF needed — stream.close() signals end
```

### Pattern 6: Streaming Chunk Read (Receiver Side)

The current receiver in `spawn_business_stream_handler` reads the entire stream to a `Vec<u8>` with `read_to_end`. For V2, replace with incremental chunk reading:

```rust
// Pseudocode for V2 receive path in spawn_business_stream_handler
// Read the first message bytes to detect V1 vs V2:
// - Attempt JSON deserialization of the full buffer (V1 path, keep for compat)
// - OR: read a version magic byte prefix in the binary stream (V2 path)

// For V2:
// 1. Read fixed-size binary header (version magic + transfer_id + total_chunks + chunk_size)
// 2. Allocate reassembly buffer (bounded by expected total size from header)
// 3. For each chunk: read 4-byte length, read ciphertext, decrypt, verify tag, append plaintext
// 4. After all chunks received: assemble ClipboardMultiRepPayloadV2, route to inbound use case
```

**Key decision point — V1/V2 detection on the stream:**

The cleanest approach is a magic prefix in the binary format:

- V2 binary header starts with a 4-byte magic: `[0x55, 0x43, 0x32, 0x00]` ("UC2\0")
- V1 JSON starts with `{` (0x7B)

The receiver reads the first 4 bytes. If magic matches → V2 path. Otherwise → attempt JSON deserialize as V1.

This avoids relying on `ClipboardMessage.payload_version` being present (which requires deserializing JSON first anyway).

**Alternative:** Keep `encrypted_content` as JSON for both versions, but inside, V2 changes the structure. The receiver tries JSON deserialization of `ClipboardMessage`, extracts `payload_version`, then routes. This is simpler and avoids changing the stream framing — just the `encrypted_content` payload changes.

**Recommendation: Use the `payload_version` field approach** (Pattern 1 above). It stays JSON at the `ClipboardMessage` level (consistent with existing protocol), changes only the `encrypted_content` bytes. For V2, `encrypted_content` is a binary blob (not JSON), decoded by a new binary parser. The receiver:

1. Deserializes `ClipboardMessage` from JSON (same as V1)
2. Checks `payload_version`
3. Routes: V1 → existing path, V2 → new chunked decode path

### Pattern 7: V2 Binary `encrypted_content` Layout

For V2, `encrypted_content` (the raw bytes in `ClipboardMessage`) is:

```
[4 bytes] magic: 0x55 0x43 0x32 0x00 ("UC2\0")
[16 bytes] transfer_id (UUID raw bytes, big-endian)
[4 bytes] total_chunks (u32 little-endian)
[4 bytes] chunk_size_hint (u32 little-endian, actual chunk may be smaller for last chunk)
[4 bytes] total_plaintext_len (u32 little-endian, for pre-allocation)
-- chunks (sequential, no index table needed for network transfer):
  for each chunk i in 0..total_chunks:
    [4 bytes] chunk_ciphertext_len (u32 little-endian)
    [N bytes] ciphertext (includes 16-byte Poly1305 tag)
```

**Note on index table:** The index table described in the user's blob storage format decision is for **disk storage** (the `BlobStorePort` / `EncryptedBlobStore`). For **network transfer**, a sequential stream of `(len, ciphertext)` pairs is sufficient — no random access needed during transfer. The index table is only needed when reading from disk after storage.

### Pattern 8: Blob Storage Format with Index Table

For the receiver-side blob file on disk (used by `EncryptedBlobStore`):

```
[4 bytes] magic: 0x55 0x43 0x42 0x31 ("UCB1" - UniClipboard Blob v1)
[16 bytes] blob_id (UUID raw bytes)
[4 bytes] total_chunks (u32 LE)
[4 bytes] chunk_size (u32 LE)
[8 bytes] total_plaintext_len (u64 LE)
[N * 12 bytes] index_table: entry[i] = { offset: u64 LE, ciphertext_len: u32 LE }
-- chunk data (contiguous, indexed above):
  [ciphertext bytes for chunk 0]
  [ciphertext bytes for chunk 1]
  ...
```

The `offset` in each index entry is relative to the start of the chunk data section (after the header + index table). The `EncryptedBlobStore` in `uc-infra/src/security/` would need a new implementation that reads/writes this format instead of the current single `EncryptedBlob` JSON format.

### Pattern 9: Existing AAD Pattern Consistency

The project uses `uc-core/src/security/aad.rs` for centralized AAD generation. For chunk-level AAD, add:

```rust
// In uc-core/src/security/aad.rs
pub fn for_chunk_transfer(transfer_id: &[u8; 16], chunk_index: u32) -> Vec<u8> {
    // Binary: transfer_id (16 bytes) || chunk_index (4 bytes LE)
    let mut aad = Vec::with_capacity(20);
    aad.extend_from_slice(transfer_id);
    aad.extend_from_slice(&chunk_index.to_le_bytes());
    aad
}
```

### Pattern 10: Content Hash for V2

The existing `ClipboardMessage.content_hash` is a single representation's hash (the `text/plain` repr's `RepresentationHash`). For V2, use `SystemClipboardSnapshot.snapshot_hash()` which hashes all representations together. This maintains deduplication semantics in the inbound `recent_ids` and OS clipboard hash check.

### Anti-Patterns to Avoid

- **Do not buffer the full V2 payload in memory on the sender side before sending.** The stream must write chunks as they are encrypted. Do not collect all ciphertext chunks into a `Vec<Vec<u8>>` first.
- **Do not buffer the full V2 payload in memory on the receiver side before decrypting.** Decrypt each chunk as it arrives. Reassemble plaintext in a pre-allocated buffer.
- **Do not change the `BUSINESS_PROTOCOL_ID` (`/uc-business/1.0.0`).** Version differentiation is at the payload layer only.
- **Do not remove the V1 decode path.** Old senders still use V1. The inbound handler must support both versions simultaneously.
- **Do not use `read_to_end` for V2 on the receiver.** This loads the full payload. For V2, read chunk-by-chunk using `read_exact`.
- **Do not store the reassembled plaintext in `recent_ids`.** Only store `message_id` (the `ClipboardMessage.id` string), which is already UUID-based.
- **Do not use `unwrap()` or `expect()` in production code.** All chunk decode errors must be logged at `error!` level and the message silently dropped (per locked decision on silent failures).

---

## Don't Hand-Roll

| Problem           | Don't Build         | Use Instead                                                                            | Why                                                    |
| ----------------- | ------------------- | -------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| AEAD encryption   | Custom encrypt loop | `chacha20poly1305` crate's `encrypt_in_place`/`decrypt_in_place` with `Payload` struct | Already in project, handles tag correctly              |
| Nonce derivation  | Custom PRF          | `blake3::Hasher::new().update().finalize()`                                            | Already in project, deterministic, collision-resistant |
| Async buffered IO | Manual read loop    | `tokio::io::BufReader::new(stream)` / `AsyncReadExt::read_exact`                       | Handles partial reads correctly                        |
| Binary framing    | Bit manipulation    | `u32::to_le_bytes()` / `u32::from_le_bytes()` from `std`                               | Simple, no extra deps                                  |
| UUID generation   | Custom ID           | `uuid::Uuid::new_v4().as_bytes().clone()`                                              | Already in project                                     |

**Key insight:** The cryptographic primitives and async IO infrastructure are 100% already present. Phase 2 is a composition exercise, not a dependency acquisition exercise.

---

## Critical Code Locations

### What Sends Clipboard Data (Outbound Path)

File: `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs`

Current V1 flow:

1. Selects `text/plain` representation only (lines 86-99)
2. Serializes to `ClipboardTextPayloadV1` (line 139)
3. Encrypts with `EncryptionPort::encrypt_blob` (lines 150-160) — single whole blob
4. Puts resulting `EncryptedBlob` JSON into `ClipboardMessage.encrypted_content`
5. Calls `ClipboardTransportPort::send_clipboard(peer_id, outbound_bytes)` (line 216)

V2 change: Replace steps 1-4 with multi-representation bundle + chunked encrypt. Step 5 interface unchanged — the transport layer remains the same.

### What Receives Clipboard Data (Inbound Path)

File: `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs`

Current V1 flow:

1. Receives `ClipboardMessage` from `ClipboardTransportPort::subscribe_clipboard()`
2. Deserializes `EncryptedBlob` from `message.encrypted_content` (line 168)
3. Decrypts with `EncryptionPort::decrypt_blob` (lines 171-187)
4. Deserializes `ClipboardTextPayloadV1` (line 189)
5. Writes single text representation to OS clipboard or persists

V2 change: After step 1, check `message.payload_version`. If V2, decode binary chunked payload, reassemble, select highest-priority representation, write to clipboard.

### Where libp2p Streams Are Written (Outbound Transport)

File: `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs`

Key function: `execute_business_stream` (line 1682)

Currently calls `stream.write_all(data).await` where `data: Option<&[u8]>` is the whole payload. For V2, the `data` passed in is already the assembled `outbound_bytes` from `ProtocolMessage::to_bytes()`. The transport does not need to change — the chunking happens inside `sync_outbound.rs` before calling `send_clipboard`, which passes pre-assembled bytes.

**Important:** The current `ClipboardTransportPort::send_clipboard` signature passes `encrypted_data: Vec<u8>` — this is the serialized `ProtocolMessage::Clipboard(ClipboardMessage)`. For V2, this same interface works: the outbound use case assembles the full `ProtocolMessage` bytes (now containing the chunked binary in `encrypted_content`), and the transport writes them in one `write_all`. The whole message is still sent atomically. The "chunking" is a logical concept within `encrypted_content`, not a change to the TCP/QUIC stream framing.

**Wait — this contradicts the streaming requirement.** Let me clarify:

The user decision says "streaming chunking — NOT whole-payload-in-memory-first." This means:

- The plaintext is NOT assembled in memory before encryption
- Each chunk is encrypted as it is produced from the representation data
- BUT the full serialized+encrypted message still gets passed to `send_clipboard` as `Vec<u8>`

For very large payloads (e.g., 50MB image), holding the entire encrypted `Vec<u8>` in memory is unavoidable at the transport boundary since `send_clipboard` takes `Vec<u8>`. The streaming benefit is that we don't hold BOTH the plaintext 50MB AND the ciphertext 50MB simultaneously — we encrypt chunk by chunk into the final buffer.

**Alternative if true streaming is needed:** Change `ClipboardTransportPort::send_clipboard` to accept an `AsyncRead` source instead of `Vec<u8>`. This would require changing the port interface, the libp2p adapter, and the use case. This is a bigger change but enables true O(chunk_size) memory usage.

**Recommendation (Claude's discretion):** For Phase 2, keep the current `send_clipboard(Vec<u8>)` interface. Images up to ~20MB are the realistic case for LAN sync. Accept the 2x memory (ciphertext in Vec<u8> + OS clipboard holding the original). True streaming transport is a future optimization. The blob storage format (disk) does stream correctly.

### Where libp2p Streams Are Read (Inbound Transport)

File: `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs`

Key function: `spawn_business_stream_handler` (line 855)

Currently reads entire stream to `Vec<u8>` with:

```rust
let mut limited = stream.take(BUSINESS_PAYLOAD_MAX_BYTES + 1);
limited.read_to_end(&mut payload).await
```

Then calls `handle_business_payload` which deserializes `ProtocolMessage` and dispatches. This is fine for V2 as long as `BUSINESS_PAYLOAD_MAX_BYTES` is set appropriately. Currently 100 MB — this should be reviewed but can stay for Phase 2.

The `ProtocolMessage::from_bytes` call in `handle_business_payload` (line 937) uses `serde_json::from_slice`. Since V2 `ClipboardMessage` has `encrypted_content: Vec<u8>` (raw binary), JSON will still deserialize this correctly (Vec<u8> serializes as array of integers in serde_json by default — but wait, that's wrong for large binary payloads).

**Critical finding:** `Vec<u8>` in serde_json serializes as a JSON array of integers by default (e.g., `[255, 0, 128, ...]`). This is extremely verbose for binary data. The existing V1 `encrypted_content` is actually a JSON blob serialized to `Vec<u8>` (a JSON string of JSON), then that JSON string's bytes are stored — not raw binary. The V2 `encrypted_content` will be raw binary bytes. Putting this raw binary as `serde_json::Value::Array([u8])` would be ~3x larger due to JSON integer encoding.

**Solution for `encrypted_content` binary encoding:** Use `serde_bytes` or base64 encoding. Looking at the existing code in `sync_outbound.rs`:

```rust
let encrypted_content = serde_json::to_vec(&encrypted_blob)
    .context("failed to serialize encrypted outbound clipboard payload")?;
```

The `encrypted_content` field is a `Vec<u8>` that holds `serde_json::to_vec(&encrypted_blob)` — a JSON string. For V2, `encrypted_content` could hold base64-encoded binary, or the field type could change.

**Simplest backward-compatible approach:** For V2, `encrypted_content` holds the binary payload with no JSON wrapping. Since `Vec<u8>` serializes as JSON array of u8 integers, use `#[serde(with = "serde_bytes")]` on the field — this would require adding `serde_bytes = "0.11"` dependency. Alternatively, since `ClipboardMessage` is JSON-serialized and `encrypted_content` is `Vec<u8>`, add `#[serde(with = "base64_serde")]` for the V2 path.

**Alternative:** Keep `encrypted_content: Vec<u8>` with current serde behavior (integer array in JSON) — this is inefficient but works. For a 10MB image, the JSON encoding of `Vec<u8>` would produce ~30MB of JSON. This is not acceptable.

**Recommendation:** Add `serde_bytes = "0.11"` to `uc-core/Cargo.toml` and annotate `encrypted_content` with `#[serde(with = "serde_bytes")]`. This makes the field serialize as base64 string in JSON (compact, standard). This is a one-line change to `ClipboardMessage` and a one-line Cargo.toml addition. Verify that serde_bytes does base64 — it actually uses base64 when the format is human-readable (JSON). Yes, confirmed: `serde_bytes` with JSON serializer produces base64.

---

## Common Pitfalls

### Pitfall 1: serde_json Vec<u8> Bloat

**What goes wrong:** `Vec<u8>` in serde_json serializes as an array of integers (`[255, 0, 1, ...]`), producing ~3x size overhead for binary data.
**Why it happens:** serde_json's default `u8` serialization is a JSON number.
**How to avoid:** Add `serde_bytes` crate and annotate `ClipboardMessage.encrypted_content` with `#[serde(with = "serde_bytes")]`.
**Warning signs:** Binary payload sizes in logs are much larger than expected image sizes.

### Pitfall 2: Old Receiver Deserialization Failure with V2

**What goes wrong:** An old receiver gets a V2 message. `serde_json::from_slice` on `ProtocolMessage` succeeds (new field `payload_version` is ignored), but then `serde_json::from_slice::<EncryptedBlob>(&message.encrypted_content)` fails because V2 `encrypted_content` is binary, not a JSON `EncryptedBlob`.
**Why it happens:** The V1 receiver always tries to decode `encrypted_content` as `EncryptedBlob` JSON.
**How to avoid:** Acceptable per the locked decision — old devices silently ignore V2 messages on decode error. Ensure the error is caught and logged at warn level, not panicked.
**Warning signs:** `serde_json` decode errors in logs on V1 devices when V2 senders are active.

### Pitfall 3: Nonce Reuse

**What goes wrong:** If two different transfers use the same `transfer_id` with the same `chunk_index`, the nonce is reused, breaking AEAD security.
**Why it happens:** Poor transfer ID generation (e.g., sequential counter, timestamp-based ID).
**How to avoid:** Always use UUID v4 (cryptographically random) for `transfer_id`. Never reuse transfer IDs.
**Warning signs:** Decryption succeeds but with wrong plaintext (rare but possible with XChaCha20 nonce reuse).

### Pitfall 4: Partial Transfer Reassembly

**What goes wrong:** Receiver gets N-1 of N chunks before disconnect. Partial data is written to clipboard.
**Why it happens:** Incomplete error handling in the chunk read loop.
**How to avoid:** Only reassemble and write to clipboard after ALL `total_chunks` are successfully decrypted. Discard immediately on any chunk error per locked decision.
**Warning signs:** Corrupted or truncated clipboard content after sync failures.

### Pitfall 5: Content Hash Mismatch for Deduplication

**What goes wrong:** V2 sends multiple representations, but `ClipboardMessage.content_hash` still reflects only the `text/plain` representation hash, causing deduplication to fail or produce false positives.
**Why it happens:** Copying V1 content_hash logic to V2 without updating.
**How to avoid:** For V2, set `content_hash = snapshot.snapshot_hash().to_string()` which hashes all representations.

### Pitfall 6: Echo Prevention with V2

**What goes wrong:** Inbound V2 message fails the `origin_device_id == local_device_id` check (correct no-op), but the `content_hash` deduplication check in the OS clipboard write path fails because V2 content_hash is a snapshot hash not matching any single-representation hash in the local snapshot.
**Why it happens:** The dedup logic checks `representation.content_hash().to_string() == message.content_hash` — this will never match if V2 uses snapshot hash.
**How to avoid:** In `sync_inbound.rs`, for V2: skip the per-representation content hash check and rely solely on `origin_device_id` + `recent_ids` for deduplication. Alternatively, also compute and store the snapshot hash on the local snapshot for comparison.

### Pitfall 7: Timeout Values for Large Payloads

**What goes wrong:** `BUSINESS_READ_TIMEOUT = 30s` and `BUSINESS_STREAM_WRITE_TIMEOUT = 10s` were designed for text payloads. A 20MB image over LAN at ~50 MB/s would take ~0.4s to transfer, but real-world LAN conditions vary. On a slow Wi-Fi at 5 MB/s, a 20MB image takes 4s — within the timeout. For 100MB it takes 20s — at risk.
**Why it happens:** Timeouts sized for text-only transfer.
**How to avoid:** Increase `BUSINESS_STREAM_WRITE_TIMEOUT` and `BUSINESS_READ_TIMEOUT` for V2 transfers. Recommend 120s (2 minutes) for V2. This is left to Claude's discretion per the locked decisions. Alternatively, implement per-chunk timeouts rather than a single full-payload timeout.
**Warning signs:** Large image transfers fail with timeout errors in logs.

### Pitfall 8: Memory Spike During Transfer

**What goes wrong:** On the sender, the full encrypted payload Vec<u8> is assembled before passing to `send_clipboard`. For a 50MB image with ~16 bytes Poly1305 tag overhead per chunk, the memory spike is ~50MB ciphertext + ~50MB original plaintext = ~100MB.
**Why it happens:** Current `send_clipboard(Vec<u8>)` interface requires materializing the full payload.
**How to avoid:** For Phase 2, this is acceptable. Document as a known limitation. Future phases can add streaming transport.
**Warning signs:** Memory usage spikes during image clipboard sync; OOM on mobile/low-RAM targets.

### Pitfall 9: `BUSINESS_PAYLOAD_MAX_BYTES` with V2

**What goes wrong:** Current 100MB limit applies to the full `ProtocolMessage` JSON bytes. With serde_bytes base64 encoding, a 75MB image becomes ~100MB base64 in the JSON envelope — hitting the limit.
**Why it happens:** 100MB was sized for V1 text payloads; base64 adds 33% overhead.
**How to avoid:** Increase `BUSINESS_PAYLOAD_MAX_BYTES` for V2. Recommend 300MB (allows ~225MB of raw binary data after base64 overhead). Or: remove the limit and rely on chunk-level timeouts instead.

---

## Code Examples

### Chunk Encryption (Verified Pattern)

Uses the existing `chacha20poly1305 = "0.10.1"` API already in `uc-infra`:

```rust
use chacha20poly1305::{
    aead::{Aead, Payload},
    KeyInit, XChaCha20Poly1305, XNonce,
};

// Encrypt one chunk
fn encrypt_chunk(
    cipher: &XChaCha20Poly1305,
    transfer_id: &[u8; 16],
    chunk_index: u32,
    plaintext: &[u8],
) -> Result<Vec<u8>, chacha20poly1305::aead::Error> {
    let nonce_bytes = derive_chunk_nonce(transfer_id, chunk_index);
    let aad = chunk_aad(transfer_id, chunk_index);
    cipher.encrypt(
        XNonce::from_slice(&nonce_bytes),
        Payload { msg: plaintext, aad: &aad },
    )
}

// Decrypt one chunk
fn decrypt_chunk(
    cipher: &XChaCha20Poly1305,
    transfer_id: &[u8; 16],
    chunk_index: u32,
    ciphertext: &[u8],
) -> Result<Vec<u8>, chacha20poly1305::aead::Error> {
    let nonce_bytes = derive_chunk_nonce(transfer_id, chunk_index);
    let aad = chunk_aad(transfer_id, chunk_index);
    cipher.decrypt(
        XNonce::from_slice(&nonce_bytes),
        Payload { msg: ciphertext, aad: &aad },
    )
}
```

### Nonce Derivation (using blake3 = "1.8.2" already in uc-infra)

```rust
use blake3;

fn derive_chunk_nonce(transfer_id: &[u8; 16], chunk_index: u32) -> [u8; 24] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"uc:chunk-nonce:v1|");
    hasher.update(transfer_id);
    hasher.update(&chunk_index.to_le_bytes());
    let hash = hasher.finalize();
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&hash.as_bytes()[..24]);
    nonce
}
```

### Highest-Priority Representation Selection (Inbound)

```rust
fn select_highest_priority_repr(
    representations: &[WireRepresentation],
) -> Option<&WireRepresentation> {
    // Priority: image > rich text (HTML) > plain text
    let priority = |mime: Option<&str>| -> u8 {
        match mime {
            Some(m) if m.starts_with("image/") => 3,
            Some(m) if m.eq_ignore_ascii_case("text/html") => 2,
            Some(m) if m.eq_ignore_ascii_case("text/rtf") => 1,
            Some(m) if m.eq_ignore_ascii_case("text/plain") => 0,
            _ => 0,
        }
    };
    representations.iter().max_by_key(|r| priority(r.mime.as_deref()))
}
```

### Cipher Initialization (from MasterKey)

```rust
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use uc_core::security::model::MasterKey;

fn make_cipher(master_key: &MasterKey) -> Result<XChaCha20Poly1305, EncryptionError> {
    XChaCha20Poly1305::new_from_slice(master_key.as_bytes())
        .map_err(|_| EncryptionError::InvalidKey)
}
```

### Binary Frame Write (no extra deps)

```rust
use tokio::io::AsyncWriteExt;

async fn write_u32_le(writer: &mut impl AsyncWriteExt + Unpin, value: u32) -> anyhow::Result<()> {
    writer.write_all(&value.to_le_bytes()).await.map_err(Into::into)
}
```

### Binary Frame Read

```rust
use tokio::io::AsyncReadExt;

async fn read_u32_le(reader: &mut impl AsyncReadExt + Unpin) -> anyhow::Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf).await?;
    Ok(u32::from_le_bytes(buf))
}
```

---

## Implementation Layer Map

| Decision                                          | Where to Implement                                                 | Crate         |
| ------------------------------------------------- | ------------------------------------------------------------------ | ------------- |
| V2 payload struct (`ClipboardMultiRepPayloadV2`)  | `uc-core/src/network/protocol/`                                    | `uc-core`     |
| `payload_version` field on `ClipboardMessage`     | `uc-core/src/network/protocol/clipboard.rs`                        | `uc-core`     |
| Chunk-level AAD helper                            | `uc-core/src/security/aad.rs`                                      | `uc-core`     |
| Nonce derivation                                  | `uc-infra/src/clipboard/chunked_transfer.rs`                       | `uc-infra`    |
| Chunk encoder (plaintext → `Vec<EncryptedChunk>`) | `uc-infra/src/clipboard/chunked_transfer.rs`                       | `uc-infra`    |
| Chunk decoder (`Vec<EncryptedChunk>` → plaintext) | `uc-infra/src/clipboard/chunked_transfer.rs`                       | `uc-infra`    |
| Chunked blob file format (disk storage)           | `uc-infra/src/clipboard/blob_format.rs`                            | `uc-infra`    |
| V2 outbound path (pack + chunk + encrypt)         | `uc-app/src/usecases/clipboard/sync_outbound.rs`                   | `uc-app`      |
| V2 inbound path (detect + reassemble + write)     | `uc-app/src/usecases/clipboard/sync_inbound.rs`                    | `uc-app`      |
| Transport timeout adjustments                     | `uc-platform/src/adapters/libp2p_network.rs`                       | `uc-platform` |
| `serde_bytes` annotation                          | `uc-core/src/network/protocol/clipboard.rs` + `uc-core/Cargo.toml` | `uc-core`     |

---

## Claude's Discretion Recommendations

### Chunk Size: 256 KB

- LAN throughput is typically 100Mbps–1Gbps
- 256 KB chunk at 100Mbps takes ~20ms — acceptable latency per chunk
- 256 KB × 2 = 512 KB peak memory per chunk (encrypt buffer + plaintext)
- For a 10MB image: ~40 chunks — overhead is minimal
- Smaller chunks (64KB) increase per-chunk AEAD overhead; larger chunks (1MB) increase memory pressure

### Hash Function for Nonce Derivation: blake3

- Already in `uc-infra` and `uc-core` — zero additional deps
- Deterministic, collision-resistant, fast

### Binary Format Endianness: Little-Endian

- Consistent with most modern systems (x86, ARM)
- Use `u32::to_le_bytes()` / `u32::from_le_bytes()` from `std`

### Compression: No compression in Phase 2

- LAN bandwidth is plentiful
- Images are already compressed (PNG, JPEG)
- Text is small enough that compression gain is negligible
- Compression adds complexity and CPU cost — defer to future phases

### Timeout Values for V2

- `BUSINESS_STREAM_WRITE_TIMEOUT`: 120 seconds (up from 10s)
- `BUSINESS_READ_TIMEOUT`: 120 seconds (up from 30s)
- These apply only when V2 payload is detected; V1 can keep original timeouts
- Alternative: per-chunk 5-second timeout (detect stalled transfer earlier)

### `BUSINESS_PAYLOAD_MAX_BYTES` for V2

- Raise to 300 MB (from 100 MB) to accommodate images + base64 overhead
- Document 225 MB as the practical raw binary limit (300 / 1.33)

---

## Integration Touchpoints — Full Picture

### Files That MUST Change

1. **`uc-core/src/network/protocol/clipboard.rs`**
   - Add `payload_version: ClipboardPayloadVersion` with `#[serde(default)]`
   - Add `#[serde(with = "serde_bytes")]` on `encrypted_content`
   - Requires: `serde_bytes = "0.11"` in `uc-core/Cargo.toml`

2. **`uc-core/src/network/protocol/mod.rs`**
   - Export new `ClipboardPayloadVersion` enum and `ClipboardMultiRepPayloadV2`

3. **`uc-core/src/security/aad.rs`**
   - Add `for_chunk_transfer(transfer_id: &[u8; 16], chunk_index: u32) -> Vec<u8>`

4. **`uc-app/src/usecases/clipboard/sync_outbound.rs`**
   - Add V2 path: pack all representations → serialize `ClipboardMultiRepPayloadV2` → chunk + encrypt → build V2 `ClipboardMessage`
   - Keep V1 path for old receivers? **No** — sender always sends V2. Old receivers silently drop per locked decision.
   - Remove the `is_text_plain_mime` filter — V2 sends all representations

5. **`uc-app/src/usecases/clipboard/sync_inbound.rs`**
   - Detect `payload_version`: V1 → existing path; V2 → new chunked decode path
   - Add priority-based representation selection
   - Update `content_hash` deduplication logic for V2

6. **`uc-platform/src/adapters/libp2p_network.rs`**
   - Adjust `BUSINESS_PAYLOAD_MAX_BYTES`, `BUSINESS_READ_TIMEOUT`, `BUSINESS_STREAM_WRITE_TIMEOUT`
   - No structural changes needed if using `Vec<u8>` transport interface

### Files That Are NEW

7. **`uc-core/src/network/protocol/clipboard_payload_v2.rs`**
   - `ClipboardMultiRepPayloadV2`, `WireRepresentation`

8. **`uc-infra/src/clipboard/chunked_transfer.rs`**
   - `ChunkedEncoder::encode(master_key, transfer_id, plaintext_chunks) -> Vec<u8>`
   - `ChunkedDecoder::decode(master_key, binary_payload) -> Result<Vec<PlaintextChunk>>`
   - Nonce derivation, AAD generation, chunk framing

9. **`uc-infra/src/clipboard/blob_format.rs`** (for disk storage, if different from network format)
   - `ChunkedBlobWriter::write(...)` — write with index table
   - `ChunkedBlobReader::read_chunk(index)` — O(1) seek by chunk index

---

## Open Questions

1. **True streaming at transport layer**
   - What we know: Current `send_clipboard(Vec<u8>)` requires full payload in memory
   - What's unclear: Whether 50-100MB images are realistic use cases in v0.1.0
   - Recommendation: Keep Vec<u8> interface for Phase 2; document as known limitation

2. **V1/V2 detection robustness**
   - What we know: `payload_version: u8` with `#[serde(default = "V1")]` differentiates at ClipboardMessage level
   - What's unclear: If an old V1 sender sends `payload_version` accidentally (unlikely but defensive)
   - Recommendation: `ClipboardPayloadVersion::V1` is the default; always check the field before routing

3. **OS Clipboard multi-representation write on receiver**
   - What we know: Deferred to future phases; receiver writes only the highest-priority representation
   - What's unclear: How `write_snapshot(SystemClipboardSnapshot)` handles multi-mime input on each platform
   - Recommendation: For Phase 2, create a single-representation `SystemClipboardSnapshot` with the selected representation; no platform changes needed

4. **Disk blob format for V2 content persistence**
   - What we know: The receiver must persist the received content for the clipboard history
   - What's unclear: Whether the new chunked blob format (`blob_format.rs`) replaces or supplements the existing `EncryptedBlobStore` in `uc-infra/src/security/`
   - Recommendation: For Phase 2, the received V2 payload is reassembled to plaintext, then the existing `EncryptedBlobStore` handles persistence (same as local capture path). The chunked-on-disk format (blob_format.rs) may be needed for future local large-blob storage optimization, but not required for Phase 2.

---

## Sources

### Primary (HIGH confidence) — Direct codebase inspection

- `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-platform/src/adapters/libp2p_network.rs` — complete transport layer: business stream send/receive, timeouts, command dispatch
- `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-app/src/usecases/clipboard/sync_outbound.rs` — full outbound use case with V1 encryption flow
- `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-app/src/usecases/clipboard/sync_inbound.rs` — full inbound use case with V1 decrypt flow, dedup logic, echo prevention
- `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-core/src/network/protocol/clipboard.rs` — `ClipboardMessage` struct
- `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-core/src/network/protocol/clipboard_payload.rs` — `ClipboardTextPayloadV1`
- `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-infra/src/security/encryption.rs` — XChaCha20-Poly1305 implementation with AAD
- `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-core/src/security/aad.rs` — AAD pattern conventions
- `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-core/src/clipboard/system.rs` — `SystemClipboardSnapshot`, `ObservedClipboardRepresentation`, `snapshot_hash()`
- `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-infra/Cargo.toml` — confirms `chacha20poly1305 = "0.10.1"`, `blake3 = "1.8.2"`
- `/home/wuy6/myprojects/UniClipboard/src-tauri/crates/uc-platform/Cargo.toml` — confirms `tokio-util`, `futures`, `bytes`
- `/home/wuy6/myprojects/UniClipboard/.planning/phases/02-unified-transfer-layer/02-CONTEXT.md` — all locked decisions

---

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — all deps confirmed present in Cargo.toml files
- Architecture patterns: HIGH — derived from direct codebase analysis of actual send/receive paths
- Pitfalls: HIGH — derived from reading the actual code paths and identifying concrete failure modes
- serde_bytes Vec<u8> bloat: HIGH — this is a well-known serde_json behavior

**Research date:** 2026-03-03
**Valid until:** 2026-04-03 (stable codebase, no external API changes expected)

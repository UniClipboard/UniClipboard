# Phase 8: Optimize large image sync pipeline - Research

**Researched:** 2026-03-05
**Domain:** Binary wire protocol, zstd compression, zero-copy fanout (Rust async, Tauri/libp2p)
**Confidence:** HIGH

## Summary

This phase replaces the V2 JSON+base64 clipboard sync protocol with a V3 binary protocol, adds zstd compression before encryption, and eliminates per-peer memory copies via `Arc<[u8]>`. The codebase already has all prerequisite infrastructure: zstd 0.13 in uc-infra, XChaCha20-Poly1305 chunked AEAD in `chunked_transfer.rs`, and a well-tested two-segment wire framing system in `ProtocolMessage`.

The changes are entirely in Rust backend code across four crates (uc-core, uc-infra, uc-app, uc-platform). No frontend changes are required. The existing V2 chunked encoder/decoder pattern provides a direct template for the V3 implementation. The main complexity lies in correctly ordering the pipeline (binary encode -> compress -> encrypt -> chunk), updating port trait signatures (`Vec<u8>` to `Arc<[u8]>`), removing all V1/V2 code paths, and adding tokio::join! parallelization for the first peer's prepare + ensure_business_path.

**Primary recommendation:** Implement in 3 waves: (1) V3 binary codec + compression in chunked_transfer, (2) port signature changes + outbound/inbound use case updates + V1/V2 removal, (3) parallelization + observability spans.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Compression lives INSIDE TransferPayloadEncryptorPort/DecryptorPort implementation (chunked_transfer.rs)
- sync_outbound and sync_inbound are unaware of compression -- port signature unchanged
- Pipeline: plaintext -> zstd compress (inside port) -> chunked encrypt -> wire bytes
- Decode pipeline reverses: wire bytes -> chunked decrypt -> zstd decompress -> plaintext
- Compression flag stored in V3 wire frame header (compression_algo + uncompressed_len fields)
- Hard-coded threshold: skip compression for payloads <= 8KB
- Hard-coded zstd level 3
- V3 wire format with magic "UC3\0" (37-byte header)
- V3 payload binary codec: length-prefixed binary encoding (no serde, pure std::io)
- Located in uc-core/network/protocol/
- ClipboardTransportPort::send_clipboard: Vec<u8> -> Arc<[u8]>
- ClipboardTransportPort::broadcast_clipboard: Vec<u8> -> Arc<[u8]>
- DELETE all V1 and V2 encoding AND decoding paths
- Parallel: V3 encode+compress+encrypt runs concurrently with first peer's ensure_business_path (tokio::join!)
- After both complete: send to first peer, then serial loop for remaining peers
- Arc<[u8]> enables zero-copy for the serial fanout loop
- V3 inbound decodes all reps, only highest-priority persisted
- Tracing spans for outbound.prepare, outbound.send (per peer), inbound.decode
- Unit tests: codec round-trip, compression on/off, large payload boundary
- Integration tests: single-peer large image e2e, encryption session not-ready regression

### Claude's Discretion

- Exact V3 payload struct naming (ClipboardPayloadV3, ClipboardBinaryPayload, etc.)
- Chunk nonce derivation update for V3 (whether to change AAD prefix from "uc:chunk-nonce:v1|" to "v3|")
- Error variant additions to ChunkedTransferError for V3-specific failures
- Whether to keep WireRepresentation struct name or rename for V3 binary context
- Exact TODO comment wording

### Deferred Ideas (OUT OF SCOPE)

- Multi-rep persistence (receiver stores all reps)
- Configurable compression parameters
- Streaming V3 encode (direct write to libp2p stream)
- Network layer observability (fine-grained libp2p spans)
  </user_constraints>

## Standard Stack

### Core

| Library          | Version    | Purpose                       | Why Standard                                                                  |
| ---------------- | ---------- | ----------------------------- | ----------------------------------------------------------------------------- |
| zstd             | 0.13       | Compression before encryption | Already in uc-infra Cargo.toml; Phase 4 blob storage uses same lib at level 3 |
| chacha20poly1305 | (existing) | AEAD per-chunk encryption     | Already used in chunked_transfer.rs                                           |
| blake3           | (existing) | Chunk nonce derivation        | Already used in derive_chunk_nonce                                            |
| uuid             | (existing) | Transfer ID generation        | Already used for transfer_id in ChunkedEncoder                                |
| tokio            | (existing) | Async runtime, join! macro    | Already the project async runtime                                             |

### Supporting

| Library                | Version | Purpose                        | When to Use                                          |
| ---------------------- | ------- | ------------------------------ | ---------------------------------------------------- |
| std::io::{Read, Write} | std     | V3 binary codec serialization  | For payload encode/decode without serde              |
| std::sync::Arc         | std     | Zero-copy fanout via Arc<[u8]> | For shared ownership of encrypted bytes across peers |

### Alternatives Considered

| Instead of     | Could Use        | Tradeoff                                                                            |
| -------------- | ---------------- | ----------------------------------------------------------------------------------- |
| zstd           | lz4              | lz4 faster but worse ratio; zstd already in project at level 3                      |
| std::io binary | bincode/postcard | Extra dependency; CONTEXT.md explicitly says "no external serialization dependency" |
| Arc<[u8]>      | Bytes crate      | Extra dependency; Arc<[u8]> is sufficient for this use case                         |

**No new dependencies required.** All libraries are already in the project.

## Architecture Patterns

### Recommended Project Structure

```
src-tauri/crates/
├── uc-core/src/network/protocol/
│   ├── clipboard_payload_v3.rs    # NEW: V3 binary payload codec (encode/decode)
│   ├── clipboard.rs               # MODIFY: Add V3 variant to ClipboardPayloadVersion
│   ├── clipboard_payload.rs       # DELETE: V1 payload (ClipboardTextPayloadV1)
│   ├── clipboard_payload_v2.rs    # DELETE: V2 payload (ClipboardMultiRepPayloadV2)
│   └── mod.rs                     # MODIFY: Update re-exports
├── uc-core/src/security/aad.rs    # MODIFY: Optionally update chunk transfer AAD
├── uc-core/src/ports/
│   ├── clipboard_transport.rs     # MODIFY: Vec<u8> -> Arc<[u8]> on send/broadcast
│   └── security/transfer_crypto.rs # NO CHANGE (compression is internal)
├── uc-infra/src/clipboard/
│   ├── chunked_transfer.rs        # MAJOR REWRITE: V3 header, compression layer
│   └── mod.rs                     # MODIFY: Update exports
├── uc-app/src/usecases/clipboard/
│   ├── sync_outbound.rs           # MODIFY: V3 encode, Arc<[u8]>, tokio::join!
│   └── sync_inbound.rs            # SIMPLIFY: Remove V1/V2 paths, V3 binary decode
└── uc-platform/src/adapters/
    └── libp2p_network.rs          # MODIFY: Arc<[u8]> in BusinessCommand::SendClipboard
```

### Pattern 1: V3 Wire Format (Inside Chunked Transfer)

**What:** The V3 encoder compresses plaintext with zstd (if > 8KB), then chunks and encrypts. The compression flag and uncompressed length are stored in the 37-byte header.
**When to use:** All clipboard network transfers.
**Example:**

```rust
// Inside TransferPayloadEncryptorAdapter::encrypt (chunked_transfer.rs)
// Port signature unchanged: fn encrypt(&self, master_key, plaintext) -> Result<Vec<u8>>
pub fn encrypt(&self, master_key: &MasterKey, plaintext: &[u8]) -> Result<Vec<u8>, TransferCryptoError> {
    let (compressed, compression_algo, uncompressed_len) = if plaintext.len() > COMPRESSION_THRESHOLD {
        let compressed = zstd::bulk::compress(plaintext, ZSTD_LEVEL)
            .map_err(|e| TransferCryptoError::EncryptionFailed(e.to_string()))?;
        (compressed, 1u8, plaintext.len() as u32)
    } else {
        // No compression -- pass plaintext through
        (plaintext.to_vec(), 0u8, plaintext.len() as u32)
    };

    let transfer_id = *Uuid::new_v4().as_bytes();
    let mut buf = Vec::new();
    ChunkedEncoderV3::encode_to(&mut buf, master_key, &transfer_id, &compressed, compression_algo, uncompressed_len)?;
    Ok(buf)
}
```

### Pattern 2: V3 Binary Payload Codec (In uc-core)

**What:** Pure std::io binary encoding without serde. Length-prefixed fields.
**When to use:** Encoding/decoding clipboard representations before compression.
**Example:**

```rust
// uc-core/src/network/protocol/clipboard_payload_v3.rs
use std::io::{Read, Write};

pub struct ClipboardBinaryPayload {
    pub ts_ms: i64,
    pub representations: Vec<BinaryWireRepresentation>,
}

pub struct BinaryWireRepresentation {
    pub format_id: String,
    pub mime: Option<String>,
    pub data: Vec<u8>,
}

impl ClipboardBinaryPayload {
    pub fn encode_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&self.ts_ms.to_le_bytes())?;
        writer.write_all(&(self.representations.len() as u16).to_le_bytes())?;
        for rep in &self.representations {
            let fid_bytes = rep.format_id.as_bytes();
            writer.write_all(&(fid_bytes.len() as u16).to_le_bytes())?;
            writer.write_all(fid_bytes)?;
            match &rep.mime {
                Some(m) => {
                    writer.write_all(&[1u8])?;
                    let m_bytes = m.as_bytes();
                    writer.write_all(&(m_bytes.len() as u16).to_le_bytes())?;
                    writer.write_all(m_bytes)?;
                }
                None => writer.write_all(&[0u8])?,
            }
            writer.write_all(&(rep.data.len() as u32).to_le_bytes())?;
            writer.write_all(&rep.data)?;
        }
        Ok(())
    }

    pub fn decode_from<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        // Symmetric decode -- read in same order
        // ...
    }
}
```

### Pattern 3: Arc<[u8]> Zero-Copy Fanout

**What:** Encrypt once, share immutable bytes across all peers.
**When to use:** In sync_outbound after encryption is complete.
**Example:**

```rust
// sync_outbound.rs -- after encryption
let outbound_bytes: Arc<[u8]> = Arc::from(framed_bytes.into_boxed_slice());

// First peer: parallel prepare + ensure_business_path
let (encrypt_result, ensure_result) = tokio::join!(
    prepare_outbound_bytes(&snapshot, &encryptor, &master_key),
    self.clipboard_network.ensure_business_path(&first_peer.peer_id)
);

// Send to first peer, then serial for remaining
self.clipboard_network.send_clipboard(&first_peer.peer_id, outbound_bytes.clone()).await?;
for peer in remaining_peers {
    self.clipboard_network.ensure_business_path(&peer.peer_id).await?;
    self.clipboard_network.send_clipboard(&peer.peer_id, outbound_bytes.clone()).await?;
}
```

### Anti-Patterns to Avoid

- **Compressing after encryption:** Encrypted data has high entropy and does not compress. Always compress plaintext BEFORE encryption.
- **Cloning Vec<u8> per peer:** With 10MB images and 3 peers, this wastes 30MB. Use Arc<[u8]> instead.
- **Keeping V1/V2 fallback paths "just in case":** CONTEXT.md explicitly says clean break -- remove all V1/V2 code.
- **Adding serde to V3 binary codec:** CONTEXT.md says "no external serialization dependency (pure std::io)".

## Don't Hand-Roll

| Problem                 | Don't Build             | Use Instead                                                   | Why                                      |
| ----------------------- | ----------------------- | ------------------------------------------------------------- | ---------------------------------------- |
| Compression             | Custom LZ77             | `zstd::bulk::compress/decompress`                             | Already in project, battle-tested        |
| AEAD encryption         | Custom streaming cipher | `chacha20poly1305` with existing chunked pattern              | Crypto must not be hand-rolled           |
| Nonce derivation        | Random nonces           | `blake3` hash of transfer_id + chunk_index (existing pattern) | Deterministic nonces prevent nonce reuse |
| Binary encoding helpers | Custom bit-packing      | `std::io::Read/Write` with to_le_bytes/from_le_bytes          | Standard, portable, well-understood      |

**Key insight:** The V3 implementation is a refinement of V2, not a greenfield design. The chunked AEAD pattern, nonce derivation, AAD generation, and two-segment wire framing all exist and should be evolved, not replaced from scratch.

## Common Pitfalls

### Pitfall 1: Compression Ratio Worse Than Expected on Small Payloads

**What goes wrong:** zstd adds ~22 bytes of framing overhead. On small payloads (< 8KB), compressed output can be larger than input.
**Why it happens:** Dictionary-based compressors need enough data to build effective dictionaries.
**How to avoid:** The 8KB threshold is already locked in CONTEXT.md. The encoder must check `compression_algo == 0` and pass plaintext through unchanged.
**Warning signs:** Unit test showing compressed output > input for small payloads.

### Pitfall 2: Header Size Mismatch Between Encoder and Decoder

**What goes wrong:** V3 header is 37 bytes (5 bytes more than V2's 32 bytes). Off-by-one in field offsets breaks all decode.
**Why it happens:** Adding compression_algo (1B) and uncompressed_len (4B) shifts subsequent fields.
**How to avoid:** Define V3 header fields as constants with explicit offset calculations. Write a header round-trip test first.
**Warning signs:** "invalid magic" or "truncated header" errors on decode.

### Pitfall 3: Forgetting to Update total_plaintext_len Semantics

**What goes wrong:** V3 header's `total_plaintext_len` field means "length of data fed to chunked AEAD" which is the COMPRESSED size when compression is active, not the original plaintext size.
**Why it happens:** V2's `total_plaintext_len` was always uncompressed. V3 reuses the field name but the semantic changes.
**How to avoid:** CONTEXT.md explicitly notes: "total_plaintext_len -- this is compressed size when compression active". The uncompressed size lives in the separate `uncompressed_len` header field.
**Warning signs:** Decoder allocates wrong-size buffer, or header validation fails.

### Pitfall 4: Arc<[u8]> Conversion Ergonomics

**What goes wrong:** `Arc<[u8]>` is not `Arc<Vec<u8>>`. Creating from `Vec<u8>` requires `Arc::from(vec.into_boxed_slice())`.
**Why it happens:** `Arc<[u8]>` is a thin pointer to heap-allocated byte slice, while `Arc<Vec<u8>>` has double indirection.
**How to avoid:** Use `Arc::from(bytes.into_boxed_slice())` at the single conversion point in sync_outbound, then `.clone()` for each peer.
**Warning signs:** Compilation errors about `From` trait not implemented.

### Pitfall 5: Port Trait Change Cascading to All Implementors

**What goes wrong:** Changing `ClipboardTransportPort::send_clipboard(Vec<u8>)` to `Arc<[u8]>` breaks all implementors.
**Why it happens:** Rust trait changes require all implementations to be updated simultaneously.
**How to avoid:** Update in one commit: trait definition + libp2p adapter + all test mocks in sync_outbound.rs and sync_inbound.rs tests.
**Warning signs:** Compilation errors across multiple crates.

### Pitfall 6: V1/V2 Removal Leaving Dead Imports

**What goes wrong:** After removing V1/V2 types, unused imports and `use` statements scattered across crates cause compilation warnings or errors.
**Why it happens:** `ClipboardTextPayloadV1`, `ClipboardMultiRepPayloadV2`, `EncryptedBlob`, `aad::for_network_clipboard` are imported in many files.
**How to avoid:** After deleting V1/V2 modules, run `cargo check` and fix all compilation errors systematically. grep for removed type names across the entire src-tauri tree.
**Warning signs:** `unused import` warnings, `unresolved import` errors.

### Pitfall 7: tokio::join! Requires Both Futures to Be Send

**What goes wrong:** `tokio::join!` with `&self` borrows can fail if self contains non-Send types.
**Why it happens:** The encryption function and ensure_business_path both capture `&self`, and `tokio::join!` needs both futures to be `Send`.
**How to avoid:** Extract the encryption work into a standalone function that takes owned/cloned dependencies, not `&self`. Or use `tokio::join!` inside the already-spawned async block.
**Warning signs:** Cryptic "future is not Send" compiler errors.

## Code Examples

### V3 Header Layout (37 bytes)

```rust
// Source: CONTEXT.md locked decision
pub const V3_MAGIC: [u8; 4] = [0x55, 0x43, 0x33, 0x00]; // "UC3\0"
pub const V3_HEADER_SIZE: usize = 37;

// Header field offsets:
// [0..4]   magic
// [4]      compression_algo (0=none, 1=zstd)
// [5..9]   uncompressed_len (u32 LE)
// [9..25]  transfer_id (16 bytes UUID v4)
// [25..29] total_chunks (u32 LE)
// [29..33] chunk_size_hint (u32 LE)
// [33..37] total_plaintext_len (u32 LE) -- compressed size when compression active
```

### V3 Binary Payload Layout (before compression)

```rust
// Source: CONTEXT.md locked decision
// [8B]  ts_ms (i64 LE)
// [2B]  rep_count (u16 LE)
// For each rep:
//   [2B]  format_id_len (u16 LE)
//   [NB]  format_id (UTF-8)
//   [1B]  has_mime (0/1)
//   [2B]  mime_len (u16 LE, if has_mime)
//   [NB]  mime (UTF-8, if has_mime)
//   [4B]  data_len (u32 LE)
//   [NB]  data (raw bytes)
```

### Compression Integration in Encryptor Adapter

```rust
const COMPRESSION_THRESHOLD: usize = 8 * 1024; // 8KB
const ZSTD_LEVEL: i32 = 3;

impl TransferPayloadEncryptorPort for TransferPayloadEncryptorAdapter {
    fn encrypt(&self, master_key: &MasterKey, plaintext: &[u8]) -> Result<Vec<u8>, TransferCryptoError> {
        let (data_to_encrypt, compression_algo, uncompressed_len) =
            if plaintext.len() > COMPRESSION_THRESHOLD {
                let compressed = zstd::bulk::compress(plaintext, ZSTD_LEVEL)
                    .map_err(|e| TransferCryptoError::EncryptionFailed(format!("zstd compress: {e}")))?;
                (compressed, 1u8, plaintext.len() as u32)
            } else {
                (plaintext.to_vec(), 0u8, plaintext.len() as u32)
            };

        let transfer_id: [u8; 16] = *Uuid::new_v4().as_bytes();
        let mut buf = Vec::new();
        ChunkedEncoderV3::encode_to(
            &mut buf, master_key, &transfer_id, &data_to_encrypt,
            compression_algo, uncompressed_len,
        )?;
        Ok(buf)
    }
}
```

### Outbound Parallelization with tokio::join!

```rust
// In sync_outbound execute_async, after building payload:
let plaintext_bytes = v3_payload.encode_to_vec()?;

let first_peer = sendable_peers.first().unwrap().clone();
let remaining_peers = &sendable_peers[1..];

// Parallel: encrypt + first peer's ensure_business_path
let (encrypted_result, ensure_result) = tokio::join!(
    async {
        let master_key = encryption_session.get_master_key().await?;
        let encrypted = transfer_encryptor.encrypt(&master_key, &plaintext_bytes)?;
        let framed = ProtocolMessage::Clipboard(header).frame_to_bytes(Some(&encrypted))?;
        Ok::<Arc<[u8]>, anyhow::Error>(Arc::from(framed.into_boxed_slice()))
    },
    clipboard_network.ensure_business_path(&first_peer.peer_id)
);

let outbound_bytes = encrypted_result?;
ensure_result?; // or handle error
clipboard_network.send_clipboard(&first_peer.peer_id, outbound_bytes.clone()).await?;

// Serial for remaining peers
for peer in remaining_peers {
    clipboard_network.ensure_business_path(&peer.peer_id).await?;
    clipboard_network.send_clipboard(&peer.peer_id, outbound_bytes.clone()).await?;
}
```

## State of the Art

| Old Approach                     | Current Approach                | When Changed | Impact                                        |
| -------------------------------- | ------------------------------- | ------------ | --------------------------------------------- |
| V1: JSON text-only payload       | V2: JSON+base64 multi-rep       | Phase 2-3    | Supports images but with base64 overhead      |
| V2: serde_json + base64 encoding | V3: binary length-prefixed      | This phase   | Eliminates ~33% base64 overhead               |
| No compression                   | zstd level 3                    | This phase   | ~60%+ reduction on image payloads             |
| Vec<u8> clone per peer           | Arc<[u8]> zero-copy             | This phase   | Memory usage O(1) vs O(N peers)               |
| Serial prepare + send            | Parallel prepare + first ensure | This phase   | Overlaps ~100ms+ ensure_business_path latency |

**Deprecated/outdated after this phase:**

- `ClipboardTextPayloadV1`: Removed entirely
- `ClipboardMultiRepPayloadV2`: Removed entirely (replaced by V3 binary codec)
- `WireRepresentation` (serde version): Removed (replaced by binary struct)
- `ClipboardPayloadVersion::V1` and `V2`: Removed from enum
- UC2 magic/ChunkedEncoder/ChunkedDecoder: Replaced by UC3 versions
- V1 inbound path (`apply_v1_inbound`): Deleted
- V2 inbound path (`apply_v2_inbound`): Deleted, replaced by V3

## Open Questions

1. **Chunk nonce derivation AAD prefix**
   - What we know: Current prefix is `"uc:chunk-nonce:v1|"` in `derive_chunk_nonce`
   - What's unclear: Whether to update to `"uc:chunk-nonce:v3|"` for V3 or keep v1
   - Recommendation: Keep `v1` prefix -- the nonce derivation algorithm itself hasn't changed, only the wire header format. Changing it would be purely cosmetic and could cause confusion about what "version" refers to. Claude's discretion per CONTEXT.md.

2. **V3 struct naming**
   - What we know: V2 used `ClipboardMultiRepPayloadV2` and `WireRepresentation`
   - What's unclear: Best naming for V3 binary equivalents
   - Recommendation: `ClipboardBinaryPayload` (no version suffix since V1/V2 are deleted) and `BinaryRepresentation` for the wire rep. Claude's discretion per CONTEXT.md.

3. **Error variants for V3**
   - What we know: `ChunkedTransferError` has V2-era variants
   - What's unclear: Which new error variants are needed for V3 (compression failures, invalid compression_algo, etc.)
   - Recommendation: Add `CompressionFailed { source: String }`, `DecompressionFailed { source: String }`, `InvalidCompressionAlgo { algo: u8 }`. Map all to `TransferCryptoError::EncryptionFailed` or `DecryptionFailed`.

## Validation Architecture

### Test Framework

| Property           | Value                                                                  |
| ------------------ | ---------------------------------------------------------------------- |
| Framework          | cargo test (built-in)                                                  |
| Config file        | src-tauri/Cargo.toml                                                   |
| Quick run command  | `cd src-tauri && cargo test -p uc-core -p uc-infra -p uc-app -- --lib` |
| Full suite command | `cd src-tauri && cargo test`                                           |

### Phase Requirements to Test Map

| Req ID      | Behavior                                   | Test Type   | Automated Command                                            | File Exists?                 |
| ----------- | ------------------------------------------ | ----------- | ------------------------------------------------------------ | ---------------------------- |
| V3-CODEC    | V3 binary payload encode/decode round-trip | unit        | `cd src-tauri && cargo test -p uc-core payload_v3 -x`        | No -- Wave 0                 |
| V3-WIRE     | V3 wire header encode/decode round-trip    | unit        | `cd src-tauri && cargo test -p uc-infra chunked_transfer -x` | Partial (V2 exists)          |
| V3-COMPRESS | Compression on/off based on 8KB threshold  | unit        | `cd src-tauri && cargo test -p uc-infra chunked_transfer -x` | No -- Wave 0                 |
| V3-LARGE    | Large payload (10MB+) encode/decode        | unit        | `cd src-tauri && cargo test -p uc-infra chunked_transfer -x` | Partial (1MB V2 test exists) |
| V3-ARC      | Arc<[u8]> zero-copy fanout                 | unit        | `cd src-tauri && cargo test -p uc-app sync_outbound -x`      | No -- Wave 0                 |
| V3-OUTBOUND | Single peer end-to-end outbound V3         | integration | `cd src-tauri && cargo test -p uc-app sync_outbound -x`      | Partial (V2 e2e exists)      |
| V3-INBOUND  | Single peer inbound V3 decode + persist    | integration | `cd src-tauri && cargo test -p uc-app sync_inbound -x`       | Partial (V2 path exists)     |
| V3-NOENC    | Encryption session not-ready regression    | unit        | `cd src-tauri && cargo test -p uc-app sync_outbound -x`      | Yes (existing test)          |
| V3-NOLEAK   | V1/V2 code fully removed, no dead imports  | smoke       | `cd src-tauri && cargo check 2>&1`                           | N/A                          |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-core -p uc-infra -p uc-app -- --lib`
- **Per wave merge:** `cd src-tauri && cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `uc-core/src/network/protocol/clipboard_payload_v3.rs` -- V3 binary codec with round-trip tests
- [ ] V3 chunked transfer tests in `uc-infra/src/clipboard/chunked_transfer.rs` -- compression on/off, large payload
- [ ] Updated test mocks in sync_outbound.rs/sync_inbound.rs for `Arc<[u8]>` port signature

## Sources

### Primary (HIGH confidence)

- Project source code: `chunked_transfer.rs`, `sync_outbound.rs`, `sync_inbound.rs`, `clipboard_payload_v2.rs`, `clipboard.rs`, `protocol_message.rs`, `aad.rs`, `libp2p_network.rs`
- CONTEXT.md locked decisions (all wire format details, pipeline order, parallelization strategy)

### Secondary (MEDIUM confidence)

- zstd crate API: `zstd::bulk::compress(data, level)` / `zstd::bulk::decompress(data, max_size)` -- verified from existing usage in `encrypted_blob_store.rs`
- std::sync::Arc<[u8]> conversion: `Arc::from(vec.into_boxed_slice())` -- standard Rust pattern

### Tertiary (LOW confidence)

- None -- all findings verified from project source code and existing usage patterns.

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - all libraries already in project, versions confirmed in Cargo.toml
- Architecture: HIGH - all patterns follow existing V2 code structure, changes are well-bounded
- Pitfalls: HIGH - derived from direct code analysis and Rust type system constraints
- Wire format: HIGH - fully specified in CONTEXT.md locked decisions

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable -- no external dependency changes expected)

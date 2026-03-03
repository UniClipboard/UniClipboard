# Phase 4: Optimize blob at-rest storage format without backward compatibility - Context

**Gathered:** 2026-03-03
**Status:** Ready for planning

<domain>
## Phase Boundary

Replace the current JSON-serialized `EncryptedBlob` at-rest format with an optimized binary format. The current format uses JSON with base64-encoded ciphertext, adding ~33% size overhead. This phase eliminates that overhead, adds zstd compression, and cleans up the storage schema. No backward compatibility with the old format — existing blobs are wiped on upgrade.

Scope: blob files on disk only. Inline data in SQLite (≤16KB) is already efficient and out of scope.

</domain>

<decisions>
## Implementation Decisions

### Binary format design

- Fixed binary header structure, consistent with the two-segment wire framing pattern from Phase 3
- Magic bytes signature ("UCBL" or similar 4-byte identifier) at file start for quick identification and version detection
- Single version byte after magic (0x01 for this format, supports 255 future versions)
- Fixed-size fields: nonce (24 bytes) directly as raw bytes, not JSON-encoded arrays
- Drop the optional AAD fingerprint field — AAD is deterministic from blob_id, storing it is redundant
- Layout: magic(4B) + version(1B) + nonce(24B) + raw ciphertext (remaining bytes)

### Compression

- zstd compression applied before encryption
- Compress plaintext → encrypt compressed data → write binary format
- Decompress after decryption on read
- Images (PNG/JPEG) already compressed so minimal gain there, but text/rich-text benefits significantly

### Content hash and dedup

- blake3 content hash computed on original plaintext (before compression)
- Dedup correctness preserved regardless of compression settings/version changes
- Same content always produces same hash

### Size tracking

- Add `compressed_size` column to blob table alongside existing `size_bytes`
- `size_bytes` = original plaintext size (unchanged)
- `compressed_size` = on-disk size after compression + encryption
- Useful for storage metrics, spool cleanup calculations, and compression ratio monitoring

### AAD versioning

- Bump AAD prefix from "uc:blob:v1" to "uc:blob:v2" for the new format
- Format: "uc:blob:v2|{blob_id}"
- Clean cryptographic separation — prevents old-format blobs from being decrypted as new format

### Migration strategy

- Wipe and re-capture on upgrade: delete all existing blob files and DB blob records
- Clipboard history starts fresh — matches "no backward compatibility" intent
- Clipboard history is ephemeral by nature; users don't expect permanence
- Schema migration adds `compressed_size` column and can tighten constraints

### Claude's Discretion

- Exact zstd compression level (balance between speed and ratio)
- Whether to skip compression for already-compressed MIME types (image/png, image/jpeg, etc.)
- Corrupt blob detection beyond AEAD tamper checking (truncated file handling)
- Exact migration implementation (Diesel migration vs runtime check)
- User notification about data reset (silent vs toast)

</decisions>

<code_context>

## Existing Code Insights

### Reusable Assets

- `EncryptedBlobStore` (`uc-infra/src/security/encrypted_blob_store.rs`): Current encryption decorator — needs refactoring from JSON to binary serialization
- `FilesystemBlobStore` (`uc-platform/src/adapters/blob_store.rs`): Raw file I/O adapter — can be reused as-is
- `BlobWriterPort` (`uc-infra/src/blob/blob_writer.rs`): Atomic write-if-absent with dedup — core logic reusable, just change serialization
- `Aad::for_blob()` (`uc-core/src/security/aad.rs`): AAD generation — update prefix from v1 to v2
- Wire framing pattern from Phase 3: Two-segment binary framing (4-byte LE prefix) — similar approach for at-rest format

### Established Patterns

- Hexagonal architecture: ports in `uc-core`, implementations in `uc-infra`/`uc-platform`
- `BlobStorePort` trait: `put(blob_id, bytes)` / `get(blob_id)` — interface stays stable, only implementation changes
- Content hash: `blake3v1:{64-hex-chars}` format with UNIQUE constraint for dedup
- Diesel ORM for schema migrations in `uc-infra/migrations/`
- `PayloadAvailability` state machine: Staged → Processing → BlobReady (unchanged by this phase)

### Integration Points

- `EncryptedBlobStore` wraps `BlobStorePort` — main change point for binary format
- `EncryptionSessionPort` provides MasterKey — no changes needed
- `ClipboardStorageConfig` has spool limits — may need to account for compressed_size
- `BlobRow` Diesel model in `uc-infra/src/db/models/blob.rs` — add compressed_size field
- `blob` table schema in Diesel migration — new migration for column addition

</code_context>

<specifics>
## Specific Ideas

- Follow the same binary framing philosophy as the wire format (Phase 3): minimal overhead, fixed-size headers, raw bytes
- The pipeline should be: plaintext → blake3 hash (dedup) → zstd compress → XChaCha20-Poly1305 encrypt → binary serialize → write to disk
- Read pipeline reverses: read file → parse binary header → decrypt → decompress → plaintext

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

_Phase: 04-optimize-blob-at-rest-storage-format-without-backward-compatibility_
_Context gathered: 2026-03-03_

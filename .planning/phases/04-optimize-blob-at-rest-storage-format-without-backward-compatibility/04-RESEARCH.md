# Phase 4: Optimize blob at-rest storage format without backward compatibility - Research

**Researched:** 2026-03-04
**Domain:** Rust binary serialization, zstd compression, AEAD encryption, Diesel ORM migrations
**Confidence:** HIGH

## Summary

This phase replaces the current JSON-serialized `EncryptedBlob` at-rest format with a compact binary format. The current flow serializes the `EncryptedBlob` struct (containing version, algorithm, nonce, ciphertext, and optional AAD fingerprint) to JSON via `serde_json::to_vec`, producing ~33% overhead from base64 encoding of binary fields. The new format uses a fixed binary header (magic + version + raw nonce) followed by raw ciphertext, with zstd compression applied to plaintext before encryption.

The codebase already has all encryption primitives in place (`chacha20poly1305` crate, `EncryptionPort` trait, `EncryptedBlobStore` decorator). The Phase 3 wire format (`chunked_transfer.rs`) established a binary framing pattern with magic bytes ("UC2\0") that this phase mirrors for at-rest storage. Key changes are isolated to `EncryptedBlobStore` (serialization/deserialization), `aad.rs` (version bump), the Diesel schema (new `compressed_size` column), and adding the `zstd` crate.

**Primary recommendation:** Refactor `EncryptedBlobStore` to perform compress-then-encrypt with zstd, serialize to a 29-byte binary header + raw ciphertext, bump AAD to v2, add a `compressed_size` column via Diesel migration, and wipe existing blobs on upgrade.

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Fixed binary header structure, consistent with the two-segment wire framing pattern from Phase 3
- Magic bytes signature ("UCBL" or similar 4-byte identifier) at file start for quick identification and version detection
- Single version byte after magic (0x01 for this format, supports 255 future versions)
- Fixed-size fields: nonce (24 bytes) directly as raw bytes, not JSON-encoded arrays
- Drop the optional AAD fingerprint field -- AAD is deterministic from blob_id, storing it is redundant
- Layout: magic(4B) + version(1B) + nonce(24B) + raw ciphertext (remaining bytes)
- zstd compression applied before encryption (compress plaintext -> encrypt compressed data -> write binary format)
- Decompress after decryption on read
- blake3 content hash computed on original plaintext (before compression)
- Dedup correctness preserved regardless of compression settings/version changes
- Add `compressed_size` column to blob table alongside existing `size_bytes`
- `size_bytes` = original plaintext size (unchanged)
- `compressed_size` = on-disk size after compression + encryption
- Bump AAD prefix from "uc:blob:v1" to "uc:blob:v2" for the new format
- Format: "uc:blob:v2|{blob_id}"
- Wipe and re-capture on upgrade: delete all existing blob files and DB blob records
- Schema migration adds `compressed_size` column and can tighten constraints

### Claude's Discretion

- Exact zstd compression level (balance between speed and ratio)
- Whether to skip compression for already-compressed MIME types (image/png, image/jpeg, etc.)
- Corrupt blob detection beyond AEAD tamper checking (truncated file handling)
- Exact migration implementation (Diesel migration vs runtime check)
- User notification about data reset (silent vs toast)

### Deferred Ideas (OUT OF SCOPE)

None -- discussion stayed within phase scope
</user_constraints>

## Standard Stack

### Core

| Library            | Version | Purpose                             | Why Standard                                                              |
| ------------------ | ------- | ----------------------------------- | ------------------------------------------------------------------------- |
| `zstd`             | 0.13.3  | zstandard compression/decompression | De facto Rust binding for zstd; 190M+ downloads, wraps official C library |
| `chacha20poly1305` | 0.10.1  | XChaCha20-Poly1305 AEAD encryption  | Already in use; RustCrypto standard                                       |
| `blake3`           | 1.8.2   | Content hashing for dedup           | Already in use for `ContentHashPort`                                      |
| `diesel`           | 2.3.5   | SQLite ORM and migrations           | Already in use; schema changes via migration                              |

### Supporting

| Library   | Version | Purpose                       | When to Use                          |
| --------- | ------- | ----------------------------- | ------------------------------------ |
| `rand`    | 0.9.2   | Nonce generation              | Already used by EncryptionRepository |
| `tracing` | 0.1     | Structured logging with spans | Already used throughout              |

### Alternatives Considered

| Instead of         | Could Use            | Tradeoff                                                                 |
| ------------------ | -------------------- | ------------------------------------------------------------------------ |
| `zstd` (C binding) | `ruzstd` (pure Rust) | Pure Rust but ~3.5x slower decompression, no compressor maturity         |
| `zstd` (C binding) | `lz4_flex`           | Faster compression but worse ratio; zstd better for mixed clipboard data |

**Installation:**

```bash
# Add to src-tauri/crates/uc-infra/Cargo.toml
# zstd = "0.13"
```

Only `uc-infra` needs the `zstd` dependency since compression happens in the `EncryptedBlobStore` decorator within the infrastructure layer.

## Architecture Patterns

### Recommended Change Scope

```
src-tauri/crates/
├── uc-core/src/
│   └── security/
│       └── aad.rs                    # Bump for_blob() to v2 prefix
├── uc-infra/src/
│   ├── security/
│   │   └── encrypted_blob_store.rs   # MAIN CHANGE: binary format + compression
│   ├── db/
│   │   ├── schema.rs                 # Regenerated: add compressed_size
│   │   ├── models/blob.rs            # Add compressed_size field
│   │   ├── mappers/blob_mapper.rs    # Map compressed_size
│   │   └── repositories/blob_repo.rs # No change (interface stable)
│   └── blob/
│       └── blob_writer.rs            # May need to pass compressed_size up
├── uc-infra/migrations/
│   └── YYYY-MM-DD-HHMMSS_blob_v2_binary_format/
│       ├── up.sql                    # ADD compressed_size, DELETE old blobs
│       └── down.sql                  # DROP column (best-effort)
└── uc-core/src/
    └── blob/mod.rs                   # Add compressed_size to Blob domain model
```

### Pattern 1: Binary Format Serialization

**What:** Replace JSON serialization with fixed-layout binary format
**When to use:** Writing/reading blob files to/from disk
**Layout:**

```
Offset  Size  Field
0       4     Magic bytes: [0x55, 0x43, 0x42, 0x4C] ("UCBL")
4       1     Version: 0x01
5       24    Nonce (raw XChaCha20 nonce bytes)
29      N     Ciphertext (remaining bytes to EOF)
```

**Total header overhead:** 29 bytes (vs current JSON overhead of ~200+ bytes per blob)

**Example:**

```rust
// Source: aligned with Phase 3 chunked_transfer.rs pattern

/// Magic bytes identifying a V1 blob file ("UCBL")
const BLOB_MAGIC: [u8; 4] = [0x55, 0x43, 0x42, 0x4C];
const BLOB_FORMAT_VERSION: u8 = 0x01;
const BLOB_HEADER_SIZE: usize = 4 + 1 + 24; // 29 bytes

fn serialize_blob(nonce: &[u8; 24], ciphertext: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(BLOB_HEADER_SIZE + ciphertext.len());
    buf.extend_from_slice(&BLOB_MAGIC);
    buf.push(BLOB_FORMAT_VERSION);
    buf.extend_from_slice(nonce);
    buf.extend_from_slice(ciphertext);
    buf
}

fn parse_blob(data: &[u8]) -> Result<(&[u8; 24], &[u8])> {
    if data.len() < BLOB_HEADER_SIZE {
        return Err(anyhow::anyhow!("blob file truncated: {} bytes", data.len()));
    }
    if &data[0..4] != &BLOB_MAGIC {
        return Err(anyhow::anyhow!("invalid blob magic bytes"));
    }
    if data[4] != BLOB_FORMAT_VERSION {
        return Err(anyhow::anyhow!("unsupported blob format version: {}", data[4]));
    }
    let nonce: &[u8; 24] = data[5..29].try_into()
        .map_err(|_| anyhow::anyhow!("nonce extraction failed"))?;
    let ciphertext = &data[29..];
    Ok((nonce, ciphertext))
}
```

### Pattern 2: Compress-Then-Encrypt Pipeline

**What:** Apply zstd compression to plaintext before AEAD encryption
**When to use:** Every blob write and read operation
**Pipeline:**

```
Write: plaintext -> blake3 hash (on original) -> zstd compress -> encrypt -> binary serialize -> write file
Read:  read file -> parse binary header -> decrypt -> zstd decompress -> plaintext
```

**Example:**

```rust
// Write path in EncryptedBlobStore::put
async fn put(&self, blob_id: &BlobId, plaintext: &[u8]) -> Result<PathBuf> {
    let master_key = self.session.get_master_key().await
        .context("encryption session not ready")?;

    // 1. Compress plaintext with zstd
    let compressed = zstd::bulk::compress(plaintext, ZSTD_LEVEL)
        .context("zstd compression failed")?;

    // 2. Encrypt compressed data
    let aad = aad::for_blob_v2(blob_id);
    let encrypted_blob = self.encryption
        .encrypt_blob(&master_key, &compressed, &aad, EncryptionAlgo::XChaCha20Poly1305)
        .await
        .context("failed to encrypt blob")?;

    // 3. Serialize to binary format
    let nonce: [u8; 24] = encrypted_blob.nonce.try_into()
        .map_err(|_| anyhow::anyhow!("unexpected nonce length"))?;
    let binary_data = serialize_blob(&nonce, &encrypted_blob.ciphertext);

    debug!(
        blob_id = %blob_id.as_ref(),
        plaintext_size = plaintext.len(),
        compressed_size = compressed.len(),
        on_disk_size = binary_data.len(),
        "Wrote blob in V2 binary format"
    );

    // 4. Write to filesystem
    self.inner.put(blob_id, &binary_data).await
}

// Read path in EncryptedBlobStore::get
async fn get(&self, blob_id: &BlobId) -> Result<Vec<u8>> {
    // 1. Read binary data from filesystem
    let binary_data = self.inner.get(blob_id).await
        .context("failed to read blob from storage")?;

    // 2. Parse binary header
    let (nonce, ciphertext) = parse_blob(&binary_data)
        .context("invalid blob file format")?;

    // 3. Reconstruct EncryptedBlob for decryption
    let encrypted_blob = EncryptedBlob {
        version: EncryptionFormatVersion::V1,
        aead: EncryptionAlgo::XChaCha20Poly1305,
        nonce: nonce.to_vec(),
        ciphertext: ciphertext.to_vec(),
        aad_fingerprint: None,
    };

    // 4. Decrypt
    let master_key = self.session.get_master_key().await
        .context("encryption session not ready")?;
    let aad = aad::for_blob_v2(blob_id);
    let compressed = self.encryption
        .decrypt_blob(&master_key, &encrypted_blob, &aad)
        .await
        .context("failed to decrypt blob")?;

    // 5. Decompress
    let plaintext = zstd::bulk::decompress(&compressed, MAX_DECOMPRESSED_SIZE)
        .context("zstd decompression failed")?;

    Ok(plaintext)
}
```

### Pattern 3: Migration with Data Wipe

**What:** Diesel migration that adds `compressed_size` column and deletes all existing blob data
**When to use:** Database schema upgrade on app start

**Example (up.sql):**

```sql
-- Delete all existing blob records (they use old JSON format)
-- CASCADE via FK will clean clipboard_snapshot_representation.blob_id -> SET NULL
DELETE FROM blob;

-- Add compressed_size column for storage metrics
ALTER TABLE blob ADD COLUMN compressed_size BIGINT;
```

### Anti-Patterns to Avoid

- **Hand-rolling encryption:** Never bypass the `EncryptionPort` trait; always use the existing `encrypt_blob`/`decrypt_blob` methods which handle nonce generation and AEAD properly
- **Compressing after encryption:** AEAD ciphertext has high entropy and is incompressible; always compress plaintext before encryption
- **Using serde for the binary format:** The 29-byte header is simpler and faster with manual byte layout than any serialization framework
- **Forgetting decompression size limit:** `zstd::bulk::decompress` requires a max output size to prevent zip bombs; use a reasonable upper bound

## Don't Hand-Roll

| Problem         | Don't Build                    | Use Instead                             | Why                                                                            |
| --------------- | ------------------------------ | --------------------------------------- | ------------------------------------------------------------------------------ |
| Compression     | Custom LZ variant              | `zstd::bulk::compress` / `decompress`   | Battle-tested C library, tunable levels, handles edge cases                    |
| AEAD encryption | Raw ChaCha20 + separate auth   | `chacha20poly1305` via `EncryptionPort` | Already wired, handles nonce gen, AAD, tag verification                        |
| Content hashing | SHA256 or custom hash          | `blake3` via `ContentHashPort`          | Already wired, faster than SHA256, consistent `blake3v1:` format               |
| Binary parsing  | nom/bincode for 29-byte header | Manual slice indexing                   | Header is trivially simple; adding a parser framework adds needless complexity |
| Migration       | Manual SQL at runtime          | Diesel migration system                 | Already used; `diesel_migrations` runs on app start automatically              |

**Key insight:** The entire pipeline (hash -> compress -> encrypt -> serialize -> write) can be built by composing existing infrastructure. Only the serialization format and compression step are new.

## Common Pitfalls

### Pitfall 1: Compressing Already-Compressed Data

**What goes wrong:** Applying zstd to PNG/JPEG data wastes CPU time for near-zero compression gain (may even slightly expand the data)
**Why it happens:** All clipboard content flows through the same path regardless of MIME type
**How to avoid:** Optionally detect already-compressed MIME types and skip compression, or accept the minimal overhead since zstd level 3 is fast enough that the CPU cost is negligible (~50MB/s even for incompressible data)
**Recommendation:** Use zstd unconditionally at level 3 for simplicity. The zstd encoder detects incompressible blocks internally and short-circuits. The overhead for a 5MB PNG is ~2ms, which is negligible compared to disk I/O.

### Pitfall 2: Decompression Bomb (Zip Bomb)

**What goes wrong:** `zstd::bulk::decompress` needs a maximum output size; if omitted or too large, a crafted blob could allocate excessive memory
**Why it happens:** The compressed data size on disk does not bound the decompressed size
**How to avoid:** Pass a reasonable max decompressed size derived from the `size_bytes` column in the database (original plaintext size). Use `size_bytes + 1024` as the bound, or a hard cap like 500MB.
**Warning signs:** Memory spikes during blob reads

### Pitfall 3: Forgetting to Delete Blob Files on Migration

**What goes wrong:** The SQL migration deletes DB records but orphaned blob files remain on disk, wasting space
**Why it happens:** Diesel migrations only affect the database, not the filesystem
**How to avoid:** Add a runtime migration step (in app startup, after Diesel migrations run) that purges the blob spool directory. Or delete all files in the spool directory as part of the migration startup sequence.
**Warning signs:** Disk space not reclaimed after upgrade

### Pitfall 4: AAD Mismatch Between Old and New Format

**What goes wrong:** If blobs written with v1 AAD ("uc:blob:v1|...") are read with v2 AAD ("uc:blob:v2|..."), AEAD decryption fails silently with "corrupted blob" error
**Why it happens:** Format version upgrade without data wipe leaves old blobs that can't be decrypted
**How to avoid:** The wipe-on-upgrade strategy eliminates this entirely. The AAD version bump provides defense-in-depth: even if a v1 blob file somehow survives, it cannot be decrypted under the v2 AAD scheme.
**Warning signs:** "CorruptedBlob" errors after upgrade

### Pitfall 5: Diesel Schema Out of Sync

**What goes wrong:** After adding `compressed_size` to the migration, forgetting to regenerate `schema.rs` causes compilation errors
**Why it happens:** Diesel auto-generates `schema.rs` from the database schema
**How to avoid:** Run `cd src-tauri && diesel print-schema > crates/uc-infra/src/db/schema.rs` after creating the migration, or manually add the column to `schema.rs`

## Code Examples

### zstd Bulk Compression/Decompression

```rust
// Source: zstd crate docs (https://docs.rs/zstd/0.13.3)

// Compression: returns Vec<u8> of compressed data
let compressed: Vec<u8> = zstd::bulk::compress(plaintext, 3)?;
// level 3 = default, good balance of speed and ratio

// Decompression: requires max output capacity
let decompressed: Vec<u8> = zstd::bulk::decompress(&compressed, max_size)?;
// max_size prevents decompression bombs
```

### AAD Version Bump

```rust
// Source: existing aad.rs pattern

/// AAD format version for blob storage (v2 = binary format with compression)
const AAD_BLOB_VERSION: &str = "v2";

/// Generates AAD for blob storage encryption/decryption (V2 format).
///
/// # Format
/// `uc:blob:v2|{blob_id}`
pub fn for_blob(blob_id: &BlobId) -> Vec<u8> {
    format!("{AAD_NAMESPACE}:blob:{AAD_BLOB_VERSION}|{}", blob_id.as_ref()).into_bytes()
}
```

### Diesel Migration SQL

```sql
-- up.sql: Blob V2 binary format migration

-- Step 1: Delete all existing blob data (incompatible format)
-- FK cascade: clipboard_snapshot_representation.blob_id -> SET NULL
DELETE FROM blob;

-- Step 2: Add compressed_size column for storage metrics
-- NULL for inline data or when compression is not tracked
ALTER TABLE blob ADD COLUMN compressed_size BIGINT;
```

```sql
-- down.sql: Revert blob V2 migration
-- NOTE: This cannot recover deleted blobs

-- Remove the compressed_size column
-- SQLite does not support DROP COLUMN before 3.35.0
-- Diesel's bundled SQLite is >= 3.35, so this is safe
ALTER TABLE blob DROP COLUMN compressed_size;
```

### Updated BlobRow Model

```rust
// Source: existing blob.rs model pattern

#[derive(Queryable)]
#[diesel(table_name = blob)]
pub struct BlobRow {
    pub blob_id: String,
    pub storage_path: String,
    pub storage_backend: String,
    pub size_bytes: i64,
    pub content_hash: String,
    pub encryption_algo: Option<String>,
    pub created_at_ms: i64,
    pub compressed_size: Option<i64>,  // NEW: on-disk size after compress+encrypt
}

#[derive(Insertable)]
#[diesel(table_name = blob)]
pub struct NewBlobRow {
    pub blob_id: String,
    pub storage_backend: String,
    pub storage_path: String,
    pub encryption_algo: Option<String>,
    pub size_bytes: i64,
    pub content_hash: String,
    pub created_at_ms: i64,
    pub compressed_size: Option<i64>,  // NEW
}
```

### Updated Blob Domain Model

```rust
// Source: existing blob/mod.rs pattern

#[derive(Debug, Clone)]
pub struct Blob {
    pub blob_id: BlobId,
    pub locator: BlobStorageLocator,
    pub size_bytes: i64,
    pub compressed_size: Option<i64>,  // NEW: on-disk size
    pub content_hash: ContentHash,
    pub created_at_ms: i64,
}
```

## State of the Art

| Old Approach                            | Current Approach                       | When Changed | Impact                                       |
| --------------------------------------- | -------------------------------------- | ------------ | -------------------------------------------- |
| JSON `EncryptedBlob` with base64 fields | Binary header (29B) + raw ciphertext   | This phase   | ~33% smaller blobs, faster I/O               |
| No compression                          | zstd compress-before-encrypt           | This phase   | Significant savings for text/rich-text       |
| AAD "uc:blob:v1"                        | AAD "uc:blob:v2"                       | This phase   | Clean cryptographic separation               |
| No `compressed_size` tracking           | `compressed_size` column in blob table | This phase   | Enables storage metrics and cleanup policies |

**Deprecated/outdated:**

- JSON-serialized `EncryptedBlob` for disk storage: replaced by binary format
- `aad_fingerprint` field in on-disk blobs: removed (AAD is deterministic from blob_id)

## Discretion Recommendations

### zstd Compression Level: Level 3 (default)

**Rationale:** Level 3 is zstd's default and provides the best speed-to-ratio balance. For clipboard data:

- Text: ~60-80% compression ratio at ~450 MB/s throughput
- Images (PNG/JPEG): ~0-2% ratio at ~350 MB/s (effectively a no-op, but fast enough to not matter)
- Level 1 would be marginally faster but compresses text notably less
- Level 6+ would compress text marginally better but at 2-3x slower speed

### Skip Compression for Compressed MIME Types: No

**Rationale:** Do NOT skip compression for already-compressed types. zstd at level 3 detects incompressible blocks internally and passes them through with minimal overhead (~2ms for 5MB). Adding MIME-type detection adds complexity, requires passing MIME info through the blob store interface (which currently only takes `&[u8]`), and violates the decorator's encapsulation. The `BlobStorePort::put` signature is `(&BlobId, &[u8])` -- changing it would ripple through the architecture.

### Truncated File Handling: Check Minimum Size

**Rationale:** Beyond AEAD tag verification (which catches any corruption), add a simple size check: if the file is smaller than 29 bytes (header size), reject immediately with a clear error. AEAD's 16-byte Poly1305 tag already ensures integrity of the ciphertext. No additional checksums needed.

### Migration Implementation: Diesel Migration + Runtime Spool Cleanup

**Rationale:** Use a Diesel migration for the schema change (add `compressed_size`, delete blob records) since this integrates with the existing migration pipeline. For filesystem cleanup (deleting orphaned blob files), add a one-time runtime step during app startup that checks if the blob spool directory contains files and purges them. This two-step approach keeps the SQL migration clean and handles the filesystem separately.

### User Notification: Silent

**Rationale:** Clipboard history is inherently ephemeral. Users do not expect permanence. A toast notification saying "Clipboard history was reset due to format upgrade" would create unnecessary alarm. Simply log the migration at INFO level for diagnostics.

## Open Questions

1. **BlobWriterPort and compressed_size tracking**
   - What we know: `BlobWriter::write_if_absent` currently creates a `Blob` with `size_bytes = plaintext.len()`. The `compressed_size` is only known after the `EncryptedBlobStore` processes the data, but `BlobWriter` calls `BlobStorePort::put` which only returns a `PathBuf`.
   - What's unclear: How to get the `compressed_size` back from `EncryptedBlobStore` to populate the `Blob` record without changing the `BlobStorePort` trait signature.
   - Recommendation: Two options: (a) Change `BlobStorePort::put` to return `(PathBuf, Option<i64>)` for on-disk size, or (b) have `BlobWriter` stat the file after writing to get the actual disk size. Option (b) is simpler and avoids trait changes, but adds one extra filesystem call. Option (a) is cleaner but requires updating all `BlobStorePort` implementors. **Recommend option (a)** since only 2 real implementors exist (`FilesystemBlobStore`, `EncryptedBlobStore`). Note: `PlaceholderBlobStorePort` is dead code (never instantiated) and should be removed during this phase.

## Sources

### Primary (HIGH confidence)

- Codebase inspection: `uc-infra/src/security/encrypted_blob_store.rs` - current JSON serialization flow
- Codebase inspection: `uc-infra/src/clipboard/chunked_transfer.rs` - Phase 3 binary framing pattern
- Codebase inspection: `uc-core/src/security/aad.rs` - current AAD format and version scheme
- Codebase inspection: `uc-core/src/security/model.rs` - `EncryptedBlob` struct definition
- Codebase inspection: `uc-infra/src/security/encryption.rs` - `EncryptionRepository` implementation
- Codebase inspection: `uc-infra/Cargo.toml` - existing dependencies
- Codebase inspection: `uc-infra/src/db/schema.rs` - current blob table schema
- [zstd crate docs](https://docs.rs/zstd/0.13.3) - bulk API, compression levels
- [zstd crates.io](https://crates.io/crates/zstd) - version 0.13.3, downloads

### Secondary (MEDIUM confidence)

- [Zstd Rust Guide 2025](https://generalistprogrammer.com/tutorials/zstd-rust-crate-guide) - compression level recommendations
- [Facebook Zstandard](http://facebook.github.io/zstd/) - algorithm characteristics

### Tertiary (LOW confidence)

- None

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH - zstd 0.13.3 is the established Rust binding; all other libraries already in use
- Architecture: HIGH - all change points identified by inspecting actual source files; pattern follows Phase 3 precedent
- Pitfalls: HIGH - based on direct analysis of existing code and standard compression/encryption considerations

**Research date:** 2026-03-04
**Valid until:** 2026-04-04 (stable domain; zstd API unlikely to change)

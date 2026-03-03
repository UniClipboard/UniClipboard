---
phase: 02-unified-transfer-layer
plan: '02'
subsystem: chunked-transfer-crypto
tags: [rust, encryption, xchacha20-poly1305, streaming, aead, blake3]
dependency_graph:
  requires:
    - '02-01' # uc-core security types: MasterKey, aad::for_chunk_transfer
  provides:
    - ChunkedEncoder::encode_to (streaming V2 wire format encoder)
    - ChunkedDecoder::decode_from (streaming V2 wire format decoder)
    - V2_MAGIC constant and CHUNK_SIZE constant
  affects:
    - '02-03' # Plan 03 libp2p transport will call encode_to / decode_from directly
tech_stack:
  added: []
  patterns:
    - TDD (RED-GREEN-REFACTOR)
    - Streaming IO (std::io::Write + std::io::Read)
    - XChaCha20-Poly1305 AEAD encryption per chunk
    - blake3 deterministic nonce derivation
key_files:
  created:
    - src-tauri/crates/uc-infra/src/clipboard/chunked_transfer.rs
  modified:
    - src-tauri/crates/uc-infra/src/clipboard/mod.rs
decisions:
  - Used thiserror::Error derive macro (already in uc-infra Cargo.toml) instead of manual Display impl
  - MasterKey([0u8; 32]) tuple struct literal for test key construction (pub field confirmed in uc-core)
  - cargo fmt reordered pub mod chunked_transfer alphabetically in mod.rs (accepted)
requirements_completed: [UTL-03, UTL-04]
metrics:
  duration: '3 minutes'
  completed_date: '2026-03-03'
  tasks_completed: 1
  tasks_total: 1
  files_created: 1
  files_modified: 1
---

# Phase 02 Plan 02: Chunked Transfer Crypto Engine Summary

**One-liner:** Streaming XChaCha20-Poly1305 AEAD encoder/decoder with blake3 nonce derivation in bounded CHUNK_SIZE × 2 memory.

## What Was Built

Implemented `chunked_transfer.rs` in `uc-infra::clipboard` — the cryptographic engine for V2 clipboard transfers. Both encoder and decoder operate in streaming fashion, reading/writing one chunk at a time without accumulating all ciphertext in memory.

## Function Signatures

```rust
// Encoder — writes V2 binary wire format chunk-by-chunk to any Write sink
pub fn ChunkedEncoder::encode_to<W: Write>(
    writer: W,
    master_key: &MasterKey,
    transfer_id: &[u8; 16],
    plaintext: &[u8],
) -> Result<(), ChunkedTransferError>

// Decoder — reads V2 binary wire format chunk-by-chunk from any Read source
pub fn ChunkedDecoder::decode_from<R: Read>(
    reader: R,
    master_key: &MasterKey,
) -> Result<Vec<u8>, ChunkedTransferError>
```

## Wire Format Byte Layout (for Plan 03 reference)

```
Header (32 bytes total):
  [0..4]    magic:              0x55 0x43 0x32 0x00  ("UC2\0")
  [4..20]   transfer_id:        UUID v4 raw bytes (16 bytes)
  [20..24]  total_chunks:       u32 LE
  [24..28]  chunk_size_hint:    u32 LE  (= CHUNK_SIZE = 262144)
  [28..32]  total_plaintext_len: u32 LE

Per chunk (repeated total_chunks times):
  [+0..+4]  chunk_ciphertext_len: u32 LE
  [+4..+4+N] ciphertext:          plaintext_chunk + 16-byte Poly1305 tag
```

**Header offset reference:**

- Magic check: `buf[0..4] == V2_MAGIC`
- transfer_id: `buf[4..20]`
- total_chunks: `u32::from_le_bytes(buf[20..24])`
- chunk_size_hint: `u32::from_le_bytes(buf[24..28])` (not needed for decode)
- total_plaintext_len: `u32::from_le_bytes(buf[28..32])`
- First chunk len prefix: `buf[32..36]`
- First chunk ciphertext: `buf[36..36+chunk0_len]`

## Streaming Contract Confirmation

- **encode_to**: Does NOT build a `Vec<u8>` accumulating all ciphertext. Each chunk is encrypted to a temporary `ciphertext: Vec<u8>`, written to the sink with `write_all`, then dropped before the next iteration. Memory peak = one plaintext slice (borrowed) + one ciphertext Vec per chunk.

- **decode_from**: Does NOT call `read_to_end`. Reads ciphertext length prefix via `read_exact` (4 bytes), allocates exactly that size, reads ciphertext via `read_exact`, decrypts, appends to output buffer, and drops the ciphertext. Memory peak = one ciphertext Vec + growing output Vec.

## Nonce Derivation

```
nonce[0..24] = blake3("uc:chunk-nonce:v1|" || transfer_id || chunk_index_le)[0..24]
```

## AAD Construction

Delegated to `uc_core::security::aad::for_chunk_transfer(transfer_id, chunk_index)` which produces `transfer_id (16 bytes) || chunk_index (4 bytes LE)` — binary format consistent with AEAD standard practices (decision from Plan 01).

## Tests (9 passing)

| Test                                         | Property Verified                                         |
| -------------------------------------------- | --------------------------------------------------------- |
| `round_trip_small`                           | encode→decode roundtrip for 11-byte input                 |
| `round_trip_empty`                           | empty plaintext: total_chunks=0, decode returns empty vec |
| `round_trip_1mb`                             | 1MB input roundtrip (4 chunks of 256KB)                   |
| `header_starts_with_magic`                   | first 4 bytes equal V2_MAGIC                              |
| `two_chunk_input_has_total_chunks_2`         | 512KB input → total_chunks=2 at bytes [20..24]            |
| `tampered_ciphertext_returns_decrypt_failed` | flipped bit → DecryptFailed error                         |
| `wrong_magic_returns_invalid_magic`          | non-V2 data → InvalidMagic error                          |
| `wrong_key_returns_decrypt_failed`           | wrong master_key → DecryptFailed error                    |
| `swapped_chunks_aad_mismatch`                | swapped chunk ciphertexts → DecryptFailed (AAD mismatch)  |

## Deviations from Plan

### Auto-applied Improvements

**1. [Rule 1 - Code Quality] Removed duplicate total_chunks calculation**

- The plan template showed two versions of the `total_chunks` calculation. Used only the correct `if plaintext.is_empty()` version.
- No plan behavior was changed.

**2. [Style] Used thiserror::Error derive instead of manual Display impl**

- Plan noted: "if thiserror is present, MAY use `#[derive(thiserror::Error)]`"
- Confirmed `thiserror = "2.0"` in uc-infra Cargo.toml, used the derive macro.

**3. [Tool] cargo fmt reordered module declarations**

- Pre-commit hook ran `cargo fmt`, which sorted `pub mod chunked_transfer` alphabetically and reordered `pub use chunked_transfer::...` within mod.rs. Accepted as correct formatting.

## Self-Check: PASSED

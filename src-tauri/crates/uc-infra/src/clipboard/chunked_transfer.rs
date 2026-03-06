//! Chunk-level AEAD streaming encoder and decoder for clipboard wire transfer.
//!
//! V3-only wire format with zstd compression for payloads exceeding `COMPRESSION_THRESHOLD`.
//!
//! # Memory Contract (LOCKED -- from CONTEXT.md)
//! Memory usage is bounded by CHUNK_SIZE x 2 regardless of total payload size:
//! - Encoder: one plaintext chunk slice (no copy) + one ciphertext Vec<u8> per iteration.
//! - Decoder: one ciphertext Vec<u8> + one plaintext Vec<u8> per chunk, appended to output.
//!
//! # V3 Wire Format (37-byte header)
//! ```text
//! [4 bytes]  magic: 0x55 0x43 0x33 0x00 ("UC3\0")
//! [1 byte]   compression_algo (0=none, 1=zstd)
//! [4 bytes]  uncompressed_len (u32 LE)
//! [16 bytes] transfer_id (UUID v4 raw bytes)
//! [4 bytes]  total_chunks (u32 LE)
//! [4 bytes]  chunk_size_hint (u32 LE)
//! [4 bytes]  total_plaintext_len (u32 LE) -- compressed size when compression active
//! then for each chunk i in 0..total_chunks:
//!   [4 bytes]  chunk_ciphertext_len (u32 LE)
//!   [N bytes]  ciphertext (plaintext_chunk + 16-byte Poly1305 tag)
//! ```

use std::io::{Cursor, Read, Write};

use chacha20poly1305::{
    aead::{Aead, Payload},
    KeyInit, XChaCha20Poly1305, XNonce,
};
use uc_core::config::RECEIVE_PLAINTEXT_CAP;
use uc_core::ports::{
    TransferCryptoError, TransferPayloadDecryptorPort, TransferPayloadEncryptorPort,
};
use uc_core::security::{aad, model::MasterKey};
use uuid::Uuid;

/// Nominal chunk size: 256 KB.
/// Peak memory per encode or decode call: ~2 x CHUNK_SIZE.
pub const CHUNK_SIZE: usize = 256 * 1024;

/// Magic bytes identifying a V3 chunked clipboard payload ("UC3\0").
pub const V3_MAGIC: [u8; 4] = [0x55, 0x43, 0x33, 0x00];

/// V3 header size in bytes: magic(4) + compression_algo(1) + uncompressed_len(4)
/// + transfer_id(16) + total_chunks(4) + chunk_size_hint(4) + total_plaintext_len(4).
pub const V3_HEADER_SIZE: usize = 37;

/// Payloads larger than this threshold are compressed with zstd before encryption.
pub const COMPRESSION_THRESHOLD: usize = 8 * 1024;

/// Maximum allowed decompressed size (128 MiB).
///
/// Bounds the allocation hint passed to `zstd::bulk::decompress` so that a
/// malicious header cannot trigger a multi-gigabyte allocation via a forged
/// `uncompressed_len` field.  128 MiB is generous for any realistic clipboard
/// content (text, images, rich-text) while keeping the OOM surface small.
pub const MAX_DECOMPRESSED_SIZE: usize = RECEIVE_PLAINTEXT_CAP;

/// Zstd compression level (consistent with Phase 4 blob at-rest choice).
pub const ZSTD_LEVEL: i32 = 3;

/// Errors that can occur during chunked transfer encoding or decoding.
///
/// These are wire-format implementation details, internal to uc-infra.
/// Adapters map these to `TransferCryptoError` at the port boundary.
#[derive(Debug, thiserror::Error)]
pub enum ChunkedTransferError {
    /// First 4 bytes do not match V3_MAGIC.
    #[error("invalid magic bytes")]
    InvalidMagic,
    /// Stream ended before the fixed-size header was fully read.
    #[error("stream ended before header was complete")]
    TruncatedHeader,
    /// Stream ended before a chunk's ciphertext was fully read.
    #[error("stream ended before chunk ciphertext was complete")]
    TruncatedChunk,
    /// AEAD tag verification failed for the given chunk index.
    #[error("AEAD decryption failed for chunk {chunk_index}")]
    DecryptFailed { chunk_index: u32 },
    /// Ciphertext length from wire is outside valid range.
    #[error("chunk {chunk_index}: ciphertext_len {ciphertext_len} outside valid range")]
    InvalidCiphertextLen {
        chunk_index: u32,
        ciphertext_len: usize,
    },
    /// Header declares a total_plaintext_len inconsistent with chunk count.
    #[error("header validation failed: {reason}")]
    InvalidHeader { reason: String },
    /// AEAD encryption failed (key size error).
    #[error("encryption failed: {0}")]
    EncryptFailed(String),
    /// Zstd compression failed.
    #[error("compression failed: {reason}")]
    CompressionFailed { reason: String },
    /// Zstd decompression failed.
    #[error("decompression failed: {reason}")]
    DecompressionFailed { reason: String },
    /// Unknown compression algorithm in V3 header.
    #[error("invalid compression algorithm: {algo}")]
    InvalidCompressionAlgo { algo: u8 },
    /// Underlying IO error while reading or writing.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<ChunkedTransferError> for TransferCryptoError {
    fn from(e: ChunkedTransferError) -> Self {
        match e {
            ChunkedTransferError::EncryptFailed(msg) => TransferCryptoError::EncryptionFailed(msg),
            ChunkedTransferError::DecryptFailed { chunk_index } => {
                TransferCryptoError::DecryptionFailed(format!(
                    "AEAD decryption failed for chunk {chunk_index}"
                ))
            }
            ChunkedTransferError::InvalidCiphertextLen {
                chunk_index,
                ciphertext_len,
            } => TransferCryptoError::InvalidFormat(format!(
                "chunk {chunk_index}: ciphertext_len {ciphertext_len} outside valid range"
            )),
            ChunkedTransferError::InvalidHeader { reason } => {
                TransferCryptoError::InvalidFormat(reason)
            }
            ChunkedTransferError::InvalidMagic => {
                TransferCryptoError::InvalidFormat("invalid magic bytes".into())
            }
            ChunkedTransferError::TruncatedHeader => {
                TransferCryptoError::InvalidFormat("stream ended before header was complete".into())
            }
            ChunkedTransferError::TruncatedChunk => TransferCryptoError::InvalidFormat(
                "stream ended before chunk ciphertext was complete".into(),
            ),
            ChunkedTransferError::CompressionFailed { reason } => {
                TransferCryptoError::EncryptionFailed(format!("compression failed: {reason}"))
            }
            ChunkedTransferError::DecompressionFailed { reason } => {
                TransferCryptoError::DecryptionFailed(format!("decompression failed: {reason}"))
            }
            ChunkedTransferError::InvalidCompressionAlgo { algo } => {
                TransferCryptoError::InvalidFormat(format!("invalid compression algorithm: {algo}"))
            }
            ChunkedTransferError::Io(e) => {
                TransferCryptoError::EncryptionFailed(format!("IO error: {e}"))
            }
        }
    }
}

/// Streaming encoder for V3 chunked clipboard transfers with compression support.
pub struct ChunkedEncoder;

/// Streaming decoder for V3 chunked clipboard transfers with decompression support.
pub struct ChunkedDecoder;

impl ChunkedEncoder {
    /// Encode `plaintext` in V3 streaming wire format, writing directly to `writer`.
    ///
    /// The caller decides compression: pass `compression_algo=1` with pre-compressed data,
    /// or `compression_algo=0` with raw plaintext. `uncompressed_len` is always the
    /// original (uncompressed) plaintext length.
    ///
    /// # Arguments
    /// * `writer`           -- destination implementing `std::io::Write`
    /// * `master_key`       -- 32-byte XChaCha20-Poly1305 key
    /// * `transfer_id`      -- 16-byte transfer identifier (UUID v4 raw bytes)
    /// * `plaintext`        -- input bytes to chunk and encrypt (possibly compressed)
    /// * `compression_algo` -- 0=none, 1=zstd
    /// * `uncompressed_len` -- original plaintext length before compression
    pub fn encode_to<W: Write>(
        mut writer: W,
        master_key: &MasterKey,
        transfer_id: &[u8; 16],
        plaintext: &[u8],
        compression_algo: u8,
        uncompressed_len: u32,
    ) -> Result<(), ChunkedTransferError> {
        let cipher = XChaCha20Poly1305::new_from_slice(master_key.as_bytes())
            .map_err(|e| ChunkedTransferError::EncryptFailed(e.to_string()))?;

        let total_plaintext_len = u32::try_from(plaintext.len()).map_err(|_| {
            ChunkedTransferError::EncryptFailed(format!(
                "plaintext length {} exceeds u32::MAX",
                plaintext.len()
            ))
        })?;
        let total_chunks = if plaintext.is_empty() {
            0u32
        } else {
            ((plaintext.len() + CHUNK_SIZE - 1) / CHUNK_SIZE) as u32
        };

        // Write V3 header (37 bytes total):
        //   [0..4]   magic
        //   [4]      compression_algo
        //   [5..9]   uncompressed_len
        //   [9..25]  transfer_id
        //   [25..29] total_chunks
        //   [29..33] chunk_size_hint
        //   [33..37] total_plaintext_len
        writer.write_all(&V3_MAGIC)?;
        writer.write_all(&[compression_algo])?;
        writer.write_all(&uncompressed_len.to_le_bytes())?;
        writer.write_all(transfer_id)?;
        writer.write_all(&total_chunks.to_le_bytes())?;
        writer.write_all(&(CHUNK_SIZE as u32).to_le_bytes())?;
        writer.write_all(&total_plaintext_len.to_le_bytes())?;

        // Write chunks incrementally -- at most one ciphertext Vec<u8> in memory at a time
        for (chunk_index, plaintext_chunk) in plaintext.chunks(CHUNK_SIZE).enumerate() {
            let chunk_index = chunk_index as u32;
            let nonce_bytes = derive_chunk_nonce(transfer_id, chunk_index);
            let aad_bytes = aad::for_chunk_transfer(transfer_id, chunk_index);

            let ciphertext = cipher
                .encrypt(
                    XNonce::from_slice(&nonce_bytes),
                    Payload {
                        msg: plaintext_chunk,
                        aad: &aad_bytes,
                    },
                )
                .map_err(|e| ChunkedTransferError::EncryptFailed(e.to_string()))?;

            writer.write_all(&(ciphertext.len() as u32).to_le_bytes())?;
            writer.write_all(&ciphertext)?;
        }

        Ok(())
    }
}

impl ChunkedDecoder {
    /// Decode a V3 streaming wire format from `reader`, returning assembled plaintext.
    ///
    /// If the V3 header indicates compression (compression_algo=1), the decrypted data
    /// is decompressed with zstd using the `uncompressed_len` from the header.
    ///
    /// # Arguments
    /// * `reader`     -- source implementing `std::io::Read`
    /// * `master_key` -- 32-byte XChaCha20-Poly1305 key
    pub fn decode_from<R: Read>(
        mut reader: R,
        master_key: &MasterKey,
    ) -> Result<Vec<u8>, ChunkedTransferError> {
        // Read V3 header: 4 + 1 + 4 + 16 + 4 + 4 + 4 = 37 bytes
        let mut header = [0u8; V3_HEADER_SIZE];
        reader
            .read_exact(&mut header)
            .map_err(|_| ChunkedTransferError::TruncatedHeader)?;

        if header[0..4] != V3_MAGIC {
            return Err(ChunkedTransferError::InvalidMagic);
        }

        let compression_algo = header[4];
        let uncompressed_len = u32::from_le_bytes(
            header[5..9]
                .try_into()
                .map_err(|_| ChunkedTransferError::TruncatedHeader)?,
        ) as usize;
        let transfer_id: [u8; 16] = header[9..25]
            .try_into()
            .map_err(|_| ChunkedTransferError::TruncatedHeader)?;
        let total_chunks = u32::from_le_bytes(
            header[25..29]
                .try_into()
                .map_err(|_| ChunkedTransferError::TruncatedHeader)?,
        );
        // chunk_size_hint at [29..33] -- not needed for decode
        let total_plaintext_len = u32::from_le_bytes(
            header[33..37]
                .try_into()
                .map_err(|_| ChunkedTransferError::TruncatedHeader)?,
        ) as usize;

        // Validate header consistency
        if total_chunks > 0 && total_plaintext_len == 0 {
            return Err(ChunkedTransferError::InvalidHeader {
                reason: "total_chunks > 0 but total_plaintext_len is 0".into(),
            });
        }
        let max_capacity = (total_chunks as usize)
            .checked_mul(CHUNK_SIZE)
            .ok_or_else(|| ChunkedTransferError::InvalidHeader {
                reason: format!(
                    "total_chunks {} * CHUNK_SIZE {} overflows usize",
                    total_chunks, CHUNK_SIZE
                ),
            })?;
        if total_plaintext_len > max_capacity {
            return Err(ChunkedTransferError::InvalidHeader {
                reason: format!(
                    "total_plaintext_len {} exceeds maximum capacity {} (total_chunks {} * CHUNK_SIZE {})",
                    total_plaintext_len, max_capacity, total_chunks, CHUNK_SIZE
                ),
            });
        }
        if total_plaintext_len > MAX_DECOMPRESSED_SIZE {
            return Err(ChunkedTransferError::InvalidHeader {
                reason: format!(
                    "total_plaintext_len {} exceeds MAX_DECOMPRESSED_SIZE {}",
                    total_plaintext_len, MAX_DECOMPRESSED_SIZE
                ),
            });
        }

        // Validate uncompressed_len against safe ceiling to prevent OOM from
        // forged headers.
        match compression_algo {
            0 => {
                // No compression: uncompressed_len must equal total_plaintext_len.
                if uncompressed_len != total_plaintext_len {
                    return Err(ChunkedTransferError::InvalidHeader {
                        reason: format!(
                            "compression_algo=0 but uncompressed_len {} != total_plaintext_len {}",
                            uncompressed_len, total_plaintext_len
                        ),
                    });
                }
            }
            1 => {
                if uncompressed_len > MAX_DECOMPRESSED_SIZE {
                    return Err(ChunkedTransferError::InvalidHeader {
                        reason: format!(
                            "uncompressed_len {} exceeds MAX_DECOMPRESSED_SIZE {}",
                            uncompressed_len, MAX_DECOMPRESSED_SIZE
                        ),
                    });
                }
            }
            _ => {} // handled later by the match on compression_algo
        }

        let cipher = XChaCha20Poly1305::new_from_slice(master_key.as_bytes())
            .map_err(|e| ChunkedTransferError::EncryptFailed(e.to_string()))?;

        let bounded_prealloc = total_plaintext_len.min(MAX_DECOMPRESSED_SIZE);
        let mut decrypted = Vec::with_capacity(bounded_prealloc);

        for chunk_index in 0..total_chunks {
            let mut len_buf = [0u8; 4];
            reader
                .read_exact(&mut len_buf)
                .map_err(|_| ChunkedTransferError::TruncatedChunk)?;
            let ciphertext_len = u32::from_le_bytes(len_buf) as usize;

            const TAG_SIZE: usize = 16;
            let max_ciphertext = CHUNK_SIZE + TAG_SIZE;
            if ciphertext_len < TAG_SIZE || ciphertext_len > max_ciphertext {
                return Err(ChunkedTransferError::InvalidCiphertextLen {
                    chunk_index,
                    ciphertext_len,
                });
            }

            let mut ciphertext = vec![0u8; ciphertext_len];
            reader
                .read_exact(&mut ciphertext)
                .map_err(|_| ChunkedTransferError::TruncatedChunk)?;

            let nonce_bytes = derive_chunk_nonce(&transfer_id, chunk_index);
            let aad_bytes = aad::for_chunk_transfer(&transfer_id, chunk_index);

            let chunk_plaintext = cipher
                .decrypt(
                    XNonce::from_slice(&nonce_bytes),
                    Payload {
                        msg: &ciphertext,
                        aad: &aad_bytes,
                    },
                )
                .map_err(|_| ChunkedTransferError::DecryptFailed { chunk_index })?;

            decrypted.extend_from_slice(&chunk_plaintext);
        }

        if decrypted.len() != total_plaintext_len {
            return Err(ChunkedTransferError::InvalidHeader {
                reason: format!(
                    "decoded {} bytes but header declared {}",
                    decrypted.len(),
                    total_plaintext_len
                ),
            });
        }

        // Post-decrypt decompression
        match compression_algo {
            0 => Ok(decrypted),
            1 => zstd::bulk::decompress(&decrypted, uncompressed_len).map_err(|e| {
                ChunkedTransferError::DecompressionFailed {
                    reason: e.to_string(),
                }
            }),
            other => Err(ChunkedTransferError::InvalidCompressionAlgo { algo: other }),
        }
    }
}

/// Adapter implementing `TransferPayloadEncryptorPort` via `ChunkedEncoder`.
///
/// Generates `transfer_id` internally (UUID v4) -- callers do not need to manage it.
/// Compresses payloads exceeding `COMPRESSION_THRESHOLD` with zstd before encryption.
pub struct TransferPayloadEncryptorAdapter;

impl TransferPayloadEncryptorPort for TransferPayloadEncryptorAdapter {
    fn encrypt(
        &self,
        master_key: &MasterKey,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, TransferCryptoError> {
        let transfer_id: [u8; 16] = *Uuid::new_v4().as_bytes();

        let uncompressed_len = u32::try_from(plaintext.len()).map_err(|_| {
            TransferCryptoError::EncryptionFailed(format!(
                "plaintext length {} exceeds u32::MAX",
                plaintext.len()
            ))
        })?;

        let (data_to_encrypt, compression_algo) = if plaintext.len() > COMPRESSION_THRESHOLD {
            let compressed = zstd::bulk::compress(plaintext, ZSTD_LEVEL).map_err(|e| {
                TransferCryptoError::EncryptionFailed(format!("compression failed: {e}"))
            })?;

            if compressed.len() < plaintext.len() {
                (compressed, 1u8)
            } else {
                (plaintext.to_vec(), 0u8)
            }
        } else {
            (plaintext.to_vec(), 0u8)
        };

        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(
            &mut buf,
            master_key,
            &transfer_id,
            &data_to_encrypt,
            compression_algo,
            uncompressed_len,
        )?;
        Ok(buf)
    }
}

/// Adapter implementing `TransferPayloadDecryptorPort` via `ChunkedDecoder`.
///
/// Only accepts V3 (`UC3\0`) wire format.
pub struct TransferPayloadDecryptorAdapter;

impl TransferPayloadDecryptorPort for TransferPayloadDecryptorAdapter {
    fn decrypt(
        &self,
        encrypted: &[u8],
        master_key: &MasterKey,
    ) -> Result<Vec<u8>, TransferCryptoError> {
        if encrypted.len() < 4 {
            return Err(TransferCryptoError::InvalidFormat(
                "data too short to contain magic bytes".into(),
            ));
        }

        let magic = &encrypted[0..4];
        if magic == V3_MAGIC {
            ChunkedDecoder::decode_from(Cursor::new(encrypted), master_key)
                .map_err(TransferCryptoError::from)
        } else {
            Err(TransferCryptoError::InvalidFormat(
                "unrecognized wire format magic bytes".into(),
            ))
        }
    }
}

/// Derive a 24-byte XChaCha20 nonce for a given chunk.
///
/// `nonce = blake3("uc:chunk-nonce:v1|" || transfer_id || chunk_index_le)[0..24]`
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use uc_core::security::model::MasterKey;

    fn test_key() -> MasterKey {
        MasterKey([0u8; 32])
    }

    fn test_key_alt() -> MasterKey {
        MasterKey([1u8; 32])
    }

    fn test_transfer_id() -> [u8; 16] {
        [0x42u8; 16]
    }

    fn round_trip_raw(plaintext: &[u8], compression_algo: u8) -> Vec<u8> {
        let key = test_key();
        let id = test_transfer_id();
        let uncompressed_len = plaintext.len() as u32;
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(
            &mut buf,
            &key,
            &id,
            plaintext,
            compression_algo,
            uncompressed_len,
        )
        .expect("encode_to failed");
        ChunkedDecoder::decode_from(Cursor::new(buf), &key).expect("decode_from failed")
    }

    #[test]
    fn round_trip_small_no_compression() {
        let pt = b"hello V3 world";
        let result = round_trip_raw(pt, 0);
        assert_eq!(result, pt);
    }

    #[test]
    fn round_trip_empty() {
        assert_eq!(round_trip_raw(b"", 0), b"");
    }

    #[test]
    fn round_trip_large_with_compression() {
        let pt = vec![0x42u8; 16 * 1024];
        let compressed = zstd::bulk::compress(&pt, ZSTD_LEVEL).unwrap();
        let key = test_key();
        let id = test_transfer_id();
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, &compressed, 1, pt.len() as u32).unwrap();
        let result = ChunkedDecoder::decode_from(Cursor::new(buf), &key).unwrap();
        assert_eq!(result, pt);
    }

    #[test]
    fn round_trip_10mb() {
        let pt = vec![0xABu8; 10 * 1024 * 1024];
        let compressed = zstd::bulk::compress(&pt, ZSTD_LEVEL).unwrap();
        let key = test_key();
        let id = test_transfer_id();
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, &compressed, 1, pt.len() as u32).unwrap();
        let result = ChunkedDecoder::decode_from(Cursor::new(buf), &key).unwrap();
        assert_eq!(result, pt);
    }

    #[test]
    fn header_magic_and_size() {
        let key = test_key();
        let id = test_transfer_id();
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, b"test", 0, 4).unwrap();
        assert_eq!(&buf[0..4], &V3_MAGIC);
        assert!(buf.len() >= V3_HEADER_SIZE);
    }

    #[test]
    fn compression_flag_no_compression() {
        let key = test_key();
        let id = test_transfer_id();
        let small = vec![0u8; 100];
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, &small, 0, small.len() as u32).unwrap();
        assert_eq!(buf[4], 0);
    }

    #[test]
    fn compression_flag_with_compression() {
        let key = test_key();
        let id = test_transfer_id();
        let large = vec![0u8; 16 * 1024];
        let compressed = zstd::bulk::compress(&large, ZSTD_LEVEL).unwrap();
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, &compressed, 1, large.len() as u32).unwrap();
        assert_eq!(buf[4], 1);
    }

    #[test]
    fn uncompressed_len_field() {
        let key = test_key();
        let id = test_transfer_id();
        let original_len = 12345u32;
        let data = vec![0u8; 100];
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, &data, 0, original_len).unwrap();
        let stored_len = u32::from_le_bytes(buf[5..9].try_into().unwrap());
        assert_eq!(stored_len, original_len);
    }

    #[test]
    fn total_plaintext_len_equals_input() {
        let key = test_key();
        let id = test_transfer_id();
        let data = vec![0u8; 500];
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, &data, 0, 500).unwrap();
        let total_pt_len = u32::from_le_bytes(buf[33..37].try_into().unwrap());
        assert_eq!(total_pt_len, 500);
    }

    #[test]
    fn tampered_ciphertext_returns_decrypt_failed() {
        let key = test_key();
        let id = test_transfer_id();
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, b"secret", 0, 6).unwrap();
        buf[41] ^= 0xFF;
        let result = ChunkedDecoder::decode_from(Cursor::new(buf), &key);
        assert!(matches!(
            result,
            Err(ChunkedTransferError::DecryptFailed { .. })
        ));
    }

    #[test]
    fn wrong_magic_returns_invalid_magic() {
        let key = test_key();
        let buf = vec![0xFFu8; 64];
        let result = ChunkedDecoder::decode_from(Cursor::new(buf), &key);
        assert!(matches!(result, Err(ChunkedTransferError::InvalidMagic)));
    }

    #[test]
    fn wrong_key_returns_decrypt_failed() {
        let key1 = test_key();
        let key2 = test_key_alt();
        let id = test_transfer_id();
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key1, &id, b"data", 0, 4).unwrap();
        let result = ChunkedDecoder::decode_from(Cursor::new(buf), &key2);
        assert!(matches!(
            result,
            Err(ChunkedTransferError::DecryptFailed { .. })
        ));
    }

    #[test]
    fn invalid_compression_algo() {
        let key = test_key();
        let id = test_transfer_id();
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, b"test", 0, 4).unwrap();
        buf[4] = 99;
        let result = ChunkedDecoder::decode_from(Cursor::new(buf), &key);
        assert!(matches!(
            result,
            Err(ChunkedTransferError::InvalidCompressionAlgo { algo: 99 })
        ));
    }

    #[test]
    fn adapter_round_trip_small_no_compression() {
        let key = test_key();
        let encryptor = TransferPayloadEncryptorAdapter;
        let decryptor = TransferPayloadDecryptorAdapter;
        let plaintext = b"small adapter test";
        let encrypted = encryptor.encrypt(&key, plaintext).expect("encrypt");
        assert_eq!(&encrypted[0..4], &V3_MAGIC);
        assert_eq!(encrypted[4], 0);
        let decrypted = decryptor.decrypt(&encrypted, &key).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn adapter_round_trip_large_with_compression() {
        let key = test_key();
        let encryptor = TransferPayloadEncryptorAdapter;
        let decryptor = TransferPayloadDecryptorAdapter;
        let plaintext = vec![0x42u8; 16 * 1024];
        let encrypted = encryptor.encrypt(&key, &plaintext).expect("encrypt");
        assert_eq!(&encrypted[0..4], &V3_MAGIC);
        assert_eq!(encrypted[4], 1);
        let decrypted = decryptor.decrypt(&encrypted, &key).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn adapter_round_trip_10mb() {
        let key = test_key();
        let encryptor = TransferPayloadEncryptorAdapter;
        let decryptor = TransferPayloadDecryptorAdapter;
        let plaintext = vec![0xCDu8; 10 * 1024 * 1024];
        let encrypted = encryptor.encrypt(&key, &plaintext).expect("encrypt");
        let decrypted = decryptor.decrypt(&encrypted, &key).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn adapter_uses_no_compression_when_zstd_not_smaller() {
        let key = test_key();
        let encryptor = TransferPayloadEncryptorAdapter;

        let mut plaintext = Vec::with_capacity(16 * 1024);
        let mut seed: u64 = 0x0123_4567_89AB_CDEF;
        for _ in 0..(16 * 1024) {
            // xorshift64* for deterministic pseudo-random bytes.
            seed ^= seed >> 12;
            seed ^= seed << 25;
            seed ^= seed >> 27;
            let byte = seed.wrapping_mul(0x2545_F491_4F6C_DD1D) as u8;
            plaintext.push(byte);
        }

        let encrypted = encryptor.encrypt(&key, &plaintext).expect("encrypt");
        assert_eq!(
            encrypted[4], 0,
            "incompressible payload should not be marked zstd"
        );
    }

    #[test]
    fn rejects_uncompressed_len_exceeding_max_decompressed_size() {
        // Forge a valid V3 header with compression_algo=1 and
        // uncompressed_len = MAX_DECOMPRESSED_SIZE + 1, which should be
        // rejected before any zstd allocation occurs.
        let key = test_key();
        let id = test_transfer_id();

        // Encode a small compressed payload normally first.
        let pt = vec![0x42u8; 16 * 1024];
        let compressed = zstd::bulk::compress(&pt, ZSTD_LEVEL).unwrap();
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, &compressed, 1, pt.len() as u32).unwrap();

        // Tamper uncompressed_len field (bytes 5..9) to exceed the ceiling.
        let forged_len = (MAX_DECOMPRESSED_SIZE as u32).saturating_add(1);
        buf[5..9].copy_from_slice(&forged_len.to_le_bytes());

        let result = ChunkedDecoder::decode_from(Cursor::new(buf), &key);
        assert!(
            matches!(result, Err(ChunkedTransferError::InvalidHeader { .. })),
            "expected InvalidHeader for oversized uncompressed_len, got: {result:?}"
        );
    }

    #[test]
    fn rejects_uncompressed_len_mismatch_when_uncompressed() {
        // When compression_algo=0, uncompressed_len must equal total_plaintext_len.
        let key = test_key();
        let id = test_transfer_id();

        let pt = b"hello world";
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, pt, 0, pt.len() as u32).unwrap();

        // Tamper uncompressed_len to a different value.
        let forged_len = 99999u32;
        buf[5..9].copy_from_slice(&forged_len.to_le_bytes());

        let result = ChunkedDecoder::decode_from(Cursor::new(buf), &key);
        assert!(
            matches!(result, Err(ChunkedTransferError::InvalidHeader { .. })),
            "expected InvalidHeader for mismatched uncompressed_len, got: {result:?}"
        );
    }

    #[test]
    fn adapter_decryptor_rejects_unknown_magic() {
        let key = test_key();
        let decryptor = TransferPayloadDecryptorAdapter;
        let buf = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00];
        let result = decryptor.decrypt(&buf, &key);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unrecognized wire format"),
            "unexpected error: {err_msg}"
        );
    }
}

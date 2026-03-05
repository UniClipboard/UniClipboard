//! Chunk-level AEAD streaming encoder and decoder for V2 clipboard wire transfer.
//!
//! # Memory Contract (LOCKED — from CONTEXT.md)
//! Memory usage is bounded by CHUNK_SIZE × 2 regardless of total payload size:
//! - Encoder: one plaintext chunk slice (no copy) + one ciphertext Vec<u8> per iteration.
//! - Decoder: one ciphertext Vec<u8> + one plaintext Vec<u8> per chunk, appended to output.
//!
//! # Wire Format (sequential, no index table needed for streaming)
//! ```text
//! [4 bytes]  magic: 0x55 0x43 0x32 0x00 ("UC2\0")
//! [16 bytes] transfer_id (UUID v4 raw bytes, little-endian UUID byte order)
//! [4 bytes]  total_chunks (u32 LE)
//! [4 bytes]  chunk_size_hint (u32 LE)
//! [4 bytes]  total_plaintext_len (u32 LE)
//! then for each chunk i in 0..total_chunks:
//!   [4 bytes]  chunk_ciphertext_len (u32 LE)
//!   [N bytes]  ciphertext (plaintext_chunk + 16-byte Poly1305 tag)
//! ```

use std::io::{Cursor, Read, Write};

use chacha20poly1305::{
    aead::{Aead, Payload},
    KeyInit, XChaCha20Poly1305, XNonce,
};
use uc_core::ports::{
    TransferCryptoError, TransferPayloadDecryptorPort, TransferPayloadEncryptorPort,
};
use uc_core::security::{aad, model::MasterKey};
use uuid::Uuid;

/// Nominal chunk size: 256 KB.
/// Peak memory per encode or decode call: ~2 × CHUNK_SIZE.
pub const CHUNK_SIZE: usize = 256 * 1024;

/// Magic bytes identifying a V2 chunked clipboard payload ("UC2\0").
pub const V2_MAGIC: [u8; 4] = [0x55, 0x43, 0x32, 0x00];

/// Errors that can occur during chunked transfer encoding or decoding.
///
/// These are wire-format implementation details, internal to uc-infra.
/// Adapters map these to `TransferCryptoError` at the port boundary.
#[derive(Debug, thiserror::Error)]
pub enum ChunkedTransferError {
    /// First 4 bytes are not V2_MAGIC.
    #[error("invalid V2 magic bytes")]
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
                TransferCryptoError::InvalidFormat("invalid V2 magic bytes".into())
            }
            ChunkedTransferError::TruncatedHeader => {
                TransferCryptoError::InvalidFormat("stream ended before header was complete".into())
            }
            ChunkedTransferError::TruncatedChunk => TransferCryptoError::InvalidFormat(
                "stream ended before chunk ciphertext was complete".into(),
            ),
            ChunkedTransferError::Io(e) => {
                TransferCryptoError::EncryptionFailed(format!("IO error: {e}"))
            }
        }
    }
}

/// Streaming encoder for V2 chunked clipboard transfers.
pub struct ChunkedEncoder;

/// Streaming decoder for V2 chunked clipboard transfers.
pub struct ChunkedDecoder;

impl ChunkedEncoder {
    /// Encode `plaintext` in V2 streaming wire format, writing directly to `writer`.
    ///
    /// Memory usage is bounded by CHUNK_SIZE × 2 regardless of `plaintext.len()`.
    /// Each chunk is encrypted and flushed to `writer` before the next chunk is processed.
    ///
    /// # Arguments
    /// * `writer`      — destination implementing `std::io::Write` (e.g., libp2p stream)
    /// * `master_key`  — 32-byte XChaCha20-Poly1305 key
    /// * `transfer_id` — 16-byte transfer identifier (UUID v4 raw bytes)
    /// * `plaintext`   — input bytes to chunk and encrypt
    pub fn encode_to<W: Write>(
        mut writer: W,
        master_key: &MasterKey,
        transfer_id: &[u8; 16],
        plaintext: &[u8],
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

        // Write fixed header (32 bytes total):
        //   [0..4]   magic
        //   [4..20]  transfer_id
        //   [20..24] total_chunks
        //   [24..28] chunk_size_hint
        //   [28..32] total_plaintext_len
        writer.write_all(&V2_MAGIC)?;
        writer.write_all(transfer_id)?;
        writer.write_all(&total_chunks.to_le_bytes())?;
        writer.write_all(&(CHUNK_SIZE as u32).to_le_bytes())?;
        writer.write_all(&total_plaintext_len.to_le_bytes())?;

        // Write chunks incrementally — at most one ciphertext Vec<u8> in memory at a time
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
            // `ciphertext` is dropped here — only one chunk buffer is alive per iteration
        }

        Ok(())
    }
}

impl ChunkedDecoder {
    /// Decode a V2 streaming wire format from `reader`, returning assembled plaintext.
    ///
    /// Reads chunks one at a time using `read_exact`. Does NOT call `read_to_end`.
    /// On any chunk AEAD failure, returns `Err` immediately (no partial data retained).
    ///
    /// # Arguments
    /// * `reader`     — source implementing `std::io::Read` (e.g., libp2p stream)
    /// * `master_key` — 32-byte XChaCha20-Poly1305 key
    pub fn decode_from<R: Read>(
        mut reader: R,
        master_key: &MasterKey,
    ) -> Result<Vec<u8>, ChunkedTransferError> {
        // Read fixed header: 4 + 16 + 4 + 4 + 4 = 32 bytes
        let mut header = [0u8; 32];
        reader
            .read_exact(&mut header)
            .map_err(|_| ChunkedTransferError::TruncatedHeader)?;

        if header[0..4] != V2_MAGIC {
            return Err(ChunkedTransferError::InvalidMagic);
        }

        let transfer_id: [u8; 16] = header[4..20]
            .try_into()
            .map_err(|_| ChunkedTransferError::TruncatedHeader)?;
        let total_chunks = u32::from_le_bytes(
            header[20..24]
                .try_into()
                .map_err(|_| ChunkedTransferError::TruncatedHeader)?,
        );
        // chunk_size_hint at [24..28] — not needed for decode; skip
        let total_plaintext_len = u32::from_le_bytes(
            header[28..32]
                .try_into()
                .map_err(|_| ChunkedTransferError::TruncatedHeader)?,
        ) as usize;

        // Validate header consistency: plaintext cannot exceed what total_chunks can hold.
        if total_chunks > 0 && total_plaintext_len == 0 {
            return Err(ChunkedTransferError::InvalidHeader {
                reason: "total_chunks > 0 but total_plaintext_len is 0".into(),
            });
        }
        if total_plaintext_len > total_chunks as usize * CHUNK_SIZE {
            return Err(ChunkedTransferError::InvalidHeader {
                reason: format!(
                    "total_plaintext_len {} exceeds maximum capacity {} (total_chunks {} * CHUNK_SIZE {})",
                    total_plaintext_len, total_chunks as usize * CHUNK_SIZE, total_chunks, CHUNK_SIZE
                ),
            });
        }

        let cipher = XChaCha20Poly1305::new_from_slice(master_key.as_bytes())
            .map_err(|e| ChunkedTransferError::EncryptFailed(e.to_string()))?;

        let mut plaintext = Vec::with_capacity(total_plaintext_len);

        for chunk_index in 0..total_chunks {
            // Read 4-byte ciphertext length prefix
            let mut len_buf = [0u8; 4];
            reader
                .read_exact(&mut len_buf)
                .map_err(|_| ChunkedTransferError::TruncatedChunk)?;
            let ciphertext_len = u32::from_le_bytes(len_buf) as usize;

            // XChaCha20-Poly1305 tag is 16 bytes. Valid ciphertext must contain at least
            // the tag, and at most one full chunk of plaintext + tag.
            const TAG_SIZE: usize = 16;
            let max_ciphertext = CHUNK_SIZE + TAG_SIZE;
            if ciphertext_len < TAG_SIZE || ciphertext_len > max_ciphertext {
                return Err(ChunkedTransferError::InvalidCiphertextLen {
                    chunk_index,
                    ciphertext_len,
                });
            }

            // Read ciphertext — one chunk in memory at a time
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

            plaintext.extend_from_slice(&chunk_plaintext);
            // `ciphertext` and `chunk_plaintext` are dropped here
        }

        if plaintext.len() != total_plaintext_len {
            return Err(ChunkedTransferError::InvalidHeader {
                reason: format!(
                    "decoded {} bytes but header declared {}",
                    plaintext.len(),
                    total_plaintext_len
                ),
            });
        }

        Ok(plaintext)
    }
}

/// Adapter implementing `TransferPayloadEncryptorPort` via `ChunkedEncoder`.
///
/// Generates `transfer_id` internally (UUID v4) — callers do not need to manage it.
pub struct TransferPayloadEncryptorAdapter;

impl TransferPayloadEncryptorPort for TransferPayloadEncryptorAdapter {
    fn encrypt(
        &self,
        master_key: &MasterKey,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, TransferCryptoError> {
        let transfer_id: [u8; 16] = *Uuid::new_v4().as_bytes();
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, master_key, &transfer_id, plaintext)?;
        Ok(buf)
    }
}

/// Adapter implementing `TransferPayloadDecryptorPort` via `ChunkedDecoder`.
pub struct TransferPayloadDecryptorAdapter;

impl TransferPayloadDecryptorPort for TransferPayloadDecryptorAdapter {
    fn decrypt(
        &self,
        encrypted: &[u8],
        master_key: &MasterKey,
    ) -> Result<Vec<u8>, TransferCryptoError> {
        ChunkedDecoder::decode_from(Cursor::new(encrypted), master_key)
            .map_err(TransferCryptoError::from)
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

    fn round_trip(plaintext: &[u8]) -> Vec<u8> {
        let key = test_key();
        let id = test_transfer_id();
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, plaintext).expect("encode_to failed");
        let cursor = Cursor::new(buf);
        ChunkedDecoder::decode_from(cursor, &key).expect("decode_from failed")
    }

    #[test]
    fn round_trip_small() {
        let pt = b"hello world";
        assert_eq!(round_trip(pt), pt);
    }

    #[test]
    fn round_trip_empty() {
        assert_eq!(round_trip(b""), b"");
    }

    #[test]
    fn round_trip_1mb() {
        let pt = vec![0u8; 1024 * 1024];
        assert_eq!(round_trip(&pt), pt);
    }

    #[test]
    fn header_starts_with_magic() {
        let key = test_key();
        let id = test_transfer_id();
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, b"test").unwrap();
        assert_eq!(&buf[0..4], &V2_MAGIC);
    }

    #[test]
    fn two_chunk_input_has_total_chunks_2() {
        let key = test_key();
        let id = test_transfer_id();
        let plaintext = vec![0u8; CHUNK_SIZE * 2];
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, &plaintext).unwrap();
        // total_chunks is at bytes [20..24]
        let total_chunks = u32::from_le_bytes(buf[20..24].try_into().unwrap());
        assert_eq!(total_chunks, 2);
    }

    #[test]
    fn tampered_ciphertext_returns_decrypt_failed() {
        let key = test_key();
        let id = test_transfer_id();
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, b"secret data").unwrap();
        // Flip a bit in the first chunk's ciphertext (after 32-byte header + 4-byte len = offset 36)
        buf[36] ^= 0xFF;
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
        ChunkedEncoder::encode_to(&mut buf, &key1, &id, b"data").unwrap();
        let result = ChunkedDecoder::decode_from(Cursor::new(buf), &key2);
        assert!(matches!(
            result,
            Err(ChunkedTransferError::DecryptFailed { .. })
        ));
    }

    #[test]
    fn swapped_chunks_aad_mismatch() {
        let key = test_key();
        let id = test_transfer_id();
        // Use two equal-size chunks for clean swap
        let plaintext = vec![0u8; CHUNK_SIZE * 2];
        let mut buf = Vec::new();
        ChunkedEncoder::encode_to(&mut buf, &key, &id, &plaintext).unwrap();
        let c0_len = u32::from_le_bytes(buf[32..36].try_into().unwrap()) as usize;
        let c1_off = 32 + 4 + c0_len;
        let c1_len = u32::from_le_bytes(buf[c1_off..c1_off + 4].try_into().unwrap()) as usize;
        assert_eq!(
            c0_len, c1_len,
            "chunks must be same size for this swap test"
        );
        let c0_range = 36..36 + c0_len;
        let c1_range = c1_off + 4..c1_off + 4 + c1_len;
        let c0 = buf[c0_range.clone()].to_vec();
        let c1 = buf[c1_range.clone()].to_vec();
        buf[c0_range].copy_from_slice(&c1);
        buf[c1_range].copy_from_slice(&c0);
        let result = ChunkedDecoder::decode_from(Cursor::new(buf), &key);
        assert!(matches!(
            result,
            Err(ChunkedTransferError::DecryptFailed { .. })
        ));
    }

    #[test]
    fn adapter_encrypt_decrypt_round_trip() {
        let key = test_key();
        let encryptor = TransferPayloadEncryptorAdapter;
        let decryptor = TransferPayloadDecryptorAdapter;
        let plaintext = b"adapter round trip test";
        let encrypted = encryptor.encrypt(&key, plaintext).expect("encrypt");
        let decrypted = decryptor.decrypt(&encrypted, &key).expect("decrypt");
        assert_eq!(decrypted, plaintext);
    }
}

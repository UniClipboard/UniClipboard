//! `EntryFileSetPathCipher` — AEAD codec for the sealed path columns of
//! `entry_file_set` (`original_text`, and later `relative_path`).
//!
//! A file path is user content — it exposes filenames and directory structure —
//! so it must never hit disk in plaintext (the persist-as-ciphertext rule). Each
//! path string is sealed with XChaCha20-Poly1305 under a per-session subkey,
//! bound to its exact manifest row via `aad::for_file_set_line(entry_id,
//! line_index)` so a ciphertext cannot be transplanted onto a different entry or
//! line.
//!
//! On-disk envelope (fixed binary header, mirroring the UCSR search-render
//! header):
//!
//! ```text
//! [magic "UCFS" 4B][format_version 1B = 0x01][nonce 24B][ciphertext+tag]
//! ```

use uc_core::crypto::aad;
use uc_core::ids::EntryId;

use crate::security::v1_aead::{decrypt_xchacha_raw, encrypt_xchacha_raw};

/// UCFS envelope magic — "UCFS" (UniClipboard File Set).
const FILE_SET_MAGIC: [u8; 4] = [0x55, 0x43, 0x46, 0x53];
/// Current UCFS envelope format version.
const FILE_SET_FORMAT_VERSION: u8 = 0x01;
/// Fixed nonce length (XChaCha20-Poly1305).
const NONCE_LEN: usize = 24;
/// Header = magic(4) + version(1) + nonce(24).
const HEADER_LEN: usize = 4 + 1 + NONCE_LEN;

/// Failure while sealing or opening a file-set path column.
///
/// The repository maps every variant to `EntryFileSetError::Storage`: a decode
/// failure on a path column means the manifest row is unreadable, which the
/// dispatch reader already handles by falling back to snapshot re-parse.
#[derive(Debug, thiserror::Error)]
pub enum FileSetCipherError {
    #[error("file-set path envelope too short: {0} bytes")]
    Truncated(usize),
    #[error("file-set path envelope magic mismatch")]
    BadMagic,
    #[error("unsupported file-set path envelope version: {0:#x}")]
    UnsupportedVersion(u8),
    #[error("file-set path AEAD encryption failed")]
    EncryptFailed,
    #[error("file-set path AEAD verification failed")]
    DecryptFailed,
    // Deliberately content-free: the decrypted plaintext is a device-local path,
    // so the underlying utf-8 error must not carry it into logs.
    #[error("file-set path plaintext is not valid UTF-8")]
    InvalidUtf8,
}

/// AEAD codec holding a per-session 32-byte subkey.
///
/// One codec is constructed per repository operation (save / load) after the
/// subkey is derived, then reused for every row in that operation. Holds no
/// database handle — pure seal/open. `Clone` is cheap (32 key bytes).
#[derive(Clone)]
pub struct EntryFileSetPathCipher {
    key: [u8; 32],
}

impl EntryFileSetPathCipher {
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    /// Seal `plaintext` into a UCFS envelope bound to `(entry_id, line_index)`.
    pub fn seal(
        &self,
        entry_id: &EntryId,
        line_index: i64,
        plaintext: &str,
    ) -> Result<Vec<u8>, FileSetCipherError> {
        let ad = aad::for_file_set_line(entry_id, line_index);
        let (nonce, ciphertext) = encrypt_xchacha_raw(&self.key, plaintext.as_bytes(), &ad)
            .map_err(|_| FileSetCipherError::EncryptFailed)?;

        let mut buf = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        buf.extend_from_slice(&FILE_SET_MAGIC);
        buf.push(FILE_SET_FORMAT_VERSION);
        buf.extend_from_slice(&nonce);
        buf.extend_from_slice(&ciphertext);
        Ok(buf)
    }

    /// Open a UCFS envelope, verifying it is bound to `(entry_id, line_index)`.
    pub fn open(
        &self,
        entry_id: &EntryId,
        line_index: i64,
        bytes: &[u8],
    ) -> Result<String, FileSetCipherError> {
        if bytes.len() < HEADER_LEN {
            return Err(FileSetCipherError::Truncated(bytes.len()));
        }
        if bytes[0..4] != FILE_SET_MAGIC {
            return Err(FileSetCipherError::BadMagic);
        }
        let version = bytes[4];
        if version != FILE_SET_FORMAT_VERSION {
            return Err(FileSetCipherError::UnsupportedVersion(version));
        }
        let nonce = &bytes[5..HEADER_LEN];
        let ciphertext = &bytes[HEADER_LEN..];
        let ad = aad::for_file_set_line(entry_id, line_index);
        let plaintext = decrypt_xchacha_raw(&self.key, nonce, ciphertext, &ad)
            .map_err(|_| FileSetCipherError::DecryptFailed)?;
        String::from_utf8(plaintext).map_err(|_| FileSetCipherError::InvalidUtf8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher(seed: u8) -> EntryFileSetPathCipher {
        EntryFileSetPathCipher::new([seed; 32])
    }

    #[test]
    fn roundtrip() {
        let c = cipher(0xA1);
        let id = EntryId::from("entry-1");
        let env = c.seal(&id, 0, "file:///Users/mark/secret.pdf").unwrap();
        assert_eq!(&env[0..4], &FILE_SET_MAGIC);
        assert_eq!(env[4], FILE_SET_FORMAT_VERSION);
        assert_eq!(c.open(&id, 0, &env).unwrap(), "file:///Users/mark/secret.pdf");
    }

    #[test]
    fn ciphertext_does_not_contain_plaintext() {
        let c = cipher(0xB2);
        let id = EntryId::from("entry-1");
        let path = "file:///Users/mark/budget/salaries.xlsx";
        let env = c.seal(&id, 0, path).unwrap();
        assert!(
            !env.windows(path.len()).any(|w| w == path.as_bytes()),
            "sealed envelope must not carry the plaintext path"
        );
    }

    #[test]
    fn wrong_entry_id_fails_aad() {
        let c = cipher(0xC3);
        let env = c.seal(&EntryId::from("entry-a"), 0, "/tmp/a").unwrap();
        assert!(matches!(
            c.open(&EntryId::from("entry-b"), 0, &env).unwrap_err(),
            FileSetCipherError::DecryptFailed
        ));
    }

    #[test]
    fn wrong_line_index_fails_aad() {
        let c = cipher(0xC4);
        let id = EntryId::from("entry-1");
        let env = c.seal(&id, 0, "/tmp/a").unwrap();
        assert!(matches!(
            c.open(&id, 1, &env).unwrap_err(),
            FileSetCipherError::DecryptFailed
        ));
    }

    #[test]
    fn wrong_key_fails() {
        let a = cipher(0x01);
        let b = cipher(0x02);
        let id = EntryId::from("entry-1");
        let env = a.seal(&id, 0, "/tmp/a").unwrap();
        assert!(matches!(
            b.open(&id, 0, &env).unwrap_err(),
            FileSetCipherError::DecryptFailed
        ));
    }

    #[test]
    fn bad_magic_rejected() {
        let c = cipher(0xD4);
        let id = EntryId::from("entry-1");
        let mut env = c.seal(&id, 0, "/tmp/a").unwrap();
        env[0] ^= 0xFF;
        assert!(matches!(
            c.open(&id, 0, &env).unwrap_err(),
            FileSetCipherError::BadMagic
        ));
    }

    #[test]
    fn unknown_version_rejected() {
        let c = cipher(0xE5);
        let id = EntryId::from("entry-1");
        let mut env = c.seal(&id, 0, "/tmp/a").unwrap();
        env[4] = 0x02;
        assert!(matches!(
            c.open(&id, 0, &env).unwrap_err(),
            FileSetCipherError::UnsupportedVersion(0x02)
        ));
    }

    #[test]
    fn truncated_rejected() {
        let c = cipher(0xF6);
        let id = EntryId::from("entry-1");
        let env = c.seal(&id, 0, "/tmp/a").unwrap();
        let short = &env[0..HEADER_LEN - 1];
        assert!(matches!(
            c.open(&id, 0, short).unwrap_err(),
            FileSetCipherError::Truncated(_)
        ));
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let c = cipher(0x28);
        let id = EntryId::from("entry-1");
        let mut env = c.seal(&id, 0, "/tmp/a").unwrap();
        let last = env.len() - 1;
        env[last] ^= 0x01;
        assert!(matches!(
            c.open(&id, 0, &env).unwrap_err(),
            FileSetCipherError::DecryptFailed
        ));
    }
}

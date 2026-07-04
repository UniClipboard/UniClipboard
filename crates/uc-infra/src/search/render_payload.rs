//! `RenderPayloadCodec` — AEAD codec for the `search_document.render_payload` column.
//!
//! The five content-derived render fields (`text_preview`, `file_names`,
//! `link_urls`, `file_paths`, `char_count`) are packed into a single JSON
//! plaintext and sealed with XChaCha20-Poly1305 under a per-session
//! [`RenderKey`], bound to the row via `aad::for_search_render(entry_id)`.
//!
//! On-disk envelope (fixed binary header, mirroring the UCBL blob header):
//!
//! ```text
//! [magic "UCSR" 4B][format_version 1B = 0x01][nonce 24B][ciphertext+tag]
//! ```
//!
//! Any decode failure (bad magic, unsupported version, truncation, AEAD
//! verification failure, malformed JSON) surfaces as [`RenderDecodeError`]; the
//! DAO maps all of them to the same single-row degrade path.

use serde::{Deserialize, Serialize};

use uc_core::crypto::aad;
use uc_core::ids::EntryId;
use uc_core::search::RenderKey;

use crate::security::v1_aead::{decrypt_xchacha_raw, encrypt_xchacha_raw};

/// UCSR envelope magic — "UCSR" (UniClipboard Search Render).
const RENDER_MAGIC: [u8; 4] = [0x55, 0x43, 0x53, 0x52];
/// Current UCSR envelope format version.
const RENDER_FORMAT_VERSION: u8 = 0x01;
/// Fixed nonce length (XChaCha20-Poly1305).
const NONCE_LEN: usize = 24;
/// Header = magic(4) + version(1) + nonce(24).
const HEADER_LEN: usize = 4 + 1 + NONCE_LEN;
/// Current render-payload JSON schema version.
const RENDER_PAYLOAD_V: u8 = 1;

/// Plaintext render fields packed into the encrypted payload.
///
/// These are exactly the content-derived columns that used to live in
/// `search_document` as plaintext. `source_device` and `payload_state` are NOT
/// here — they stay as plaintext columns (SQL-filtered / availability flag).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RenderFields {
    /// JSON schema version, so the payload structure can evolve independently
    /// of the envelope `format_version`.
    pub v: u8,
    #[serde(default)]
    pub text_preview: Option<String>,
    #[serde(default)]
    pub file_names: Vec<String>,
    #[serde(default)]
    pub link_urls: Vec<String>,
    #[serde(default)]
    pub file_paths: Vec<String>,
    #[serde(default)]
    pub char_count: Option<i64>,
}

impl RenderFields {
    /// Construct a `RenderFields` with the current schema version stamped.
    pub fn new(
        text_preview: Option<String>,
        file_names: Vec<String>,
        link_urls: Vec<String>,
        file_paths: Vec<String>,
        char_count: Option<i64>,
    ) -> Self {
        Self {
            v: RENDER_PAYLOAD_V,
            text_preview,
            file_names,
            link_urls,
            file_paths,
            char_count,
        }
    }
}

/// Failure while decoding a stored `render_payload`.
///
/// The DAO treats every variant identically: return the row with render fields
/// blanked and schedule a re-projection repair.
#[derive(Debug, thiserror::Error)]
pub enum RenderDecodeError {
    #[error("render payload too short: {0} bytes")]
    Truncated(usize),
    #[error("render payload magic mismatch")]
    BadMagic,
    #[error("unsupported render payload format version: {0:#x}")]
    UnsupportedVersion(u8),
    #[error("render payload AEAD verification failed")]
    DecryptFailed,
    // Deliberately content-free: this parse runs on decrypted plaintext, so the
    // underlying serde error string could carry render content. Carrying only the
    // category keeps it safe to log (see `SearchDocumentRow::to_domain`).
    #[error("render payload JSON malformed")]
    MalformedJson,
    #[error("unsupported render payload schema version: {0}")]
    UnsupportedPayloadVersion(u8),
}

/// Encoding error — an internal fault (key/serialization), not a data-corruption
/// signal. Distinct from [`RenderDecodeError`] so the write path can surface it
/// as a hard error rather than a degrade.
#[derive(Debug, thiserror::Error)]
pub enum RenderEncodeError {
    #[error("render payload AEAD encryption failed")]
    EncryptFailed,
    #[error("render payload JSON serialization failed: {0}")]
    SerializeJson(String),
}

/// AEAD codec holding a per-session [`RenderKey`].
///
/// One codec is constructed per outer operation (query page / single index /
/// rebuild run) after the render key is derived, then reused for every row in
/// that operation. Holds no database handle — pure encode/decode. `Clone` is
/// cheap (32 key bytes) so a rebuild can hand a copy to each per-entry closure.
#[derive(Clone)]
pub struct RenderPayloadCodec {
    render_key: RenderKey,
}

impl RenderPayloadCodec {
    pub fn new(render_key: RenderKey) -> Self {
        Self { render_key }
    }

    /// Seal `fields` into a UCSR envelope bound to `entry_id`.
    pub fn encrypt(
        &self,
        entry_id: &EntryId,
        fields: &RenderFields,
    ) -> Result<Vec<u8>, RenderEncodeError> {
        let plaintext = serde_json::to_vec(fields)
            .map_err(|e| RenderEncodeError::SerializeJson(e.to_string()))?;
        let ad = aad::for_search_render(entry_id);
        let (nonce, ciphertext) = encrypt_xchacha_raw(self.render_key.as_bytes(), &plaintext, &ad)
            .map_err(|_| RenderEncodeError::EncryptFailed)?;

        let mut buf = Vec::with_capacity(HEADER_LEN + ciphertext.len());
        buf.extend_from_slice(&RENDER_MAGIC);
        buf.push(RENDER_FORMAT_VERSION);
        buf.extend_from_slice(&nonce);
        buf.extend_from_slice(&ciphertext);
        Ok(buf)
    }

    /// Open a UCSR envelope, verifying it is bound to `entry_id`.
    pub fn decrypt(
        &self,
        entry_id: &EntryId,
        bytes: &[u8],
    ) -> Result<RenderFields, RenderDecodeError> {
        if bytes.len() < HEADER_LEN {
            return Err(RenderDecodeError::Truncated(bytes.len()));
        }
        if bytes[0..4] != RENDER_MAGIC {
            return Err(RenderDecodeError::BadMagic);
        }
        let version = bytes[4];
        if version != RENDER_FORMAT_VERSION {
            return Err(RenderDecodeError::UnsupportedVersion(version));
        }
        let nonce = &bytes[5..HEADER_LEN];
        let ciphertext = &bytes[HEADER_LEN..];
        let ad = aad::for_search_render(entry_id);
        let plaintext = decrypt_xchacha_raw(self.render_key.as_bytes(), nonce, ciphertext, &ad)
            .map_err(|_| RenderDecodeError::DecryptFailed)?;
        let fields: RenderFields =
            serde_json::from_slice(&plaintext).map_err(|_| RenderDecodeError::MalformedJson)?;
        // The inner schema version is independent of the envelope `format_version`
        // (which gates nonce/header layout). An unknown schema version means the
        // field set may not be what this binary expects, so fail into the degrade
        // path rather than return a half-populated struct via serde defaults.
        if fields.v != RENDER_PAYLOAD_V {
            return Err(RenderDecodeError::UnsupportedPayloadVersion(fields.v));
        }
        Ok(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> RenderKey {
        RenderKey::from_bytes(&[seed; 32]).unwrap()
    }

    fn sample() -> RenderFields {
        RenderFields::new(
            Some("hello world".to_string()),
            vec!["a.txt".to_string(), "b.png".to_string()],
            vec!["https://example.com".to_string()],
            vec!["/tmp/a.txt".to_string()],
            Some(11),
        )
    }

    #[test]
    fn roundtrip_full_fields() {
        let codec = RenderPayloadCodec::new(key(0xA1));
        let id = EntryId::from("entry-1");
        let env = codec.encrypt(&id, &sample()).unwrap();
        assert_eq!(&env[0..4], &RENDER_MAGIC);
        assert_eq!(env[4], RENDER_FORMAT_VERSION);
        let back = codec.decrypt(&id, &env).unwrap();
        assert_eq!(back, sample());
    }

    #[test]
    fn roundtrip_empty_fields() {
        let codec = RenderPayloadCodec::new(key(0xB2));
        let id = EntryId::from("entry-empty");
        let fields = RenderFields::new(None, vec![], vec![], vec![], None);
        let env = codec.encrypt(&id, &fields).unwrap();
        assert_eq!(codec.decrypt(&id, &env).unwrap(), fields);
    }

    #[test]
    fn roundtrip_large_payload() {
        let codec = RenderPayloadCodec::new(key(0xC3));
        let id = EntryId::from("entry-large");
        let big = "x".repeat(200_000);
        let fields = RenderFields::new(Some(big), vec![], vec![], vec![], Some(200_000));
        let env = codec.encrypt(&id, &fields).unwrap();
        assert_eq!(codec.decrypt(&id, &env).unwrap(), fields);
    }

    #[test]
    fn wrong_entry_id_fails_aad() {
        let codec = RenderPayloadCodec::new(key(0xD4));
        let env = codec.encrypt(&EntryId::from("entry-a"), &sample()).unwrap();
        let err = codec.decrypt(&EntryId::from("entry-b"), &env).unwrap_err();
        assert!(matches!(err, RenderDecodeError::DecryptFailed));
    }

    #[test]
    fn wrong_key_fails() {
        let a = RenderPayloadCodec::new(key(0x01));
        let b = RenderPayloadCodec::new(key(0x02));
        let id = EntryId::from("entry-1");
        let env = a.encrypt(&id, &sample()).unwrap();
        assert!(matches!(
            b.decrypt(&id, &env).unwrap_err(),
            RenderDecodeError::DecryptFailed
        ));
    }

    #[test]
    fn bad_magic_rejected() {
        let codec = RenderPayloadCodec::new(key(0xE5));
        let id = EntryId::from("entry-1");
        let mut env = codec.encrypt(&id, &sample()).unwrap();
        env[0] ^= 0xFF;
        assert!(matches!(
            codec.decrypt(&id, &env).unwrap_err(),
            RenderDecodeError::BadMagic
        ));
    }

    #[test]
    fn unknown_version_rejected() {
        let codec = RenderPayloadCodec::new(key(0xF6));
        let id = EntryId::from("entry-1");
        let mut env = codec.encrypt(&id, &sample()).unwrap();
        env[4] = 0x02;
        assert!(matches!(
            codec.decrypt(&id, &env).unwrap_err(),
            RenderDecodeError::UnsupportedVersion(0x02)
        ));
    }

    #[test]
    fn truncated_rejected() {
        let codec = RenderPayloadCodec::new(key(0x17));
        let id = EntryId::from("entry-1");
        let env = codec.encrypt(&id, &sample()).unwrap();
        let short = &env[0..HEADER_LEN - 1];
        assert!(matches!(
            codec.decrypt(&id, short).unwrap_err(),
            RenderDecodeError::Truncated(_)
        ));
    }

    #[test]
    fn unknown_schema_version_rejected() {
        // A payload sealed with a future inner schema version (v=2) must fail into
        // the degrade path even though the envelope + AEAD are perfectly valid.
        let codec = RenderPayloadCodec::new(key(0x39));
        let id = EntryId::from("entry-1");
        let mut fields = sample();
        fields.v = 2;
        let env = codec.encrypt(&id, &fields).unwrap();
        assert!(matches!(
            codec.decrypt(&id, &env).unwrap_err(),
            RenderDecodeError::UnsupportedPayloadVersion(2)
        ));
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let codec = RenderPayloadCodec::new(key(0x28));
        let id = EntryId::from("entry-1");
        let mut env = codec.encrypt(&id, &sample()).unwrap();
        let last = env.len() - 1;
        env[last] ^= 0x01;
        assert!(matches!(
            codec.decrypt(&id, &env).unwrap_err(),
            RenderDecodeError::DecryptFailed
        ));
    }
}

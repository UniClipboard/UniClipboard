//! SearchKey — 32-byte HMAC key derived from MasterKey via SearchKeyDerivationPort.
//!
//! Opaque newtype: no Serialize/Deserialize, redacted Debug.
//! Pattern mirrors `crypto::model::MasterKey`.

use std::fmt;

use zeroize::Zeroize;

/// Opaque 32-byte search key derived from the master key.
///
/// - Do NOT implement Serialize/Deserialize — keys must never appear in JSON.
/// - The HMAC computation (`term_tag = HMAC(search_key, token)`) is a Phase 90
///   infra concern; this type is a pure data contract.
/// - Only `as_bytes()` exposes the raw bytes, for use by infra HMAC adapters.
#[derive(Clone, PartialEq, Eq)]
pub struct SearchKey(pub [u8; 32]);

impl SearchKey {
    /// Length of a SearchKey in bytes.
    pub const LEN: usize = 32;

    /// Access the raw key bytes — for use by uc-infra HMAC adapters only.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Construct a SearchKey from a byte slice, validating length.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, crate::search::error::SearchError> {
        if bytes.len() != Self::LEN {
            return Err(crate::search::error::SearchError::Internal(format!(
                "invalid SearchKey length: expected {}, got {}",
                Self::LEN,
                bytes.len()
            )));
        }
        let mut buf = [0u8; Self::LEN];
        buf.copy_from_slice(bytes);
        Ok(SearchKey(buf))
    }
}

impl fmt::Debug for SearchKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SearchKey([REDACTED])")
    }
}

impl Drop for SearchKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Opaque 32-byte render-payload key derived from the master key.
///
/// Distinct from [`SearchKey`] to keep key usage separated: `SearchKey` is an
/// HMAC-PRF key for inverted-index term tags, while `RenderKey` is an AEAD key
/// for encrypting the per-entry render payload. Deriving a dedicated subkey (a
/// different HKDF `info` label) prevents a single key from serving two
/// cryptographic purposes.
///
/// - Do NOT implement Serialize/Deserialize — keys must never appear in JSON.
/// - Only `as_bytes()` exposes the raw bytes, for use by infra AEAD adapters.
#[derive(Clone, PartialEq, Eq)]
pub struct RenderKey(pub [u8; 32]);

impl RenderKey {
    /// Length of a RenderKey in bytes.
    pub const LEN: usize = 32;

    /// Access the raw key bytes — for use by uc-infra AEAD adapters only.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Construct a RenderKey from a byte slice, validating length.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, crate::search::error::SearchError> {
        if bytes.len() != Self::LEN {
            return Err(crate::search::error::SearchError::Internal(format!(
                "invalid RenderKey length: expected {}, got {}",
                Self::LEN,
                bytes.len()
            )));
        }
        let mut buf = [0u8; Self::LEN];
        buf.copy_from_slice(bytes);
        Ok(RenderKey(buf))
    }
}

impl fmt::Debug for RenderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RenderKey([REDACTED])")
    }
}

impl Drop for RenderKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

//! SearchKey — 32-byte HMAC key derived from MasterKey via SearchKeyDerivationPort.
//!
//! Opaque newtype: no Serialize/Deserialize, redacted Debug.
//! Pattern mirrors `crypto::model::MasterKey`.

use std::fmt;

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

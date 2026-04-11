//! HKDF-SHA256 backed implementation of `SearchKeyDerivationPort`.
//!
//! Key derivation: `HKDF-SHA256(ikm = master_key, salt = profile_id, info = b"uniclipboard-search-index/v1")`
//!
//! This module also provides `term_tag()` — an `pub(crate)` helper that
//! computes `HMAC-SHA256(search_key, normalized_token)` and returns 32 bytes.
//!
//! No raw `MasterKey` bytes are accepted by the HMAC helper; only `SearchKey`.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use uc_core::ports::search::search_key::SearchKeyDerivationPort;
use uc_core::ports::security::encryption_session::EncryptionSessionPort;
use uc_core::ports::security::key_scope::KeyScopePort;
use uc_core::search::error::SearchError;
use uc_core::search::key::SearchKey;

const SEARCH_KEY_INFO: &[u8] = b"uniclipboard-search-index/v1";

/// Type alias for HMAC-SHA256 — used for term-tag computation.
type HmacSha256 = Hmac<Sha256>;

/// HKDF-SHA256 implementation of `SearchKeyDerivationPort`.
///
/// Derives a profile-scoped `SearchKey` from the unlocked `MasterKey`.
/// The derivation is deterministic: same master key + same profile_id always
/// produces the same `SearchKey`. Different profiles produce different keys.
pub struct HkdfSearchKeyDerivation {
    encryption_session: Arc<dyn EncryptionSessionPort>,
    key_scope: Arc<dyn KeyScopePort>,
}

impl HkdfSearchKeyDerivation {
    /// Create a new `HkdfSearchKeyDerivation`.
    pub fn new(
        encryption_session: Arc<dyn EncryptionSessionPort>,
        key_scope: Arc<dyn KeyScopePort>,
    ) -> Self {
        Self {
            encryption_session,
            key_scope,
        }
    }
}

#[async_trait]
impl SearchKeyDerivationPort for HkdfSearchKeyDerivation {
    async fn derive_search_key(&self) -> Result<SearchKey, SearchError> {
        // 1. Get master key — map session errors to SessionLocked.
        let master_key = self
            .encryption_session
            .get_master_key()
            .await
            .map_err(|_| SearchError::SessionLocked)?;

        // 2. Get current scope — map scope errors to Internal.
        let scope = self
            .key_scope
            .current_scope()
            .await
            .map_err(|e| SearchError::Internal(format!("failed to get key scope: {e}")))?;

        // 3. Derive via HKDF-SHA256.
        // salt = profile_id bytes (profile-scopes the key)
        // ikm  = master_key bytes
        let hkdf = Hkdf::<Sha256>::new(Some(scope.profile_id.as_bytes()), master_key.as_bytes());

        let mut okm = [0u8; 32];
        hkdf.expand(SEARCH_KEY_INFO, &mut okm)
            .map_err(|e| SearchError::Internal(format!("HKDF expand failed: {e}")))?;

        // 4. Wrap as SearchKey.
        SearchKey::from_bytes(&okm)
    }
}

/// Compute an HMAC-SHA256 tag over `normalized_token` using the given `SearchKey`.
///
/// Returns a 32-byte tag (`Vec<u8>`).
///
/// Note: This function deliberately accepts `&SearchKey` (not `&MasterKey`) to
/// enforce that HMAC tagging is always done with the derived search key, never
/// raw master key bytes.
pub(crate) fn term_tag(search_key: &SearchKey, normalized_token: &str) -> Result<Vec<u8>> {
    let mut mac = HmacSha256::new_from_slice(search_key.as_bytes())
        .map_err(|e| anyhow!("HMAC init failed: {e}"))?;
    mac.update(normalized_token.as_bytes());
    Ok(mac.finalize().into_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use uc_core::ports::security::encryption_session::EncryptionSessionPort;
    use uc_core::ports::security::key_scope::{KeyScopePort, ScopeError};
    use uc_core::search::error::SearchError;
    use uc_core::security::model::{EncryptionError, KeyScope, MasterKey};

    // ── Stubs ──────────────────────────────────────────────────────────────

    struct OkEncryptionSession {
        key: MasterKey,
    }

    impl OkEncryptionSession {
        fn with_bytes(bytes: [u8; 32]) -> Self {
            Self {
                key: MasterKey(bytes),
            }
        }
    }

    #[async_trait]
    impl EncryptionSessionPort for OkEncryptionSession {
        async fn is_ready(&self) -> bool {
            true
        }
        async fn get_master_key(&self) -> Result<MasterKey, EncryptionError> {
            Ok(self.key.clone())
        }
        async fn set_master_key(&self, _: MasterKey) -> Result<(), EncryptionError> {
            Ok(())
        }
        async fn clear(&self) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    struct LockedEncryptionSession;

    #[async_trait]
    impl EncryptionSessionPort for LockedEncryptionSession {
        async fn is_ready(&self) -> bool {
            false
        }
        async fn get_master_key(&self) -> Result<MasterKey, EncryptionError> {
            Err(EncryptionError::Locked)
        }
        async fn set_master_key(&self, _: MasterKey) -> Result<(), EncryptionError> {
            Ok(())
        }
        async fn clear(&self) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    struct FixedScope {
        profile_id: String,
    }

    #[async_trait]
    impl KeyScopePort for FixedScope {
        async fn current_scope(&self) -> Result<KeyScope, ScopeError> {
            Ok(KeyScope {
                profile_id: self.profile_id.clone(),
            })
        }
    }

    fn make_derivation(key_bytes: [u8; 32], profile_id: &str) -> HkdfSearchKeyDerivation {
        HkdfSearchKeyDerivation::new(
            Arc::new(OkEncryptionSession::with_bytes(key_bytes)),
            Arc::new(FixedScope {
                profile_id: profile_id.to_string(),
            }),
        )
    }

    // ── Tests ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn same_master_key_same_profile_produces_same_search_key() {
        let d1 = make_derivation([0x42u8; 32], "profile-abc");
        let d2 = make_derivation([0x42u8; 32], "profile-abc");
        let k1 = d1.derive_search_key().await.unwrap();
        let k2 = d2.derive_search_key().await.unwrap();
        assert_eq!(k1, k2, "same inputs must yield identical SearchKey");
    }

    #[tokio::test]
    async fn same_master_key_different_profile_produces_different_search_key() {
        let d1 = make_derivation([0x42u8; 32], "profile-abc");
        let d2 = make_derivation([0x42u8; 32], "profile-xyz");
        let k1 = d1.derive_search_key().await.unwrap();
        let k2 = d2.derive_search_key().await.unwrap();
        assert_ne!(
            k1, k2,
            "different profiles must produce different SearchKey"
        );
    }

    #[tokio::test]
    async fn locked_session_returns_session_locked_error() {
        let d = HkdfSearchKeyDerivation::new(
            Arc::new(LockedEncryptionSession),
            Arc::new(FixedScope {
                profile_id: "profile-abc".to_string(),
            }),
        );
        let result = d.derive_search_key().await;
        assert!(
            matches!(result, Err(SearchError::SessionLocked)),
            "locked session must produce SessionLocked, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn term_tag_returns_32_bytes() {
        let key = SearchKey([0x11u8; 32]);
        let tag = term_tag(&key, "hello").unwrap();
        assert_eq!(tag.len(), 32, "HMAC-SHA256 output must be 32 bytes");
    }

    #[tokio::test]
    async fn term_tag_does_not_accept_master_key_directly() {
        // This is a compile-time invariant verified by the type system:
        // `term_tag` accepts `&SearchKey`, not `&MasterKey`.
        // If this test compiles without any MasterKey argument, the constraint holds.
        //
        // We call term_tag with a SearchKey to confirm the function signature.
        let key = SearchKey([0x22u8; 32]);
        let result = term_tag(&key, "test_token");
        assert!(result.is_ok());
        // MasterKey cannot be passed — the following would NOT compile:
        // let mk = MasterKey([0x22u8; 32]);
        // let _ = term_tag(&mk, "test");
    }
}

//! HKDF-SHA256 backed implementation of `SearchKeyDerivationPort`.
//!
//! Slice 3 起通过 `SpaceAccessPort::derive_subkey` 派生——adapter 内部
//! 用 IKM = MasterKey + HKDF-SHA256,本 adapter 只负责构造 salt (profile_id)
//! 与 info ("uniclipboard-search-index/v1"),并把 32 字节字节包装成 `SearchKey`。
//!
//! 不再持有 `EncryptionSessionPort`——会话状态由 SpaceAccessPort adapter
//! 端到端管理,本 adapter 不直接接触 master_key。

use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use uc_core::ports::search::search_key::SearchKeyDerivationPort;
use uc_core::ports::security::current_profile::CurrentProfilePort;
use uc_core::ports::space::{SpaceAccessError, SpaceAccessPort};
use uc_core::search::error::SearchError;
use uc_core::search::key::SearchKey;

const SEARCH_KEY_INFO: &[u8] = b"uniclipboard-search-index/v1";

/// Type alias for HMAC-SHA256 — used for term-tag computation.
type HmacSha256 = Hmac<Sha256>;

/// HKDF-SHA256 implementation of `SearchKeyDerivationPort`.
///
/// 派生公式:`HKDF-SHA256(ikm = master_key, salt = profile_id, info = "uniclipboard-search-index/v1")`
/// 行为与历史一致——同一 master_key + 同一 profile_id 总是派生出同一 SearchKey,
/// 不同 profile 派生不同 key。
pub struct HkdfSearchKeyDerivation {
    space_access: Arc<dyn SpaceAccessPort>,
    current_profile: Arc<dyn CurrentProfilePort>,
}

impl HkdfSearchKeyDerivation {
    pub fn new(
        space_access: Arc<dyn SpaceAccessPort>,
        current_profile: Arc<dyn CurrentProfilePort>,
    ) -> Self {
        Self {
            space_access,
            current_profile,
        }
    }
}

#[async_trait]
impl SearchKeyDerivationPort for HkdfSearchKeyDerivation {
    async fn derive_search_key(&self) -> Result<SearchKey, SearchError> {
        let profile =
            self.current_profile.current_profile().await.map_err(|e| {
                SearchError::Internal(format!("failed to get current profile: {e}"))
            })?;

        let okm = self
            .space_access
            .derive_subkey(profile.as_ref().as_bytes(), SEARCH_KEY_INFO)
            .await
            .map_err(|e| match e {
                SpaceAccessError::NotUnlocked => SearchError::SessionLocked,
                other => SearchError::Internal(format!("derive_subkey: {other}")),
            })?;

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

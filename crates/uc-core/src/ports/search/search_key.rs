//! SearchKeyDerivationPort — derives a SearchKey from the unlocked MasterKey.
//!
//! Implementation lives in uc-infra (Phase 90). The derivation is scoped per
//! profile via HKDF-SHA256 (per architecture spec). uc-core only sees the
//! opaque SearchKey output; no raw MasterKey bytes cross the port boundary.

use crate::search::{RenderKey, SearchError, SearchKey};
use async_trait::async_trait;

/// Port for deriving search subkeys from the currently-unlocked encryption session.
///
/// Implemented by uc-infra (Phase 90). Injected as `Arc<dyn SearchKeyDerivationPort + Send + Sync>`.
#[async_trait]
pub trait SearchKeyDerivationPort: Send + Sync {
    /// Derive a SearchKey for the currently-unlocked encryption session.
    ///
    /// Returns `SearchError::SessionLocked` if no master key is available.
    /// The derivation uses HKDF-SHA256 scoped to the active profile.
    async fn derive_search_key(&self) -> Result<SearchKey, SearchError>;

    /// Derive a RenderKey for the currently-unlocked encryption session.
    ///
    /// Distinct from [`derive_search_key`](Self::derive_search_key): the render
    /// key is an AEAD key for the per-entry render payload, derived with a
    /// separate HKDF `info` label so it never doubles as the HMAC term-tag key.
    ///
    /// Returns `SearchError::SessionLocked` if no master key is available.
    /// The derivation uses HKDF-SHA256 scoped to the active profile.
    async fn derive_render_key(&self) -> Result<RenderKey, SearchError>;
}

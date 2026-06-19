//! Intent port for advancing the cross-device active-clipboard register.

use async_trait::async_trait;

use crate::clipboard::ActiveClipboardState;

/// Error surface for active-clipboard register persistence.
#[derive(Debug, thiserror::Error)]
pub enum ActiveClipboardRegisterError {
    #[error("active clipboard register storage failure: {0}")]
    Storage(String),
}

/// Conditionally advance the single-row active-clipboard register.
#[async_trait]
pub trait AdvanceActiveClipboardPort: Send + Sync {
    /// Advance the register to `state` iff it supersedes the currently
    /// stored value under the LWW order `(activated_at_ms, activated_by)`.
    ///
    /// The comparison and write are a single atomic step: a value that
    /// loses the LWW comparison (stale timestamp, or an exact-key
    /// duplicate already stored) leaves the register unchanged.
    ///
    /// Returns `true` when the register actually advanced, `false` when
    /// the call was a no-op because `state` did not supersede the stored
    /// value.
    async fn advance(
        &self,
        state: &ActiveClipboardState,
    ) -> Result<bool, ActiveClipboardRegisterError>;
}

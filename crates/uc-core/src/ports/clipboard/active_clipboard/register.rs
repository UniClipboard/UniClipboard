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

/// Read the current value of the single-row active-clipboard register.
#[async_trait]
pub trait LoadActiveClipboardPort: Send + Sync {
    /// Return the register's current value, or `None` when it has never
    /// been written.
    ///
    /// This is a point-in-time read: a concurrent
    /// [`AdvanceActiveClipboardPort::advance`] may change the value
    /// immediately afterwards. Callers that need the read and a conditional
    /// write to be atomic must rely on `advance`'s own compare-and-set
    /// rather than gating it on a prior `load`.
    async fn load(&self) -> Result<Option<ActiveClipboardState>, ActiveClipboardRegisterError>;
}

/// Unconditionally clear the locally-recorded active-clipboard value.
#[async_trait]
pub trait ResetActiveClipboardPort: Send + Sync {
    /// Clear the register so a subsequent [`LoadActiveClipboardPort::load`]
    /// returns `None`, regardless of the value currently stored.
    ///
    /// Unlike [`AdvanceActiveClipboardPort::advance`], which only writes when
    /// the incoming value supersedes the stored one under the LWW order, this
    /// is an unconditional local reset: the stored value's timestamp does not
    /// gate it. After a successful reset the register holds no value and any
    /// subsequently observed state supersedes it.
    ///
    /// Idempotent: clearing an already-empty register is a successful no-op.
    async fn reset(&self) -> Result<(), ActiveClipboardRegisterError>;
}

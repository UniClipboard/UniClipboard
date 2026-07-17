//! Port for moving prepared content to its final location without ever
//! replacing what already lives there.

use std::fmt;
use std::path::Path;

use async_trait::async_trait;

/// Why an atomic publication did not take place.
///
/// Variants never carry the paths involved: a destination name is user
/// content, and this error is allowed to surface in contexts that must stay
/// free of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishError {
    /// The destination name is already taken. Whatever holds the name is
    /// untouched, and the source remains where it was.
    DestinationExists,
    /// The destination's volume cannot honor the no-replace guarantee, or
    /// source and destination do not share a volume.
    Unsupported,
    /// The operation failed for a reason outside this classification.
    Io(String),
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DestinationExists => write!(f, "destination name is already taken"),
            Self::Unsupported => write!(f, "volume cannot publish without replacing"),
            Self::Io(detail) => write!(f, "publish failed: {detail}"),
        }
    }
}

impl std::error::Error for PublishError {}

/// Move prepared content into place atomically.
#[async_trait]
pub trait AtomicPublishPort: Send + Sync {
    /// Make `source` visible at `destination` in a single atomic step: either
    /// the move takes effect in full, or neither path is altered.
    ///
    /// Never replaces and never merges. When `destination` already names
    /// anything at all — file, directory, or link — the result is
    /// [`PublishError::DestinationExists`] and that content is left intact.
    ///
    /// The guarantee is not available on every volume; where it cannot be
    /// honored the result is [`PublishError::Unsupported`] and nothing is
    /// moved. [`Self::supports_no_replace`] answers that question in advance.
    ///
    /// Both paths must be on one volume. A cross-volume request yields
    /// [`PublishError::Unsupported`] rather than degrading to a copy, which
    /// would forfeit atomicity.
    async fn publish_no_replace(
        &self,
        source: &Path,
        destination: &Path,
    ) -> Result<(), PublishError>;

    /// Make `source` visible at `destination` in a single atomic step, where
    /// the caller already holds `destination` free by its own means.
    ///
    /// Carries no no-replace guarantee: whether an occupied `destination`
    /// is refused or replaced is the volume's choice, so a caller that cannot
    /// establish the name is free must use [`Self::publish_no_replace`] and
    /// accept its narrower reach.
    ///
    /// In exchange the move works on every volume that can move within
    /// itself, including those where [`Self::supports_no_replace`] answers
    /// `false`. Atomicity and the same-volume requirement are unchanged.
    async fn publish_into_free_name(
        &self,
        source: &Path,
        destination: &Path,
    ) -> Result<(), PublishError>;

    /// Whether `probe_dir`'s volume honors [`Self::publish_no_replace`]'s
    /// no-replace guarantee.
    ///
    /// The answer is obtained by exercising the operation inside `probe_dir`
    /// and reflects that filesystem's real behavior, not a platform
    /// assumption. Any transient failure to establish support answers
    /// `false`, so `true` means the guarantee was observed to hold.
    ///
    /// Leaves `probe_dir` as it was found.
    async fn supports_no_replace(&self, probe_dir: &Path) -> bool;
}

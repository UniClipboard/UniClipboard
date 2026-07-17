//! Port for keeping working areas out of the user's way.

use std::path::Path;

use async_trait::async_trait;

/// Hide a path from ordinary directory listings.
#[async_trait]
pub trait MarkHiddenPort: Send + Sync {
    /// Ask that `path` not be shown by default when its parent is browsed.
    ///
    /// Best-effort and idempotent. A filesystem or platform with no notion of
    /// hiding, or one that refuses, simply leaves `path` as it was — which is
    /// why this cannot fail and reports nothing. Visibility is cosmetic, so no
    /// caller's correctness may rest on the outcome.
    ///
    /// Naming conventions that some platforms treat as hidden are not this
    /// port's concern: it is asked for the attribute, and answers for the
    /// attribute alone.
    async fn mark_hidden(&self, path: &Path);
}

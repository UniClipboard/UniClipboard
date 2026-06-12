//! Clipboard Representation Normalizer Port
//!
//! This port converts platform-layer `ObservedClipboardRepresentation` to
//! domain-layer `PersistedClipboardRepresentation`.
//!
//! **Semantic:** "normalize" = type conversion / normalization from platform format to domain format

use crate::clipboard::{ObservedClipboardRepresentation, PersistedClipboardRepresentation};

#[async_trait::async_trait]
pub trait ClipboardRepresentationNormalizerPort: Send + Sync {
    /// Normalize an observed clipboard representation into a persisted representation.
    ///
    /// # Post-conditions
    /// - Returned `PersistedClipboardRepresentation` contains valid metadata (mime, size)
    /// - `inline_data` is populated by strategy:
    ///   - Small data (< threshold): full storage
    ///   - Large data: preview/placeholder
    /// - `blob_id` is initially `None` (will be set later during resolve phase)
    async fn normalize(
        &self,
        observed: &ObservedClipboardRepresentation,
    ) -> anyhow::Result<PersistedClipboardRepresentation>;
}

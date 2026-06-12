//! Selection Resolver Port
//!
//! This port loads the complete selection context for an entry.
//!
//! **Semantic:** "resolve" = loading related entities

use crate::clipboard::{ClipboardEntry, PersistedClipboardRepresentation};
use crate::ids::EntryId;

#[async_trait::async_trait]
pub trait SelectionResolverPort: Send + Sync {
    /// Resolve the complete selection context for an entry.
    ///
    /// # Returns
    /// - Tuple of (ClipboardEntry, PersistedClipboardRepresentation)
    ///
    /// # Loading flow
    /// 1. Load `ClipboardEntry` from `EntryRepository` (get `event_id`)
    /// 2. Load `SelectionDecision` from `SelectionRepository` (get `primary_rep_id`)
    /// 3. Load target `PersistedClipboardRepresentation` from `RepresentationRepository`
    async fn resolve_selection(
        &self,
        entry_id: &EntryId,
    ) -> anyhow::Result<(ClipboardEntry, PersistedClipboardRepresentation)>;
}

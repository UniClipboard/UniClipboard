//! Selection Resolver Implementation
//!
//! Loads complete selection context for an entry.

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use uc_core::clipboard::{ClipboardEntry, PersistedClipboardRepresentation};
use uc_core::ids::EntryId;
use uc_core::ports::{
    ClipboardEntryRepositoryPort, ClipboardRepresentationRepositoryPort,
    ClipboardSelectionRepositoryPort, SelectionResolverPort,
};

/// Selection resolver implementation
pub struct SelectionResolver {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
}

impl SelectionResolver {
    pub fn new(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    ) -> Self {
        Self {
            entry_repo,
            selection_repo,
            representation_repo,
        }
    }
}

#[async_trait]
impl SelectionResolverPort for SelectionResolver {
    async fn resolve_selection(
        &self,
        entry_id: &EntryId,
    ) -> Result<(ClipboardEntry, PersistedClipboardRepresentation)> {
        // 1. Load ClipboardEntry
        let entry = self
            .entry_repo
            .get_entry(entry_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Entry {} not found", entry_id))?;

        // 2. Load SelectionDecision
        let selection_decision = self
            .selection_repo
            .get_selection(entry_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Selection for entry {} not found", entry_id))?;

        // 3. Load target PersistedClipboardRepresentation
        let primary_rep_id = selection_decision.selection.primary_rep_id;
        let representation = self
            .representation_repo
            .get_representation(&entry.event_id, &primary_rep_id)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Representation {} not found for event {}",
                    primary_rep_id,
                    entry.event_id
                )
            })?;

        Ok((entry, representation))
    }
}

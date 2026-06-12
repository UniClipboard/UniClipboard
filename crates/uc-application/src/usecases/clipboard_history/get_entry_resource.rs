use anyhow::Result;
use std::sync::Arc;
use uc_core::{
    ids::EntryId,
    ports::clipboard::ResolvedClipboardPayload,
    ports::{
        ClipboardEntryRepositoryPort, ClipboardPayloadResolverPort,
        ClipboardRepresentationRepositoryPort, ClipboardSelectionRepositoryPort,
    },
    BlobId,
};

/// Get clipboard entry resource metadata (blob reference only).
pub(crate) struct GetEntryResourceUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
}

#[derive(Debug, Clone)]
pub(crate) struct EntryResourceResult {
    pub(crate) blob_id: Option<BlobId>,
    pub(crate) mime_type: Option<String>,
    pub(crate) size_bytes: i64,
    pub(crate) url: Option<String>,
    pub(crate) inline_data: Option<Vec<u8>>,
}

impl GetEntryResourceUseCase {
    pub(crate) fn new(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    ) -> Self {
        Self {
            entry_repo,
            selection_repo,
            representation_repo,
            payload_resolver,
        }
    }

    pub(crate) async fn execute(&self, entry_id: &EntryId) -> Result<EntryResourceResult> {
        let entry = self
            .entry_repo
            .get_entry(entry_id)
            .await?
            .ok_or(anyhow::anyhow!("Entry not found"))?;

        let selection = self
            .selection_repo
            .get_selection(entry_id)
            .await?
            .ok_or(anyhow::anyhow!("Selection not found"))?;

        let preview_rep = self
            .representation_repo
            .get_representation(&entry.event_id, &selection.selection.preview_rep_id)
            .await?
            .ok_or(anyhow::anyhow!("Preview representation not found"))?;

        let payload = self.payload_resolver.resolve(&preview_rep).await?;

        match payload {
            ResolvedClipboardPayload::Inline { mime, bytes } => Ok(EntryResourceResult {
                blob_id: None,
                mime_type: Some(mime),
                size_bytes: preview_rep.size_bytes,
                url: None,
                inline_data: Some(bytes),
            }),
            ResolvedClipboardPayload::BlobRef { mime, blob_id } => {
                let blob_id_clone = blob_id.clone();
                Ok(EntryResourceResult {
                    blob_id: Some(blob_id),
                    mime_type: Some(mime),
                    size_bytes: preview_rep.size_bytes,
                    url: Some(format!("/clipboard/blobs/{}", blob_id_clone)),
                    inline_data: None,
                })
            }
        }
    }
}

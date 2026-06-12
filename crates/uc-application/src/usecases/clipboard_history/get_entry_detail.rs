use anyhow::Result;
use std::sync::Arc;

use uc_core::{
    blob::ports::BlobReaderPort,
    clipboard::MimeType,
    ids::EntryId,
    ports::clipboard::ResolvedClipboardPayload,
    ports::{
        ClipboardEntryRepositoryPort, ClipboardPayloadResolverPort,
        ClipboardRepresentationRepositoryPort, ClipboardSelectionRepositoryPort,
    },
};

/// Get full clipboard entry detail.
pub(crate) struct GetEntryDetailUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    blob_store: Arc<dyn BlobReaderPort>,
    payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
}

#[derive(Debug)]
pub(crate) struct EntryDetailResult {
    pub(crate) id: String,
    pub(crate) content: String,
    pub(crate) size_bytes: i64,
    pub(crate) created_at_ms: i64,
    pub(crate) active_time_ms: i64,
    pub(crate) mime_type: Option<String>,
}

impl GetEntryDetailUseCase {
    pub(crate) fn new(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        blob_store: Arc<dyn BlobReaderPort>,
        payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    ) -> Self {
        Self {
            entry_repo,
            selection_repo,
            representation_repo,
            blob_store,
            payload_resolver,
        }
    }

    pub(crate) async fn execute(&self, entry_id: &EntryId) -> Result<EntryDetailResult> {
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

        if !Self::is_text_mime(&preview_rep.mime_type) {
            return Err(anyhow::anyhow!("Entry is not text content"));
        }

        let mime_type_str = preview_rep.mime_type.as_ref().map(|mt| mt.as_str());

        let payload = self.payload_resolver.resolve(&preview_rep).await?;

        let full_content = match payload {
            ResolvedClipboardPayload::Inline { bytes, .. } => {
                String::from_utf8_lossy(&bytes).to_string()
            }
            ResolvedClipboardPayload::BlobRef { blob_id, .. } => {
                let blob_content = self.blob_store.get(&blob_id).await?;
                String::from_utf8_lossy(&blob_content).to_string()
            }
        };

        Ok(EntryDetailResult {
            id: entry.entry_id.to_string(),
            content: full_content,
            size_bytes: preview_rep.size_bytes,
            created_at_ms: entry.created_at_ms,
            active_time_ms: entry.active_time_ms,
            mime_type: mime_type_str.map(String::from),
        })
    }

    fn is_text_mime(mime: &Option<MimeType>) -> bool {
        match mime {
            None => false,
            Some(mt) => {
                let s = mt.as_str();
                s.starts_with("text/")
                    || s.contains("json")
                    || s.contains("xml")
                    || s.contains("javascript")
            }
        }
    }
}

use anyhow::Result;
use std::sync::Arc;
use uc_core::{
    ids::EntryId,
    ports::clipboard::ResolvedClipboardPayload,
    ports::clipboard::{
        GetClipboardEntryPort, GetRepresentationPort, ListRepresentationsForEventPort,
    },
    ports::{ClipboardPayloadResolverPort, ClipboardSelectionRepositoryPort},
    BlobId,
};

/// Get clipboard entry resource metadata (blob reference only).
pub(crate) struct GetEntryResourceUseCase {
    entry_repo: Arc<dyn GetClipboardEntryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn GetRepresentationPort>,
    rep_list_for_event: Arc<dyn ListRepresentationsForEventPort>,
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
        entry_repo: Arc<dyn GetClipboardEntryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn GetRepresentationPort>,
        rep_list_for_event: Arc<dyn ListRepresentationsForEventPort>,
        payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    ) -> Self {
        Self {
            entry_repo,
            selection_repo,
            representation_repo,
            rep_list_for_event,
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

        // Resolve the representation whose bytes back an image preview. An image
        // *file* (a copied `.png`, or one inside a multi-file selection) has its
        // paste/preview rep on the uri-list, not the bitmap, so resolving the
        // preview rep would hand the caller `text/uri-list` bytes that no `<img>`
        // can render. Prefer an image representation; fall back to the preview
        // rep for non-image entries (plain text, plain files), preserving the
        // prior behavior.
        let image_rep = self
            .rep_list_for_event
            .get_representations_for_event(&entry.event_id)
            .await?
            .into_iter()
            .find(|rep| rep.mime_type.as_ref().is_some_and(|m| m.is_image()));

        let target_rep = match image_rep {
            Some(rep) => rep,
            None => self
                .representation_repo
                .get_representation(&entry.event_id, &selection.selection.preview_rep_id)
                .await?
                .ok_or(anyhow::anyhow!("Preview representation not found"))?,
        };

        let payload = self.payload_resolver.resolve(&target_rep).await?;

        match payload {
            ResolvedClipboardPayload::Inline { mime, bytes } => Ok(EntryResourceResult {
                blob_id: None,
                mime_type: Some(mime),
                size_bytes: target_rep.size_bytes,
                url: None,
                inline_data: Some(bytes),
            }),
            ResolvedClipboardPayload::BlobRef { mime, blob_id } => {
                let blob_id_clone = blob_id.clone();
                Ok(EntryResourceResult {
                    blob_id: Some(blob_id),
                    mime_type: Some(mime),
                    size_bytes: target_rep.size_bytes,
                    url: Some(format!("/clipboard/blobs/{}", blob_id_clone)),
                    inline_data: None,
                })
            }
        }
    }
}

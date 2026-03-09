use anyhow::Result;
use std::sync::Arc;

use uc_core::{
    clipboard::MimeType,
    ids::EntryId,
    ports::{
        BlobStorePort, ClipboardEntryRepositoryPort, ClipboardRepresentationRepositoryPort,
        ClipboardSelectionRepositoryPort,
    },
};

/// Get full clipboard entry detail
/// 获取剪贴板条目完整详情
pub struct GetEntryDetailUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    blob_store: Arc<dyn BlobStorePort>,
}

/// Detail result from GetEntryDetailUseCase
/// GetEntryDetailUseCase 返回的详情结果
#[derive(Debug)]
pub struct EntryDetailResult {
    pub id: String,
    pub content: String,
    pub size_bytes: i64,
    pub created_at_ms: i64,
    pub active_time_ms: i64,
    pub mime_type: Option<String>,
}

impl GetEntryDetailUseCase {
    pub fn new(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        blob_store: Arc<dyn BlobStorePort>,
    ) -> Self {
        Self {
            entry_repo,
            selection_repo,
            representation_repo,
            blob_store,
        }
    }

    pub async fn execute(&self, entry_id: &EntryId) -> Result<EntryDetailResult> {
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

        // Use preview_rep_id for detail view (PlainText preferred for UI preview)
        let preview_rep = self
            .representation_repo
            .get_representation(&entry.event_id, &selection.selection.preview_rep_id)
            .await?
            .ok_or(anyhow::anyhow!("Preview representation not found"))?;

        if !Self::is_text_mime(&preview_rep.mime_type) {
            return Err(anyhow::anyhow!("Entry is not text content"));
        }

        let mime_type_str = preview_rep.mime_type.as_ref().map(|mt| mt.as_str());

        let full_content = if let Some(blob_id) = &preview_rep.blob_id {
            let blob_content = self.blob_store.get(blob_id).await?;
            String::from_utf8_lossy(&blob_content).to_string()
        } else {
            String::from_utf8_lossy(
                preview_rep
                    .inline_data
                    .as_ref()
                    .ok_or(anyhow::anyhow!("No inline data"))?,
            )
            .to_string()
        };

        Ok(EntryDetailResult {
            id: entry.entry_id.to_string(),
            content: full_content,
            size_bytes: entry.total_size,
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

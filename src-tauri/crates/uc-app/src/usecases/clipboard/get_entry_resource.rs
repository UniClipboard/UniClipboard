use anyhow::Result;
use std::sync::Arc;

use uc_core::{
    clipboard::MimeType,
    ids::EntryId,
    ports::{
        ClipboardEntryRepositoryPort, ClipboardRepresentationRepositoryPort,
        ClipboardSelectionRepositoryPort,
    },
    BlobId,
};

/// Get clipboard entry resource metadata (blob reference only).
/// 获取剪贴板条目资源元信息（仅 blob 引用）。
pub struct GetEntryResourceUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
}

/// Resource metadata result from GetEntryResourceUseCase
/// GetEntryResourceUseCase 返回的资源元信息结果
#[derive(Debug, Clone)]
pub struct EntryResourceResult {
    pub entry_id: String,
    pub blob_id: BlobId,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub url: String,
}

impl GetEntryResourceUseCase {
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

    pub async fn execute(&self, entry_id: &EntryId) -> Result<EntryResourceResult> {
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

        let blob_id = preview_rep
            .blob_id
            .clone()
            .ok_or(anyhow::anyhow!("Preview representation has no blob_id"))?;

        let mime_type_str = preview_rep.mime_type.as_ref().map(MimeType::as_str);

        Ok(EntryResourceResult {
            entry_id: entry.entry_id.to_string(),
            blob_id: blob_id.clone(),
            mime_type: mime_type_str.map(String::from),
            size_bytes: preview_rep.size_bytes,
            url: format!("uc://blob/{}", blob_id),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use uc_core::clipboard::{
        ClipboardEntry, ClipboardSelection, ClipboardSelectionDecision,
        PersistedClipboardRepresentation, SelectionPolicyVersion,
    };
    use uc_core::ids::{EventId, FormatId, RepresentationId};

    struct MockEntryRepository {
        entry: Option<ClipboardEntry>,
    }

    struct MockSelectionRepository {
        selection: Option<ClipboardSelectionDecision>,
    }

    struct MockRepresentationRepository {
        rep: Option<PersistedClipboardRepresentation>,
    }

    #[async_trait]
    impl ClipboardEntryRepositoryPort for MockEntryRepository {
        async fn save_entry_and_selection(
            &self,
            _entry: &ClipboardEntry,
            _selection: &ClipboardSelectionDecision,
        ) -> Result<()> {
            Ok(())
        }

        async fn get_entry(&self, _entry_id: &EntryId) -> Result<Option<ClipboardEntry>> {
            Ok(self.entry.clone())
        }

        async fn list_entries(&self, _limit: usize, _offset: usize) -> Result<Vec<ClipboardEntry>> {
            Ok(vec![])
        }

        async fn delete_entry(&self, _entry_id: &EntryId) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardSelectionRepositoryPort for MockSelectionRepository {
        async fn get_selection(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<ClipboardSelectionDecision>> {
            Ok(self.selection.clone())
        }

        async fn delete_selection(&self, _entry_id: &EntryId) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardRepresentationRepositoryPort for MockRepresentationRepository {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            _representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(self.rep.clone())
        }

        async fn get_representation_by_id(
            &self,
            _representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_blob_id(
            &self,
            _blob_id: &BlobId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn update_blob_id(
            &self,
            _representation_id: &RepresentationId,
            _blob_id: &BlobId,
        ) -> Result<()> {
            Ok(())
        }

        async fn update_blob_id_if_none(
            &self,
            _representation_id: &RepresentationId,
            _blob_id: &BlobId,
        ) -> Result<bool> {
            Ok(false)
        }

        async fn update_processing_result(
            &self,
            _rep_id: &RepresentationId,
            _expected_states: &[uc_core::clipboard::PayloadAvailability],
            _blob_id: Option<&BlobId>,
            _new_state: uc_core::clipboard::PayloadAvailability,
            _last_error: Option<&str>,
        ) -> Result<uc_core::ports::clipboard::ProcessingUpdateOutcome> {
            Ok(uc_core::ports::clipboard::ProcessingUpdateOutcome::NotFound)
        }
    }

    #[tokio::test]
    async fn test_get_entry_resource_returns_blob_info() {
        let entry_id = EntryId::from("entry-1");
        let event_id = EventId::from("event-1");
        let rep_id = RepresentationId::from("rep-1");

        let entry = ClipboardEntry::new(entry_id.clone(), event_id.clone(), 1234, None, 2048);
        let selection = ClipboardSelectionDecision::new(
            entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: rep_id.clone(),
                secondary_rep_ids: vec![],
                preview_rep_id: rep_id.clone(),
                paste_rep_id: rep_id.clone(),
                policy_version: SelectionPolicyVersion::V1,
            },
        );
        let representation = PersistedClipboardRepresentation::new(
            rep_id,
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType::text_plain()),
            4096,
            None,
            Some(BlobId::from("blob-1")),
        );

        let uc = GetEntryResourceUseCase::new(
            Arc::new(MockEntryRepository { entry: Some(entry) }),
            Arc::new(MockSelectionRepository {
                selection: Some(selection),
            }),
            Arc::new(MockRepresentationRepository {
                rep: Some(representation),
            }),
        );

        let result = uc.execute(&entry_id).await.unwrap();

        assert_eq!(result.entry_id, "entry-1");
        assert_eq!(result.blob_id, BlobId::from("blob-1"));
        assert_eq!(result.mime_type, Some("text/plain".to_string()));
        assert_eq!(result.size_bytes, 4096);
        assert_eq!(result.url, "uc://blob/blob-1");
    }
}

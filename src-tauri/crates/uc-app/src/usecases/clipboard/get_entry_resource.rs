use anyhow::Result;
use std::sync::Arc;

#[cfg(not(test))]
use uc_core::{
    ids::EntryId,
    ports::clipboard::ResolvedClipboardPayload,
    ports::{
        ClipboardEntryRepositoryPort, ClipboardPayloadResolverPort,
        ClipboardRepresentationRepositoryPort, ClipboardSelectionRepositoryPort,
    },
    BlobId,
};

#[cfg(test)]
use uc_core::{
    clipboard::MimeType,
    ids::EntryId,
    ports::clipboard::ResolvedClipboardPayload,
    ports::{
        ClipboardEntryRepositoryPort, ClipboardPayloadResolverPort,
        ClipboardRepresentationRepositoryPort, ClipboardSelectionRepositoryPort,
    },
    BlobId,
};

/// Get clipboard entry resource metadata (blob reference only).
/// 获取剪贴板条目资源元信息（仅 blob 引用）。
pub struct GetEntryResourceUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
}

/// Resource metadata result from GetEntryResourceUseCase
/// GetEntryResourceUseCase 返回的资源元信息结果
#[derive(Debug, Clone)]
pub struct EntryResourceResult {
    pub entry_id: String,
    pub blob_id: Option<BlobId>,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub url: Option<String>,
    /// Inline data bytes when content is stored inline (small content).
    /// When present, consumers should use this directly instead of fetching via URL.
    pub inline_data: Option<Vec<u8>>,
}

impl GetEntryResourceUseCase {
    pub fn new(
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

        // Use payload resolver to handle Staged/Processing states correctly
        // This will attempt to get bytes from cache/spool when blob is not yet materialized
        let payload = self.payload_resolver.resolve(&preview_rep).await?;

        match payload {
            ResolvedClipboardPayload::Inline { mime, bytes } => Ok(EntryResourceResult {
                entry_id: entry.entry_id.to_string(),
                blob_id: None,
                mime_type: Some(mime),
                size_bytes: preview_rep.size_bytes,
                url: None,
                inline_data: Some(bytes),
            }),
            ResolvedClipboardPayload::BlobRef { mime, blob_id } => {
                let blob_id_clone = blob_id.clone();
                Ok(EntryResourceResult {
                    entry_id: entry.entry_id.to_string(),
                    blob_id: Some(blob_id),
                    mime_type: Some(mime),
                    size_bytes: preview_rep.size_bytes,
                    url: Some(format!("uc://blob/{}", blob_id_clone)),
                    inline_data: None,
                })
            }
        }
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
    use uc_core::ports::clipboard::ResolvedClipboardPayload;

    struct MockEntryRepository {
        entry: Option<ClipboardEntry>,
    }

    struct MockSelectionRepository {
        selection: Option<ClipboardSelectionDecision>,
    }

    struct MockRepresentationRepository {
        rep: Option<PersistedClipboardRepresentation>,
    }

    struct MockPayloadResolver {
        payload: Option<ResolvedClipboardPayload>,
    }

    impl MockPayloadResolver {
        fn new() -> Self {
            Self { payload: None }
        }

        fn with_inline_payload(mut self, mime: String, bytes: Vec<u8>) -> Self {
            self.payload = Some(ResolvedClipboardPayload::Inline { mime, bytes });
            self
        }

        fn with_blob_ref_payload(mut self, mime: String, blob_id: BlobId) -> Self {
            self.payload = Some(ResolvedClipboardPayload::BlobRef { mime, blob_id });
            self
        }
    }

    #[async_trait]
    impl ClipboardPayloadResolverPort for MockPayloadResolver {
        async fn resolve(
            &self,
            _representation: &PersistedClipboardRepresentation,
        ) -> anyhow::Result<ResolvedClipboardPayload> {
            self.payload
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Payload not available"))
        }
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
            Arc::new(
                MockPayloadResolver::new()
                    .with_blob_ref_payload("text/plain".to_string(), BlobId::from("blob-1")),
            ),
        );

        let result = uc.execute(&entry_id).await.unwrap();

        assert_eq!(result.entry_id, "entry-1");
        assert_eq!(result.blob_id, Some(BlobId::from("blob-1")));
        assert_eq!(result.mime_type, Some("text/plain".to_string()));
        assert_eq!(result.size_bytes, 4096);
        assert_eq!(result.url, Some("uc://blob/blob-1".to_string()));
        assert!(result.inline_data.is_none());
    }

    #[tokio::test]
    async fn test_get_entry_resource_returns_inline_data_when_no_blob() {
        let entry_id = EntryId::from("entry-2");
        let event_id = EventId::from("event-2");
        let rep_id = RepresentationId::from("rep-2");

        let entry = ClipboardEntry::new(entry_id.clone(), event_id.clone(), 1234, None, 13);
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
        // Inline representation: has inline_data but no blob_id
        let representation = PersistedClipboardRepresentation::new(
            rep_id,
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType::text_plain()),
            13,
            Some(b"Hello, world!".to_vec()),
            None, // No blob_id
        );

        let uc = GetEntryResourceUseCase::new(
            Arc::new(MockEntryRepository { entry: Some(entry) }),
            Arc::new(MockSelectionRepository {
                selection: Some(selection),
            }),
            Arc::new(MockRepresentationRepository {
                rep: Some(representation),
            }),
            Arc::new(
                MockPayloadResolver::new()
                    .with_inline_payload("text/plain".to_string(), b"Hello, world!".to_vec()),
            ),
        );

        let result = uc.execute(&entry_id).await.unwrap();

        assert_eq!(result.entry_id, "entry-2");
        assert!(result.blob_id.is_none());
        assert_eq!(result.mime_type, Some("text/plain".to_string()));
        assert_eq!(result.size_bytes, 13);
        assert!(result.url.is_none());
        assert_eq!(result.inline_data, Some(b"Hello, world!".to_vec()));
    }
}

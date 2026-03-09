//! Use case for listing clipboard entry projections
//! 列出剪贴板条目投影的用例

use anyhow::Result;
use std::sync::Arc;
use tracing::warn;
use uc_core::clipboard::PayloadAvailability;
use uc_core::network::protocol::MIME_IMAGE_PREFIX;
use uc_core::ports::{
    ClipboardEntryRepositoryPort, ClipboardRepresentationRepositoryPort,
    ClipboardSelectionRepositoryPort, ThumbnailRepositoryPort,
};

/// DTO for clipboard entry projection (returned to command layer)
/// 剪贴板条目投影 DTO（返回给命令层）
#[derive(Debug, Clone, PartialEq)]
pub struct EntryProjectionDto {
    pub id: String,
    pub preview: String,
    pub has_detail: bool,
    pub size_bytes: i64,
    pub captured_at: i64,
    pub content_type: String,
    pub thumbnail_url: Option<String>,
    // TODO: is_encrypted, is_favorited to be implemented later
    pub is_encrypted: bool,
    pub is_favorited: bool,
    pub updated_at: i64,
    pub active_time: i64,
}

/// Error type for list projections use case
#[derive(Debug, thiserror::Error)]
pub enum ListProjectionsError {
    #[error("Invalid limit: {0}")]
    InvalidLimit(String),

    #[error("Repository error: {0}")]
    RepositoryError(String),

    #[error("Selection not found for entry {0}")]
    SelectionNotFound(String),

    #[error("Representation not found: {0}")]
    RepresentationNotFound(String),
}

/// Use case for listing clipboard entry projections
pub struct ListClipboardEntryProjections {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    max_limit: usize,
}

impl ListClipboardEntryProjections {
    /// Create a new use case instance
    pub fn new(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    ) -> Self {
        Self {
            entry_repo,
            selection_repo,
            representation_repo,
            thumbnail_repo,
            max_limit: 1000,
        }
    }

    /// Execute the use case
    pub async fn execute(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<EntryProjectionDto>, ListProjectionsError> {
        // Validate limit
        if limit == 0 {
            return Err(ListProjectionsError::InvalidLimit(format!(
                "Must be at least 1, got {}",
                limit
            )));
        }

        if limit > self.max_limit {
            return Err(ListProjectionsError::InvalidLimit(format!(
                "Must be at most {}, got {}",
                self.max_limit, limit
            )));
        }

        // Query entries from repository
        let entries = self
            .entry_repo
            .list_entries(limit, offset)
            .await
            .map_err(|e| ListProjectionsError::RepositoryError(e.to_string()))?;

        let mut projections = Vec::with_capacity(entries.len());

        for entry in entries {
            let entry_id_str = entry.entry_id.inner().clone();
            let event_id_str = entry.event_id.inner().clone();
            let captured_at = entry.created_at_ms;
            let active_time = entry.active_time_ms;

            // Get selection for this entry
            let selection = match self.selection_repo.get_selection(&entry.entry_id).await {
                Ok(Some(selection)) => selection,
                Ok(None) => {
                    warn!(
                        entry_id = %entry_id_str,
                        "Skipping entry without selection while listing projections"
                    );
                    continue;
                }
                Err(e) => {
                    warn!(
                        entry_id = %entry_id_str,
                        error = %e,
                        "Skipping entry due to selection lookup failure"
                    );
                    continue;
                }
            };

            // Get preview representation
            let preview_rep_id = selection.selection.preview_rep_id.inner().clone();
            let representation = match self
                .representation_repo
                .get_representation(&entry.event_id, &selection.selection.preview_rep_id)
                .await
            {
                Ok(Some(rep)) => rep,
                Ok(None) => {
                    warn!(
                        event_id = %event_id_str,
                        preview_rep_id = %preview_rep_id,
                        "Skipping entry because preview representation is missing"
                    );
                    continue;
                }
                Err(e) => {
                    warn!(
                        event_id = %event_id_str,
                        preview_rep_id = %preview_rep_id,
                        error = %e,
                        "Skipping entry due to preview representation lookup failure"
                    );
                    continue;
                }
            };

            let is_image = representation
                .mime_type
                .as_ref()
                .map(|mt| {
                    mt.as_str()
                        .to_ascii_lowercase()
                        .starts_with(MIME_IMAGE_PREFIX)
                })
                .unwrap_or(false);

            let preview = if let Some(data) = representation.inline_data.as_ref() {
                String::from_utf8_lossy(data).trim().to_string()
            } else if is_image {
                format!("Image ({} bytes)", representation.size_bytes)
            } else {
                entry
                    .title
                    .as_ref()
                    .map(|title| title.trim().to_string())
                    .filter(|title| !title.is_empty())
                    .unwrap_or_else(|| {
                        "Text content (full payload in background processing)".to_string()
                    })
            };

            // Get content type from representation
            let content_type = representation
                .mime_type
                .as_ref()
                .map(|mt| mt.as_str().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let thumbnail_url = if is_image {
                match self
                    .thumbnail_repo
                    .get_by_representation_id(&selection.selection.preview_rep_id)
                    .await
                {
                    Ok(Some(_metadata)) => Some(format!("uc://thumbnail/{}", preview_rep_id)),
                    Ok(None) => None,
                    Err(err) => {
                        tracing::error!(
                            error = %err,
                            entry_id = %entry_id_str,
                            "Failed to fetch thumbnail metadata"
                        );
                        None
                    }
                }
            } else {
                None
            };

            // has_detail controls whether frontend should try fetching full content.
            // For staged/processing payloads, full content may become available via blob shortly.
            let has_detail = representation.blob_id.is_some()
                || matches!(
                    representation.payload_state(),
                    PayloadAvailability::Staged | PayloadAvailability::Processing
                );

            projections.push(EntryProjectionDto {
                id: entry_id_str,
                preview,
                has_detail,
                size_bytes: entry.total_size,
                captured_at,
                content_type,
                thumbnail_url,
                is_encrypted: false, // TODO: implement later
                is_favorited: false, // TODO: implement later
                updated_at: captured_at,
                active_time,
            });
        }

        Ok(projections)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use uc_core::clipboard::{
        ClipboardEntry, ClipboardSelection, MimeType, PersistedClipboardRepresentation,
        SelectionPolicyVersion, ThumbnailMetadata,
    };
    use uc_core::ids::{EntryId, EventId, FormatId, RepresentationId};
    use uc_core::BlobId;
    use uc_core::ClipboardSelectionDecision;

    // Mock repositories for testing
    struct MockEntryRepository {
        entries: Vec<ClipboardEntry>,
    }

    struct MockSelectionRepository {
        selections: std::collections::HashMap<String, uc_core::ClipboardSelectionDecision>,
    }

    struct MockRepresentationRepository {
        representations:
            std::collections::HashMap<(String, String), uc_core::PersistedClipboardRepresentation>,
        fail_keys: HashSet<(String, String)>,
    }

    struct MockThumbnailRepository {
        thumbnails: HashMap<String, ThumbnailMetadata>,
    }

    #[async_trait::async_trait]
    impl ClipboardEntryRepositoryPort for MockEntryRepository {
        async fn save_entry_and_selection(
            &self,
            _entry: &ClipboardEntry,
            _selection: &ClipboardSelectionDecision,
        ) -> Result<()> {
            unimplemented!()
        }

        async fn get_entry(&self, _entry_id: &EntryId) -> Result<Option<ClipboardEntry>> {
            unimplemented!()
        }

        async fn list_entries(&self, limit: usize, offset: usize) -> Result<Vec<ClipboardEntry>> {
            Ok(self
                .entries
                .iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect())
        }

        async fn delete_entry(&self, _entry_id: &EntryId) -> Result<()> {
            unimplemented!()
        }
    }

    #[async_trait::async_trait]
    impl ClipboardSelectionRepositoryPort for MockSelectionRepository {
        async fn get_selection(
            &self,
            entry_id: &EntryId,
        ) -> Result<Option<uc_core::ClipboardSelectionDecision>> {
            Ok(self.selections.get(entry_id.inner()).cloned())
        }

        async fn delete_selection(&self, _entry_id: &EntryId) -> Result<()> {
            unimplemented!()
        }
    }

    #[async_trait::async_trait]
    impl ClipboardRepresentationRepositoryPort for MockRepresentationRepository {
        async fn get_representation(
            &self,
            event_id: &EventId,
            rep_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            let key = (event_id.inner().clone(), rep_id.inner().clone());
            if self.fail_keys.contains(&key) {
                return Err(anyhow::anyhow!("payload_state BlobReady requires blob_id"));
            }
            Ok(self.representations.get(&key).cloned())
        }

        async fn get_representation_by_id(
            &self,
            _representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_blob_id(
            &self,
            _blob_id: &uc_core::BlobId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn update_blob_id(
            &self,
            _representation_id: &RepresentationId,
            _blob_id: &uc_core::BlobId,
        ) -> Result<()> {
            unimplemented!()
        }

        async fn update_blob_id_if_none(
            &self,
            _representation_id: &RepresentationId,
            _blob_id: &uc_core::BlobId,
        ) -> Result<bool> {
            unimplemented!()
        }

        async fn update_processing_result(
            &self,
            _rep_id: &RepresentationId,
            _expected_states: &[uc_core::clipboard::PayloadAvailability],
            _blob_id: Option<&uc_core::BlobId>,
            _new_state: uc_core::clipboard::PayloadAvailability,
            _last_error: Option<&str>,
        ) -> Result<uc_core::ports::clipboard::ProcessingUpdateOutcome> {
            unimplemented!()
        }
    }

    #[async_trait::async_trait]
    impl uc_core::ports::clipboard::ThumbnailRepositoryPort for MockThumbnailRepository {
        async fn get_by_representation_id(
            &self,
            representation_id: &RepresentationId,
        ) -> Result<Option<ThumbnailMetadata>> {
            Ok(self.thumbnails.get(representation_id.inner()).map(|meta| {
                ThumbnailMetadata::new(
                    meta.representation_id.clone(),
                    meta.thumbnail_blob_id.clone(),
                    meta.thumbnail_mime_type.clone(),
                    meta.original_width,
                    meta.original_height,
                    meta.original_size_bytes,
                    meta.created_at_ms,
                )
            }))
        }

        async fn insert_thumbnail(&self, _metadata: &ThumbnailMetadata) -> Result<()> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn test_validates_limit_zero() {
        let entry_repo = Arc::new(MockEntryRepository { entries: vec![] });
        let selection_repo = Arc::new(MockSelectionRepository {
            selections: std::collections::HashMap::new(),
        });
        let representation_repo = Arc::new(MockRepresentationRepository {
            representations: std::collections::HashMap::new(),
            fail_keys: HashSet::new(),
        });
        let thumbnail_repo = Arc::new(MockThumbnailRepository {
            thumbnails: HashMap::new(),
        });

        let use_case = ListClipboardEntryProjections::new(
            entry_repo,
            selection_repo,
            representation_repo,
            thumbnail_repo,
        );

        let result = use_case.execute(0, 0).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ListProjectionsError::InvalidLimit(_)));
    }

    #[tokio::test]
    async fn test_validates_limit_exceeds_max() {
        let entry_repo = Arc::new(MockEntryRepository { entries: vec![] });
        let selection_repo = Arc::new(MockSelectionRepository {
            selections: std::collections::HashMap::new(),
        });
        let representation_repo = Arc::new(MockRepresentationRepository {
            representations: std::collections::HashMap::new(),
            fail_keys: HashSet::new(),
        });
        let thumbnail_repo = Arc::new(MockThumbnailRepository {
            thumbnails: HashMap::new(),
        });

        let use_case = ListClipboardEntryProjections::new(
            entry_repo,
            selection_repo,
            representation_repo,
            thumbnail_repo,
        );

        let result = use_case.execute(2000, 0).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ListProjectionsError::InvalidLimit(_)));
    }

    #[tokio::test]
    async fn test_representation_repo_requires_blob_lookup() {
        // 编译期失败即为预期：新增方法未实现
        let representation_repo = MockRepresentationRepository {
            representations: std::collections::HashMap::new(),
            fail_keys: HashSet::new(),
        };
        let blob_id = uc_core::BlobId::from("test-blob");
        let _ = representation_repo
            .get_representation_by_blob_id(&blob_id)
            .await;
    }

    #[tokio::test]
    async fn test_includes_thumbnail_url_for_image_preview() {
        let entry_id = EntryId::from("entry-1");
        let event_id = EventId::from("event-1");
        let rep_id = RepresentationId::from("rep-1");
        let thumb_blob_id = BlobId::from("thumb-1");

        let entry = ClipboardEntry::new(entry_id.clone(), event_id.clone(), 123, None, 2048);

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
            rep_id.clone(),
            FormatId::from("public.png"),
            Some(MimeType("image/png".to_string())),
            2048,
            None,
            Some(BlobId::from("blob-1")),
        );

        let thumbnail = ThumbnailMetadata::new(
            rep_id.clone(),
            thumb_blob_id.clone(),
            MimeType("image/webp".to_string()),
            120,
            80,
            1024,
            None,
        );

        let entry_repo = Arc::new(MockEntryRepository {
            entries: vec![entry],
        });
        let selection_repo = Arc::new(MockSelectionRepository {
            selections: HashMap::from([(entry_id.inner().clone(), selection)]),
        });
        let representation_repo = Arc::new(MockRepresentationRepository {
            representations: HashMap::from([(
                (event_id.inner().clone(), rep_id.inner().clone()),
                representation,
            )]),
            fail_keys: HashSet::new(),
        });
        let thumbnail_repo = Arc::new(MockThumbnailRepository {
            thumbnails: HashMap::from([(rep_id.inner().clone(), thumbnail)]),
        });

        let use_case = ListClipboardEntryProjections::new(
            entry_repo,
            selection_repo,
            representation_repo,
            thumbnail_repo,
        );

        let result = use_case.execute(50, 0).await.expect("expected projections");
        let projection = result.first().expect("expected projection");

        assert_eq!(
            projection.thumbnail_url,
            Some(format!("uc://thumbnail/{}", rep_id.inner()))
        );
    }

    #[tokio::test]
    async fn test_staged_text_projection_uses_title_preview_and_has_detail() {
        let entry_id = EntryId::from("entry-staged-text");
        let event_id = EventId::from("event-staged-text");
        let rep_id = RepresentationId::from("rep-staged-text");

        let entry = ClipboardEntry::new(
            entry_id.clone(),
            event_id.clone(),
            999,
            Some("  staged text title  ".to_string()),
            42_000,
        );

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

        let representation = PersistedClipboardRepresentation::new_with_state(
            rep_id.clone(),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType("text/plain".to_string())),
            42_000,
            None,
            None,
            uc_core::clipboard::PayloadAvailability::Staged,
            None,
        )
        .expect("valid staged representation");

        let entry_repo = Arc::new(MockEntryRepository {
            entries: vec![entry],
        });
        let selection_repo = Arc::new(MockSelectionRepository {
            selections: HashMap::from([(entry_id.inner().clone(), selection)]),
        });
        let representation_repo = Arc::new(MockRepresentationRepository {
            representations: HashMap::from([(
                (event_id.inner().clone(), rep_id.inner().clone()),
                representation,
            )]),
            fail_keys: HashSet::new(),
        });
        let thumbnail_repo = Arc::new(MockThumbnailRepository {
            thumbnails: HashMap::new(),
        });

        let use_case = ListClipboardEntryProjections::new(
            entry_repo,
            selection_repo,
            representation_repo,
            thumbnail_repo,
        );

        let result = use_case.execute(50, 0).await.expect("expected projections");
        let projection = result.first().expect("expected projection");

        assert_eq!(projection.preview, "staged text title");
        assert!(projection.has_detail);
        assert_eq!(projection.thumbnail_url, None);
    }

    #[tokio::test]
    async fn test_skips_corrupted_representation_and_returns_other_entries() {
        let good_entry_id = EntryId::from("entry-good");
        let good_event_id = EventId::from("event-good");
        let good_rep_id = RepresentationId::from("rep-good");

        let bad_entry_id = EntryId::from("entry-bad");
        let bad_event_id = EventId::from("event-bad");
        let bad_rep_id = RepresentationId::from("rep-bad");

        let good_entry = ClipboardEntry::new(
            good_entry_id.clone(),
            good_event_id.clone(),
            100,
            Some("good title".to_string()),
            12,
        );
        let bad_entry = ClipboardEntry::new(
            bad_entry_id.clone(),
            bad_event_id.clone(),
            101,
            Some("bad title".to_string()),
            34,
        );

        let good_selection = ClipboardSelectionDecision::new(
            good_entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: good_rep_id.clone(),
                secondary_rep_ids: vec![],
                preview_rep_id: good_rep_id.clone(),
                paste_rep_id: good_rep_id.clone(),
                policy_version: SelectionPolicyVersion::V1,
            },
        );
        let bad_selection = ClipboardSelectionDecision::new(
            bad_entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: bad_rep_id.clone(),
                secondary_rep_ids: vec![],
                preview_rep_id: bad_rep_id.clone(),
                paste_rep_id: bad_rep_id.clone(),
                policy_version: SelectionPolicyVersion::V1,
            },
        );

        let good_representation = PersistedClipboardRepresentation::new(
            good_rep_id.clone(),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType("text/plain".to_string())),
            12,
            Some(b"good-content".to_vec()),
            None,
        );

        let entry_repo = Arc::new(MockEntryRepository {
            entries: vec![bad_entry, good_entry],
        });
        let selection_repo = Arc::new(MockSelectionRepository {
            selections: HashMap::from([
                (good_entry_id.inner().clone(), good_selection),
                (bad_entry_id.inner().clone(), bad_selection),
            ]),
        });
        let representation_repo = Arc::new(MockRepresentationRepository {
            representations: HashMap::from([(
                (good_event_id.inner().clone(), good_rep_id.inner().clone()),
                good_representation,
            )]),
            fail_keys: HashSet::from([(bad_event_id.inner().clone(), bad_rep_id.inner().clone())]),
        });
        let thumbnail_repo = Arc::new(MockThumbnailRepository {
            thumbnails: HashMap::new(),
        });

        let use_case = ListClipboardEntryProjections::new(
            entry_repo,
            selection_repo,
            representation_repo,
            thumbnail_repo,
        );

        let result = use_case
            .execute(50, 0)
            .await
            .expect("corrupted representation should be skipped");

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, good_entry_id.inner().clone());
        assert_eq!(result[0].preview, "good-content");
    }
}

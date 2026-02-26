use crate::usecases::clipboard::ClipboardIntegrationMode;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

use uc_core::{
    clipboard::{
        ObservedClipboardRepresentation, PersistedClipboardRepresentation, SystemClipboardSnapshot,
    },
    ids::{EntryId, EventId, RepresentationId},
    ports::{
        BlobStorePort, ClipboardChangeOriginPort, ClipboardEntryRepositoryPort,
        ClipboardRepresentationRepositoryPort, ClipboardSelectionRepositoryPort,
        SystemClipboardPort,
    },
    ClipboardChangeOrigin,
};

/// Reconstructs a system clipboard state from a historical clipboard entry,
/// restoring the primary selected representation only.
pub struct RestoreClipboardSelectionUseCase {
    clipboard_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    local_clipboard: Arc<dyn SystemClipboardPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    blob_store: Arc<dyn BlobStorePort>,
    clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
    mode: ClipboardIntegrationMode,
}

impl RestoreClipboardSelectionUseCase {
    /// Creates a new use case instance that copies clipboard entries from history to the system clipboard.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use uc_app::usecases::clipboard::restore_clipboard_selection::RestoreClipboardSelectionUseCase;
    /// use uc_core::ports::{BlobStorePort, ClipboardChangeOriginPort, ClipboardEntryRepositoryPort, ClipboardRepresentationRepositoryPort, ClipboardSelectionRepositoryPort, SystemClipboardPort};
    /// // All parameters must implement their respective ports
    /// // let use_case = RestoreClipboardSelectionUseCase::new(
    /// //     Arc::new(clipboard_repo),
    /// //     Arc::new(local_clipboard),
    /// //     Arc::new(selection_repo),
    /// //     Arc::new(representation_repo),
    /// //     Arc::new(blob_store),
    /// //     Arc::new(clipboard_change_origin),
    /// // );
    /// ```
    pub fn new(
        clipboard_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        local_clipboard: Arc<dyn SystemClipboardPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        blob_store: Arc<dyn BlobStorePort>,
        clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
        mode: ClipboardIntegrationMode,
    ) -> Self {
        Self {
            clipboard_repo,
            local_clipboard,
            selection_repo,
            representation_repo,
            blob_store,
            clipboard_change_origin,
            mode,
        }
    }

    pub async fn build_snapshot(&self, entry_id: &EntryId) -> Result<SystemClipboardSnapshot> {
        // 1. 读取 Entry
        let entry = self
            .clipboard_repo
            .get_entry(entry_id)
            .await?
            .ok_or(anyhow::anyhow!("Entry not found"))?;

        // 2. 获取 Selection 决策
        let selection = self
            .selection_repo
            .get_selection(entry_id)
            .await?
            .ok_or(anyhow::anyhow!("Selection not found"))?;

        // 3. 收集候选 representations
        let mut candidate_ids = Vec::new();
        candidate_ids.push(selection.selection.paste_rep_id.clone());
        candidate_ids.push(selection.selection.primary_rep_id.clone());
        candidate_ids.push(selection.selection.preview_rep_id.clone());
        candidate_ids.extend(selection.selection.secondary_rep_ids.clone());

        let mut seen = std::collections::HashSet::new();
        candidate_ids.retain(|rep_id| seen.insert(rep_id.clone()));

        let mut candidates = Vec::new();
        for rep_id in &candidate_ids {
            let rep = self
                .representation_repo
                .get_representation(&entry.event_id, rep_id)
                .await?;
            if let Some(rep) = rep {
                candidates.push(rep);
            } else if *rep_id == selection.selection.paste_rep_id {
                return Err(anyhow::anyhow!(
                    "Representation {} not found for event {}",
                    rep_id,
                    entry.event_id
                ));
            }
        }

        let restore_rep = Self::select_restore_representation(
            &candidates,
            &selection.selection.paste_rep_id,
            &entry.event_id,
        )?;

        let bytes = if let Some(inline_data) = &restore_rep.inline_data {
            inline_data.clone()
        } else if let Some(blob_id) = &restore_rep.blob_id {
            self.blob_store.get(blob_id).await?
        } else {
            return Err(anyhow::anyhow!(
                "Representation has no data: {}",
                restore_rep.id
            ));
        };

        let representations = vec![ObservedClipboardRepresentation {
            id: restore_rep.id.clone(),
            format_id: restore_rep.format_id.clone(),
            mime: restore_rep.mime_type.clone(),
            bytes,
        }];

        // 5. 构造 Snapshot
        Ok(SystemClipboardSnapshot {
            ts_ms: chrono::Utc::now().timestamp_millis(),
            representations,
        })
    }

    fn select_restore_representation<'a>(
        candidates: &'a [PersistedClipboardRepresentation],
        paste_rep_id: &RepresentationId,
        event_id: &EventId,
    ) -> Result<&'a PersistedClipboardRepresentation> {
        if let Some(rep) = candidates
            .iter()
            .find(|rep| Self::is_plain_text_representation(*rep))
        {
            return Ok(rep);
        }

        candidates
            .iter()
            .find(|rep| rep.id == *paste_rep_id)
            .ok_or(anyhow::anyhow!(
                "Representation {} not found for event {}",
                paste_rep_id,
                event_id
            ))
    }

    fn is_plain_text_representation(rep: &PersistedClipboardRepresentation) -> bool {
        if let Some(mime) = &rep.mime_type {
            let mime_str = mime.as_str();
            let mime_lower = mime_str.to_ascii_lowercase();
            if mime_lower == "text/plain" || mime_lower.starts_with("text/plain;") {
                return true;
            }
        }

        let format_id = rep.format_id.as_ref();
        format_id.eq_ignore_ascii_case("text")
            || format_id.eq_ignore_ascii_case("public.utf8-plain-text")
            || format_id.eq_ignore_ascii_case("public.text")
            || format_id.eq_ignore_ascii_case("NSStringPboardType")
    }

    pub async fn restore_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        if !self.mode.allow_os_write() {
            return Err(anyhow::anyhow!(
                "System clipboard writes disabled (UC_CLIPBOARD_MODE=passive)"
            ));
        }

        self.clipboard_change_origin
            .set_next_origin(ClipboardChangeOrigin::LocalRestore, Duration::from_secs(2))
            .await;

        if let Err(err) = self.local_clipboard.write_snapshot(snapshot) {
            self.clipboard_change_origin
                .consume_origin_or_default(ClipboardChangeOrigin::LocalCapture)
                .await;
            return Err(err);
        }

        Ok(())
    }

    pub async fn execute(&self, entry_id: &EntryId) -> Result<()> {
        let snapshot = self.build_snapshot(entry_id).await?;
        self.restore_snapshot(snapshot).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use uc_core::clipboard::{
        ClipboardEntry, ClipboardSelection, ClipboardSelectionDecision, MimeType,
        PersistedClipboardRepresentation, SelectionPolicyVersion,
    };
    use uc_core::ids::{EventId, FormatId, RepresentationId};
    use uc_core::ports::clipboard::ClipboardChangeOriginPort;
    use uc_core::ports::clipboard::ProcessingUpdateOutcome;
    use uc_core::ClipboardChangeOrigin;

    struct MockEntryRepository {
        entry: Option<ClipboardEntry>,
    }

    struct MockSelectionRepository {
        selection: Option<ClipboardSelectionDecision>,
    }

    struct MockRepresentationRepository {
        reps: HashMap<RepresentationId, PersistedClipboardRepresentation>,
    }

    struct MockBlobStore;

    struct MockSystemClipboard;

    struct MockClipboardChangeOrigin {
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    struct NoopClipboardChangeOrigin;

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
            representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(self.reps.get(representation_id).cloned())
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
            Ok(())
        }

        async fn update_blob_id_if_none(
            &self,
            _representation_id: &RepresentationId,
            _blob_id: &uc_core::BlobId,
        ) -> Result<bool> {
            Ok(false)
        }

        async fn update_processing_result(
            &self,
            _rep_id: &RepresentationId,
            _expected_states: &[uc_core::clipboard::PayloadAvailability],
            _blob_id: Option<&uc_core::BlobId>,
            _new_state: uc_core::clipboard::PayloadAvailability,
            _last_error: Option<&str>,
        ) -> Result<ProcessingUpdateOutcome> {
            Ok(ProcessingUpdateOutcome::NotFound)
        }
    }

    #[async_trait]
    impl BlobStorePort for MockBlobStore {
        async fn put(
            &self,
            _blob_id: &uc_core::BlobId,
            _data: &[u8],
        ) -> Result<std::path::PathBuf> {
            Ok(std::path::PathBuf::from("/tmp/mock"))
        }

        async fn get(&self, _blob_id: &uc_core::BlobId) -> Result<Vec<u8>> {
            Err(anyhow::anyhow!("unexpected blob fetch"))
        }
    }

    impl SystemClipboardPort for MockSystemClipboard {
        fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
            Ok(SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            })
        }

        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> Result<()> {
            Ok(())
        }
    }

    struct FailingSystemClipboard {
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    struct TrackingSystemClipboard {
        write_calls: Arc<AtomicUsize>,
    }

    impl SystemClipboardPort for FailingSystemClipboard {
        fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
            Ok(SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            })
        }

        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> Result<()> {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push("write_snapshot");
            }
            Err(anyhow::anyhow!("write failed"))
        }
    }

    impl SystemClipboardPort for TrackingSystemClipboard {
        fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
            Ok(SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            })
        }

        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> Result<()> {
            self.write_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardChangeOriginPort for MockClipboardChangeOrigin {
        async fn set_next_origin(&self, _origin: ClipboardChangeOrigin, _ttl: Duration) {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push("set_origin");
            }
        }

        async fn consume_origin_or_default(
            &self,
            default_origin: ClipboardChangeOrigin,
        ) -> ClipboardChangeOrigin {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push("consume_origin");
            }
            default_origin
        }
    }

    #[async_trait]
    impl ClipboardChangeOriginPort for NoopClipboardChangeOrigin {
        async fn set_next_origin(&self, _origin: ClipboardChangeOrigin, _ttl: Duration) {}

        async fn consume_origin_or_default(
            &self,
            default_origin: ClipboardChangeOrigin,
        ) -> ClipboardChangeOrigin {
            default_origin
        }
    }

    #[tokio::test]
    async fn build_snapshot_returns_only_paste_representation() {
        let entry_id = EntryId::from("entry-1");
        let event_id = EventId::from("event-1");
        let paste_rep_id = RepresentationId::from("rep-paste");
        let secondary_rep_id = RepresentationId::from("rep-secondary");

        let selection = ClipboardSelection {
            primary_rep_id: paste_rep_id.clone(),
            secondary_rep_ids: vec![secondary_rep_id.clone()],
            preview_rep_id: paste_rep_id.clone(),
            paste_rep_id: paste_rep_id.clone(),
            policy_version: SelectionPolicyVersion::V1,
        };

        let entry = ClipboardEntry::new(entry_id.clone(), event_id.clone(), 1, None, 0);

        let primary_representation = PersistedClipboardRepresentation::new(
            paste_rep_id.clone(),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType::text_plain()),
            3,
            Some(vec![1, 2, 3]),
            None,
        );

        let secondary_representation = PersistedClipboardRepresentation::new(
            secondary_rep_id.clone(),
            FormatId::from("public.html"),
            Some(MimeType::text_html()),
            3,
            Some(vec![4, 5, 6]),
            None,
        );

        let uc = RestoreClipboardSelectionUseCase::new(
            Arc::new(MockEntryRepository { entry: Some(entry) }),
            Arc::new(MockSystemClipboard),
            Arc::new(MockSelectionRepository {
                selection: Some(ClipboardSelectionDecision::new(entry_id.clone(), selection)),
            }),
            Arc::new(MockRepresentationRepository {
                reps: HashMap::from([
                    (paste_rep_id.clone(), primary_representation),
                    (secondary_rep_id.clone(), secondary_representation),
                ]),
            }),
            Arc::new(MockBlobStore),
            Arc::new(NoopClipboardChangeOrigin),
            ClipboardIntegrationMode::Full,
        );

        let snapshot = uc.build_snapshot(&entry_id).await.unwrap();

        assert_eq!(snapshot.representations.len(), 1);
        assert_eq!(snapshot.representations[0].id, paste_rep_id);
    }

    #[tokio::test]
    async fn build_snapshot_prefers_plain_text_over_rich_text() {
        let entry_id = EntryId::from("entry-plain-preferred");
        let event_id = EventId::from("event-plain-preferred");
        let plain_rep_id = RepresentationId::from("rep-plain");
        let rich_rep_id = RepresentationId::from("rep-rich");

        let selection = ClipboardSelection {
            primary_rep_id: rich_rep_id.clone(),
            secondary_rep_ids: vec![plain_rep_id.clone()],
            preview_rep_id: rich_rep_id.clone(),
            paste_rep_id: rich_rep_id.clone(),
            policy_version: SelectionPolicyVersion::V1,
        };

        let entry = ClipboardEntry::new(entry_id.clone(), event_id.clone(), 1, None, 0);

        let plain_representation = PersistedClipboardRepresentation::new(
            plain_rep_id.clone(),
            FormatId::from("text"),
            Some(MimeType::text_plain()),
            5,
            Some(b"hello".to_vec()),
            None,
        );

        let rich_representation = PersistedClipboardRepresentation::new(
            rich_rep_id.clone(),
            FormatId::from("html"),
            Some(MimeType::text_html()),
            12,
            Some(b"<b>hi</b>".to_vec()),
            None,
        );

        let uc = RestoreClipboardSelectionUseCase::new(
            Arc::new(MockEntryRepository { entry: Some(entry) }),
            Arc::new(MockSystemClipboard),
            Arc::new(MockSelectionRepository {
                selection: Some(ClipboardSelectionDecision::new(entry_id.clone(), selection)),
            }),
            Arc::new(MockRepresentationRepository {
                reps: HashMap::from([
                    (plain_rep_id.clone(), plain_representation),
                    (rich_rep_id.clone(), rich_representation),
                ]),
            }),
            Arc::new(MockBlobStore),
            Arc::new(NoopClipboardChangeOrigin),
            ClipboardIntegrationMode::Full,
        );

        let snapshot = uc.build_snapshot(&entry_id).await.unwrap();

        assert_eq!(snapshot.representations.len(), 1);
        assert_eq!(snapshot.representations[0].id, plain_rep_id);
    }

    #[tokio::test]
    async fn restore_snapshot_clears_origin_on_write_error() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let uc = RestoreClipboardSelectionUseCase::new(
            Arc::new(MockEntryRepository { entry: None }),
            Arc::new(FailingSystemClipboard {
                calls: calls.clone(),
            }),
            Arc::new(MockSelectionRepository { selection: None }),
            Arc::new(MockRepresentationRepository {
                reps: HashMap::new(),
            }),
            Arc::new(MockBlobStore),
            Arc::new(MockClipboardChangeOrigin {
                calls: calls.clone(),
            }),
            ClipboardIntegrationMode::Full,
        );

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![],
        };

        let result = uc.restore_snapshot(snapshot).await;

        assert!(result.is_err());
        let calls = calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec!["set_origin", "write_snapshot", "consume_origin"]
        );
    }

    #[tokio::test]
    async fn restore_snapshot_returns_error_in_passive_mode_without_writing() {
        let write_calls = Arc::new(AtomicUsize::new(0));
        let origin_calls = Arc::new(Mutex::new(Vec::new()));
        let uc = RestoreClipboardSelectionUseCase::new(
            Arc::new(MockEntryRepository { entry: None }),
            Arc::new(TrackingSystemClipboard {
                write_calls: write_calls.clone(),
            }),
            Arc::new(MockSelectionRepository { selection: None }),
            Arc::new(MockRepresentationRepository {
                reps: HashMap::new(),
            }),
            Arc::new(MockBlobStore),
            Arc::new(MockClipboardChangeOrigin {
                calls: origin_calls.clone(),
            }),
            ClipboardIntegrationMode::Passive,
        );

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![],
        };

        let result = uc.restore_snapshot(snapshot).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("System clipboard writes disabled (UC_CLIPBOARD_MODE=passive)"));
        assert_eq!(write_calls.load(Ordering::SeqCst), 0);
        assert!(origin_calls.lock().unwrap().is_empty());
    }
}

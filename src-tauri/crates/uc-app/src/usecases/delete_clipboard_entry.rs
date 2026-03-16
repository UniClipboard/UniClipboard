use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, info_span, warn, Instrument};
use uc_core::ids::EntryId;
use uc_core::ports::{
    ClipboardEntryRepositoryPort, ClipboardEventWriterPort, ClipboardRepresentationRepositoryPort,
    ClipboardSelectionRepositoryPort,
};

/// Use case for deleting clipboard entries with all associated data.
/// 删除剪贴板条目及其所有关联数据的用例。
pub struct DeleteClipboardEntry {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    event_writer: Arc<dyn ClipboardEventWriterPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    /// The managed file-cache directory. Only files located inside this directory
    /// are deleted from disk when an entry is removed. Files outside this boundary
    /// are user-owned originals and must never be touched.
    file_cache_dir: Option<PathBuf>,
}

impl DeleteClipboardEntry {
    /// Constructs a `DeleteClipboardEntry` use case from repository and event-writer ports.
    pub fn from_ports(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        event_writer: Arc<dyn ClipboardEventWriterPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    ) -> Self {
        Self {
            entry_repo,
            selection_repo,
            event_writer,
            representation_repo,
            file_cache_dir: None,
        }
    }

    /// Sets the managed file-cache directory.
    ///
    /// Only files whose path is inside this directory will be deleted from disk when
    /// an entry is removed. This prevents the deletion of user-owned original files.
    pub fn with_file_cache_dir(mut self, dir: PathBuf) -> Self {
        self.file_cache_dir = Some(dir);
        self
    }

    /// Deletes a clipboard entry and its associated selection, event, and snapshot representations in the required order.
    /// For file entries (text/uri-list), also deletes the cache files from disk.
    ///
    /// Deletion order (respecting foreign key constraints):
    /// 1. Verify the entry exists (returns an error if missing).
    /// 1b. If entry has text/uri-list representation, delete cache files from disk.
    /// 2. Delete the clipboard selection associated with the entry.
    /// 3. Delete the clipboard entry (must be deleted before its referenced event).
    /// 4. Delete the event and its snapshot representations using the entry's `event_id`.
    #[tracing::instrument(
        name = "usecase.delete_clipboard_entry.execute",
        skip(self),
        fields(entry_id = %entry_id)
    )]
    pub async fn execute(&self, entry_id: &EntryId) -> Result<()> {
        // 1. Fetch entry to verify existence and get event_id
        let entry = async {
            self.entry_repo
                .get_entry(entry_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Clipboard entry not found: {}", entry_id))
        }
        .instrument(info_span!(
            "fetch_entry",
            entry_id = %entry_id
        ))
        .await?;
        let event_id = entry.event_id.clone();

        // 1b. Check for file representations and delete cache files.
        // Only files that live inside the managed file_cache_dir are deleted.
        // Files outside that boundary are user-owned originals and must not be touched.
        async {
            let Some(ref cache_dir) = self.file_cache_dir else {
                // No cache dir configured — skip file deletion entirely to be safe.
                return;
            };

            if let Ok(representations) = self
                .representation_repo
                .get_representations_for_event(&event_id)
                .await
            {
                for rep in &representations {
                    let mime = rep.mime_type.as_ref().map(|m| m.as_str()).unwrap_or("");
                    if mime.contains("uri-list") {
                        // Parse URI list content and delete only files inside the cache dir
                        if let Some(ref inline) = rep.inline_data {
                            let uri_text = String::from_utf8_lossy(inline);
                            for line in uri_text.lines() {
                                let line = line.trim();
                                if line.is_empty() || line.starts_with('#') {
                                    continue;
                                }
                                // Support both file:// URIs and native paths
                                let path = if line.starts_with("file://") {
                                    url::Url::parse(line)
                                        .ok()
                                        .and_then(|u| u.to_file_path().ok())
                                } else {
                                    Some(std::path::PathBuf::from(line))
                                };

                                let Some(path) = path else {
                                    continue;
                                };

                                // Guard: only delete files that are inside the managed cache dir.
                                // This prevents accidental deletion of user-owned original files.
                                if !path.starts_with(cache_dir) {
                                    info!(
                                        path = %path.display(),
                                        cache_dir = %cache_dir.display(),
                                        "Skipping file deletion — path is outside the managed file-cache directory (user-owned file)"
                                    );
                                    continue;
                                }

                                if let Err(e) = tokio::fs::remove_file(&path).await {
                                    warn!(
                                        path = %path.display(),
                                        error = %e,
                                        "Failed to delete cache file during entry cleanup"
                                    );
                                } else {
                                    info!(
                                        path = %path.display(),
                                        "Deleted cache file during entry cleanup"
                                    );
                                    // Try to remove the parent directory (e.g. UUID dir) if it's
                                    // now empty and still inside the cache boundary.
                                    if let Some(parent) = path.parent() {
                                        if parent != cache_dir.as_path()
                                            && parent.starts_with(cache_dir)
                                        {
                                            // remove_dir only succeeds when dir is empty
                                            let _ = tokio::fs::remove_dir(parent).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        .instrument(info_span!("cleanup_cache_files", event_id = %event_id))
        .await;

        // 2. Delete selection (references entry)
        self.selection_repo
            .delete_selection(entry_id)
            .instrument(info_span!(
                "delete_selection",
                entry_id = %entry_id
            ))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete selection: {}", e))?;

        // 3. Delete entry (references event - must delete before event)
        self.entry_repo
            .delete_entry(entry_id)
            .instrument(info_span!(
                "delete_entry",
                entry_id = %entry_id
            ))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete entry: {}", e))?;

        // 4. Delete event and representations (now safe since entry is gone)
        self.event_writer
            .delete_event_and_representations(&event_id)
            .instrument(info_span!(
                "delete_event",
                event_id = %event_id
            ))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete event: {}", e))?;

        info!(
            entry_id = %entry_id,
            event_id = %event_id,
            "Deleted clipboard entry successfully"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use uc_core::clipboard::{ClipboardEntry, PersistedClipboardRepresentation};
    use uc_core::ids::{EntryId, EventId};

    // Mock entry repository
    struct MockEntryRepo {
        entry: Option<ClipboardEntry>,
        should_fail_get: bool,
        should_fail_delete: bool,
        delete_called: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait]
    impl ClipboardEntryRepositoryPort for MockEntryRepo {
        /// Mock implementation of `get_entry` used by tests.
        ///
        /// Returns the configured `entry` wrapped in `Ok(Some(_))` when `should_fail_get` is `false`,
        /// returns `Ok(None)` if no entry is configured, and returns an `Err` when `should_fail_get` is `true`.
        ///
        /// # Examples
        ///
        /// ```
        /// // Construct a mock repository that will return a clipboard entry.
        /// let entry = ClipboardEntry { /* fields omitted */ };
        /// let repo = MockEntryRepo { entry: Some(entry.clone()), should_fail_get: false };
        /// let fetched = futures::executor::block_on(repo.get_entry(&entry.id)).unwrap();
        /// assert_eq!(fetched, Some(entry));
        /// ```
        async fn get_entry(&self, _entry_id: &EntryId) -> Result<Option<ClipboardEntry>> {
            if self.should_fail_get {
                return Err(anyhow::anyhow!("Mock get_entry error"));
            }
            Ok(self.entry.clone())
        }

        async fn delete_entry(&self, _entry_id: &EntryId) -> Result<()> {
            self.delete_called
                .store(true, std::sync::atomic::Ordering::SeqCst);
            if self.should_fail_delete {
                return Err(anyhow::anyhow!("Mock delete_entry error"));
            }
            Ok(())
        }

        /// Persists a clipboard entry together with its selection decision to the underlying stores.

        ///

        /// This saves both the provided `ClipboardEntry` and its associated `ClipboardSelectionDecision`.

        ///

        /// # Arguments

        ///

        /// * `entry` - The clipboard entry to persist.

        /// * `selection` - The selection decision associated with the entry.

        ///

        /// # Returns

        ///

        /// `Ok(())` on success, or an error if persistence fails.

        ///

        /// # Examples

        ///

        /// ```no_run

        /// # use std::sync::Arc;

        /// # async fn _example() -> Result<(), Box<dyn std::error::Error>> {

        /// // assuming `repo` is an implementation that provides this async method:

        /// // repo.save_entry_and_selection(&entry, &selection).await?;

        /// # Ok(())

        /// # }

        /// ```
        async fn save_entry_and_selection(
            &self,
            _entry: &ClipboardEntry,
            _selection: &uc_core::clipboard::ClipboardSelectionDecision,
        ) -> Result<()> {
            unimplemented!("Not used in tests")
        }

        /// Returns a page of clipboard entries using pagination parameters.
        ///
        /// # Parameters
        ///
        /// - `limit`: Maximum number of entries to return.
        /// - `offset`: Number of entries to skip before collecting results.
        ///
        /// # Returns
        ///
        /// A vector of `ClipboardEntry` containing up to `limit` entries starting at `offset`.
        ///
        /// # Examples
        ///
        /// ```
        /// # use futures::executor::block_on;
        /// # // `usecase` and `ClipboardEntry` would be available in real usage.
        /// # async fn _demo(usecase: &impl std::fmt::Debug) {}
        /// // let entries = block_on(usecase.list_entries(10, 0)).unwrap();
        /// // assert!(entries.len() <= 10);
        /// ```
        async fn list_entries(&self, _limit: usize, _offset: usize) -> Result<Vec<ClipboardEntry>> {
            unimplemented!("Not used in tests")
        }
    }

    // Mock selection repository
    struct MockSelectionRepo {
        should_fail_delete: bool,
        delete_called: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait]
    impl ClipboardSelectionRepositoryPort for MockSelectionRepo {
        /// Retrieves the clipboard selection decision for the specified entry.
        ///
        /// Returns `Some(ClipboardSelectionDecision)` if a selection exists for the given entry, `None` otherwise.
        ///
        /// # Examples
        ///
        /// ```
        /// # async fn example<S: ClipboardSelectionRepositoryPort>(repo: &S, entry_id: &EntryId) {
        /// let decision = repo.get_selection(entry_id).await.unwrap();
        /// if let Some(d) = decision {
        ///     // use d
        /// } else {
        ///     // no selection for this entry
        /// }
        /// # }
        /// ```
        async fn get_selection(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<uc_core::clipboard::ClipboardSelectionDecision>> {
            unimplemented!("Not used in tests")
        }

        /// Mock implementation of deleting a selection used in tests.
        ///
        /// Records that a deletion was attempted and, if configured, returns an error to simulate failure.
        ///
        /// # Examples
        ///
        /// ```
        /// // setup
        /// let mock = MockSelectionRepo { delete_called: std::sync::atomic::AtomicBool::new(false), should_fail_delete: false };
        /// // call (inside an async context)
        /// futures::executor::block_on(async {
        ///     mock.delete_selection(&EntryId::new()).await.unwrap();
        ///     assert!(mock.delete_called.load(std::sync::atomic::Ordering::SeqCst));
        /// });
        /// ```
        async fn delete_selection(&self, _entry_id: &EntryId) -> Result<()> {
            self.delete_called
                .store(true, std::sync::atomic::Ordering::SeqCst);
            if self.should_fail_delete {
                return Err(anyhow::anyhow!("Mock delete_selection error"));
            }
            Ok(())
        }
    }

    // Mock event writer
    struct MockEventWriter {
        should_fail_delete: bool,
        delete_called: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait]
    impl ClipboardEventWriterPort for MockEventWriter {
        /// Placeholder implementation of `insert_event` for the mock event writer that panics if invoked.
        ///
        /// This mock method is not used in tests and will panic with the message `"Not used in tests"` when called.
        ///
        /// # Examples
        ///
        /// ```should_panic
        /// // In tests the mock's `insert_event` is not used; calling it will panic.
        /// // let mock = MockEventWriter::new();
        /// // futures::executor::block_on(mock.insert_event(&event, &representations));
        /// panic!("Not used in tests");
        /// ```
        async fn insert_event(
            &self,
            _event: &uc_core::clipboard::ClipboardEvent,
            _representations: &Vec<uc_core::clipboard::PersistedClipboardRepresentation>,
        ) -> Result<()> {
            unimplemented!("Not used in tests")
        }

        /// Simulates deletion of an event and its representations for testing.
        ///
        /// This mock marks that a deletion was attempted and either succeeds or returns an error
        /// based on the mock's configuration.
        ///
        /// # Parameters
        ///
        /// - `event_id`: Identifier of the event to delete (may be unused by the mock).
        ///
        /// # Returns
        ///
        /// `Ok(())` on success, `Err` with a descriptive message if the mock is configured to fail.
        ///
        /// # Examples
        ///
        /// ```rust,ignore
        /// // Construct a mock configured to succeed and verify deletion is recorded.
        /// let mock = MockEventWriter { delete_called: std::sync::atomic::AtomicBool::new(false), should_fail_delete: false };
        /// let event_id = EventId::new(); // example placeholder
        /// tokio::runtime::Runtime::new().unwrap().block_on(async {
        ///     let res = mock.delete_event_and_representations(&event_id).await;
        ///     assert!(res.is_ok());
        ///     assert!(mock.delete_called.load(std::sync::atomic::Ordering::SeqCst));
        /// });
        /// ```
        async fn delete_event_and_representations(&self, _event_id: &EventId) -> Result<()> {
            self.delete_called
                .store(true, std::sync::atomic::Ordering::SeqCst);
            if self.should_fail_delete {
                return Err(anyhow::anyhow!(
                    "Mock delete_event_and_representations error"
                ));
            }
            Ok(())
        }
    }

    // Mock representation repository (returns empty by default)
    struct MockRepresentationRepo;

    #[async_trait]
    impl ClipboardRepresentationRepositoryPort for MockRepresentationRepo {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            _representation_id: &uc_core::ids::RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_id(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
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
            _representation_id: &uc_core::ids::RepresentationId,
            _blob_id: &uc_core::BlobId,
        ) -> Result<()> {
            Ok(())
        }

        async fn update_blob_id_if_none(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
            _blob_id: &uc_core::BlobId,
        ) -> Result<bool> {
            Ok(false)
        }

        async fn update_processing_result(
            &self,
            _rep_id: &uc_core::ids::RepresentationId,
            _expected_states: &[uc_core::clipboard::PayloadAvailability],
            _blob_id: Option<&uc_core::BlobId>,
            _new_state: uc_core::clipboard::PayloadAvailability,
            _last_error: Option<&str>,
        ) -> Result<uc_core::ports::clipboard::ProcessingUpdateOutcome> {
            Ok(uc_core::ports::clipboard::ProcessingUpdateOutcome::NotFound)
        }
    }

    #[tokio::test]
    async fn test_execute_deletes_all_related_data() {
        // Setup: Create mock repositories
        let delete_entry_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let delete_selection_called =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let delete_event_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let entry_id = EntryId::from("test-entry".to_string());
        let event_id = EventId::from("test-event".to_string());

        let entry = ClipboardEntry::new(
            entry_id.clone(),
            event_id.clone(),
            1234567890,
            Some("Test Entry".to_string()),
            1024,
        );

        let entry_repo = MockEntryRepo {
            entry: Some(entry),
            should_fail_get: false,
            should_fail_delete: false,
            delete_called: delete_entry_called.clone(),
        };

        let selection_repo = MockSelectionRepo {
            should_fail_delete: false,
            delete_called: delete_selection_called.clone(),
        };

        let event_writer = MockEventWriter {
            should_fail_delete: false,
            delete_called: delete_event_called.clone(),
        };

        // Create use case with mocks
        let use_case = DeleteClipboardEntry::from_ports(
            Arc::new(entry_repo),
            Arc::new(selection_repo),
            Arc::new(event_writer),
            Arc::new(MockRepresentationRepo),
        );

        // Execute deletion
        let result = use_case.execute(&entry_id).await;

        // Verify success
        assert!(result.is_ok(), "Deletion should succeed");

        // Verify all repositories were called in correct order
        assert!(delete_selection_called.load(std::sync::atomic::Ordering::SeqCst));
        assert!(delete_event_called.load(std::sync::atomic::Ordering::SeqCst));
        assert!(delete_entry_called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_execute_returns_not_found_for_nonexistent_entry() {
        // Setup: Mock entry repo returns None
        let delete_entry_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let delete_selection_called =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let delete_event_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let entry_id = EntryId::from("nonexistent".to_string());

        let entry_repo = MockEntryRepo {
            entry: None,
            should_fail_get: false,
            should_fail_delete: false,
            delete_called: delete_entry_called.clone(),
        };

        let selection_repo = MockSelectionRepo {
            should_fail_delete: false,
            delete_called: delete_selection_called.clone(),
        };

        let event_writer = MockEventWriter {
            should_fail_delete: false,
            delete_called: delete_event_called.clone(),
        };

        let use_case = DeleteClipboardEntry::from_ports(
            Arc::new(entry_repo),
            Arc::new(selection_repo),
            Arc::new(event_writer),
            Arc::new(MockRepresentationRepo),
        );

        // Execute deletion
        let result = use_case.execute(&entry_id).await;

        // Assert error contains "not found"
        assert!(result.is_err(), "Should return error for nonexistent entry");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found"),
            "Error should contain 'not found': {}",
            err
        );

        // Verify delete methods were NOT called (entry didn't exist)
        assert!(!delete_selection_called.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!delete_event_called.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!delete_entry_called.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// A representation repository that returns a single text/uri-list representation
    /// with configurable inline content. Used for file-ownership tests.
    struct MockRepresentationRepoWithUriList {
        uri_list_content: String,
    }

    #[async_trait]
    impl ClipboardRepresentationRepositoryPort for MockRepresentationRepoWithUriList {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            _representation_id: &uc_core::ids::RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_id(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
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
            _representation_id: &uc_core::ids::RepresentationId,
            _blob_id: &uc_core::BlobId,
        ) -> Result<()> {
            Ok(())
        }

        async fn update_blob_id_if_none(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
            _blob_id: &uc_core::BlobId,
        ) -> Result<bool> {
            Ok(false)
        }

        async fn update_processing_result(
            &self,
            _rep_id: &uc_core::ids::RepresentationId,
            _expected_states: &[uc_core::clipboard::PayloadAvailability],
            _blob_id: Option<&uc_core::BlobId>,
            _new_state: uc_core::clipboard::PayloadAvailability,
            _last_error: Option<&str>,
        ) -> Result<uc_core::ports::clipboard::ProcessingUpdateOutcome> {
            Ok(uc_core::ports::clipboard::ProcessingUpdateOutcome::NotFound)
        }

        async fn get_representations_for_event(
            &self,
            _event_id: &EventId,
        ) -> Result<Vec<PersistedClipboardRepresentation>> {
            use uc_core::clipboard::MimeType;
            use uc_core::ids::{FormatId, RepresentationId};
            let rep = PersistedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("files"),
                Some(MimeType::uri_list()),
                self.uri_list_content.len() as i64,
                Some(self.uri_list_content.as_bytes().to_vec()),
                None,
            );
            Ok(vec![rep])
        }
    }

    fn make_test_use_case_with_uri_list(
        uri_list: &str,
        file_cache_dir: Option<std::path::PathBuf>,
    ) -> DeleteClipboardEntry {
        use std::sync::atomic::AtomicBool;
        let entry_id = EntryId::from("test-entry".to_string());
        let event_id = EventId::from("test-event".to_string());
        let entry = ClipboardEntry::new(
            entry_id.clone(),
            event_id.clone(),
            1234567890,
            Some("Test Entry".to_string()),
            1024,
        );
        let entry_repo = MockEntryRepo {
            entry: Some(entry),
            should_fail_get: false,
            should_fail_delete: false,
            delete_called: Arc::new(AtomicBool::new(false)),
        };
        let selection_repo = MockSelectionRepo {
            should_fail_delete: false,
            delete_called: Arc::new(AtomicBool::new(false)),
        };
        let event_writer = MockEventWriter {
            should_fail_delete: false,
            delete_called: Arc::new(AtomicBool::new(false)),
        };
        let rep_repo = MockRepresentationRepoWithUriList {
            uri_list_content: uri_list.to_string(),
        };

        let uc = DeleteClipboardEntry::from_ports(
            Arc::new(entry_repo),
            Arc::new(selection_repo),
            Arc::new(event_writer),
            Arc::new(rep_repo),
        );
        if let Some(dir) = file_cache_dir {
            uc.with_file_cache_dir(dir)
        } else {
            uc
        }
    }

    /// Synced (cache) files whose path is inside file_cache_dir SHOULD be deleted.
    #[tokio::test]
    async fn test_cache_file_is_deleted_when_inside_cache_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_dir = tmp.path().join("file-cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        // Create a real file inside the cache dir to be deleted
        let cached_file = cache_dir.join("transfer-abc").join("photo.png");
        std::fs::create_dir_all(cached_file.parent().unwrap()).unwrap();
        std::fs::write(&cached_file, b"fake file data").unwrap();
        assert!(cached_file.exists());

        let uri_list = cached_file.to_string_lossy().to_string();
        let entry_id = EntryId::from("test-entry".to_string());

        let uc = make_test_use_case_with_uri_list(&uri_list, Some(cache_dir.clone()));
        uc.execute(&entry_id).await.unwrap();

        assert!(
            !cached_file.exists(),
            "Cached file inside cache_dir should have been deleted"
        );
        assert!(
            !cached_file.parent().unwrap().exists(),
            "Empty parent directory inside cache_dir should also be removed"
        );
    }

    /// Local (user-owned) files whose path is OUTSIDE file_cache_dir must NOT be deleted.
    #[tokio::test]
    async fn test_local_file_is_not_deleted_when_outside_cache_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_dir = tmp.path().join("file-cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        // Create a user-owned file OUTSIDE the cache dir
        let user_file = tmp.path().join("documents").join("report.pdf");
        std::fs::create_dir_all(user_file.parent().unwrap()).unwrap();
        std::fs::write(&user_file, b"important document").unwrap();
        assert!(user_file.exists());

        let uri_list = user_file.to_string_lossy().to_string();
        let entry_id = EntryId::from("test-entry".to_string());

        let uc = make_test_use_case_with_uri_list(&uri_list, Some(cache_dir.clone()));
        uc.execute(&entry_id).await.unwrap();

        assert!(
            user_file.exists(),
            "User-owned file outside cache_dir must NOT be deleted"
        );
    }

    /// When no file_cache_dir is configured, no files should be deleted (safe default).
    #[tokio::test]
    async fn test_no_files_deleted_when_cache_dir_not_configured() {
        let tmp = tempfile::TempDir::new().unwrap();

        let some_file = tmp.path().join("file.txt");
        std::fs::write(&some_file, b"data").unwrap();
        assert!(some_file.exists());

        let uri_list = some_file.to_string_lossy().to_string();
        let entry_id = EntryId::from("test-entry".to_string());

        // No file_cache_dir provided
        let uc = make_test_use_case_with_uri_list(&uri_list, None);
        uc.execute(&entry_id).await.unwrap();

        assert!(
            some_file.exists(),
            "File must not be deleted when no cache_dir is configured"
        );
    }

    /// Synced files referenced via file:// URI scheme inside cache_dir should also be deleted.
    #[tokio::test]
    async fn test_cache_file_uri_scheme_is_deleted() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_dir = tmp.path().join("file-cache");
        std::fs::create_dir_all(&cache_dir).unwrap();

        let cached_file = cache_dir.join("xfer-1").join("image.jpg");
        std::fs::create_dir_all(cached_file.parent().unwrap()).unwrap();
        std::fs::write(&cached_file, b"image bytes").unwrap();
        assert!(cached_file.exists());

        // Use file:// URI format (legacy format used by older synced entries)
        let uri = url::Url::from_file_path(&cached_file).unwrap().to_string();
        let entry_id = EntryId::from("test-entry".to_string());

        let uc = make_test_use_case_with_uri_list(&uri, Some(cache_dir.clone()));
        uc.execute(&entry_id).await.unwrap();

        assert!(
            !cached_file.exists(),
            "Cached file referenced via file:// URI inside cache_dir should have been deleted"
        );
        assert!(
            !cached_file.parent().unwrap().exists(),
            "Empty parent directory inside cache_dir should also be removed (file:// URI case)"
        );
    }

    #[tokio::test]
    async fn test_execute_propagates_repository_errors() {
        // Setup: Mock returns error
        let delete_entry_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let delete_selection_called =
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let delete_event_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let entry_id = EntryId::from("test-entry".to_string());
        let event_id = EventId::from("test-event".to_string());

        let entry = ClipboardEntry::new(
            entry_id.clone(),
            event_id.clone(),
            1234567890,
            Some("Test Entry".to_string()),
            1024,
        );

        let entry_repo = MockEntryRepo {
            entry: Some(entry),
            should_fail_get: false,
            should_fail_delete: false,
            delete_called: delete_entry_called.clone(),
        };

        let selection_repo = MockSelectionRepo {
            should_fail_delete: true, // Will fail on delete_selection
            delete_called: delete_selection_called.clone(),
        };

        let event_writer = MockEventWriter {
            should_fail_delete: false,
            delete_called: delete_event_called.clone(),
        };

        let use_case = DeleteClipboardEntry::from_ports(
            Arc::new(entry_repo),
            Arc::new(selection_repo),
            Arc::new(event_writer),
            Arc::new(MockRepresentationRepo),
        );

        // Execute deletion
        let result = use_case.execute(&entry_id).await;

        // Assert error is propagated
        assert!(result.is_err(), "Should return error when repo fails");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to delete selection"),
            "Error should indicate which operation failed: {}",
            err
        );
    }
}

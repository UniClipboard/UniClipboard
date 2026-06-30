use std::sync::Arc;

use uc_core::ids::EntryId;
use uc_core::ports::clipboard::SetClipboardEntryFavoritePort;
use uc_core::ports::search::search_index::SearchIndexPort;

/// Set the favorite state of a clipboard entry.
///
/// 设置剪贴板条目的收藏状态。
pub(crate) struct ToggleFavoriteClipboardEntryUseCase {
    entry_repo: Arc<dyn SetClipboardEntryFavoritePort>,
    /// Optional search index used to mirror the favorite state into the derived
    /// `favorited` tag. When absent, only the authoritative store is updated.
    search_mirror: Option<Arc<dyn SearchIndexPort>>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ToggleFavoriteError {
    #[error("Repository error: {0}")]
    RepositoryError(String),
}

impl ToggleFavoriteClipboardEntryUseCase {
    pub(crate) fn new(entry_repo: Arc<dyn SetClipboardEntryFavoritePort>) -> Self {
        Self {
            entry_repo,
            search_mirror: None,
        }
    }

    /// Attach the search index so favorite toggles also update the derived
    /// `favorited` tag membership. Without it the mirror step is skipped.
    pub(crate) fn with_search_mirror(mut self, search_mirror: Arc<dyn SearchIndexPort>) -> Self {
        self.search_mirror = Some(search_mirror);
        self
    }

    /// Persist `is_favorited` for the entry. Returns `Ok(true)` when the entry
    /// exists and the flag was stored, `Ok(false)` when no entry matches
    /// `entry_id`, and `Err` on repository failures.
    #[tracing::instrument(name = "usecase.toggle_favorite_clipboard_entry.execute", skip(self))]
    pub(crate) async fn execute(
        &self,
        entry_id: &EntryId,
        is_favorited: bool,
    ) -> Result<bool, ToggleFavoriteError> {
        let updated = self
            .entry_repo
            .set_favorite(entry_id, is_favorited)
            .await
            .map_err(|e| ToggleFavoriteError::RepositoryError(e.to_string()))?;

        if updated {
            // Mirror the user-state into the derived `favorited` tag so search
            // reflects it without waiting for a rebuild. The store is the source
            // of truth; a mirror failure must not lose the persisted flag, and a
            // later rebuild reconciles the tag from the stored state.
            if let Some(mirror) = &self.search_mirror {
                if let Err(e) = mirror.set_entry_favorite_tag(entry_id, is_favorited).await {
                    tracing::warn!(
                        entry_id = %entry_id,
                        is_favorited,
                        error = %e,
                        "favorite persisted but search tag mirror failed; rebuild will reconcile"
                    );
                }
            }
            tracing::info!(entry_id = %entry_id, is_favorited, "Favorite state persisted");
        } else {
            tracing::warn!(
                entry_id = %entry_id,
                is_favorited,
                "Favorite toggle ignored: no entry matches the id"
            );
        }
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use tokio::sync::mpsc::Sender;
    use uc_core::clipboard::ClipboardRepositoryError;
    use uc_core::search::{
        RebuildProgress, SearchDocument, SearchError, SearchIndexMeta, SearchPosting, SearchQuery,
        SearchResultsPage,
    };

    /// Records the last `set_entry_favorite_tag` call; other port methods are
    /// unused by this use case.
    struct RecordingMirror {
        last: Mutex<Option<(String, bool)>>,
    }

    #[async_trait]
    impl SearchIndexPort for RecordingMirror {
        async fn index_entry(
            &self,
            _document: SearchDocument,
            _postings: Vec<SearchPosting>,
        ) -> Result<(), SearchError> {
            Ok(())
        }
        async fn remove_entry(&self, _entry_id: &EntryId) -> Result<(), SearchError> {
            Ok(())
        }
        async fn search(&self, _query: SearchQuery) -> Result<SearchResultsPage, SearchError> {
            unreachable!("toggle favorite never queries the index")
        }
        async fn rebuild(
            &self,
            _entries: Vec<(SearchDocument, Vec<SearchPosting>)>,
            _progress_tx: Sender<RebuildProgress>,
        ) -> Result<(), SearchError> {
            Ok(())
        }
        async fn get_index_meta(&self) -> Result<SearchIndexMeta, SearchError> {
            unreachable!("toggle favorite never reads index meta")
        }
        async fn set_entry_favorite_tag(
            &self,
            entry_id: &EntryId,
            favorited: bool,
        ) -> Result<(), SearchError> {
            *self.last.lock().unwrap() = Some((entry_id.to_string(), favorited));
            Ok(())
        }
    }

    /// Records the last `set_favorite` call and replays a fixed outcome.
    struct FakeFavoritePort {
        /// `Ok(found)` flips the existence reply; `Err(_)` simulates storage failure.
        result: Result<bool, ()>,
        last_call: Mutex<Option<(String, bool)>>,
    }

    #[async_trait]
    impl SetClipboardEntryFavoritePort for FakeFavoritePort {
        async fn set_favorite(
            &self,
            entry_id: &EntryId,
            is_favorited: bool,
        ) -> Result<bool, ClipboardRepositoryError> {
            *self.last_call.lock().unwrap() = Some((entry_id.to_string(), is_favorited));
            self.result
                .map_err(|()| ClipboardRepositoryError::Storage("boom".into()))
        }
    }

    #[tokio::test]
    async fn execute_persists_and_reports_found_entry() {
        let port = Arc::new(FakeFavoritePort {
            result: Ok(true),
            last_call: Mutex::new(None),
        });
        let uc = ToggleFavoriteClipboardEntryUseCase::new(port.clone());

        let found = uc
            .execute(&EntryId::from("entry-1"), true)
            .await
            .expect("ok");

        assert!(found, "an existing entry reports a successful toggle");
        assert_eq!(
            *port.last_call.lock().unwrap(),
            Some(("entry-1".to_string(), true)),
            "the favorite value is forwarded to the persistence port verbatim"
        );
    }

    #[tokio::test]
    async fn execute_reports_not_found_when_no_row_updated() {
        let port = Arc::new(FakeFavoritePort {
            result: Ok(false),
            last_call: Mutex::new(None),
        });
        let uc = ToggleFavoriteClipboardEntryUseCase::new(port);

        let found = uc
            .execute(&EntryId::from("missing"), true)
            .await
            .expect("ok");

        assert!(
            !found,
            "a missing entry reports not-found rather than erroring"
        );
    }

    #[tokio::test]
    async fn execute_translates_repository_failure() {
        let port = Arc::new(FakeFavoritePort {
            result: Err(()),
            last_call: Mutex::new(None),
        });
        let uc = ToggleFavoriteClipboardEntryUseCase::new(port);

        let err = uc
            .execute(&EntryId::from("entry-1"), false)
            .await
            .expect_err("storage failure must surface as an error");

        assert!(matches!(err, ToggleFavoriteError::RepositoryError(_)));
    }

    #[tokio::test]
    async fn execute_mirrors_favorite_into_search_when_present() {
        let port = Arc::new(FakeFavoritePort {
            result: Ok(true),
            last_call: Mutex::new(None),
        });
        let mirror = Arc::new(RecordingMirror {
            last: Mutex::new(None),
        });
        let uc = ToggleFavoriteClipboardEntryUseCase::new(port).with_search_mirror(mirror.clone());

        uc.execute(&EntryId::from("entry-1"), true)
            .await
            .expect("ok");

        assert_eq!(
            *mirror.last.lock().unwrap(),
            Some(("entry-1".to_string(), true)),
            "the favorite state is mirrored into the search tag membership"
        );
    }

    #[tokio::test]
    async fn execute_skips_mirror_when_entry_missing() {
        let port = Arc::new(FakeFavoritePort {
            result: Ok(false), // no row updated → entry does not exist
            last_call: Mutex::new(None),
        });
        let mirror = Arc::new(RecordingMirror {
            last: Mutex::new(None),
        });
        let uc = ToggleFavoriteClipboardEntryUseCase::new(port).with_search_mirror(mirror.clone());

        let found = uc
            .execute(&EntryId::from("missing"), true)
            .await
            .expect("ok");

        assert!(!found);
        assert_eq!(
            *mirror.last.lock().unwrap(),
            None,
            "no mirror write when the entry does not exist"
        );
    }
}

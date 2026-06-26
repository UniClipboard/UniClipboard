use std::sync::Arc;

use uc_core::ids::EntryId;
use uc_core::ports::clipboard::SetClipboardEntryFavoritePort;

/// Set the favorite state of a clipboard entry.
///
/// 设置剪贴板条目的收藏状态。
pub(crate) struct ToggleFavoriteClipboardEntryUseCase {
    entry_repo: Arc<dyn SetClipboardEntryFavoritePort>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ToggleFavoriteError {
    #[error("Repository error: {0}")]
    RepositoryError(String),
}

impl ToggleFavoriteClipboardEntryUseCase {
    pub(crate) fn new(entry_repo: Arc<dyn SetClipboardEntryFavoritePort>) -> Self {
        Self { entry_repo }
    }

    /// Persist `is_favorited` for the entry. Returns `Ok(true)` when the entry
    /// exists and the flag was stored, `Ok(false)` when no entry matches
    /// `entry_id`, and `Err` on repository failures.
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
    use uc_core::clipboard::ClipboardRepositoryError;

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
}

use std::sync::Arc;

use uc_core::ids::EntryId;
use uc_core::ports::ClipboardEntryRepositoryPort;

/// Toggle favorite state for a clipboard entry.
///
/// 切换剪贴板条目的收藏状态。
pub(crate) struct ToggleFavoriteClipboardEntryUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ToggleFavoriteError {
    #[error("Repository error: {0}")]
    RepositoryError(String),
}

impl ToggleFavoriteClipboardEntryUseCase {
    pub(crate) fn new(entry_repo: Arc<dyn ClipboardEntryRepositoryPort>) -> Self {
        Self { entry_repo }
    }

    /// Returns Ok(true) when the entry exists and the favorite flag was acknowledged,
    /// Ok(false) when the entry does not exist, and Err on repository failures.
    ///
    /// NOTE: The domain model does not yet persist a favorite flag on
    /// ClipboardEntry. This implementation validates entry existence so
    /// callers get correct found/not-found semantics. Actual persistence
    /// will land when the schema is extended with a `is_favorited` column.
    pub(crate) async fn execute(
        &self,
        entry_id: &EntryId,
        is_favorited: bool,
    ) -> Result<bool, ToggleFavoriteError> {
        let entry = self
            .entry_repo
            .get_entry(entry_id)
            .await
            .map_err(|e| ToggleFavoriteError::RepositoryError(e.to_string()))?;

        match entry {
            Some(_) => {
                tracing::info!(
                    entry_id = %entry_id,
                    is_favorited,
                    "Favorite toggle acknowledged for existing entry"
                );
                Ok(true)
            }
            None => Ok(false),
        }
    }
}

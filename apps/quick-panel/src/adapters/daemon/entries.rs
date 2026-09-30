use std::time::Duration;

use quick_panel_core::ports::{EntryAction, Options, ServiceError};
use quick_panel_core::query::filters::BUILTIN_TAGS;

use super::DaemonHistory;

const RESTORE_TIMEOUT: Duration = Duration::from_secs(8);

impl DaemonHistory {
    pub(super) async fn restore_entry(&self, id: String, plain: bool) -> Result<(), ServiceError> {
        let operation = async {
            self.context()?
                .clipboard_client()
                .restore_clipboard_entry_with_options(&id, plain)
                .await
                .map_err(|_| ServiceError::RestoreFailed)
        };
        tokio::time::timeout(RESTORE_TIMEOUT, operation)
            .await
            .map_err(|_| ServiceError::RestoreTimeout)?
    }

    pub(super) async fn load_options(&self) -> Result<Options, ServiceError> {
        let context = self.context()?;
        let mut tags: Vec<String> = BUILTIN_TAGS.iter().map(|s| (*s).into()).collect();
        let fetched = context
            .search_client()
            .tags()
            .await
            .map_err(|_| ServiceError::TagsUnavailable)?;
        for tag in fetched {
            if !tags.contains(&tag.tag_id) {
                tags.push(tag.tag_id);
            }
        }
        let members = context
            .query_client()
            .get_paired_devices()
            .await
            .map_err(|_| ServiceError::DevicesUnavailable)?;
        let settings = context.settings_client().get_settings().await.ok();
        Ok(Options {
            tags,
            members,
            settings,
        })
    }

    pub(super) async fn entry_action(
        &self,
        id: String,
        action: EntryAction,
    ) -> Result<(), ServiceError> {
        let context = self.context()?;
        let result = match action {
            EntryAction::Favorite(value) => {
                context.clipboard_client().set_favorite(&id, value).await
            }
            EntryAction::Delete => context.clipboard_client().delete_entry(&id).await,
            EntryAction::Send(peer) => context
                .clipboard_client()
                .resend_entry(&id, peer.map(|p| vec![p]))
                .await
                .map(|_| ()),
        };
        result.map_err(|_| ServiceError::ActionFailed)
    }
}

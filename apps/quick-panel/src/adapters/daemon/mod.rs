//! The clipboard history served by the local daemon.
//!
//! One [`DaemonHistory`] shares a single connection state, so the session token obtained from
//! `/auth/connect` is cached across searches, previews and actions instead of being exchanged for
//! each of them.

mod connection;
mod entries;
mod live;
mod preview;
mod search;
#[cfg(test)]
mod tests;

use async_trait::async_trait;
use quick_panel_core::ports::{
    EntryAction, HistoryService, Live, Options, PreviewData, SearchFailure, ServiceError,
};
use quick_panel_core::query::filters::Filters;
use uc_daemon_contract::api::dto::search::SearchQueryResultDto;

pub use connection::DaemonHistory;

#[async_trait]
impl HistoryService for DaemonHistory {
    async fn search(&self, filters: Filters) -> Result<SearchQueryResultDto, SearchFailure> {
        self.search_entries(filters).await
    }

    async fn count(&self, filters: Filters) -> Option<u32> {
        self.count_entries(filters).await
    }

    async fn restore(&self, id: String, plain: bool) -> Result<(), ServiceError> {
        self.restore_entry(id, plain).await
    }

    async fn options(&self) -> Result<Options, ServiceError> {
        self.load_options().await
    }

    async fn action(&self, id: String, action: EntryAction) -> Result<(), ServiceError> {
        self.entry_action(id, action).await
    }

    async fn preview(&self, id: String, kind: String) -> Result<PreviewData, ServiceError> {
        self.load_preview(id, kind).await
    }

    async fn watch(&self, send: tokio::sync::mpsc::Sender<Live>) {
        self.watch_changes(send).await;
    }
}

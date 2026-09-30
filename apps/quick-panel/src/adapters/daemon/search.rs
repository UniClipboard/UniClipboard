use std::time::Duration;

use quick_panel_core::ports::SearchFailure;
use quick_panel_core::query::filters::{Filters, SearchQuery};
use uc_daemon_contract::api::dto::search::SearchQueryResultDto;

use super::DaemonHistory;

const SEARCH_TIMEOUT: Duration = Duration::from_secs(8);

impl DaemonHistory {
    pub(super) async fn search_entries(
        &self,
        filters: Filters,
    ) -> Result<SearchQueryResultDto, SearchFailure> {
        let operation = async {
            let client = self
                .context()
                .map_err(|_| SearchFailure::Disconnected)?
                .search_client();
            match client.query(to_request(filters.request())).await {
                Ok(result) => Ok(result),
                // Text the index cannot search for (a single Latin letter, say) is an answer, not
                // a failure: nothing matches.
                Err(error) if nothing_searchable(&error) => Ok(SearchQueryResultDto {
                    items: vec![],
                    total: 0,
                    has_more: false,
                    state: "ready".into(),
                }),
                Err(error) => Err(search_failure(error)),
            }
        };
        tokio::time::timeout(SEARCH_TIMEOUT, operation)
            .await
            .map_err(|_| SearchFailure::Timeout)?
    }

    pub(super) async fn count_entries(&self, filters: Filters) -> Option<u32> {
        let mut request = to_request(filters.request());
        request.limit = 1;
        let operation = async {
            let client = self.context().ok()?.search_client();
            client.query(request).await.ok().map(|result| result.total)
        };
        tokio::time::timeout(SEARCH_TIMEOUT, operation)
            .await
            .ok()
            .flatten()
    }
}

fn to_request(query: SearchQuery) -> uc_daemon_client::SearchQueryRequest {
    uc_daemon_client::SearchQueryRequest {
        query: query.query,
        operator: query.operator,
        time_preset: query.time_preset,
        from_ms: query.from_ms,
        to_ms: query.to_ms,
        content_types: query.content_types,
        tags: query.tags,
        extensions: query.extensions,
        source_devices: query.source_devices,
        limit: query.limit,
        offset: query.offset,
    }
}

/// The daemon refused the query because it holds no term the search index can match.
fn nothing_searchable(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<uc_daemon_client::DaemonRequestError>(),
        Some(uc_daemon_client::DaemonRequestError::Status { status, code, .. })
            if is_unsearchable(status.as_u16(), code.as_deref())
    )
}

pub(super) fn is_unsearchable(status: u16, code: Option<&str>) -> bool {
    status == 400 && code == Some("invalid_query")
}

fn search_failure(error: anyhow::Error) -> SearchFailure {
    use uc_daemon_client::DaemonRequestError as Error;
    if matches!(
        error.downcast_ref::<Error>(),
        Some(Error::NotConnected | Error::Transport { .. } | Error::Auth { .. })
    ) {
        return SearchFailure::Disconnected;
    }
    if error
        .downcast_ref::<Error>()
        .and_then(|e| e.status())
        .is_some_and(|s| s.as_u16() == 423)
    {
        SearchFailure::Locked
    } else {
        SearchFailure::Failed
    }
}

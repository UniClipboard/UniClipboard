use std::sync::{Arc, OnceLock};

use thiserror::Error;

mod coordinator;
mod projection;

use uc_core::ids::DeviceId;
use uc_core::ports::SearchIndexPort;
use uc_core::search::tag::TagId;
use uc_core::search::{ContentType, QueryOperator, SearchError, SearchQuery, TimeRangeFilter};

use crate::usecases::search::SearchClipboardEntriesUseCase;

pub use coordinator::{
    ManualRebuildResult, SearchCoordinator, SearchCoordinatorDeps, SearchCoordinatorEvent,
    SearchRebuildProgressView, SearchStatusSnapshot,
};
pub use projection::SearchProjectionBuilder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQueryInput {
    pub query: String,
    pub operator: Option<String>,
    pub time_preset: Option<String>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub content_types: Option<String>,
    pub extensions: Option<String>,
    /// Comma-separated source device ids; restricts results to those origins.
    pub source_devices: Option<String>,
    /// Comma-separated tag ids (e.g. `link,favorited`); restricts to entries
    /// carrying any of them.
    pub tags: Option<String>,
    pub limit: u32,
    pub offset: u32,
}

/// Response freshness for a `query()` page: the index served the page.
pub const SEARCH_STATE_READY: &str = "ready";
/// The index was not ready and this filter-less browse was served from the main
/// store instead (§4.7).
pub const SEARCH_STATE_DEGRADED: &str = "degraded";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPageView {
    pub total: u32,
    pub has_more: bool,
    pub items: Vec<SearchResultView>,
    /// [`SEARCH_STATE_READY`] when served from the index, or
    /// [`SEARCH_STATE_DEGRADED`] when the index was not ready and this filter-less
    /// browse was served from the main store (§4.7).
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResultView {
    pub entry_id: String,
    pub content_type: String,
    pub active_time_ms: i64,
    /// Tag ids as transparent strings (e.g. `"link"`, `"favorited"`).
    pub tags: Vec<String>,
    pub text_preview: Option<String>,
    /// Full character count of the entry's primary text content, so the UI can
    /// show the real total length instead of the capped preview length. `None`
    /// for entries with no inline text.
    pub char_count: Option<i64>,
    pub mime_type: String,
    pub file_extensions: Vec<String>,
    pub file_names: Vec<String>,
    pub link_urls: Vec<String>,
    pub source_device: Option<String>,
    pub payload_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchStatusView {
    pub state: String,
    pub reason: Option<String>,
    pub last_rebuild_started_at_ms: Option<i64>,
    pub last_rebuild_completed_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRebuildAcceptedView {
    pub accepted: bool,
}

/// A tag and its entry count, plus whether it is a builtin (always visible) or a
/// custom tag (hidden while the session is locked — gating is applied by the
/// caller).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTagView {
    pub tag_id: String,
    pub count: u32,
    pub is_builtin: bool,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum SearchFacadeError {
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("bad search request: {0}")]
    BadRequest(String),
    #[error("search session is locked")]
    SessionLocked,
    #[error("search index is not ready")]
    IndexNotReady,
    /// The index is not ready and the request carried a keyword or filter, so it
    /// cannot be served from the main-store browse fallback (§4.7). A filter-less
    /// browse degrades to a 200 instead; this is the non-browse counterpart.
    #[error("search index is rebuilding")]
    IndexRebuilding,
    #[error("search index is unavailable")]
    IndexUnavailable,
    #[error("search service is unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("search rebuild is already running")]
    RebuildAlreadyRunning,
    #[error("search failed: {0}")]
    Internal(String),
}

/// Dependency bundle for `SearchFacade`. Composition roots build this once
/// and pass it to `SearchFacade::new`. The query side runs through the
/// internal `SearchClipboardEntriesUseCase`; status/rebuild delegate to the
/// optional `SearchCoordinator` which manages background reindexing.
pub struct SearchFacadeDeps {
    pub search_index: Arc<dyn SearchIndexPort>,
    pub coordinator: Option<Arc<SearchCoordinator>>,
}

pub struct SearchFacade {
    query_uc: SearchClipboardEntriesUseCase,
    /// daemon-lifecycle 资源: GUI shell 启动期为空, daemon 启动时由
    /// `set_coordinator` 一次性装入 (方案 C 后 daemon 进程内只起一次)。
    /// 进程退出 = Arc drop, 无需显式 clear。GUI command (search_status
    /// / search_rebuild) 通过 facade 访问。
    coordinator: OnceLock<Arc<SearchCoordinator>>,
}

impl SearchFacade {
    pub fn new(deps: SearchFacadeDeps) -> Self {
        let SearchFacadeDeps {
            search_index,
            coordinator,
        } = deps;
        let coordinator_cell = OnceLock::new();
        if let Some(coordinator) = coordinator {
            let _ = coordinator_cell.set(coordinator);
        }
        Self {
            query_uc: SearchClipboardEntriesUseCase::from_port(search_index),
            coordinator: coordinator_cell,
        }
    }

    /// 由 daemon-lifecycle 装配在 daemon 启动时调,装入绑 daemon search
    /// assembly 的 coordinator。方案 C 后 daemon 进程内只装一次, 重复装入
    /// 视为编程错误。
    pub fn set_coordinator(&self, coordinator: Arc<SearchCoordinator>) {
        self.coordinator
            .set(coordinator)
            .map_err(|_| ())
            .expect("search coordinator already installed; daemon is process-singleton");
    }

    pub async fn query(
        &self,
        input: SearchQueryInput,
    ) -> Result<SearchPageView, SearchFacadeError> {
        let query = parse_search_query(input)?;
        // Captured before `query` is moved into the index search: decides whether
        // an unavailable index can degrade to a main-store browse (§4.7).
        let pure_browse = is_pure_browse(&query);
        let limit = query.limit as usize;
        let offset = query.offset as usize;

        match self.query_uc.execute(query).await {
            Ok(page) => Ok(search_page_to_view(page, SEARCH_STATE_READY)),
            // §4.7: a filter-less browse degrades to a direct main-store read so
            // the user keeps browsing during a rebuild; a keyword or filtered
            // query instead surfaces a stable rebuilding error.
            Err(SearchError::IndexNotReady) if pure_browse => {
                let coordinator = self
                    .coordinator
                    .get()
                    .ok_or(SearchFacadeError::IndexRebuilding)?;
                let page = coordinator
                    .browse_projection(limit, offset)
                    .await
                    .map_err(map_search_error)?;
                Ok(search_page_to_view(page, SEARCH_STATE_DEGRADED))
            }
            Err(SearchError::IndexNotReady) => Err(SearchFacadeError::IndexRebuilding),
            Err(other) => Err(map_search_error(other)),
        }
    }

    /// List the tags present in the index with their entry counts. Returns both
    /// builtin and custom tags; the caller applies lock-based visibility (custom
    /// tags are hidden while the session is locked, §4.6).
    pub async fn tags(&self) -> Result<Vec<SearchTagView>, SearchFacadeError> {
        let counts = self.query_uc.list_tags().await.map_err(map_search_error)?;
        Ok(counts
            .into_iter()
            .map(|c| SearchTagView {
                is_builtin: c.tag_id.is_builtin(),
                tag_id: c.tag_id.to_string(),
                count: c.count,
            })
            .collect())
    }

    pub async fn status(&self) -> Result<SearchStatusView, SearchFacadeError> {
        if let Some(coordinator) = self.coordinator.get() {
            return coordinator.status_view().await.map_err(map_search_error);
        }

        let meta = self.query_uc.index_meta().await.map_err(map_search_error)?;
        let state = if meta.search_blocked {
            "unavailable"
        } else {
            "ready"
        };
        Ok(SearchStatusView {
            state: state.to_string(),
            reason: meta.search_blocked.then(|| "search_blocked".to_string()),
            last_rebuild_started_at_ms: meta.last_rebuild_started_at_ms,
            last_rebuild_completed_at_ms: meta.last_rebuild_completed_at_ms,
        })
    }

    pub async fn request_rebuild(&self) -> Result<SearchRebuildAcceptedView, SearchFacadeError> {
        let coordinator = self.coordinator.get().ok_or_else(|| {
            SearchFacadeError::ServiceUnavailable("search coordinator unavailable".to_string())
        })?;

        match coordinator.request_manual_rebuild().await {
            ManualRebuildResult::Accepted => Ok(SearchRebuildAcceptedView { accepted: true }),
            ManualRebuildResult::AlreadyInProgress => Err(SearchFacadeError::RebuildAlreadyRunning),
        }
    }

    pub async fn rebuild_now(&self) -> Result<SearchRebuildAcceptedView, SearchFacadeError> {
        let coordinator = self.coordinator.get().ok_or_else(|| {
            SearchFacadeError::ServiceUnavailable("search coordinator unavailable".to_string())
        })?;

        match coordinator.run_manual_rebuild_now().await {
            ManualRebuildResult::Accepted => Ok(SearchRebuildAcceptedView { accepted: true }),
            ManualRebuildResult::AlreadyInProgress => Err(SearchFacadeError::RebuildAlreadyRunning),
        }
    }
}

/// True when the query carries no keyword and no filters — a plain browse. Only
/// such queries qualify for the §4.7 degraded main-store fallback; anything with
/// a keyword or filter needs the index and surfaces `IndexRebuilding` instead.
fn is_pure_browse(query: &SearchQuery) -> bool {
    query.query_string.trim().is_empty()
        && query.content_types.is_empty()
        && query.tags.is_empty()
        && query.source_devices.is_empty()
        && query.extensions.is_empty()
        && query.time_range.is_none()
}

fn search_page_to_view(page: uc_core::search::SearchResultsPage, state: &str) -> SearchPageView {
    SearchPageView {
        state: state.to_string(),
        total: page.total,
        has_more: page.has_more,
        items: page
            .items
            .into_iter()
            .map(|item| SearchResultView {
                entry_id: item.entry_id.to_string(),
                content_type: search_content_type_to_string(&item.content_type),
                active_time_ms: item.active_time_ms,
                tags: item.tags.iter().map(|t| t.to_string()).collect(),
                text_preview: item.text_preview,
                char_count: item.char_count,
                mime_type: item.mime_type,
                file_extensions: item.file_extensions,
                file_names: item.file_names,
                link_urls: item.link_urls,
                source_device: item.source_device,
                payload_state: item.payload_state,
            })
            .collect(),
    }
}

fn search_content_type_to_string(content_type: &ContentType) -> String {
    match content_type {
        ContentType::Text => "text",
        ContentType::Html => "html",
        ContentType::File => "file",
        ContentType::Image => "image",
        ContentType::Other => "other",
    }
    .to_string()
}

fn parse_search_query(input: SearchQueryInput) -> Result<SearchQuery, SearchFacadeError> {
    let (query_string, inferred_operator) = strip_and_infer_operator(&input.query)?;

    let operator = if let Some(operator) = input.operator.as_deref() {
        match operator.to_lowercase().as_str() {
            "and" => QueryOperator::And,
            "or" => QueryOperator::Or,
            _ => {
                return Err(SearchFacadeError::BadRequest(format!(
                    "invalid operator: {operator}"
                )))
            }
        }
    } else {
        inferred_operator.unwrap_or(QueryOperator::And)
    };

    Ok(SearchQuery {
        query_string,
        operator,
        time_range: parse_time_range(&input)?,
        content_types: parse_content_types(input.content_types.as_deref())?,
        tags: parse_tags(input.tags.as_deref()),
        extensions: parse_extensions(input.extensions.as_deref()),
        source_devices: parse_source_devices(input.source_devices.as_deref()),
        limit: input.limit.min(200),
        offset: input.offset,
    })
}

fn strip_and_infer_operator(
    raw: &str,
) -> Result<(String, Option<QueryOperator>), SearchFacadeError> {
    let tokens: Vec<&str> = raw.split_whitespace().collect();

    let mut has_and = false;
    let mut has_or = false;
    let mut non_operator_tokens: Vec<&str> = Vec::new();

    for token in &tokens {
        match token.to_uppercase().as_str() {
            "AND" => has_and = true,
            "OR" => has_or = true,
            _ => non_operator_tokens.push(token),
        }
    }

    if has_and && has_or {
        return Err(SearchFacadeError::InvalidQuery(
            "mixed AND/OR operators are not supported".to_string(),
        ));
    }

    let inferred = if has_and {
        Some(QueryOperator::And)
    } else if has_or {
        Some(QueryOperator::Or)
    } else {
        None
    };

    Ok((non_operator_tokens.join(" "), inferred))
}

fn parse_time_range(
    input: &SearchQueryInput,
) -> Result<Option<TimeRangeFilter>, SearchFacadeError> {
    let has_from = input.from_ms.is_some();
    let has_to = input.to_ms.is_some();

    if has_from != has_to {
        return Err(SearchFacadeError::BadRequest(
            "fromMs and toMs must both be present or both absent".to_string(),
        ));
    }

    if let (Some(from_ms), Some(to_ms)) = (input.from_ms, input.to_ms) {
        if from_ms < 0 || to_ms < 0 {
            return Err(SearchFacadeError::BadRequest(
                "fromMs and toMs must be non-negative".to_string(),
            ));
        }
        return Ok(Some(TimeRangeFilter::Absolute {
            from_ms: from_ms as u64,
            to_ms: to_ms as u64,
        }));
    }

    let Some(preset) = input.time_preset.as_deref() else {
        return Ok(None);
    };

    let filter = match preset {
        "today" => TimeRangeFilter::Today,
        "yesterday" => TimeRangeFilter::Yesterday,
        "last_24h" => TimeRangeFilter::Last24h,
        "last_7d" => TimeRangeFilter::Last7d,
        "last_30d" => TimeRangeFilter::Last30d,
        "this_week" => TimeRangeFilter::ThisWeek,
        "this_month" => TimeRangeFilter::ThisMonth,
        other => {
            return Err(SearchFacadeError::BadRequest(format!(
                "invalid timePreset: {other}"
            )))
        }
    };
    Ok(Some(filter))
}

fn parse_content_types(raw: Option<&str>) -> Result<Vec<ContentType>, SearchFacadeError> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };

    let mut result = Vec::new();
    for value in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let content_type = match value {
            "text" => ContentType::Text,
            "html" => ContentType::Html,
            "file" => ContentType::File,
            "image" => ContentType::Image,
            "other" => ContentType::Other,
            // `link` is no longer a content_type; it is a derived tag filtered
            // via the `tags` query parameter.
            unknown => {
                return Err(SearchFacadeError::BadRequest(format!(
                    "invalid fileType: {unknown}"
                )))
            }
        };
        result.push(content_type);
    }
    Ok(result)
}

/// Parse a comma-separated tag id list (e.g. `link,favorited`). Unknown/custom
/// ids are passed through as opaque [`TagId`]s; the route-layer lock guard and
/// the (future) custom-tag registry decide acceptance. None/empty yields no tag
/// restriction.
fn parse_tags(raw: Option<&str>) -> Vec<TagId> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(TagId::new)
        .collect()
}

fn parse_extensions(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    raw.split(',')
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn parse_source_devices(raw: Option<&str>) -> Vec<DeviceId> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(DeviceId::new)
        .collect()
}

pub fn map_search_error(error: SearchError) -> SearchFacadeError {
    match error {
        SearchError::InvalidQuery(message) => SearchFacadeError::InvalidQuery(message),
        SearchError::SessionLocked => SearchFacadeError::SessionLocked,
        SearchError::IndexNotReady => SearchFacadeError::IndexNotReady,
        SearchError::IndexUnavailable => SearchFacadeError::IndexUnavailable,
        SearchError::Internal(message) => SearchFacadeError::Internal(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browse_query() -> SearchQuery {
        SearchQuery {
            query_string: String::new(),
            operator: QueryOperator::And,
            time_range: None,
            content_types: Vec::new(),
            tags: Vec::new(),
            extensions: Vec::new(),
            source_devices: Vec::new(),
            limit: 50,
            offset: 0,
        }
    }

    #[test]
    fn is_pure_browse_true_for_empty_query_and_filters() {
        assert!(is_pure_browse(&browse_query()));
        // Whitespace-only keyword is still a browse.
        let mut q = browse_query();
        q.query_string = "   ".to_string();
        assert!(is_pure_browse(&q));
    }

    #[test]
    fn is_pure_browse_false_when_any_keyword_or_filter_present() {
        let mut keyword = browse_query();
        keyword.query_string = "hello".to_string();
        assert!(!is_pure_browse(&keyword));

        let mut typed = browse_query();
        typed.content_types = vec![ContentType::Image];
        assert!(!is_pure_browse(&typed));

        let mut tagged = browse_query();
        tagged.tags = vec![TagId::link()];
        assert!(!is_pure_browse(&tagged));

        let mut sourced = browse_query();
        sourced.source_devices = vec![DeviceId::new("dev-1")];
        assert!(!is_pure_browse(&sourced));

        let mut extended = browse_query();
        extended.extensions = vec!["md".to_string()];
        assert!(!is_pure_browse(&extended));

        let mut timed = browse_query();
        timed.time_range = Some(TimeRangeFilter::Today);
        assert!(!is_pure_browse(&timed));
    }
}

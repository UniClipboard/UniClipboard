use std::sync::{Arc, OnceLock};

use thiserror::Error;

mod coordinator;
mod projection;

use uc_core::ports::SearchIndexPort;
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
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchPageView {
    pub total: u32,
    pub has_more: bool,
    pub items: Vec<SearchResultView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResultView {
    pub entry_id: String,
    pub content_type: String,
    pub active_time_ms: i64,
    pub text_preview: Option<String>,
    pub mime_type: String,
    pub file_extensions: Vec<String>,
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
        let page = self
            .query_uc
            .execute(query)
            .await
            .map_err(map_search_error)?;
        Ok(search_page_to_view(page))
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

fn search_page_to_view(page: uc_core::search::SearchResultsPage) -> SearchPageView {
    SearchPageView {
        total: page.total,
        has_more: page.has_more,
        items: page
            .items
            .into_iter()
            .map(|item| SearchResultView {
                entry_id: item.entry_id.to_string(),
                content_type: search_content_type_to_string(&item.content_type),
                active_time_ms: item.active_time_ms,
                text_preview: item.text_preview,
                mime_type: item.mime_type,
                file_extensions: item.file_extensions,
            })
            .collect(),
    }
}

fn search_content_type_to_string(content_type: &ContentType) -> String {
    match content_type {
        ContentType::Text => "text",
        ContentType::Html => "html",
        ContentType::Link => "link",
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
        extensions: parse_extensions(input.extensions.as_deref()),
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
            "link" => ContentType::Link,
            "file" => ContentType::File,
            "image" => ContentType::Image,
            "other" => ContentType::Other,
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

fn parse_extensions(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    raw.split(',')
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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

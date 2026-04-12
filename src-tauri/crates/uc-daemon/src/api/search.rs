//! HTTP route handlers for search endpoints (Phase 92).
//!
//! All routes are protected by the auth_extractor + rate_limit middleware chain
//! applied at the router level (see routes::router_l2_plus).
//!
//! Lock guard: every handler checks `runtime.is_encryption_ready()` and returns
//! HTTP 423 with `session_locked` if the encryption session is not ready.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tracing::{debug, info, instrument};
use uc_app::usecases::CoreUseCases;
use uc_core::network::daemon_api_strings::http_route;
use uc_core::search::{ContentType, QueryOperator, SearchQuery, TimeRangeFilter};

use crate::api::dto::error::ApiError;
use crate::api::dto::search::{
    SearchQueryResponse, SearchRebuildAcceptedData, SearchRebuildAcceptedResponse, SearchResultDto,
    SearchStatusData, SearchStatusResponse,
};
use crate::api::server::DaemonApiState;
use crate::search::coordinator::ManualRebuildResult;

// ---------------------------------------------------------------------------
// Raw query params (deserialized from URL query string)
// ---------------------------------------------------------------------------

/// Raw query parameters as parsed from the URL query string.
///
/// Repeated params (`fileTypes[]`, `extensions[]`) are handled through
/// comma-separated strings because the standard `Query` extractor cannot
/// bind repeated params to `Vec<T>` without extra middleware.
/// The client sends `?fileTypes=text,html` or `?fileTypes=text&fileTypes=html`.
/// The parser handles both forms: each value is split on commas.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQueryParams {
    /// Required free-text query string.
    pub query: String,
    /// Optional explicit operator: "and" or "or".
    pub operator: Option<String>,
    /// Optional time preset: today, yesterday, last_24h, last_7d, last_30d, this_week, this_month.
    pub time_preset: Option<String>,
    /// Absolute range start (ms since epoch). Must be paired with `to_ms`.
    pub from_ms: Option<i64>,
    /// Absolute range end (ms since epoch). Must be paired with `from_ms`.
    pub to_ms: Option<i64>,
    /// Comma-separated file types (text, html, link, file, image, other).
    pub content_types: Option<String>,
    /// Comma-separated file extensions (e.g. "md,txt").
    pub extensions: Option<String>,
    /// Maximum results. Default 50, clamped to 200.
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Pagination offset. Default 0.
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 {
    50
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse raw `SearchQueryParams` into a domain `SearchQuery`.
///
/// Rules:
/// - Strip standalone AND/OR tokens from the query string before passing it through.
/// - If `operator` is absent, infer from the remaining standalone tokens.
/// - Mixed standalone AND and OR tokens → `invalid_query` 400.
/// - Invalid `timePreset` → `bad_request` 400.
/// - Invalid `fileTypes` value → `bad_request` 400.
/// - Mismatched `fromMs` / `toMs` (one present, other absent) → `bad_request` 400.
pub(crate) fn parse_search_query(params: &SearchQueryParams) -> Result<SearchQuery, ApiError> {
    // --- operator inference from raw query string ---
    let raw_query = &params.query;
    let (query_string, inferred_operator) = strip_and_infer_operator(raw_query)?;

    // --- explicit operator wins over inferred ---
    let operator = if let Some(ref op_str) = params.operator {
        match op_str.to_lowercase().as_str() {
            "and" => QueryOperator::And,
            "or" => QueryOperator::Or,
            _ => {
                return Err(ApiError {
                    status: StatusCode::BAD_REQUEST,
                    code: "bad_request".to_string(),
                    message: format!("invalid operator: {op_str}"),
                })
            }
        }
    } else {
        inferred_operator.unwrap_or(QueryOperator::And)
    };

    // --- time range ---
    let time_range = parse_time_range(params)?;

    // --- file types ---
    let content_types = parse_content_types(params.content_types.as_deref())?;

    // --- extensions ---
    let extensions = parse_extensions(params.extensions.as_deref());

    // --- limit (clamp to 200) ---
    let limit = params.limit.min(200);

    Ok(SearchQuery {
        query_string,
        operator,
        time_range,
        content_types,
        extensions,
        limit,
        offset: params.offset,
    })
}

/// Strip standalone boolean operator tokens from the raw query string.
///
/// Returns `(cleaned_query, inferred_operator)` where:
/// - Standalone `AND` or `OR` tokens (case-insensitive, surrounded by whitespace or at
///   start/end) are removed from the returned query string.
/// - `inferred_operator` is `Some(And)`, `Some(Or)`, or `None` (no operator tokens found).
/// - Mixed AND and OR tokens return an `invalid_query` error.
fn strip_and_infer_operator(raw: &str) -> Result<(String, Option<QueryOperator>), ApiError> {
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
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_query".to_string(),
            message: "mixed AND/OR operators are not supported".to_string(),
        });
    }

    let cleaned = non_operator_tokens.join(" ");
    let inferred = if has_and {
        Some(QueryOperator::And)
    } else if has_or {
        Some(QueryOperator::Or)
    } else {
        None
    };

    Ok((cleaned, inferred))
}

fn parse_time_range(params: &SearchQueryParams) -> Result<Option<TimeRangeFilter>, ApiError> {
    // fromMs/toMs pair — both or neither
    let has_from = params.from_ms.is_some();
    let has_to = params.to_ms.is_some();

    if has_from != has_to {
        return Err(ApiError::bad_request(
            "fromMs and toMs must both be present or both absent",
        ));
    }

    if let (Some(from_ms), Some(to_ms)) = (params.from_ms, params.to_ms) {
        if from_ms < 0 || to_ms < 0 {
            return Err(ApiError::bad_request(
                "fromMs and toMs must be non-negative",
            ));
        }
        return Ok(Some(TimeRangeFilter::Absolute {
            from_ms: from_ms as u64,
            to_ms: to_ms as u64,
        }));
    }

    // timePreset takes effect when no absolute range is given
    if let Some(ref preset) = params.time_preset {
        let filter = match preset.as_str() {
            "today" => TimeRangeFilter::Today,
            "yesterday" => TimeRangeFilter::Yesterday,
            "last_24h" => TimeRangeFilter::Last24h,
            "last_7d" => TimeRangeFilter::Last7d,
            "last_30d" => TimeRangeFilter::Last30d,
            "this_week" => TimeRangeFilter::ThisWeek,
            "this_month" => TimeRangeFilter::ThisMonth,
            other => {
                return Err(ApiError::bad_request(format!(
                    "invalid timePreset: {other}"
                )))
            }
        };
        return Ok(Some(filter));
    }

    Ok(None)
}

fn parse_content_types(raw: Option<&str>) -> Result<Vec<ContentType>, ApiError> {
    let Some(raw) = raw else {
        return Ok(vec![]);
    };

    let mut result = Vec::new();
    for value in raw.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let ft = match value {
            "text" => ContentType::Text,
            "html" => ContentType::Html,
            "link" => ContentType::Link,
            "file" => ContentType::File,
            "image" => ContentType::Image,
            "other" => ContentType::Other,
            unknown => {
                return Err(ApiError::bad_request(format!(
                    "invalid fileType: {unknown}"
                )))
            }
        };
        result.push(ft);
    }
    Ok(result)
}

fn parse_extensions(raw: Option<&str>) -> Vec<String> {
    let Some(raw) = raw else {
        return vec![];
    };
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Session lock guard
// ---------------------------------------------------------------------------

/// Returns `Err(session_locked ApiError)` if the encryption session is not ready.
async fn require_encryption_ready(state: &DaemonApiState) -> Result<(), ApiError> {
    let runtime = state.runtime_or_error()?;
    if !runtime.is_encryption_ready().await {
        return Err(ApiError {
            status: StatusCode::LOCKED,
            code: "session_locked".to_string(),
            message: "encryption session is locked".to_string(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

/// Build the search sub-router. Routes are mounted under the L2+ protected chain.
pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route(http_route::SEARCH_QUERY, get(search_query_handler))
        .route(http_route::SEARCH_STATUS, get(search_status_handler))
        .route(http_route::SEARCH_REBUILD, post(search_rebuild_handler))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /search/query
///
/// Execute a structured search query against the local encrypted search index.
/// Returns HTTP 423 if the encryption session is locked.
#[instrument(
    name = "api.search_query",
    level = "info",
    skip(state, params),
    fields(query = %params.query, limit = params.limit, offset = params.offset)
)]
async fn search_query_handler(
    State(state): State<DaemonApiState>,
    Query(params): Query<SearchQueryParams>,
) -> Result<Json<SearchQueryResponse>, ApiError> {
    require_encryption_ready(&state).await?;

    let runtime = state.runtime_or_error()?;
    let query = parse_search_query(&params)?;
    debug!(query_string = %query.query_string, operator = ?query.operator, "parsed search query");

    let usecases = CoreUseCases::new(runtime.as_ref());

    let page = usecases
        .search_clipboard_entries()
        .execute(query)
        .await
        .map_err(|e| map_search_error(e))?;

    let result_count = page.items.len();
    let data: Vec<SearchResultDto> = page
        .items
        .into_iter()
        .map(|r| SearchResultDto {
            entry_id: r.entry_id.to_string(),
            content_type: r.content_type,
            active_time_ms: r.active_time_ms,
            text_preview: r.text_preview,
            mime_type: r.mime_type,
            file_extensions: r.file_extensions,
        })
        .collect();

    info!(
        total = page.total,
        returned = result_count,
        has_more = page.has_more,
        "search query completed"
    );

    Ok(Json(SearchQueryResponse {
        total: page.total,
        has_more: page.has_more,
        data,
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// GET /search/status
///
/// Returns the current search index availability snapshot (coordinator status + index meta timestamps).
/// Returns HTTP 423 if the encryption session is locked.
#[instrument(name = "api.search_status", level = "info", skip(state))]
async fn search_status_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<SearchStatusResponse>, ApiError> {
    require_encryption_ready(&state).await?;

    let runtime = state.runtime_or_error()?;

    // Get coordinator snapshot (status/reason)
    let snapshot = if let Some(coordinator) = state.search_coordinator() {
        coordinator.status_snapshot().await
    } else {
        return Err(ApiError::service_unavailable(
            "search coordinator unavailable",
        ));
    };

    // Get index meta for timestamps — access the search index port directly
    let meta = runtime
        .wiring_deps()
        .search
        .search_index
        .get_index_meta()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    debug!(
        state = %snapshot.status,
        reason = ?snapshot.reason,
        "search status queried"
    );

    Ok(Json(SearchStatusResponse {
        data: SearchStatusData {
            state: snapshot.status,
            reason: snapshot.reason,
            last_rebuild_started_at_ms: meta.last_rebuild_started_at_ms,
            last_rebuild_completed_at_ms: meta.last_rebuild_completed_at_ms,
        },
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// POST /search/rebuild
///
/// Trigger a manual full rebuild of the search index.
/// Returns HTTP 202 on accept, HTTP 409 with `rebuild_already_running` when another rebuild is in progress.
/// Returns HTTP 423 if the encryption session is locked.
#[instrument(name = "api.search_rebuild", level = "info", skip(state))]
async fn search_rebuild_handler(
    State(state): State<DaemonApiState>,
) -> Result<(StatusCode, Json<SearchRebuildAcceptedResponse>), ApiError> {
    require_encryption_ready(&state).await?;

    let coordinator = state
        .search_coordinator()
        .ok_or_else(|| ApiError::service_unavailable("search coordinator unavailable"))?;

    match coordinator.request_manual_rebuild().await {
        ManualRebuildResult::Accepted => {
            info!("manual search index rebuild accepted");
            Ok((
                StatusCode::ACCEPTED,
                Json(SearchRebuildAcceptedResponse {
                    data: SearchRebuildAcceptedData { accepted: true },
                    ts: chrono::Utc::now().timestamp_millis(),
                }),
            ))
        }
        ManualRebuildResult::AlreadyInProgress => {
            debug!("manual rebuild rejected — already in progress");
            Err(ApiError {
                status: StatusCode::CONFLICT,
                code: "rebuild_already_running".to_string(),
                message: "a rebuild is already in progress".to_string(),
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

fn map_search_error(e: uc_core::search::SearchError) -> ApiError {
    match e {
        uc_core::search::SearchError::InvalidQuery(msg) => ApiError {
            status: StatusCode::BAD_REQUEST,
            code: "invalid_query".to_string(),
            message: msg,
        },
        uc_core::search::SearchError::SessionLocked => ApiError {
            status: StatusCode::LOCKED,
            code: "session_locked".to_string(),
            message: "encryption session is locked".to_string(),
        },
        uc_core::search::SearchError::IndexNotReady => ApiError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "index_not_ready".to_string(),
            message: "search index not ready".to_string(),
        },
        uc_core::search::SearchError::IndexUnavailable => ApiError {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "index_unavailable".to_string(),
            message: "search index unavailable".to_string(),
        },
        uc_core::search::SearchError::Internal(msg) => ApiError::internal(msg),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_params(query: &str) -> SearchQueryParams {
        SearchQueryParams {
            query: query.to_string(),
            operator: None,
            time_preset: None,
            from_ms: None,
            to_ms: None,
            content_types: None,
            extensions: None,
            limit: 50,
            offset: 0,
        }
    }

    #[test]
    fn search_query_route_parses_filters_and_rejects_mixed_operators() {
        // Basic query with no operators
        let params = make_params("clipboard sync");
        let q = parse_search_query(&params).expect("basic query should parse");
        assert_eq!(q.query_string, "clipboard sync");
        assert_eq!(q.operator, QueryOperator::And); // default

        // Explicit operator
        let params_or = SearchQueryParams {
            operator: Some("or".to_string()),
            ..make_params("hello world")
        };
        let q_or = parse_search_query(&params_or).expect("or operator should parse");
        assert_eq!(q_or.operator, QueryOperator::Or);

        // AND inferred from token
        let params_and = make_params("hello AND world");
        let q_and = parse_search_query(&params_and).expect("AND token should parse");
        assert_eq!(q_and.operator, QueryOperator::And);
        assert_eq!(q_and.query_string, "hello world");

        // OR inferred from token
        let params_or2 = make_params("hello OR world");
        let q_or2 = parse_search_query(&params_or2).expect("OR token should parse");
        assert_eq!(q_or2.operator, QueryOperator::Or);
        assert_eq!(q_or2.query_string, "hello world");

        // Mixed AND/OR → invalid_query
        let params_mixed = make_params("hello AND world OR foo");
        let err = parse_search_query(&params_mixed).expect_err("mixed operators should fail");
        assert_eq!(err.code, "invalid_query");
        assert!(err
            .message
            .contains("mixed AND/OR operators are not supported"));

        // fileTypes parsing
        let params_ft = SearchQueryParams {
            content_types: Some("text,html".to_string()),
            ..make_params("test")
        };
        let q_ft = parse_search_query(&params_ft).expect("file types should parse");
        assert_eq!(
            q_ft.content_types,
            vec![ContentType::Text, ContentType::Html]
        );

        // Invalid fileType
        let params_bad_ft = SearchQueryParams {
            content_types: Some("bad_type".to_string()),
            ..make_params("test")
        };
        let err_ft = parse_search_query(&params_bad_ft).expect_err("invalid fileType should fail");
        assert_eq!(err_ft.code, "bad_request");

        // timePreset valid
        let params_preset = SearchQueryParams {
            time_preset: Some("last_7d".to_string()),
            ..make_params("test")
        };
        let q_preset = parse_search_query(&params_preset).expect("time preset should parse");
        assert_eq!(q_preset.time_range, Some(TimeRangeFilter::Last7d));

        // Invalid timePreset
        let params_bad_preset = SearchQueryParams {
            time_preset: Some("last_5_years".to_string()),
            ..make_params("test")
        };
        let err_preset =
            parse_search_query(&params_bad_preset).expect_err("invalid preset should fail");
        assert_eq!(err_preset.code, "bad_request");

        // Mismatched fromMs/toMs
        let params_mismatch = SearchQueryParams {
            from_ms: Some(1000),
            to_ms: None,
            ..make_params("test")
        };
        let err_mismatch =
            parse_search_query(&params_mismatch).expect_err("mismatched time range should fail");
        assert_eq!(err_mismatch.code, "bad_request");

        // Absolute time range
        let params_abs = SearchQueryParams {
            from_ms: Some(1000),
            to_ms: Some(2000),
            ..make_params("test")
        };
        let q_abs = parse_search_query(&params_abs).expect("absolute range should parse");
        assert_eq!(
            q_abs.time_range,
            Some(TimeRangeFilter::Absolute {
                from_ms: 1000,
                to_ms: 2000,
            })
        );

        // limit clamped to 200
        let params_limit = SearchQueryParams {
            limit: 500,
            ..make_params("test")
        };
        let q_limit = parse_search_query(&params_limit).expect("limit should parse");
        assert_eq!(q_limit.limit, 200);

        // extensions
        let params_ext = SearchQueryParams {
            extensions: Some("md,txt".to_string()),
            ..make_params("test")
        };
        let q_ext = parse_search_query(&params_ext).expect("extensions should parse");
        assert_eq!(q_ext.extensions, vec!["md", "txt"]);
    }
}

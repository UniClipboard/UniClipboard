//! HTTP route handlers for search endpoints (Phase 92).
//!
//! All routes are protected by the auth_extractor + rate_limit middleware chain
//! applied at the router level (see routes::router_l2_plus).
//!
//! Lock guard: every search handler (`query`, `tags`, `status`, `rebuild`)
//! requires an unlocked encryption session and returns HTTP 423 `session_locked`
//! otherwise. The render fields of every result row now live in an encrypted
//! payload, so even a filter-only browse must decrypt to render — there is no
//! "served while locked" path. The engine still surfaces `session_locked` as
//! defense-in-depth behind the handlers' front pre-check.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tracing::{debug, info, instrument};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::constants::http_route;
use uc_engine::internal::encryption::{
    execute_query_encryption_state, QUERY_ENCRYPTION_STATE_FAILED_CODE,
};
use uc_engine::internal::search::{
    execute_query_search_status, execute_query_search_tags, execute_rebuild_search_index,
    execute_search_entries, SEARCH_BAD_REQUEST_CODE, SEARCH_FAILED_CODE,
    SEARCH_INDEX_NOT_READY_CODE, SEARCH_INDEX_REBUILDING_CODE, SEARCH_INDEX_UNAVAILABLE_CODE,
    SEARCH_INVALID_QUERY_CODE, SEARCH_REBUILD_ALREADY_RUNNING_CODE,
    SEARCH_SERVICE_UNAVAILABLE_CODE, SEARCH_SESSION_LOCKED_CODE,
};
use uc_engine::{EngineError, OperationResult, SearchEntriesInput};
use utoipa::IntoParams;

use crate::api::dto::error::{log_facade_failure, ApiError};
// `SearchQueryEnvelope`/`SearchStatusEnvelope`/`SearchRebuildEnvelope` (the alias
// names referenced as `#[utoipa::path]` response bodies) are the utoipa-v4
// `ApiEnvelope<T>` aliases declared in the contract's `dto/envelope.rs`. The
// concrete payload DTOs below are re-exported through `crate::api::dto::search`.
use crate::api::dto::search::{
    SearchQueryResultDto, SearchRebuildAcceptedData, SearchResultDto, SearchStatusData,
    SearchTagDto,
};
use crate::api::projection::IntoApiDto;
use crate::api::server::DaemonApiState;

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
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
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
    /// Comma-separated file types (text, html, file, image, other). `image`
    /// here is the physical type of a pure bitmap; copied image *files* are
    /// `file` and matched via the `image` tag instead (see `tags`).
    pub content_types: Option<String>,
    /// Comma-separated file extensions (e.g. "md,txt").
    pub extensions: Option<String>,
    /// Comma-separated source device ids; restricts results to those origins.
    pub source_devices: Option<String>,
    /// Comma-separated tag ids (e.g. "link,favorited,image"); restricts results
    /// to entries carrying any of them. Custom tag ids require an unlocked
    /// session.
    pub tags: Option<String>,
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

fn search_input_from_params(params: SearchQueryParams) -> SearchEntriesInput {
    SearchEntriesInput {
        query: params.query,
        operator: params.operator,
        time_preset: params.time_preset,
        from_ms: params.from_ms,
        to_ms: params.to_ms,
        content_types: params.content_types,
        extensions: params.extensions,
        source_devices: params.source_devices,
        tags: params.tags,
        limit: params.limit,
        offset: params.offset,
    }
}

// ---------------------------------------------------------------------------
// Session lock guard
// ---------------------------------------------------------------------------

/// Returns `Err(session_locked ApiError)` if the encryption session is not ready.
async fn require_encryption_ready(state: &DaemonApiState) -> Result<(), ApiError> {
    let app = state.app_facade_or_error()?;
    let result = execute_query_encryption_state(app.as_ref())
        .await
        .map_err(|error| {
            let variant = if error.code() == QUERY_ENCRYPTION_STATE_FAILED_CODE {
                "query_failed"
            } else {
                "unexpected_engine_error"
            };
            let api = ApiError::internal("encryption state unavailable");
            log_facade_failure(
                "encryption",
                "encryption_state_probe",
                variant,
                api.status,
                &api.message,
            );
            api
        })?;
    let OperationResult::EncryptionState(encryption_state) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected encryption-state result",
        ));
    };
    if !encryption_state.session_ready {
        return Err(ApiError {
            status: StatusCode::LOCKED,
            code: "session_locked".to_string(),
            message: "encryption session is locked".to_string(),
            details: None,
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
        .route(http_route::SEARCH_TAGS, get(search_tags_handler))
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /search/query
///
/// Execute a structured search query against the local encrypted search index.
/// Every query requires an unlocked session because result rendering decrypts
/// the encrypted payload.
///
/// ADR-008 wire change: `total`/`hasMore` are no longer top-level siblings of
/// the envelope — they are folded INTO the `data` payload alongside the renamed
/// `items` array (`SearchQueryResultDto`). The response is the canonical
/// `ApiEnvelope<SearchQueryResultDto>` (`{ data: { items, total, hasMore, state }, ts }`).
///
/// Index-not-ready handling is query-type-aware (§4.7): a filter-less browse
/// degrades to a direct main-store read and returns HTTP 200 with
/// `state: "degraded"`; a keyword or filtered query instead returns HTTP 503
/// `index_rebuilding`.
#[utoipa::path(
    get,
    path = "/search/query",
    tag = "search",
    operation_id = "searchQuery",
    params(SearchQueryParams),
    responses(
        (status = 200, description = "Search results page (state ready or degraded)", body = SearchQueryEnvelope),
        (status = 400, description = "Invalid or malformed query", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked (all query forms require an unlocked session)", body = ApiErrorResponse),
        (status = 503, description = "Search index not ready, rebuilding, or unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(
    name = "api.search_query",
    level = "info",
    skip(state, params),
    fields(limit = params.limit, offset = params.offset)
)]
async fn search_query_handler(
    State(state): State<DaemonApiState>,
    Query(params): Query<SearchQueryParams>,
) -> Result<Json<ApiEnvelope<SearchQueryResultDto>>, ApiError> {
    // Blanket lock guard (§3.3): every query form — including filter-only browse
    // — must decrypt each row's render payload, so all require an unlocked
    // session. Returns 423 `session_locked` when locked.
    require_encryption_ready(&state).await?;
    let app = state.app_facade_or_error()?;
    let input = search_input_from_params(params);
    debug!(
        has_query = !input.query.trim().is_empty(),
        "dispatching search query through engine"
    );

    let result = execute_search_entries(app.as_ref(), input)
        .await
        .map_err(|error| map_search_engine_error("search_query", error))?;
    let OperationResult::SearchPage(page) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected search-page result",
        ));
    };

    let result_count = page.items.len();
    let total = page.total;
    let has_more = page.has_more;
    let state = page.state.clone();
    let items: Vec<SearchResultDto> = page.into_api_dto();

    info!(
        total,
        returned = result_count,
        has_more,
        state = %state,
        "search query completed"
    );

    // Fold `total`/`hasMore`/`state` into the payload (ADR-008 §0.1) and wrap in
    // the canonical envelope.
    Ok(Json(ApiEnvelope::now(SearchQueryResultDto {
        items,
        total,
        has_more,
        state,
    })))
}

/// GET /search/tags
///
/// List the tags present in the index with their entry counts. Builtin tags
/// (link/code/favorited/image) are always listed (filter-only over the membership
/// table, so no search key is needed); custom tags are listed only when the
/// session is unlocked (§4.6).
#[utoipa::path(
    get,
    path = "/search/tags",
    tag = "search",
    operation_id = "getSearchTags",
    responses(
        (status = 200, description = "Tag list with entry counts", body = SearchTagsEnvelope),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Search index unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.search_tags", level = "info", skip(state))]
async fn search_tags_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<Vec<SearchTagDto>>>, ApiError> {
    // §3.3: tag counts are content-derived (they leak entry count/type
    // distribution), so the whole endpoint is gated behind an unlocked session
    // — same 423 contract as `query`. No "builtin tags while locked" path.
    require_encryption_ready(&state).await?;
    let app = state.app_facade_or_error()?;
    let result = execute_query_search_tags(app.as_ref())
        .await
        .map_err(|error| map_search_engine_error("search_tags", error))?;
    let OperationResult::SearchTags(views) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected search-tags result",
        ));
    };

    let items: Vec<SearchTagDto> = views.into_iter().map(IntoApiDto::into_api_dto).collect();

    debug!(tag_count = items.len(), "search tags listed");
    Ok(Json(ApiEnvelope::now(items)))
}

/// GET /search/status
///
/// Returns the current search index availability snapshot (coordinator status + index meta timestamps).
/// Returns HTTP 423 if the encryption session is locked.
///
/// Already on `{ data, ts }`; the bespoke wrapper is replaced by the canonical
/// `ApiEnvelope<SearchStatusData>` (identical JSON, not a wire change).
#[utoipa::path(
    get,
    path = "/search/status",
    tag = "search",
    operation_id = "getSearchStatus",
    responses(
        (status = 200, description = "Search index availability snapshot", body = SearchStatusEnvelope),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Search index unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.search_status", level = "info", skip(state))]
async fn search_status_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<SearchStatusData>>, ApiError> {
    require_encryption_ready(&state).await?;

    let app = state.app_facade_or_error()?;
    let result = execute_query_search_status(app.as_ref())
        .await
        .map_err(|error| map_search_engine_error("search_status", error))?;
    let OperationResult::SearchStatus(view) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected search-status result",
        ));
    };

    debug!(
        state = %view.state,
        reason = ?view.reason,
        "search status queried"
    );

    Ok(Json(ApiEnvelope::now(view.into_api_dto())))
}

/// POST /search/rebuild
///
/// Trigger a manual full rebuild of the search index.
/// Returns HTTP 202 on accept, HTTP 409 with `rebuild_already_running` when another rebuild is in progress.
/// Returns HTTP 423 if the encryption session is locked.
///
/// Already on `{ data, ts }`; the bespoke wrapper is replaced by the canonical
/// `ApiEnvelope<SearchRebuildAcceptedData>` (identical JSON, not a wire change).
#[utoipa::path(
    post,
    path = "/search/rebuild",
    tag = "search",
    operation_id = "rebuildSearchIndex",
    responses(
        (status = 202, description = "Rebuild accepted", body = SearchRebuildEnvelope),
        (status = 409, description = "A rebuild is already in progress", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Search index unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(name = "api.search_rebuild", level = "info", skip(state))]
async fn search_rebuild_handler(
    State(state): State<DaemonApiState>,
) -> Result<(StatusCode, Json<ApiEnvelope<SearchRebuildAcceptedData>>), ApiError> {
    require_encryption_ready(&state).await?;

    let app = state.app_facade_or_error()?;
    let result = execute_rebuild_search_index(app.as_ref())
        .await
        .map_err(|error| map_search_engine_error("search_rebuild", error))?;
    let OperationResult::SearchRebuildAccepted { accepted } = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected search-rebuild result",
        ));
    };

    info!("manual search index rebuild accepted");
    Ok((
        StatusCode::ACCEPTED,
        Json(ApiEnvelope::now(SearchRebuildAcceptedData { accepted })),
    ))
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

fn map_search_engine_error(op: &'static str, error: EngineError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match error.code() {
        SEARCH_INVALID_QUERY_CODE => (
            "invalid_query",
            ApiError {
                status: StatusCode::BAD_REQUEST,
                code: "invalid_query".to_string(),
                message: "invalid search query".to_string(),
                details: None,
            },
        ),
        SEARCH_BAD_REQUEST_CODE => (
            "bad_request",
            ApiError {
                status: StatusCode::BAD_REQUEST,
                code: "bad_request".to_string(),
                message: "invalid search request".to_string(),
                details: None,
            },
        ),
        SEARCH_SESSION_LOCKED_CODE => (
            "session_locked",
            ApiError {
                status: StatusCode::LOCKED,
                code: "session_locked".to_string(),
                message: "encryption session is locked".to_string(),
                details: None,
            },
        ),
        SEARCH_INDEX_NOT_READY_CODE => (
            "index_not_ready",
            ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: "index_not_ready".to_string(),
                message: "search index not ready".to_string(),
                details: None,
            },
        ),
        SEARCH_INDEX_REBUILDING_CODE => (
            "index_rebuilding",
            ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: "index_rebuilding".to_string(),
                message: "search index is rebuilding; clear filters to browse".to_string(),
                details: None,
            },
        ),
        SEARCH_INDEX_UNAVAILABLE_CODE => (
            "index_unavailable",
            ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: "index_unavailable".to_string(),
                message: "search index unavailable".to_string(),
                details: None,
            },
        ),
        SEARCH_SERVICE_UNAVAILABLE_CODE => (
            "service_unavailable",
            ApiError::service_unavailable("search service unavailable"),
        ),
        SEARCH_REBUILD_ALREADY_RUNNING_CODE => {
            debug!("manual rebuild rejected — already in progress");
            (
                "rebuild_already_running",
                ApiError {
                    status: StatusCode::CONFLICT,
                    code: "rebuild_already_running".to_string(),
                    message: "a rebuild is already in progress".to_string(),
                    details: None,
                },
            )
        }
        SEARCH_FAILED_CODE => ("internal", ApiError::internal("search operation failed")),
        _ => (
            "unexpected_engine_error",
            ApiError::internal("search operation failed"),
        ),
    };
    log_facade_failure("search", op, variant, api.status, &api.message);
    api
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_rebuilding_maps_to_503_index_rebuilding() {
        let api = map_search_engine_error(
            "search_query",
            EngineError::new(
                SEARCH_INDEX_REBUILDING_CODE,
                uc_engine::EngineErrorCategory::Unavailable,
                true,
            ),
        );
        assert_eq!(api.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(api.code, "index_rebuilding");
    }
}

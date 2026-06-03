//! HTTP route handlers for search endpoints (Phase 92).
//!
//! All routes are protected by the auth_extractor + rate_limit middleware chain
//! applied at the router level (see routes::router_l2_plus).
//!
//! Lock guard: every handler checks `app_facade.encryption.state().session_ready`
//! and returns HTTP 423 with `session_locked` if the encryption session is not ready.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tracing::{debug, info, instrument};
use uc_application::facade::{
    SearchFacadeError, SearchPageView, SearchQueryInput, SearchStatusView,
};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::constants::http_route;
use utoipa::IntoParams;

use crate::api::dto::error::{log_facade_failure, ApiError};
// `SearchQueryEnvelope`/`SearchStatusEnvelope`/`SearchRebuildEnvelope` (the alias
// names referenced as `#[utoipa::path]` response bodies) are the utoipa-v4
// `ApiEnvelope<T>` aliases declared in the contract's `dto/envelope.rs`. The
// concrete payload DTOs below are re-exported through `crate::api::dto::search`.
use crate::api::dto::search::{
    SearchQueryResultDto, SearchRebuildAcceptedData, SearchResultDto, SearchStatusData,
};
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

fn search_input_from_params(params: SearchQueryParams) -> SearchQueryInput {
    SearchQueryInput {
        query: params.query,
        operator: params.operator,
        time_preset: params.time_preset,
        from_ms: params.from_ms,
        to_ms: params.to_ms,
        content_types: params.content_types,
        extensions: params.extensions,
        limit: params.limit,
        offset: params.offset,
    }
}

// ---------------------------------------------------------------------------
// Session lock guard
// ---------------------------------------------------------------------------

/// Returns `Err(session_locked ApiError)` if the encryption session is not ready.
async fn require_encryption_ready(state: &DaemonApiState) -> Result<(), ApiError> {
    let app_facade = state.app_facade_or_error()?;
    let encryption_state = app_facade.encryption.state().await.map_err(|e| {
        let api = ApiError::internal(format!("encryption state unavailable: {e}"));
        log_facade_failure(
            "encryption",
            "encryption_state_probe",
            "call_failed",
            api.status,
            &api.message,
        );
        api
    })?;
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
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /search/query
///
/// Execute a structured search query against the local encrypted search index.
/// Returns HTTP 423 if the encryption session is locked.
///
/// ADR-008 wire change: `total`/`hasMore` are no longer top-level siblings of
/// the envelope — they are folded INTO the `data` payload alongside the renamed
/// `items` array (`SearchQueryResultDto`). The response is the canonical
/// `ApiEnvelope<SearchQueryResultDto>` (`{ data: { items, total, hasMore }, ts }`).
#[utoipa::path(
    get,
    path = "/search/query",
    tag = "search",
    operation_id = "searchQuery",
    params(SearchQueryParams),
    responses(
        (status = 200, description = "Search results page", body = SearchQueryEnvelope),
        (status = 400, description = "Invalid or malformed query", body = ApiErrorResponse),
        (status = 423, description = "Encryption session is locked", body = ApiErrorResponse),
        (status = 503, description = "Search index not ready or unavailable", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
#[instrument(
    name = "api.search_query",
    level = "info",
    skip(state, params),
    fields(query = %params.query, limit = params.limit, offset = params.offset)
)]
async fn search_query_handler(
    State(state): State<DaemonApiState>,
    Query(params): Query<SearchQueryParams>,
) -> Result<Json<ApiEnvelope<SearchQueryResultDto>>, ApiError> {
    require_encryption_ready(&state).await?;

    let app = state.app_facade_or_error()?;
    let input = search_input_from_params(params);
    debug!(query = %input.query, "dispatching search query through app facade");

    let page = app
        .search
        .query(input)
        .await
        .map_err(|e| map_search_error("search_query", e))?;

    let result_count = page.items.len();
    let total = page.total;
    let has_more = page.has_more;
    let items = search_page_to_dto(page);

    info!(
        total,
        returned = result_count,
        has_more,
        "search query completed"
    );

    // Fold `total`/`hasMore` into the payload (ADR-008 §0.1) and wrap in the
    // canonical envelope.
    Ok(Json(ApiEnvelope::now(SearchQueryResultDto {
        items,
        total,
        has_more,
    })))
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
    let view = app
        .search
        .status()
        .await
        .map_err(|e| map_search_error("search_status", e))?;

    debug!(
        state = %view.state,
        reason = ?view.reason,
        "search status queried"
    );

    Ok(Json(ApiEnvelope::now(search_status_to_dto(view))))
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
    let accepted = app
        .search
        .request_rebuild()
        .await
        .map_err(|e| map_search_error("search_rebuild", e))?;

    info!("manual search index rebuild accepted");
    Ok((
        StatusCode::ACCEPTED,
        Json(ApiEnvelope::now(SearchRebuildAcceptedData {
            accepted: accepted.accepted,
        })),
    ))
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

fn map_search_error(op: &'static str, error: SearchFacadeError) -> ApiError {
    use SearchFacadeError as E;
    let (variant, api): (&'static str, ApiError) = match error {
        E::InvalidQuery(message) => (
            "invalid_query",
            ApiError {
                status: StatusCode::BAD_REQUEST,
                code: "invalid_query".to_string(),
                message,
                details: None,
            },
        ),
        E::BadRequest(message) => (
            "bad_request",
            ApiError {
                status: StatusCode::BAD_REQUEST,
                code: "bad_request".to_string(),
                message,
                details: None,
            },
        ),
        E::SessionLocked => (
            "session_locked",
            ApiError {
                status: StatusCode::LOCKED,
                code: "session_locked".to_string(),
                message: "encryption session is locked".to_string(),
                details: None,
            },
        ),
        E::IndexNotReady => (
            "index_not_ready",
            ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: "index_not_ready".to_string(),
                message: "search index not ready".to_string(),
                details: None,
            },
        ),
        E::IndexUnavailable => (
            "index_unavailable",
            ApiError {
                status: StatusCode::SERVICE_UNAVAILABLE,
                code: "index_unavailable".to_string(),
                message: "search index unavailable".to_string(),
                details: None,
            },
        ),
        E::ServiceUnavailable(message) => (
            "service_unavailable",
            ApiError::service_unavailable(message),
        ),
        E::RebuildAlreadyRunning => {
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
        E::Internal(message) => ("internal", ApiError::internal(message)),
    };
    log_facade_failure("search", op, variant, api.status, &api.message);
    api
}

fn search_status_to_dto(view: SearchStatusView) -> SearchStatusData {
    SearchStatusData {
        state: view.state,
        reason: view.reason,
        last_rebuild_started_at_ms: view.last_rebuild_started_at_ms,
        last_rebuild_completed_at_ms: view.last_rebuild_completed_at_ms,
    }
}

fn search_page_to_dto(page: SearchPageView) -> Vec<SearchResultDto> {
    page.items
        .into_iter()
        .map(|result| SearchResultDto {
            entry_id: result.entry_id,
            content_type: result.content_type,
            active_time_ms: result.active_time_ms,
            text_preview: result.text_preview,
            mime_type: result.mime_type,
            file_extensions: result.file_extensions,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

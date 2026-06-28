//! Feature-specific daemon search client (Phase 92.1).
//!
//! Provides `DaemonSearchClient` that sends the exact `/search/query`,
//! `/search/status`, and `/search/rebuild` transport contract without
//! rebuilding daemon-side query parsing locally.

use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;

use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;
use uc_daemon_contract::api::dto::search::{
    SearchQueryResultDto, SearchRebuildAcceptedData, SearchStatusData,
};
use uc_daemon_contract::constants::http_route;

/// Search query parameters sent to the daemon `GET /search/query` endpoint.
///
/// All values are forwarded verbatim — no daemon-side query grammar is reproduced here.
/// The daemon strips inline AND/OR, infers operators, and enforces lock semantics.
#[derive(Debug, Clone)]
pub struct SearchQueryRequest {
    pub query: String,
    pub operator: Option<String>,
    pub time_preset: Option<String>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub content_types: Vec<String>,
    /// Tag ids (e.g. "link", "favorited"); custom tags require an unlocked session.
    pub tags: Vec<String>,
    pub extensions: Vec<String>,
    pub limit: u32,
    pub offset: u32,
}

/// Feature-specific daemon search client.
///
/// Shares connection state and HTTP client with `DaemonClientContext`.
/// Constructed via `DaemonClientContext::search_client()`.
///
/// Failed requests carry `DaemonRequestError::Status` (downcastable from the
/// returned `anyhow::Error`) with the daemon's stable `code` — e.g.
/// `session_locked`, `invalid_query`, `rebuild_already_running` — so callers
/// can branch without string scraping.
#[derive(Clone)]
pub struct DaemonSearchClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonSearchClient {
    pub fn new(connection_state: DaemonConnectionState) -> Self {
        Self {
            http: Arc::new(reqwest::Client::new()),
            connection_state,
            client_type: "gui".to_string(),
        }
    }

    pub(crate) fn with_http_conn_state_and_type(
        http: Arc<reqwest::Client>,
        connection_state: DaemonConnectionState,
        client_type: String,
    ) -> Self {
        Self {
            http,
            connection_state,
            client_type,
        }
    }

    /// Execute a structured search query against the daemon.
    ///
    /// Sends `GET /search/query` with camelCase query params.
    /// Does NOT strip inline AND/OR or infer operators locally.
    pub async fn query(&self, request: SearchQueryRequest) -> Result<SearchQueryResultDto> {
        let mut params: Vec<(&str, String)> = vec![
            ("query", request.query),
            ("limit", request.limit.to_string()),
            ("offset", request.offset.to_string()),
        ];
        if let Some(operator) = request.operator {
            params.push(("operator", operator));
        }
        if let Some(preset) = request.time_preset {
            params.push(("timePreset", preset));
        }
        if let Some(from_ms) = request.from_ms {
            params.push(("fromMs", from_ms.to_string()));
        }
        if let Some(to_ms) = request.to_ms {
            params.push(("toMs", to_ms.to_string()));
        }
        if !request.content_types.is_empty() {
            params.push(("contentTypes", request.content_types.join(",")));
        }
        if !request.tags.is_empty() {
            params.push(("tags", request.tags.join(",")));
        }
        if !request.extensions.is_empty() {
            params.push(("extensions", request.extensions.join(",")));
        }

        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            http_route::SEARCH_QUERY,
            |r| r.query(&params),
        )
        .await?)
    }

    /// Fetch the current search index availability status.
    ///
    /// Sends `GET /search/status`.
    pub async fn status(&self) -> Result<SearchStatusData> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            http_route::SEARCH_STATUS,
            |r| r,
        )
        .await?)
    }

    /// Trigger a manual search index rebuild on the daemon.
    ///
    /// Sends `POST /search/rebuild`. A `rebuild_already_running` daemon response
    /// surfaces as `DaemonRequestError::Status` with that code so callers can
    /// distinguish it from other failures.
    pub async fn rebuild(&self) -> Result<SearchRebuildAcceptedData> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::SEARCH_REBUILD,
            |r| r,
        )
        .await?)
    }
}

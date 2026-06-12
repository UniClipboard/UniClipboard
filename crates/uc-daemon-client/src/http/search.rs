//! Feature-specific daemon search client (Phase 92.1).
//!
//! Provides `DaemonSearchClient` that sends the exact `/search/query`,
//! `/search/status`, and `/search/rebuild` transport contract without
//! rebuilding daemon-side query parsing locally.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::{Method, RequestBuilder, StatusCode};

use crate::http::authorized_daemon_request_with_type;
use crate::DaemonConnectionState;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
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
    pub extensions: Vec<String>,
    pub limit: u32,
    pub offset: u32,
}

/// Structured error from a failed daemon search request.
///
/// Preserves the HTTP status code and the daemon's JSON error body so the CLI
/// can distinguish `session_locked`, `invalid_query`, and `rebuild_already_running`
/// without string scraping.
#[derive(Debug, Clone)]
pub struct DaemonSearchRequestError {
    pub path: String,
    pub status: StatusCode,
    pub code: Option<String>,
    pub message: String,
}

impl std::fmt::Display for DaemonSearchRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(code) = self.code.as_deref() {
            write!(
                f,
                "daemon search request {} failed with status {} [{}]: {}",
                self.path, self.status, code, self.message
            )
        } else {
            write!(
                f,
                "daemon search request {} failed with status {}: {}",
                self.path, self.status, self.message
            )
        }
    }
}

impl std::error::Error for DaemonSearchRequestError {}

/// Feature-specific daemon search client.
///
/// Shares connection state and HTTP client with `DaemonClientContext`.
/// Constructed via `DaemonClientContext::search_client()`.
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
        let path = http_route::SEARCH_QUERY;
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
        if !request.extensions.is_empty() {
            params.push(("extensions", request.extensions.join(",")));
        }

        let response = self
            .authorized_request(Method::GET, path)
            .await?
            .query(&params)
            .send()
            .await
            .with_context(|| format!("failed to call daemon search route {path}"))?;

        Self::decode_json_response(response, path).await
    }

    /// Fetch the current search index availability status.
    ///
    /// Sends `GET /search/status`.
    pub async fn status(&self) -> Result<SearchStatusData> {
        let path = http_route::SEARCH_STATUS;
        let response = self
            .authorized_request(Method::GET, path)
            .await?
            .send()
            .await
            .with_context(|| format!("failed to call daemon search route {path}"))?;

        Self::decode_json_response(response, path).await
    }

    /// Trigger a manual search index rebuild on the daemon.
    ///
    /// Sends `POST /search/rebuild`. A `rebuild_already_running` daemon response
    /// is returned as a structured `DaemonSearchRequestError` rather than a plain string
    /// so callers can distinguish it from other failures.
    pub async fn rebuild(&self) -> Result<SearchRebuildAcceptedData> {
        let path = http_route::SEARCH_REBUILD;
        let response = self
            .authorized_request(Method::POST, path)
            .await?
            .send()
            .await
            .with_context(|| format!("failed to call daemon search route {path}"))?;

        Self::decode_json_response(response, path).await
    }

    async fn authorized_request(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
        authorized_daemon_request_with_type(
            &*self.http,
            &self.connection_state,
            method,
            path,
            connection.pid,
            &self.client_type,
        )
        .await
    }

    /// Decode an enveloped search success body (ADR-008 §H: query/status/rebuild
    /// are all `ApiEnvelope<T>` now) and return the unwrapped payload `T`, or map
    /// a non-success body to a structured `DaemonSearchRequestError`.
    async fn decode_json_response<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
        path: &str,
    ) -> Result<T> {
        let status = response.status();
        if status.is_success() {
            let envelope = response
                .json::<ApiEnvelope<T>>()
                .await
                .with_context(|| format!("failed to decode daemon search response for {path}"))?;
            return Ok(envelope.data);
        }

        Err(Self::decode_error_response(response, path).await)
    }

    async fn decode_error_response(response: reqwest::Response, path: &str) -> anyhow::Error {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable response body>".to_string());

        #[derive(serde::Deserialize)]
        struct SearchApiErrorBody {
            code: Option<String>,
            message: Option<String>,
        }

        let maybe_error = serde_json::from_str::<SearchApiErrorBody>(&body).ok();
        let error = DaemonSearchRequestError {
            path: path.to_string(),
            status,
            code: maybe_error.as_ref().and_then(|e| e.code.clone()),
            message: maybe_error.and_then(|e| e.message).unwrap_or(body),
        };

        anyhow!(error)
    }
}

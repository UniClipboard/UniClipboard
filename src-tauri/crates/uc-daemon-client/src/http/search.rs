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
use uc_core::network::daemon_api_strings::http_route;
use uc_daemon_contract::api::dto::search::{
    SearchQueryResponse, SearchRebuildAcceptedResponse, SearchStatusResponse,
};

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
    pub async fn query(&self, request: SearchQueryRequest) -> Result<SearchQueryResponse> {
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
    pub async fn status(&self) -> Result<SearchStatusResponse> {
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
    pub async fn rebuild(&self) -> Result<SearchRebuildAcceptedResponse> {
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

    async fn decode_json_response<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
        path: &str,
    ) -> Result<T> {
        let status = response.status();
        if status.is_success() {
            return response
                .json::<T>()
                .await
                .with_context(|| format!("failed to decode daemon search response for {path}"));
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

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;

    async fn with_session_cache<F>(token: &str, f: F)
    where
        F: std::future::Future<Output = ()>,
    {
        use crate::http::SESSION_TOKEN_CACHE;
        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 300;
        {
            let mut cache = SESSION_TOKEN_CACHE.write().await;
            *cache = Some((token.to_string(), expires_at));
        }
        f.await;
        {
            let mut cache = SESSION_TOKEN_CACHE.write().await;
            *cache = None;
        }
    }

    #[tokio::test]
    async fn daemon_search_client_encodes_query_filters_for_daemon_api() {
        use super::SearchQueryRequest;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let size = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);

            assert!(
                request.contains("/search/query"),
                "missing route: {request}"
            );
            // reqwest encodes spaces as '+' in query params (application/x-www-form-urlencoded).
            assert!(
                request.contains("query=clipboard+sync")
                    || request.contains("query=clipboard%20sync"),
                "missing query param: {request}"
            );
            assert!(
                request.contains("operator=or"),
                "missing operator param: {request}"
            );
            assert!(
                request.contains("timePreset=last_7d"),
                "missing timePreset param: {request}"
            );
            assert!(
                request.contains("contentTypes=text%2Cfile"),
                "missing contentTypes param: {request}"
            );
            assert!(
                request.contains("extensions=md%2Ctxt"),
                "missing extensions param: {request}"
            );
            assert!(
                request.contains("limit=25"),
                "missing limit param: {request}"
            );
            assert!(
                request.contains("offset=5"),
                "missing offset param: {request}"
            );

            let body = serde_json::json!({
                "data": [],
                "total": 0,
                "hasMore": false,
                "ts": 1000
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let connection_state = crate::DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-bearer".to_string(),
            pid: 54321,
        });
        let client = super::DaemonSearchClient::new(connection_state);

        with_session_cache("test-session", async move {
            let result = client
                .query(SearchQueryRequest {
                    query: "clipboard sync".to_string(),
                    operator: Some("or".to_string()),
                    time_preset: Some("last_7d".to_string()),
                    from_ms: None,
                    to_ms: None,
                    content_types: vec!["text".to_string(), "file".to_string()],
                    extensions: vec!["md".to_string(), "txt".to_string()],
                    limit: 25,
                    offset: 5,
                })
                .await
                .unwrap();
            assert_eq!(result.total, 0);
            assert!(!result.has_more);
        })
        .await;
    }

    #[tokio::test]
    async fn daemon_search_client_fetches_status_from_daemon_api() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let size = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);

            assert!(
                request.starts_with("GET /search/status HTTP/1.1\r\n"),
                "wrong request: {request}"
            );
            assert!(
                request.contains("authorization: Session test-session\r\n"),
                "missing session header: {request}"
            );

            let body = serde_json::json!({
                "data": {
                    "state": "ready",
                    "reason": null,
                    "lastRebuildStartedAtMs": 1_000_000i64,
                    "lastRebuildCompletedAtMs": 1_001_000i64
                },
                "ts": 2000
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let connection_state = crate::DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-bearer".to_string(),
            pid: 54321,
        });
        let client = super::DaemonSearchClient::new(connection_state);

        with_session_cache("test-session", async move {
            let result = client.status().await.unwrap();
            assert_eq!(result.data.state, "ready");
            assert!(result.data.reason.is_none());
            assert_eq!(result.data.last_rebuild_started_at_ms, Some(1_000_000));
            assert_eq!(result.data.last_rebuild_completed_at_ms, Some(1_001_000));
        })
        .await;
    }

    #[tokio::test]
    async fn daemon_search_client_decodes_structured_search_error() {
        use super::DaemonSearchRequestError;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let _ = stream.read(&mut request).await.unwrap();

            let body = serde_json::json!({
                "code": "session_locked",
                "message": "encryption session is locked"
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 403 Forbidden\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let connection_state = crate::DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-bearer".to_string(),
            pid: 54321,
        });
        let client = super::DaemonSearchClient::new(connection_state);

        with_session_cache("test-session", async move {
            let err = client.status().await.unwrap_err();
            let search_err = err.downcast::<DaemonSearchRequestError>().unwrap();
            assert_eq!(search_err.code.as_deref(), Some("session_locked"));
            assert_eq!(search_err.message, "encryption session is locked");
            assert_eq!(search_err.status, reqwest::StatusCode::FORBIDDEN);
        })
        .await;
    }

    #[tokio::test]
    async fn daemon_search_client_posts_rebuild_to_daemon_api() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let size = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);

            assert!(
                request.starts_with("POST /search/rebuild HTTP/1.1\r\n"),
                "wrong request: {request}"
            );
            assert!(
                request.contains("authorization: Session test-session\r\n"),
                "missing session header: {request}"
            );

            let body = serde_json::json!({
                "data": { "accepted": true },
                "ts": 3000
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let connection_state = crate::DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-bearer".to_string(),
            pid: 54321,
        });
        let client = super::DaemonSearchClient::new(connection_state);

        with_session_cache("test-session", async move {
            let result = client.rebuild().await.unwrap();
            assert!(result.data.accepted);
        })
        .await;
    }
}

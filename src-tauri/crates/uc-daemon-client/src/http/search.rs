//! Feature-specific daemon search client (Phase 92.1).
//!
//! Provides `DaemonSearchClient` that sends the exact `/search/query`,
//! `/search/status`, and `/search/rebuild` transport contract without
//! rebuilding daemon-side query parsing locally.

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;

    use crate::DaemonClientContext;

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

            assert!(request.contains("/search/query"), "missing route: {request}");
            assert!(
                request.contains("query=clipboard%20sync"),
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
                request.contains("fileTypes=text%2Cfile"),
                "missing fileTypes param: {request}"
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

        let connection_info = DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-bearer".to_string(),
            pid: 54321,
        };
        let ctx = DaemonClientContext::with_connection_info(connection_info, "cli".to_string());
        let client = ctx.search_client();

        with_session_cache("test-session", async move {
            let result = client
                .query(SearchQueryRequest {
                    query: "clipboard sync".to_string(),
                    operator: Some("or".to_string()),
                    time_preset: Some("last_7d".to_string()),
                    from_ms: None,
                    to_ms: None,
                    file_types: vec!["text".to_string(), "file".to_string()],
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

        let connection_info = DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-bearer".to_string(),
            pid: 54321,
        };
        let ctx = DaemonClientContext::with_connection_info(connection_info, "cli".to_string());
        let client = ctx.search_client();

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

        let connection_info = DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-bearer".to_string(),
            pid: 54321,
        };
        let ctx = DaemonClientContext::with_connection_info(connection_info, "cli".to_string());
        let client = ctx.search_client();

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

        let connection_info = DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-bearer".to_string(),
            pid: 54321,
        };
        let ctx = DaemonClientContext::with_connection_info(connection_info, "cli".to_string());
        let client = ctx.search_client();

        with_session_cache("test-session", async move {
            let result = client.rebuild().await.unwrap();
            assert!(result.data.accepted);
        })
        .await;
    }
}

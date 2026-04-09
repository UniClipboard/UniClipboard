use std::sync::Arc;

use anyhow::{Context, Result};
use reqwest::{Method, RequestBuilder};
use uc_daemon_contract::api::dto::setup as dto;

use crate::http::authorized_daemon_request_with_type;
use crate::DaemonConnectionState;

#[derive(Clone)]
pub struct DaemonSetupClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonSetupClient {
    pub fn new() -> Self {
        Self {
            http: Arc::new(reqwest::Client::new()),
            connection_state: DaemonConnectionState::default(),
            client_type: "gui".to_string(),
        }
    }

    pub fn with_conn_state(connection_state: DaemonConnectionState) -> Self {
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

    pub async fn get_setup_state(&self) -> Result<dto::GetSetupStateResponse> {
        self.send_json::<(), dto::GetSetupStateResponse>(Method::GET, "/setup/state", None)
            .await
    }

    pub async fn start_new_space(&self) -> Result<dto::SetupActionResponse> {
        self.send_json::<(), dto::SetupActionResponse>(Method::POST, "/setup/new", None)
            .await
    }

    pub async fn start_join_space(&self) -> Result<dto::SetupActionResponse> {
        self.send_json::<(), dto::SetupActionResponse>(Method::POST, "/setup/join", None)
            .await
    }

    pub async fn select_device(&self, peer_id: String) -> Result<dto::SetupActionResponse> {
        self.send_json(
            Method::POST,
            "/setup/select-peer",
            Some(&dto::SetupSelectPeerRequest { peer_id }),
        )
        .await
    }

    pub async fn confirm_peer_trust(&self) -> Result<dto::SetupActionResponse> {
        self.send_json::<(), dto::SetupActionResponse>(Method::POST, "/setup/confirm-peer", None)
            .await
    }

    pub async fn submit_passphrase(&self, passphrase: String) -> Result<dto::SetupActionResponse> {
        self.send_json(
            Method::POST,
            "/setup/submit-passphrase",
            Some(&dto::SetupSubmitPassphraseRequest { passphrase }),
        )
        .await
    }

    pub async fn verify_passphrase(&self, passphrase: String) -> Result<dto::SetupActionResponse> {
        self.send_json(
            Method::POST,
            "/setup/verify-passphrase",
            Some(&dto::SetupSubmitPassphraseRequest { passphrase }),
        )
        .await
    }

    pub async fn cancel_setup(&self) -> Result<dto::SetupActionResponse> {
        self.send_json::<(), dto::SetupActionResponse>(Method::POST, "/setup/cancel", None)
            .await
    }

    /// Calls `POST /setup/clear-transient` to clear the daemon's in-memory setup session.
    ///
    /// This endpoint clears selected peer, pairing session, joiner offer, and passphrase
    /// while preserving whether the device has already completed setup.
    pub async fn clear_transient_state(&self) -> Result<dto::SetupActionResponse> {
        let request = self
            .authorized_request(Method::POST, "/setup/clear-transient")
            .await?;
        let response = request
            .send()
            .await
            .context("failed to call daemon setup route /setup/clear-transient")?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read body>".to_string());
            return Err(anyhow::anyhow!(
                "daemon setup request /setup/clear-transient failed with status {}: {}",
                status,
                body
            ));
        }

        response
            .json::<dto::SetupActionResponse>()
            .await
            .with_context(|| "failed to decode daemon setup response for /setup/clear-transient")
    }

    pub async fn reset_setup(&self) -> Result<dto::SetupResetResponse> {
        self.send_json::<(), dto::SetupResetResponse>(Method::POST, "/setup/reset", None)
            .await
    }

    async fn authorized_request(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow::anyhow!("daemon connection info is not available"))?;
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

    async fn send_json<TReq, TResp>(
        &self,
        method: Method,
        path: &str,
        payload: Option<&TReq>,
    ) -> Result<TResp>
    where
        TReq: serde::Serialize + ?Sized,
        TResp: serde::de::DeserializeOwned,
    {
        let request = self.authorized_request(method, path).await?;
        let request = if let Some(payload) = payload {
            request.json(payload)
        } else {
            request
        };

        let response = request
            .send()
            .await
            .with_context(|| format!("failed to call daemon setup route {path}"))?;
        let status = response.status();

        if status.is_success() {
            return response
                .json::<TResp>()
                .await
                .with_context(|| format!("failed to decode daemon setup response for {path}"));
        }

        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow::anyhow!(
            "daemon setup request {path} failed with status {}: {}",
            status,
            body
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;
    use uc_daemon_contract::api::dto::setup as dto;

    use super::*;
    use crate::DaemonConnectionState;

    // Pre-cache a session token so HTTP requests use it without triggering a real exchange.
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
    async fn daemon_setup_client_fetches_setup_state_from_daemon_api() {
        let inner = dto::SetupStateResponseDto {
            state: serde_json::json!({
                "JoinSpaceSelectDevice": {
                    "deviceNames": []
                }
            }),
            session_id: Some("session-1".to_string()),
            next_step_hint: "join-select-peer".to_string(),
            profile: "default".to_string(),
            clipboard_mode: "full".to_string(),
            device_name: "Peer A".to_string(),
            peer_id: "peer-a".to_string(),
            selected_peer_id: None,
            selected_peer_name: None,
            has_completed: false,
        };
        let expected = dto::GetSetupStateResponse {
            data: inner.clone(),
            ts: 1710000000000,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 2048];
            let size = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(request.starts_with("GET /setup/state HTTP/1.1\r\n"));
            // After session exchange, header is "Session <session-token>".
            assert!(request.contains("authorization: Session test-session\r\n"));

            let body = serde_json::to_string(&expected).unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let connection_state = DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-bearer".to_string(),
            pid: 54321,
        });

        let client = DaemonSetupClient::with_conn_state(connection_state);
        with_session_cache("test-session", async move {
            let result = client.get_setup_state().await.unwrap();
            assert_eq!(result.data.state, inner.state);
            assert_eq!(result.data.session_id, inner.session_id);
            assert_eq!(result.data.next_step_hint, inner.next_step_hint);
        })
        .await;
    }

    #[tokio::test]
    async fn daemon_setup_client_posts_submit_passphrase_to_daemon_api() {
        let inner = dto::SetupStateResponseDto {
            state: serde_json::json!({
                "JoinSpaceConfirmPeer": {
                    "shortCode": "123456"
                }
            }),
            session_id: Some("session-2".to_string()),
            next_step_hint: "host-confirm-peer".to_string(),
            profile: "default".to_string(),
            clipboard_mode: "full".to_string(),
            device_name: "Peer A".to_string(),
            peer_id: "peer-a".to_string(),
            selected_peer_id: None,
            selected_peer_name: None,
            has_completed: false,
        };
        let expected = dto::SetupActionResponse {
            data: inner.clone(),
            ts: 1710000000000,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let size = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(request.starts_with("POST /setup/submit-passphrase HTTP/1.1\r\n"));
            // After session exchange, header is "Session <session-token>".
            assert!(request.contains("authorization: Session test-session\r\n"));
            assert!(request.contains("\r\n\r\n{\"passphrase\":\"secret-passphrase\"}"));

            let body = serde_json::to_string(&expected).unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let connection_state = DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-bearer".to_string(),
            pid: 54321,
        });

        let client = DaemonSetupClient::with_conn_state(connection_state);
        with_session_cache("test-session", async move {
            let result = client
                .submit_passphrase("secret-passphrase".to_string())
                .await
                .unwrap();
            assert_eq!(result.data.state, inner.state);
            assert_eq!(result.data.session_id, inner.session_id);
            assert_eq!(result.data.next_step_hint, inner.next_step_hint);
        })
        .await;
    }

    #[tokio::test]
    async fn daemon_setup_client_posts_verify_passphrase_to_daemon_api() {
        let inner = dto::SetupStateResponseDto {
            state: serde_json::json!({
                "ProcessingJoinSpace": {
                    "message": "Verifying passphrase…"
                }
            }),
            session_id: Some("session-3".to_string()),
            next_step_hint: "join-waiting-for-host".to_string(),
            profile: "default".to_string(),
            clipboard_mode: "full".to_string(),
            device_name: "Peer B".to_string(),
            peer_id: "peer-b".to_string(),
            selected_peer_id: None,
            selected_peer_name: None,
            has_completed: false,
        };
        let expected = dto::SetupActionResponse {
            data: inner.clone(),
            ts: 1710000000001,
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let size = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(request.starts_with("POST /setup/verify-passphrase HTTP/1.1\r\n"));
            assert!(request.contains("authorization: Session test-session\r\n"));
            assert!(request.contains("\r\n\r\n{\"passphrase\":\"join-secret\"}"));

            let body = serde_json::to_string(&expected).unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let connection_state = DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-bearer".to_string(),
            pid: 54321,
        });

        let client = DaemonSetupClient::with_conn_state(connection_state);
        with_session_cache("test-session", async move {
            let result = client
                .verify_passphrase("join-secret".to_string())
                .await
                .unwrap();
            assert_eq!(result.data.state, inner.state);
            assert_eq!(result.data.session_id, inner.session_id);
            assert_eq!(result.data.next_step_hint, inner.next_step_hint);
        })
        .await;
    }
}

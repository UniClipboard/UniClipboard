use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::{Method, RequestBuilder};

use crate::http::authorized_daemon_request_with_type;
use crate::DaemonConnectionState;
use uc_daemon_contract::api::types::{PairedDeviceDto, PeerSnapshotDto, StatusResponse};

#[derive(Clone)]
pub struct DaemonQueryClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonQueryClient {
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

    pub async fn get_peers(&self) -> Result<Vec<PeerSnapshotDto>> {
        self.get_json(Method::GET, "/peers").await
    }

    pub async fn get_paired_devices(&self) -> Result<Vec<PairedDeviceDto>> {
        self.get_json(Method::GET, "/paired-devices").await
    }

    pub async fn get_status(&self) -> Result<StatusResponse> {
        self.get_json(Method::GET, "/status").await
    }

    /// Unlock the encryption session via the daemon keyring (auto-unlock).
    pub async fn unlock_encryption(&self) -> Result<bool> {
        let response = self
            .authorized_request(Method::POST, "/encryption/unlock")
            .await?
            .send()
            .await
            .with_context(|| "failed to call daemon /encryption/unlock")?;

        if response.status().is_success() {
            let body: serde_json::Value = response
                .json()
                .await
                .with_context(|| "failed to decode /encryption/unlock response")?;
            let success = body
                .pointer("/data/success")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            return Ok(success);
        }
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow!(
            "daemon /encryption/unlock failed with status {}: {}",
            status,
            body,
        ))
    }

    /// Retry the lifecycle boot on the daemon (starts network, opens clipboard capture gate).
    pub async fn lifecycle_retry(&self) -> Result<()> {
        let response = self
            .authorized_request(Method::POST, "/lifecycle/retry")
            .await?
            .send()
            .await
            .with_context(|| "failed to call daemon /lifecycle/retry")?;

        if response.status().is_success() {
            return Ok(());
        }
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow!(
            "daemon /lifecycle/retry failed with status {}: {}",
            status,
            body,
        ))
    }

    /// Signal the daemon that the GUI has unlocked and clipboard capture can begin.
    pub async fn signal_lifecycle_ready(&self) -> Result<()> {
        let response = self
            .authorized_request(Method::POST, "/lifecycle/ready")
            .await?
            .send()
            .await
            .with_context(|| "failed to call daemon /lifecycle/ready")?;

        if response.status().is_success() {
            return Ok(());
        }
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow!(
            "daemon /lifecycle/ready failed with status {}: {}",
            status,
            body,
        ))
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

    async fn get_json<T>(&self, method: Method, path: &str) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = self
            .authorized_request(method, path)
            .await?
            .send()
            .await
            .with_context(|| format!("failed to call daemon query route {path}"))?;

        let status = response.status();
        if status.is_success() {
            return response
                .json::<T>()
                .await
                .with_context(|| format!("failed to decode daemon query response for {path}"));
        }

        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow!(
            "daemon query request {path} failed with status {}: {}",
            status,
            body
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;
    use uc_daemon_contract::api::types::{PairedDeviceDto, PeerSnapshotDto};

    // Pre-cache a session token so HTTP requests use it without triggering a real exchange.
    /// Temporarily sets the global session token cache, runs the provided future, then clears the cache.
    ///
    /// The function writes `(token, expires_at)` into `crate::http::SESSION_TOKEN_CACHE` with an
    /// expiration 300 seconds from now, awaits `f`, and then clears the cache.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn run_example() {
    /// use crate::http::SESSION_TOKEN_CACHE;
    ///
    /// // Run an async block with the session cache set to "test-session"
    /// with_session_cache("test-session", async {
    ///     // code that needs the session token in `SESSION_TOKEN_CACHE`
    /// }).await;
    ///
    /// // After completion the cache has been cleared
    /// # }
    /// ```
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
    async fn daemon_query_client_fetches_peer_snapshots_from_daemon_api() {
        let peers = vec![PeerSnapshotDto {
            peer_id: "peer-daemon".to_string(),
            device_name: Some("Daemon Peer".to_string()),
            addresses: vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            is_paired: false,
            connected: true,
            pairing_state: "NotPaired".to_string(),
        }];

        let expected_peers = peers.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            let body = serde_json::to_string(&peers).unwrap();
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

        let client = DaemonQueryClient::new(connection_state);
        with_session_cache("test-session", async move {
            let result = client.get_peers().await.unwrap();
            assert_eq!(result, expected_peers);
        })
        .await;
    }

    #[tokio::test]
    async fn daemon_query_client_fetches_paired_devices_from_daemon_api() {
        let paired_devices = vec![PairedDeviceDto {
            peer_id: "peer-daemon".to_string(),
            device_name: "Daemon Peer".to_string(),
            pairing_state: "Paired".to_string(),
            last_seen_at_ms: Some(1_718_888_000),
            connected: true,
        }];

        let expected_paired_devices = paired_devices.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 1024];
            let size = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(request.starts_with("GET /paired-devices HTTP/1.1\r\n"));
            // After session exchange, header is "Session <session-token>".
            assert!(request.contains("authorization: Session test-session\r\n"));

            let body = serde_json::to_string(&paired_devices).unwrap();
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

        let client = DaemonQueryClient::new(connection_state);
        with_session_cache("test-session", async move {
            let result = client.get_paired_devices().await.unwrap();
            assert_eq!(result, expected_paired_devices);
        })
        .await;
    }
}

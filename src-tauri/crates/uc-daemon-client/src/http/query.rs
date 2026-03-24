use anyhow::{anyhow, Context, Result};
use reqwest::{Method, RequestBuilder};

use crate::http::authorized_daemon_request;
use crate::DaemonConnectionState;
use uc_daemon::api::types::{PairedDeviceDto, PeerSnapshotDto};

#[derive(Clone)]
pub struct DaemonQueryClient {
    http: reqwest::Client,
    connection_state: DaemonConnectionState,
}

impl DaemonQueryClient {
    pub fn new(connection_state: DaemonConnectionState) -> Self {
        Self {
            http: reqwest::Client::new(),
            connection_state,
        }
    }

    pub async fn get_peers(&self) -> Result<Vec<PeerSnapshotDto>> {
        self.get_json(Method::GET, "/peers").await
    }

    pub async fn get_paired_devices(&self) -> Result<Vec<PairedDeviceDto>> {
        self.get_json(Method::GET, "/paired-devices").await
    }

    fn authorized_request(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        authorized_daemon_request(&self.http, &self.connection_state, method, path)
    }

    async fn get_json<T>(&self, method: Method, path: &str) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = self
            .authorized_request(method, path)?
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
    use uc_daemon::api::auth::DaemonConnectionInfo;
    use uc_daemon::api::types::{PairedDeviceDto, PeerSnapshotDto};

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
            token: "test-token".to_string(),
        });

        let client = DaemonQueryClient::new(connection_state);
        let result = client.get_peers().await.unwrap();

        assert_eq!(result, expected_peers);
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
            assert!(request.contains("authorization: Bearer test-token\r\n"));

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
            token: "test-token".to_string(),
        });

        let client = DaemonQueryClient::new(connection_state);
        let result = client.get_paired_devices().await.unwrap();

        assert_eq!(result, expected_paired_devices);
    }
}

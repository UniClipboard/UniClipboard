use anyhow::{Context, Result};
use reqwest::{Method, RequestBuilder};

use crate::http::authorized_daemon_request;
use crate::DaemonConnectionState;
use uc_daemon::api::types::{
    SetupActionAckResponse, SetupSelectPeerRequest, SetupStateResponse,
    SetupSubmitPassphraseRequest,
};

#[derive(Clone)]
pub struct DaemonSetupClient {
    http: reqwest::Client,
    connection_state: DaemonConnectionState,
}

impl DaemonSetupClient {
    pub fn new(connection_state: DaemonConnectionState) -> Self {
        Self {
            http: reqwest::Client::new(),
            connection_state,
        }
    }

    pub async fn get_setup_state(&self) -> Result<SetupStateResponse> {
        self.send_json::<(), SetupStateResponse>(Method::GET, "/setup/state", None)
            .await
    }

    pub async fn start_new_space(&self) -> Result<SetupActionAckResponse> {
        self.send_json::<(), SetupActionAckResponse>(Method::POST, "/setup/host", None)
            .await
    }

    pub async fn start_join_space(&self) -> Result<SetupActionAckResponse> {
        self.send_json::<(), SetupActionAckResponse>(Method::POST, "/setup/join", None)
            .await
    }

    pub async fn select_device(&self, peer_id: String) -> Result<SetupActionAckResponse> {
        self.send_json(
            Method::POST,
            "/setup/select-peer",
            Some(&SetupSelectPeerRequest { peer_id }),
        )
        .await
    }

    pub async fn confirm_peer_trust(&self) -> Result<SetupActionAckResponse> {
        self.send_json::<(), SetupActionAckResponse>(Method::POST, "/setup/confirm-peer", None)
            .await
    }

    pub async fn submit_passphrase(&self, passphrase: String) -> Result<SetupActionAckResponse> {
        self.send_json(
            Method::POST,
            "/setup/submit-passphrase",
            Some(&SetupSubmitPassphraseRequest { passphrase }),
        )
        .await
    }

    pub async fn cancel_setup(&self) -> Result<SetupActionAckResponse> {
        self.send_json::<(), SetupActionAckResponse>(Method::POST, "/setup/cancel", None)
            .await
    }

    fn authorized_request(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        authorized_daemon_request(&self.http, &self.connection_state, method, path)
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
        let request = self.authorized_request(method, path)?;
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
    use uc_daemon::api::auth::DaemonConnectionInfo;
    use uc_daemon::api::types::{SetupActionAckResponse, SetupStateResponse};

    use super::*;
    use crate::DaemonConnectionState;

    #[tokio::test]
    async fn daemon_setup_client_fetches_setup_state_from_daemon_api() {
        let expected = SetupStateResponse {
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

        let expected_response = expected.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 2048];
            let size = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(request.starts_with("GET /setup/state HTTP/1.1\r\n"));
            assert!(request.contains("authorization: Bearer test-token\r\n"));

            let body = serde_json::to_string(&expected_response).unwrap();
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

        let client = DaemonSetupClient::new(connection_state);
        let result = client.get_setup_state().await.unwrap();

        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn daemon_setup_client_posts_submit_passphrase_to_daemon_api() {
        let expected = SetupActionAckResponse {
            state: serde_json::json!({
                "JoinSpaceConfirmPeer": {
                    "shortCode": "123456"
                }
            }),
            session_id: Some("session-2".to_string()),
            next_step_hint: "host-confirm-peer".to_string(),
        };

        let expected_response = expected.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let size = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(request.starts_with("POST /setup/submit-passphrase HTTP/1.1\r\n"));
            assert!(request.contains("authorization: Bearer test-token\r\n"));
            assert!(request.contains("\r\n\r\n{\"passphrase\":\"secret-passphrase\"}"));

            let body = serde_json::to_string(&expected_response).unwrap();
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

        let client = DaemonSetupClient::new(connection_state);
        let result = client
            .submit_passphrase("secret-passphrase".to_string())
            .await
            .unwrap();

        assert_eq!(result, expected);
    }
}

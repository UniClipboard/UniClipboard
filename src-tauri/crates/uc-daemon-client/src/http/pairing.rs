use anyhow::{anyhow, Context, Result};
use reqwest::{Method, RequestBuilder, StatusCode};

use crate::http::authorized_daemon_request;
use crate::DaemonConnectionState;
use uc_daemon::api::pairing::{
    AckedPairingCommandResponse, InitiatePairingRequest, InitiatePairingResponse,
    PairingApiErrorResponse, PairingGuiLeaseRequest, PairingSessionCommandRequest,
    SetPairingDiscoverabilityRequest, SetPairingParticipantRequest, UnpairDeviceRequest,
    VerifyPairingRequest,
};

#[derive(Clone)]
pub struct DaemonPairingClient {
    http: reqwest::Client,
    connection_state: DaemonConnectionState,
}

#[derive(Debug, Clone)]
pub struct DaemonPairingRequestError {
    pub path: String,
    pub status: StatusCode,
    pub code: Option<String>,
    pub message: String,
}

impl std::fmt::Display for DaemonPairingRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(code) = self.code.as_deref() {
            write!(
                f,
                "daemon pairing request {} failed with status {} [{}]: {}",
                self.path, self.status, code, self.message
            )
        } else {
            write!(
                f,
                "daemon pairing request {} failed with status {}: {}",
                self.path, self.status, self.message
            )
        }
    }
}

impl std::error::Error for DaemonPairingRequestError {}

impl DaemonPairingClient {
    pub fn new(connection_state: DaemonConnectionState) -> Self {
        Self {
            http: reqwest::Client::new(),
            connection_state,
        }
    }

    pub async fn initiate_pairing(&self, peer_id: String) -> Result<InitiatePairingResponse> {
        self.send_json(
            Method::POST,
            "/pairing/initiate",
            Some(&InitiatePairingRequest { peer_id }),
        )
        .await
    }

    pub async fn accept_pairing(&self, session_id: &str) -> Result<()> {
        self.send_json_no_content(
            Method::POST,
            "/pairing/accept",
            &PairingSessionCommandRequest {
                session_id: session_id.to_string(),
            },
        )
        .await
    }

    pub async fn reject_pairing(&self, session_id: &str) -> Result<()> {
        self.send_json_no_content(
            Method::POST,
            "/pairing/reject",
            &PairingSessionCommandRequest {
                session_id: session_id.to_string(),
            },
        )
        .await
    }

    pub async fn cancel_pairing(&self, session_id: &str) -> Result<()> {
        self.send_json_no_content(
            Method::POST,
            "/pairing/cancel",
            &PairingSessionCommandRequest {
                session_id: session_id.to_string(),
            },
        )
        .await
    }

    pub async fn verify_pairing(
        &self,
        session_id: &str,
        pin_matches: bool,
    ) -> Result<AckedPairingCommandResponse> {
        self.send_json(
            Method::POST,
            &format!("/pairing/sessions/{session_id}/verify"),
            Some(&VerifyPairingRequest { pin_matches }),
        )
        .await
    }

    pub async fn register_gui_participant(
        &self,
        enabled: bool,
        lease_ttl_ms: Option<u64>,
    ) -> Result<()> {
        self.send_json_no_content(
            Method::POST,
            "/pairing/gui/lease",
            &PairingGuiLeaseRequest {
                enabled,
                lease_ttl_ms,
            },
        )
        .await
    }

    pub async fn set_pairing_discoverability(
        &self,
        client_kind: &str,
        discoverable: bool,
        lease_ttl_ms: Option<u64>,
    ) -> Result<AckedPairingCommandResponse> {
        self.send_json(
            Method::PUT,
            "/pairing/discoverability/current",
            Some(&SetPairingDiscoverabilityRequest {
                client_kind: client_kind.to_string(),
                discoverable,
                lease_ttl_ms,
            }),
        )
        .await
    }

    pub async fn set_pairing_participant_ready(
        &self,
        client_kind: &str,
        ready: bool,
        lease_ttl_ms: Option<u64>,
    ) -> Result<AckedPairingCommandResponse> {
        self.send_json(
            Method::PUT,
            "/pairing/participants/current",
            Some(&SetPairingParticipantRequest {
                client_kind: client_kind.to_string(),
                ready,
                lease_ttl_ms,
            }),
        )
        .await
    }

    pub async fn unpair_device(&self, peer_id: String) -> Result<()> {
        self.send_json_no_content(
            Method::POST,
            "/pairing/unpair",
            &UnpairDeviceRequest { peer_id },
        )
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
            .with_context(|| format!("failed to call daemon pairing route {path}"))?;

        Self::decode_json_response(response, path).await
    }

    async fn send_json_no_content<T: serde::Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        payload: &T,
    ) -> Result<()> {
        let request = self.authorized_request(method, path)?;
        let response = request
            .json(payload)
            .send()
            .await
            .with_context(|| format!("failed to call daemon pairing route {path}"))?;

        Self::decode_no_content_response(response, path).await
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
                .with_context(|| format!("failed to decode daemon pairing response for {path}"));
        }

        Err(Self::decode_error_response(response, path).await)
    }

    async fn decode_no_content_response(response: reqwest::Response, path: &str) -> Result<()> {
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }

        Err(Self::decode_error_response(response, path).await)
    }

    async fn decode_error_response(response: reqwest::Response, path: &str) -> anyhow::Error {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unreadable response body>".to_string());
        let maybe_error = serde_json::from_str::<PairingApiErrorResponse>(&body).ok();
        let error = DaemonPairingRequestError {
            path: path.to_string(),
            status,
            code: maybe_error.as_ref().map(|error| error.code.clone()),
            message: maybe_error.map(|error| error.message).unwrap_or(body),
        };

        anyhow!(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    use reqwest::header::AUTHORIZATION;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use uc_daemon::api::auth::DaemonConnectionInfo;

    #[test]
    fn authorized_request_builds_bearer_header() {
        let connection_state = DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: "http://127.0.0.1:42715".to_string(),
            ws_url: "ws://127.0.0.1:42715/ws".to_string(),
            token: "test-token".to_string(),
        });
        let client = DaemonPairingClient::new(connection_state);

        let request = client
            .authorized_request(Method::POST, "/pairing/initiate")
            .expect("request should build")
            .build()
            .expect("request should be valid");

        let auth_header = request
            .headers()
            .get(AUTHORIZATION)
            .expect("authorization header should exist")
            .to_str()
            .expect("authorization header should be utf-8");
        assert_eq!(auth_header, "Bearer test-token");
    }

    #[tokio::test]
    async fn daemon_pairing_client_posts_unpair_to_daemon_api() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let size = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(request.starts_with("POST /pairing/unpair HTTP/1.1\r\n"));
            assert!(request.contains("authorization: Bearer test-token\r\n"));
            assert!(request.contains("\r\n\r\n{\"peerId\":\"peer-daemon\"}"));

            let response = "HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n";
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let connection_state = DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-token".to_string(),
        });

        let client = DaemonPairingClient::new(connection_state);
        client
            .unpair_device("peer-daemon".to_string())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn daemon_pairing_client_posts_gui_lease_request_to_daemon_api() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 4096];
            let size = stream.read(&mut request).await.unwrap();
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(request.starts_with("POST /pairing/gui/lease HTTP/1.1\r\n"));
            assert!(request.contains("authorization: Bearer test-token\r\n"));
            assert!(request.contains("\r\n\r\n{\"enabled\":true,\"leaseTtlMs\":300000}"));

            let response = "HTTP/1.1 204 No Content\r\ncontent-length: 0\r\n\r\n";
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let connection_state = DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: format!("http://{addr}"),
            ws_url: format!("ws://{addr}/ws"),
            token: "test-token".to_string(),
        });

        let client = DaemonPairingClient::new(connection_state);
        client
            .register_gui_participant(true, Some(300_000))
            .await
            .unwrap();
    }
}

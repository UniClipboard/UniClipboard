use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::{Method, RequestBuilder};

use crate::http::authorized_daemon_request_with_type;
use crate::DaemonConnectionState;
use uc_daemon_contract::api::dto::device::LocalDeviceInfoDto;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::types::{
    PeerSnapshotDto, PresenceRefreshResponse, SpaceMemberDto, StatusResponse,
};

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

    pub async fn get_paired_devices(&self) -> Result<Vec<SpaceMemberDto>> {
        self.get_json(Method::GET, "/paired-devices").await
    }

    pub async fn get_status(&self) -> Result<StatusResponse> {
        self.get_json(Method::GET, "/status").await
    }

    pub async fn get_local_device_info(&self) -> Result<LocalDeviceInfoDto> {
        self.get_json(Method::GET, "/device/me").await
    }

    pub async fn refresh_presence(&self) -> Result<PresenceRefreshResponse> {
        self.get_json(Method::POST, "/presence/refresh").await
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

    /// GET a query route whose body is the canonical `ApiEnvelope<T>` (ADR-008 §H:
    /// `/peers`, `/paired-devices`, `/status` are all enveloped now). Decodes
    /// `{ data: T, ts }` and returns the unwrapped `T`; the public method return
    /// types (`Vec<PeerSnapshotDto>`, `Vec<SpaceMemberDto>`, `StatusResponse`)
    /// stay the inner payload.
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
            let envelope = response
                .json::<ApiEnvelope<T>>()
                .await
                .with_context(|| format!("failed to decode daemon query response for {path}"))?;
            return Ok(envelope.data);
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

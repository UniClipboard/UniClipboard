use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::dto::device::LocalDeviceInfoDto;
use uc_daemon_contract::api::types::{
    PeerSnapshotDto, PresenceRefreshResponse, SpaceMemberDto, StatusResponse,
};

use crate::http::enveloped::{empty_request, enveloped_request};
use crate::DaemonConnectionState;

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
        self.enveloped(Method::GET, "/peers").await
    }

    pub async fn get_paired_devices(&self) -> Result<Vec<SpaceMemberDto>> {
        self.enveloped(Method::GET, "/paired-devices").await
    }

    pub async fn get_status(&self) -> Result<StatusResponse> {
        self.enveloped(Method::GET, "/status").await
    }

    pub async fn get_local_device_info(&self) -> Result<LocalDeviceInfoDto> {
        self.enveloped(Method::GET, "/device/me").await
    }

    pub async fn refresh_presence(&self) -> Result<PresenceRefreshResponse> {
        self.enveloped(Method::POST, "/presence/refresh").await
    }

    /// Unlock the encryption session via the daemon keyring (auto-unlock).
    ///
    /// Decodes the payload tolerantly (`serde_json::Value`): older daemons may
    /// omit `success`, which defaults to `true`.
    pub async fn unlock_encryption(&self) -> Result<bool> {
        let data: serde_json::Value = self.enveloped(Method::POST, "/encryption/unlock").await?;
        Ok(data
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(true))
    }

    /// Retry the lifecycle boot on the daemon (starts network, opens clipboard capture gate).
    pub async fn lifecycle_retry(&self) -> Result<()> {
        Ok(empty_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/lifecycle/retry",
            |r| r,
        )
        .await?)
    }

    /// Signal the daemon that the GUI has unlocked and clipboard capture can begin.
    pub async fn signal_lifecycle_ready(&self) -> Result<()> {
        Ok(empty_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/lifecycle/ready",
            |r| r,
        )
        .await?)
    }

    /// Body-less enveloped request (ADR-008 §H: query routes are all enveloped;
    /// the public method return types stay the inner payload).
    async fn enveloped<T>(&self, method: Method, path: &str) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            method,
            path,
            |r| r,
        )
        .await?)
    }
}

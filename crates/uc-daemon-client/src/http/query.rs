use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::dto::device::LocalDeviceInfoDto;
use uc_daemon_contract::api::dto::encryption::EncryptionStateResponse;
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
    pub fn new(connection_state: DaemonConnectionState) -> Result<Self> {
        Ok(Self {
            http: Arc::new(crate::build_local_http_client()?),
            connection_state,
            client_type: "gui".to_string(),
        })
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

    /// Report a host event; the Engine owns all connection attempts and retries.
    pub async fn notify_connectivity_opportunity(
        &self,
        reason: uc_daemon_contract::api::dto::device::ConnectivityOpportunity,
    ) -> Result<()> {
        let request =
            uc_daemon_contract::api::dto::device::ConnectivityOpportunityRequest { reason };
        Ok(empty_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/presence/opportunity",
            |builder| builder.json(&request),
        )
        .await?)
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

    /// Fetch the daemon's current encryption state (`initialized` +
    /// `session_ready`).
    ///
    /// Cold-launch startup restore (issue #1169) polls this to wait until the
    /// encryption session is unlocked before reading/writing encrypted history:
    /// both the pre-restore capture-current (encrypt) and the restore itself
    /// (decrypt) fail while the session is still locked.
    pub async fn get_encryption_state(&self) -> Result<EncryptionStateResponse> {
        self.enveloped(Method::GET, "/encryption/state").await
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

#[cfg(test)]
mod connectivity_tests {
    use super::*;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;
    use uc_daemon_contract::api::dto::device::ConnectivityOpportunity;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn host_opportunities_use_authenticated_requests_without_refreshing() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/auth/connect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "sessionToken": "test-session", "expiresInSecs": 300, "refreshAtSecs": 240 }, "ts": 1
            }))).mount(&server).await;
        let state = DaemonConnectionState::default();
        state.set(DaemonConnectionInfo {
            base_url: server.uri(),
            ws_url: "ws://127.0.0.1/unused".into(),
            token: "test-bearer".into(),
            pid: 42,
        });
        let client = DaemonQueryClient::new(state).unwrap();
        for (reason, value) in [
            (ConnectivityOpportunity::Foreground, "foreground"),
            (ConnectivityOpportunity::SystemWake, "system_wake"),
            (ConnectivityOpportunity::NetworkChanged, "network_changed"),
        ] {
            Mock::given(method("POST"))
                .and(path("/presence/opportunity"))
                .and(body_json(serde_json::json!({"reason": value})))
                .respond_with(ResponseTemplate::new(204))
                .expect(1)
                .mount(&server)
                .await;
            client
                .notify_connectivity_opportunity(reason)
                .await
                .unwrap();
        }
        assert!(server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.url.path() != "/presence/refresh"));
    }
}

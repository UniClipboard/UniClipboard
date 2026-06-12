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

//! Feature-specific daemon upgrade client (ADR-008 P5).
//!
//! Provides `DaemonUpgradeClient` that sends the exact `/upgrade/status`
//! and `/upgrade/ack` transport contract without rebuilding the facade
//! logic locally.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::{Method, RequestBuilder};

use crate::http::authorized_daemon_request_with_type;
use crate::DaemonConnectionState;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::dto::upgrade::{AckUpgradePayload, UpgradeStatusDto};
use uc_daemon_contract::constants::http_route;

/// Feature-specific daemon upgrade client.
///
/// Shares connection state and HTTP client with `DaemonClientContext`.
/// Constructed via `DaemonClientContext::upgrade_client()`.
#[derive(Clone)]
pub struct DaemonUpgradeClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonUpgradeClient {
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

    /// Detect upgrade status by calling `GET /upgrade/status`.
    ///
    /// The daemon compares its running build version against the persisted
    /// cursor and returns a discriminated status DTO.
    pub async fn status(&self) -> Result<UpgradeStatusDto> {
        let path = http_route::UPGRADE_STATUS;
        let response = self
            .authorized_request(Method::GET, path)
            .await?
            .send()
            .await
            .with_context(|| format!("failed to call daemon upgrade route {path}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("upgrade status request failed ({}): {}", status, body);
        }

        let envelope = response
            .json::<ApiEnvelope<UpgradeStatusDto>>()
            .await
            .with_context(|| format!("failed to decode upgrade status response for {path}"))?;
        Ok(envelope.data)
    }

    /// Acknowledge the current daemon version by calling `POST /upgrade/ack`.
    ///
    /// Advances the version cursor to the running build. Subsequent `status`
    /// calls will report `NoChange` until the binary version moves again.
    pub async fn acknowledge(&self) -> Result<AckUpgradePayload> {
        let path = http_route::UPGRADE_ACK;
        let response = self
            .authorized_request(Method::POST, path)
            .await?
            .send()
            .await
            .with_context(|| format!("failed to call daemon upgrade route {path}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("upgrade ack request failed ({}): {}", status, body);
        }

        let envelope = response
            .json::<ApiEnvelope<AckUpgradePayload>>()
            .await
            .with_context(|| format!("failed to decode upgrade ack response for {path}"))?;
        Ok(envelope.data)
    }

    async fn authorized_request(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
        authorized_daemon_request_with_type(
            &self.http,
            &self.connection_state,
            method,
            path,
            connection.pid,
            &self.client_type,
        )
        .await
    }
}

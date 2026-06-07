use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::{Method, RequestBuilder};

use crate::http::authorized_daemon_request_with_type;
use crate::DaemonConnectionState;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::dto::v2::setup::IssueInvitationResponse;

#[derive(Clone)]
pub struct DaemonSetupV2Client {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonSetupV2Client {
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

    pub async fn issue_invitation(&self) -> Result<IssueInvitationResponse> {
        let response = self
            .authorized_request(Method::POST, "/v2/setup/issue-invitation")
            .await?
            .send()
            .await
            .with_context(|| "failed to call POST /v2/setup/issue-invitation")?;

        let status = response.status();
        if status.is_success() {
            let envelope = response
                .json::<ApiEnvelope<IssueInvitationResponse>>()
                .await
                .with_context(|| "failed to decode issue-invitation response")?;
            return Ok(envelope.data);
        }

        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow!(
            "POST /v2/setup/issue-invitation failed with status {}: {}",
            status,
            body
        ))
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

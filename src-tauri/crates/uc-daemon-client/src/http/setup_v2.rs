use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::{Method, RequestBuilder};

use crate::http::authorized_daemon_request_with_type;
use crate::DaemonConnectionState;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::dto::v2::setup::{
    InitializeSpaceRequest, InitializeSpaceResponse, IssueInvitationResponse, RedeemRequest,
    RedeemResponse, SwitchSpaceRequest, SwitchSpaceResponse,
};

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
        Err(anyhow!("{}", extract_error_message(status, &body)))
    }

    pub async fn initialize_space(
        &self,
        req: &InitializeSpaceRequest,
    ) -> Result<InitializeSpaceResponse> {
        let response = self
            .authorized_request(Method::POST, "/v2/setup/initialize")
            .await?
            .json(req)
            .send()
            .await
            .with_context(|| "failed to call POST /v2/setup/initialize")?;

        let status = response.status();
        if status.is_success() {
            let envelope = response
                .json::<ApiEnvelope<InitializeSpaceResponse>>()
                .await
                .with_context(|| "failed to decode initialize-space response")?;
            return Ok(envelope.data);
        }

        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow!("{}", extract_error_message(status, &body)))
    }

    pub async fn redeem_invitation(&self, req: &RedeemRequest) -> Result<RedeemResponse> {
        let response = self
            .authorized_request(Method::POST, "/v2/setup/redeem")
            .await?
            .json(req)
            .send()
            .await
            .with_context(|| "failed to call POST /v2/setup/redeem")?;

        let status = response.status();
        if status.is_success() {
            let envelope = response
                .json::<ApiEnvelope<RedeemResponse>>()
                .await
                .with_context(|| "failed to decode redeem response")?;
            return Ok(envelope.data);
        }

        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow!("{}", extract_error_message(status, &body)))
    }

    pub async fn switch_space(&self, req: &SwitchSpaceRequest) -> Result<SwitchSpaceResponse> {
        let response = self
            .authorized_request(Method::POST, "/v2/setup/switch-space")
            .await?
            .json(req)
            .send()
            .await
            .with_context(|| "failed to call POST /v2/setup/switch-space")?;

        let status = response.status();
        if status.is_success() {
            let envelope = response
                .json::<ApiEnvelope<SwitchSpaceResponse>>()
                .await
                .with_context(|| "failed to decode switch-space response")?;
            return Ok(envelope.data);
        }

        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow!("{}", extract_error_message(status, &body)))
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

fn extract_error_message(status: reqwest::StatusCode, body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("message")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| format!("request failed ({status}): {body}"))
}

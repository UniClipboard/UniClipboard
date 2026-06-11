use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;

use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;
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
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/v2/setup/issue-invitation",
            |r| r,
        )
        .await?)
    }

    pub async fn initialize_space(
        &self,
        req: &InitializeSpaceRequest,
    ) -> Result<InitializeSpaceResponse> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/v2/setup/initialize",
            |r| r.json(req),
        )
        .await?)
    }

    pub async fn redeem_invitation(&self, req: &RedeemRequest) -> Result<RedeemResponse> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/v2/setup/redeem",
            |r| r.json(req),
        )
        .await?)
    }

    pub async fn switch_space(&self, req: &SwitchSpaceRequest) -> Result<SwitchSpaceResponse> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/v2/setup/switch-space",
            |r| r.json(req),
        )
        .await?)
    }
}

use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::dto::diagnostics::{
    DebugStatusDto, LogExportRequestDto, LogExportResultDto, UpdateDebugModeRequestDto,
    UpdateDebugModeResultDto,
};
use uc_daemon_contract::constants::http_route;

use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;

#[derive(Clone)]
pub struct DaemonDiagnosticsClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonDiagnosticsClient {
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

    pub async fn debug_status(&self) -> Result<DebugStatusDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            http_route::DIAGNOSTICS_DEBUG,
            |r| r,
        )
        .await?)
    }

    pub async fn set_debug_mode(&self, enabled: bool) -> Result<UpdateDebugModeResultDto> {
        let req_body = UpdateDebugModeRequestDto { enabled };
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::PUT,
            http_route::DIAGNOSTICS_DEBUG,
            |r| r.json(&req_body),
        )
        .await?)
    }

    pub async fn export_logs(&self, since_hours: Option<u32>) -> Result<LogExportResultDto> {
        let req_body = LogExportRequestDto { since_hours };
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::DIAGNOSTICS_LOG_EXPORT,
            |r| r.json(&req_body),
        )
        .await?)
    }
}

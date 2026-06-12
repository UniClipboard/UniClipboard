//! Feature-specific daemon upgrade client (ADR-008 P5).
//!
//! Provides `DaemonUpgradeClient` that sends the exact `/upgrade/status`
//! and `/upgrade/ack` transport contract without rebuilding the facade
//! logic locally.

use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;

use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;
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
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            http_route::UPGRADE_STATUS,
            |r| r,
        )
        .await?)
    }

    /// Acknowledge the current daemon version by calling `POST /upgrade/ack`.
    ///
    /// Advances the version cursor to the running build. Subsequent `status`
    /// calls will report `NoChange` until the binary version moves again.
    pub async fn acknowledge(&self) -> Result<AckUpgradePayload> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::UPGRADE_ACK,
            |r| r,
        )
        .await?)
    }
}

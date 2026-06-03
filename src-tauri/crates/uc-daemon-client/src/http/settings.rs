use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::Method;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::dto::settings::{
    RelayProbeOutcomeDto, RelayProbeRequestDto, SettingsDto, SettingsPatchDto,
    SettingsUpdateResultDto,
};
use uc_daemon_contract::constants::http_route;

use crate::http::authorized_daemon_request_with_type;
use crate::DaemonConnectionState;

/// Loopback HTTP client for the daemon's `/settings` endpoints.
///
/// ADR-008 P3-3 B2': the GUI is becoming a pure client, so its Tauri settings
/// commands read/write through the daemon over loopback HTTP instead of an
/// in-process `AppFacade`. OS-side effects (autostart / shortcut registration)
/// stay native in the command; only the settings domain read/write moves here.
#[derive(Clone)]
pub struct DaemonSettingsClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonSettingsClient {
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

    /// `GET /settings` — read the current persisted settings.
    pub async fn get_settings(&self) -> Result<SettingsDto> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
        let request = authorized_daemon_request_with_type(
            &self.http,
            &self.connection_state,
            Method::GET,
            http_route::SETTINGS,
            connection.pid,
            &self.client_type,
        )
        .await?;

        let response = request
            .send()
            .await
            .context("failed to call daemon get settings")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("get settings failed ({}): {}", status, body);
        }

        // Wire shape (ADR-008 §H): `{ data: SettingsDto, ts }`.
        let envelope = response
            .json::<ApiEnvelope<SettingsDto>>()
            .await
            .context("failed to decode settings response")?;
        Ok(envelope.data)
    }

    /// `PUT /settings` — persist a settings patch. Unlike the Tauri command this
    /// applies NO OS-level side effects; the caller keeps those native.
    pub async fn update_settings(
        &self,
        patch: SettingsPatchDto,
    ) -> Result<SettingsUpdateResultDto> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
        let request = authorized_daemon_request_with_type(
            &self.http,
            &self.connection_state,
            Method::PUT,
            http_route::SETTINGS,
            connection.pid,
            &self.client_type,
        )
        .await?;

        let response = request
            .json(&patch)
            .send()
            .await
            .context("failed to call daemon update settings")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("update settings failed ({}): {}", status, body);
        }

        // Wire shape (ADR-008 §H): `{ data: SettingsUpdateResultDto, ts }`.
        let envelope = response
            .json::<ApiEnvelope<SettingsUpdateResultDto>>()
            .await
            .context("failed to decode update settings response")?;
        Ok(envelope.data)
    }

    /// `POST /settings/relay-probe` — probe a candidate relay URL. A probe that
    /// fails to reach the relay is a NORMAL categorized 200 outcome (carried on
    /// `RelayProbeOutcomeDto`), not an error; only adapter-missing / transport
    /// faults surface as `Err`.
    pub async fn probe_relay_url(&self, url: &str) -> Result<RelayProbeOutcomeDto> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
        let req_body = RelayProbeRequestDto {
            url: url.to_string(),
        };
        let request = authorized_daemon_request_with_type(
            &self.http,
            &self.connection_state,
            Method::POST,
            http_route::SETTINGS_RELAY_PROBE,
            connection.pid,
            &self.client_type,
        )
        .await?;

        let response = request
            .json(&req_body)
            .send()
            .await
            .context("failed to call daemon relay probe")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("relay probe failed ({}): {}", status, body);
        }

        // Wire shape (ADR-008 §H): `{ data: RelayProbeOutcomeDto, ts }`.
        let envelope = response
            .json::<ApiEnvelope<RelayProbeOutcomeDto>>()
            .await
            .context("failed to decode relay probe response")?;
        Ok(envelope.data)
    }
}

use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::dto::settings::{
    RelayProbeOutcomeDto, RelayProbeRequestDto, SettingsDto, SettingsPatchDto,
    SettingsUpdateResultDto,
};
use uc_daemon_contract::constants::http_route;

use crate::http::enveloped::enveloped_request;
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
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            http_route::SETTINGS,
            |r| r,
        )
        .await?)
    }

    /// `PUT /settings` — persist a settings patch. Unlike the Tauri command this
    /// applies NO OS-level side effects; the caller keeps those native.
    pub async fn update_settings(
        &self,
        patch: SettingsPatchDto,
    ) -> Result<SettingsUpdateResultDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::PUT,
            http_route::SETTINGS,
            |r| r.json(&patch),
        )
        .await?)
    }

    /// `POST /settings/relay-probe` — probe a candidate relay URL. A probe that
    /// fails to reach the relay is a NORMAL categorized 200 outcome (carried on
    /// `RelayProbeOutcomeDto`), not an error; only adapter-missing / transport
    /// faults surface as `Err`.
    pub async fn probe_relay_url(&self, url: &str) -> Result<RelayProbeOutcomeDto> {
        let req_body = RelayProbeRequestDto {
            url: url.to_string(),
        };
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::SETTINGS_RELAY_PROBE,
            |r| r.json(&req_body),
        )
        .await?)
    }
}

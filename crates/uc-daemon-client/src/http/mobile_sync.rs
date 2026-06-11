use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;

use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;
use uc_daemon_contract::api::dto::mobile_sync::{
    LanInterfaceViewDto, MobileDeviceViewDto, MobileSyncActionResultDto, MobileSyncSettingsViewDto,
    RegisterMobileDeviceRequest, RegisterMobileDeviceResultDto, UpdateMobileSyncSettingsRequest,
    UpdateMobileSyncSettingsResultDto,
};

#[derive(Clone)]
pub struct DaemonMobileSyncClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonMobileSyncClient {
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

    // ── Public API ─────────────────────────────────────────────────────

    /// GET /mobile-sync/settings
    pub async fn get_settings(&self) -> Result<MobileSyncSettingsViewDto> {
        self.enveloped(Method::GET, "/mobile-sync/settings").await
    }

    /// PATCH /mobile-sync/settings
    pub async fn update_settings(
        &self,
        req: &UpdateMobileSyncSettingsRequest,
    ) -> Result<UpdateMobileSyncSettingsResultDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::PATCH,
            "/mobile-sync/settings",
            |r| r.json(req),
        )
        .await?)
    }

    /// GET /mobile-sync/devices
    pub async fn list_devices(&self) -> Result<Vec<MobileDeviceViewDto>> {
        self.enveloped(Method::GET, "/mobile-sync/devices").await
    }

    /// POST /mobile-sync/devices
    pub async fn register_device(
        &self,
        req: &RegisterMobileDeviceRequest,
    ) -> Result<RegisterMobileDeviceResultDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/mobile-sync/devices",
            |r| r.json(req),
        )
        .await?)
    }

    /// DELETE /mobile-sync/devices/{device_id}
    pub async fn revoke_device(&self, device_id: &str) -> Result<MobileSyncActionResultDto> {
        let path = format!("/mobile-sync/devices/{device_id}");
        self.enveloped(Method::DELETE, &path).await
    }

    /// GET /mobile-sync/lan-interfaces
    pub async fn list_lan_interfaces(&self) -> Result<Vec<LanInterfaceViewDto>> {
        self.enveloped(Method::GET, "/mobile-sync/lan-interfaces")
            .await
    }

    // ── Private helpers ────────────────────────────────────────────────

    /// Body-less enveloped request.
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

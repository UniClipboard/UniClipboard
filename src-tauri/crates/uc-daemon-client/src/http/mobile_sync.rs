use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::{Method, RequestBuilder};

use crate::http::authorized_daemon_request_with_type;
use crate::DaemonConnectionState;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
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
        self.get_json("/mobile-sync/settings").await
    }

    /// PATCH /mobile-sync/settings
    pub async fn update_settings(
        &self,
        req: &UpdateMobileSyncSettingsRequest,
    ) -> Result<UpdateMobileSyncSettingsResultDto> {
        self.request_json(Method::PATCH, "/mobile-sync/settings", req)
            .await
    }

    /// GET /mobile-sync/devices
    pub async fn list_devices(&self) -> Result<Vec<MobileDeviceViewDto>> {
        self.get_json("/mobile-sync/devices").await
    }

    /// POST /mobile-sync/devices
    pub async fn register_device(
        &self,
        req: &RegisterMobileDeviceRequest,
    ) -> Result<RegisterMobileDeviceResultDto> {
        self.request_json(Method::POST, "/mobile-sync/devices", req)
            .await
    }

    /// DELETE /mobile-sync/devices/{device_id}
    pub async fn revoke_device(&self, device_id: &str) -> Result<MobileSyncActionResultDto> {
        let path = format!("/mobile-sync/devices/{device_id}");
        let response = self
            .authorized_request(Method::DELETE, &path)
            .await?
            .send()
            .await
            .with_context(|| format!("failed to call DELETE {path}"))?;

        let status = response.status();
        if status.is_success() {
            let envelope = response
                .json::<ApiEnvelope<MobileSyncActionResultDto>>()
                .await
                .with_context(|| format!("failed to decode response for DELETE {path}"))?;
            return Ok(envelope.data);
        }

        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow!("{}", extract_error_message(status, &body)))
    }

    /// GET /mobile-sync/lan-interfaces
    pub async fn list_lan_interfaces(&self) -> Result<Vec<LanInterfaceViewDto>> {
        self.get_json("/mobile-sync/lan-interfaces").await
    }

    // ── Private helpers ────────────────────────────────────────────────

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

    /// GET helper: send request, unwrap `ApiEnvelope<T>`.
    async fn get_json<T>(&self, path: &str) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = self
            .authorized_request(Method::GET, path)
            .await?
            .send()
            .await
            .with_context(|| format!("failed to call GET {path}"))?;

        let status = response.status();
        if status.is_success() {
            let envelope = response
                .json::<ApiEnvelope<T>>()
                .await
                .with_context(|| format!("failed to decode response for GET {path}"))?;
            return Ok(envelope.data);
        }

        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow!("{}", extract_error_message(status, &body)))
    }

    /// Body-bearing request helper: attach `.json(body)`, send, unwrap `ApiEnvelope<T>`.
    async fn request_json<B, T>(&self, method: Method, path: &str, body: &B) -> Result<T>
    where
        B: serde::Serialize,
        T: serde::de::DeserializeOwned,
    {
        let response = self
            .authorized_request(method.clone(), path)
            .await?
            .json(body)
            .send()
            .await
            .with_context(|| format!("failed to call {method} {path}"))?;

        let status = response.status();
        if status.is_success() {
            let envelope = response
                .json::<ApiEnvelope<T>>()
                .await
                .with_context(|| format!("failed to decode response for {method} {path}"))?;
            return Ok(envelope.data);
        }

        let body_text = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow!("{}", extract_error_message(status, &body_text)))
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

//! Loopback HTTP client for the daemon's `/config/*` configuration-migration
//! endpoints (issue #1110: portable ↔ installer config import/export).
//!
//! The GUI is a pure client of the daemon, so its Tauri config commands drive
//! export / preview / staged-import through these enveloped requests. Request
//! bodies carry the bundle password and MUST never be logged.

use std::sync::Arc;

use reqwest::Method;
use uc_daemon_contract::api::dto::config::{
    ExportConfigRequest, ExportConfigResponse, ImportConfigRequest, ImportConfigResponse,
    PreviewImportRequest, PreviewImportResponse,
};
use uc_daemon_contract::constants::http_route;

use crate::http::enveloped::{enveloped_request, DaemonRequestError};
use crate::DaemonConnectionState;

/// Loopback HTTP client for the daemon's `/config/*` endpoints.
#[derive(Clone)]
pub struct DaemonConfigClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonConfigClient {
    pub fn new(connection_state: DaemonConnectionState) -> anyhow::Result<Self> {
        Ok(Self {
            http: Arc::new(crate::build_local_http_client()?),
            connection_state,
            client_type: "gui".to_string(),
        })
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

    /// `POST /config/export` — package the current configuration into an
    /// encrypted `.ucbundle` at `target_path`. Requires an unlocked session; the
    /// bundle is sealed with the installation's own key material (no export
    /// password). Returns the absolute path the bundle was written to.
    pub async fn export(
        &self,
        target_path: String,
    ) -> Result<ExportConfigResponse, DaemonRequestError> {
        let body = ExportConfigRequest { target_path };
        enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::CONFIG_EXPORT,
            |r| r.json(&body),
        )
        .await
    }

    /// `POST /config/import/preview` — decrypt only the bundle manifest and
    /// return its non-secret descriptive metadata for operator confirmation.
    pub async fn preview_import(
        &self,
        password: String,
        source_path: String,
    ) -> Result<PreviewImportResponse, DaemonRequestError> {
        let body = PreviewImportRequest {
            password,
            source_path,
        };
        enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::CONFIG_IMPORT_PREVIEW,
            |r| r.json(&body),
        )
        .await
    }

    /// `POST /config/import` — validate the bundle and stage it for the next
    /// boot to apply. `confirmed` must be `true`; the import is a device-identity
    /// move (see issue #1110 §2.0). Staging only — the caller restarts the
    /// daemon afterwards to apply.
    pub async fn import(
        &self,
        password: String,
        source_path: String,
        confirmed: bool,
    ) -> Result<ImportConfigResponse, DaemonRequestError> {
        let body = ImportConfigRequest {
            password,
            source_path,
            confirmed,
        };
        enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::CONFIG_IMPORT,
            |r| r.json(&body),
        )
        .await
    }
}

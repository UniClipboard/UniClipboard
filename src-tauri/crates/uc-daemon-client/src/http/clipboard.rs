use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::Method;
use uc_daemon_contract::api::dto::clipboard_command::{
    CancelTransferRequest, CancelTransferResponse, DispatchOutcomeResponse, DispatchTextRequest,
    ResendRequest, ResendResponse,
};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::constants::http_route;

use crate::http::authorized_daemon_request_with_type;
use crate::service::FileExport;
use crate::DaemonConnectionState;

#[derive(Clone)]
pub struct DaemonClipboardClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonClipboardClient {
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

    /// Restore a clipboard entry to the OS clipboard via daemon.
    /// Returns Ok(()) on success. The daemon handles origin tracking; no outbound sync
    /// occurs because CaptureClipboardUseCase skips capture for LocalRestore origin.
    pub async fn restore_clipboard_entry(&self, entry_id: &str) -> Result<()> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
        let path = format!(
            "{}/{entry_id}",
            uc_daemon_contract::constants::http_route::CLIPBOARD_RESTORE
        );
        let request = authorized_daemon_request_with_type(
            &*self.http,
            &self.connection_state,
            Method::POST,
            &path,
            connection.pid,
            &self.client_type,
        )
        .await?;

        let response = request
            .send()
            .await
            .with_context(|| format!("failed to call daemon clipboard restore route {path}"))?;
        let status = response.status();

        if status.is_success() {
            return Ok(());
        }

        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());

        // Use structured error prefix so callers can distinguish 404 from other errors
        // without parsing free-form text (F-2 round 3).
        if status == reqwest::StatusCode::NOT_FOUND {
            Err(anyhow::anyhow!("[NOT_FOUND] {path}: {body}"))
        } else {
            Err(anyhow::anyhow!(
                "daemon clipboard restore request {path} failed with status {}: {}",
                status,
                body
            ))
        }
    }

    pub async fn dispatch_text(
        &self,
        text: &str,
        peers: Option<Vec<String>>,
    ) -> Result<DispatchOutcomeResponse> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
        let req_body = DispatchTextRequest {
            text: text.to_string(),
            peers,
        };
        let request = authorized_daemon_request_with_type(
            &*self.http,
            &self.connection_state,
            Method::POST,
            http_route::CLIPBOARD_DISPATCH,
            connection.pid,
            &self.client_type,
        )
        .await?;

        let response = request
            .json(&req_body)
            .send()
            .await
            .context("failed to call daemon clipboard dispatch")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("clipboard dispatch failed ({}): {}", status, body);
        }

        // Wire shape (ADR-008 §H): `{ data: DispatchOutcomeResponse, ts }`.
        // Unwrap the envelope; the public return type stays `DispatchOutcomeResponse`.
        let envelope = response
            .json::<ApiEnvelope<DispatchOutcomeResponse>>()
            .await
            .context("failed to decode dispatch response")?;
        Ok(envelope.data)
    }

    pub async fn resend_entry(
        &self,
        entry_id: &str,
        peers: Option<Vec<String>>,
    ) -> Result<ResendResponse> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
        let req_body = ResendRequest {
            entry_id: entry_id.to_string(),
            peers,
        };
        let request = authorized_daemon_request_with_type(
            &*self.http,
            &self.connection_state,
            Method::POST,
            http_route::CLIPBOARD_RESEND,
            connection.pid,
            &self.client_type,
        )
        .await?;

        let response = request
            .json(&req_body)
            .send()
            .await
            .context("failed to call daemon clipboard resend")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("clipboard resend failed ({}): {}", status, body);
        }

        // Wire shape (ADR-008 §H): `{ data: ResendResponse, ts }`.
        let envelope = response
            .json::<ApiEnvelope<ResendResponse>>()
            .await
            .context("failed to decode resend response")?;
        Ok(envelope.data)
    }

    pub async fn cancel_transfer(
        &self,
        transfer_id: &str,
        reason: &str,
    ) -> Result<CancelTransferResponse> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
        let path = format!("{}/{transfer_id}", http_route::CLIPBOARD_CANCEL_TRANSFER);
        let req_body = CancelTransferRequest {
            reason: reason.to_string(),
        };
        let request = authorized_daemon_request_with_type(
            &*self.http,
            &self.connection_state,
            Method::POST,
            &path,
            connection.pid,
            &self.client_type,
        )
        .await?;

        let response = request
            .json(&req_body)
            .send()
            .await
            .context("failed to call daemon cancel transfer")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("cancel transfer failed ({}): {}", status, body);
        }

        // Wire shape (ADR-008 §H): `{ data: CancelTransferResponse, ts }`.
        let envelope = response
            .json::<ApiEnvelope<CancelTransferResponse>>()
            .await
            .context("failed to decode cancel transfer response")?;
        Ok(envelope.data)
    }

    /// Export an entry's first materialized free-file (ADR-008 P5-1b).
    ///
    /// Calls the binary endpoint `GET /clipboard/entries/{id}/file`, which is
    /// exempt from the JSON envelope (raw bytes + `Content-Disposition`).
    /// Returns `Ok(Some(_))` on 200, `Ok(None)` on 404 (no materialized file),
    /// and `Err` on any other status / transport failure. The filename comes
    /// from `Content-Disposition`; it falls back to `entry_id` when the header
    /// is absent or unparseable.
    pub async fn export_entry_file(&self, entry_id: &str) -> Result<Option<FileExport>> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
        // The entry-file endpoint is `GET /clipboard/entries/:id/file`; build it
        // off the shared entries base so the route lives in one place.
        let path = format!("{}/{entry_id}/file", http_route::CLIPBOARD_ENTRIES);
        let request = authorized_daemon_request_with_type(
            &self.http,
            &self.connection_state,
            Method::GET,
            &path,
            connection.pid,
            &self.client_type,
        )
        .await?;

        let response = request
            .send()
            .await
            .with_context(|| format!("failed to call daemon entry-file route {path}"))?;
        let status = response.status();

        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("entry-file export failed ({}): {}", status, body);
        }

        let filename = response
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .and_then(filename_from_content_disposition)
            .unwrap_or_else(|| entry_id.to_string());

        let bytes = response
            .bytes()
            .await
            .context("failed to read entry-file bytes")?
            .to_vec();

        Ok(Some(FileExport { filename, bytes }))
    }
}

/// Extract the `filename="..."` (or bare `filename=...`) value from a
/// `Content-Disposition` header. Returns `None` when no usable filename token
/// is present. The basename is already sanitized on the daemon side; callers
/// still sanitize again before touching the filesystem.
fn filename_from_content_disposition(header: &str) -> Option<String> {
    for part in header.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("filename=") {
            let value = rest.trim().trim_matches('"').to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_filename() {
        assert_eq!(
            filename_from_content_disposition("attachment; filename=\"report.pdf\""),
            Some("report.pdf".to_string())
        );
    }

    #[test]
    fn parses_bare_filename() {
        assert_eq!(
            filename_from_content_disposition("attachment; filename=data.bin"),
            Some("data.bin".to_string())
        );
    }

    #[test]
    fn returns_none_without_filename() {
        assert_eq!(filename_from_content_disposition("attachment"), None);
    }
}

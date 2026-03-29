use anyhow::{Context, Result};
use reqwest::Method;

use crate::http::authorized_daemon_request;
use crate::DaemonConnectionState;

#[derive(Clone)]
pub struct DaemonClipboardClient {
    http: reqwest::Client,
    connection_state: DaemonConnectionState,
}

impl DaemonClipboardClient {
    pub fn new(connection_state: DaemonConnectionState) -> Self {
        Self {
            http: reqwest::Client::new(),
            connection_state,
        }
    }

    /// Restore a clipboard entry to the OS clipboard via daemon.
    /// Returns Ok(()) on success. The daemon handles origin tracking; no outbound sync
    /// occurs because CaptureClipboardUseCase skips capture for LocalRestore origin.
    pub async fn restore_clipboard_entry(&self, entry_id: &str) -> Result<()> {
        let path = format!(
            "{}/{entry_id}",
            uc_core::network::daemon_api_strings::http_route::CLIPBOARD_RESTORE
        );
        let request = authorized_daemon_request(
            &self.http,
            &self.connection_state,
            Method::POST,
            &path,
        )?;

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
}

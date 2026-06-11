use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::dto::clipboard::{
    EntryDetailDto, EntryProjectionResponseDto, EntryResourceDto,
};
use uc_daemon_contract::api::dto::clipboard_command::{
    CancelTransferRequest, CancelTransferResponse, DispatchOutcomeResponse, DispatchTextRequest,
    ResendRequest, ResendResponse,
};
use uc_daemon_contract::constants::http_route;

use crate::http::enveloped::{empty_request, enveloped_request, send_checked, DaemonRequestError};
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
    /// A 404 surfaces as `DaemonRequestError::Status` (downcastable from anyhow).
    pub async fn restore_clipboard_entry(&self, entry_id: &str) -> Result<()> {
        let path = format!("{}/{entry_id}", http_route::CLIPBOARD_RESTORE);
        empty_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            &path,
            |r| r,
        )
        .await?;
        Ok(())
    }

    pub async fn dispatch_text(
        &self,
        text: &str,
        peers: Option<Vec<String>>,
    ) -> Result<DispatchOutcomeResponse> {
        let req_body = DispatchTextRequest {
            text: text.to_string(),
            peers,
        };
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::CLIPBOARD_DISPATCH,
            |r| r.json(&req_body),
        )
        .await?)
    }

    pub async fn resend_entry(
        &self,
        entry_id: &str,
        peers: Option<Vec<String>>,
    ) -> Result<ResendResponse> {
        let req_body = ResendRequest {
            entry_id: entry_id.to_string(),
            peers,
        };
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::CLIPBOARD_RESEND,
            |r| r.json(&req_body),
        )
        .await?)
    }

    pub async fn cancel_transfer(
        &self,
        transfer_id: &str,
        reason: &str,
    ) -> Result<CancelTransferResponse> {
        let path = format!("{}/{transfer_id}", http_route::CLIPBOARD_CANCEL_TRANSFER);
        let req_body = CancelTransferRequest {
            reason: reason.to_string(),
        };
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            &path,
            |r| r.json(&req_body),
        )
        .await?)
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
        // The entry-file endpoint is `GET /clipboard/entries/:id/file`; build it
        // off the shared entries base so the route lives in one place.
        let path = format!("{}/{entry_id}/file", http_route::CLIPBOARD_ENTRIES);
        let response = match send_checked(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            &path,
            |r| r,
        )
        .await
        {
            Ok(response) => response,
            Err(err) if err.is_not_found() => return Ok(None),
            Err(err) => return Err(err.into()),
        };

        let filename = response
            .headers()
            .get(reqwest::header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .and_then(filename_from_content_disposition)
            .unwrap_or_else(|| entry_id.to_string());

        let bytes = response
            .bytes()
            .await
            .map_err(|source| DaemonRequestError::Decode { path, source })?
            .to_vec();

        Ok(Some(FileExport { filename, bytes }))
    }

    /// List clipboard history entry projections, newest first.
    ///
    /// Calls `GET /clipboard/entries?limit={limit}&offset={offset}`, which
    /// returns the canonical `ApiEnvelope<Vec<EntryProjectionResponseDto>>`.
    /// The daemon clamps `limit` to 1000. This is the real-time list view (not
    /// the search index), so it is the reliable source for "the latest synced
    /// entry" on a headless node.
    pub async fn list_entries(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<EntryProjectionResponseDto>> {
        let path = format!(
            "{}?limit={limit}&offset={offset}",
            http_route::CLIPBOARD_ENTRIES
        );
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            &path,
            |r| r,
        )
        .await?)
    }

    /// Fetch an entry's full text detail (ADR-008 §H envelope).
    ///
    /// Calls `GET /clipboard/entries/{id}`. Returns `Ok(Some(_))` on 200,
    /// `Ok(None)` on 404 (no such entry), and `Err` for any other status
    /// (notably 422 when the entry is not text content) or transport failure.
    pub async fn entry_detail(&self, entry_id: &str) -> Result<Option<EntryDetailDto>> {
        let path = format!("{}/{entry_id}", http_route::CLIPBOARD_ENTRIES);
        match enveloped_request::<EntryDetailDto>(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            &path,
            |r| r,
        )
        .await
        {
            Ok(detail) => Ok(Some(detail)),
            Err(err) if err.is_not_found() => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Fetch an entry's resource metadata (blob pointer or inline data).
    ///
    /// Calls `GET /clipboard/entries/{id}/resource`. Used to materialize image
    /// entries (which are NOT free-files): the returned DTO carries either
    /// `inline_data` (base64) for small images stored inline, or a `blob_id`
    /// to fetch the bytes with [`fetch_blob`](Self::fetch_blob). Returns
    /// `Ok(None)` on 404.
    pub async fn entry_resource(&self, entry_id: &str) -> Result<Option<EntryResourceDto>> {
        let path = format!("{}/{entry_id}/resource", http_route::CLIPBOARD_ENTRIES);
        match enveloped_request::<EntryResourceDto>(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            &path,
            |r| r,
        )
        .await
        {
            Ok(resource) => Ok(Some(resource)),
            Err(err) if err.is_not_found() => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    /// Fetch raw blob bytes by blob id.
    ///
    /// Calls the binary endpoint `GET /clipboard/blobs/{blob_id}` (raw bytes,
    /// exempt from the JSON envelope). Returns `Ok(Some(_))` on 200,
    /// `Ok(None)` on 404, and `Err` on any other status / transport failure.
    pub async fn fetch_blob(&self, blob_id: &str) -> Result<Option<Vec<u8>>> {
        let path = format!("{}/{blob_id}", http_route::CLIPBOARD_BLOBS);
        let response = match send_checked(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            &path,
            |r| r,
        )
        .await
        {
            Ok(response) => response,
            Err(err) if err.is_not_found() => return Ok(None),
            Err(err) => return Err(err.into()),
        };

        let bytes = response
            .bytes()
            .await
            .map_err(|source| DaemonRequestError::Decode { path, source })?
            .to_vec();
        Ok(Some(bytes))
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

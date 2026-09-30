use std::time::Duration;

use base64::Engine;
use quick_panel_core::ports::{ImagePayload, PreviewData, ServiceError};

use super::DaemonHistory;

const PREVIEW_TIMEOUT: Duration = Duration::from_secs(10);

impl DaemonHistory {
    pub(super) async fn load_preview(
        &self,
        id: String,
        kind: String,
    ) -> Result<PreviewData, ServiceError> {
        tokio::time::timeout(PREVIEW_TIMEOUT, self.load_preview_inner(id, kind))
            .await
            .map_err(|_| ServiceError::PreviewTimeout)?
    }

    async fn load_preview_inner(
        &self,
        id: String,
        kind: String,
    ) -> Result<PreviewData, ServiceError> {
        let client = self.context()?.clipboard_client();
        if kind == "text" || kind == "richtext" {
            let detail = client
                .entry_detail(&id)
                .await
                .map_err(|_| ServiceError::PreviewUnreadable)?
                .ok_or(ServiceError::EntryGone)?;
            return Ok(PreviewData {
                text: Some(detail.content),
                image: None,
                size: detail.size_bytes,
            });
        }
        if kind == "image" {
            let resource = client
                .entry_resource(&id)
                .await
                .map_err(|_| ServiceError::ImageUnreadable)?
                .ok_or(ServiceError::ImageGone)?;
            let bytes = if let Some(inline) = resource.inline_data {
                base64::engine::general_purpose::STANDARD
                    .decode(inline)
                    .map_err(|_| ServiceError::ImageBadFormat)?
            } else if let Some(blob) = resource.blob_id {
                client
                    .fetch_blob(&blob)
                    .await
                    .map_err(|_| ServiceError::ImageUnreadable)?
                    .ok_or(ServiceError::ImageGone)?
            } else {
                return Err(ServiceError::ImageGone);
            };
            let format = image::guess_format(&bytes).map_err(|_| ServiceError::ImageUnsupported)?;
            let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
            let (width, height) = reader
                .into_dimensions()
                .map_err(|_| ServiceError::ImageUndecodable)?;
            return Ok(PreviewData {
                text: None,
                image: Some(ImagePayload {
                    bytes,
                    mime: format.to_mime_type().into(),
                    width,
                    height,
                }),
                size: resource.size_bytes,
            });
        }
        Ok(PreviewData {
            text: None,
            image: None,
            size: 0,
        })
    }
}

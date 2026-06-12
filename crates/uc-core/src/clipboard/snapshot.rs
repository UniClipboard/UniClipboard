use crate::clipboard::PayloadAvailability;
use crate::ids::{FormatId, RepresentationId};
use crate::{BlobId, MimeType};

#[derive(Debug, Clone)]
pub struct PersistedClipboardRepresentation {
    pub id: RepresentationId,

    /// Clipboard format identifier (e.g. public.utf8-plain-text)
    pub format_id: FormatId,

    pub mime_type: Option<MimeType>,

    /// Logical size in bytes of the original clipboard representation payload.
    /// This value represents the real size observed from the system clipboard,
    /// independent of storage strategy (inline / blob / lazy materialization).
    pub size_bytes: i64,

    /// Inline stored payload, only present when size is below inline threshold.
    pub inline_data: Option<Vec<u8>>,

    /// Blob identifier if the payload has been materialized into blob storage.
    pub blob_id: Option<BlobId>,

    /// Availability state for the payload.
    pub payload_state: PayloadAvailability,

    /// Last processing error message, if any.
    pub last_error: Option<String>,
}

impl PersistedClipboardRepresentation {
    pub fn new(
        id: RepresentationId,
        format_id: FormatId,
        mime_type: Option<MimeType>,
        size_bytes: i64,
        inline_data: Option<Vec<u8>>,
        blob_id: Option<BlobId>,
    ) -> Self {
        debug_assert!(
            !(inline_data.is_some() && blob_id.is_some()),
            "inline_data and blob_id should not both be set in normal flow"
        );

        let payload_state = match (&inline_data, &blob_id) {
            (Some(_), None) => PayloadAvailability::Inline,
            (None, Some(_)) => PayloadAvailability::BlobReady,
            _ => PayloadAvailability::Staged,
        };

        Self {
            id,
            format_id,
            mime_type,
            size_bytes,
            inline_data,
            blob_id,
            payload_state,
            last_error: None,
        }
    }

    pub fn new_with_state(
        id: RepresentationId,
        format_id: FormatId,
        mime_type: Option<MimeType>,
        size_bytes: i64,
        inline_data: Option<Vec<u8>>,
        blob_id: Option<BlobId>,
        payload_state: PayloadAvailability,
        last_error: Option<String>,
    ) -> anyhow::Result<Self> {
        if inline_data.is_some() && blob_id.is_some() {
            return Err(anyhow::anyhow!(
                "inline_data and blob_id should not both be set"
            ));
        }

        if payload_state.requires_inline_data() && inline_data.is_none() {
            return Err(anyhow::anyhow!("payload_state Inline requires inline_data"));
        }

        if payload_state.requires_blob_id() && blob_id.is_none() {
            return Err(anyhow::anyhow!("payload_state BlobReady requires blob_id"));
        }

        if let PayloadAvailability::Failed {
            last_error: state_error,
        } = &payload_state
        {
            let last_error_value = last_error
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("payload_state Failed requires last_error"))?;
            if last_error_value != state_error {
                return Err(anyhow::anyhow!(
                    "payload_state Failed last_error does not match last_error field"
                ));
            }
        }

        Ok(Self {
            id,
            format_id,
            mime_type,
            size_bytes,
            inline_data,
            blob_id,
            payload_state,
            last_error,
        })
    }

    /// Create a staged representation for large content awaiting blob materialization.
    pub fn new_staged(
        id: RepresentationId,
        format_id: FormatId,
        mime_type: Option<MimeType>,
        size_bytes: i64,
    ) -> Self {
        Self {
            id,
            format_id,
            mime_type,
            size_bytes,
            inline_data: None,
            blob_id: None,
            payload_state: PayloadAvailability::Staged,
            last_error: None,
        }
    }

    pub fn payload_state(&self) -> PayloadAvailability {
        self.payload_state.clone()
    }

    pub fn is_inline(&self) -> bool {
        self.inline_data.is_some()
    }

    pub fn is_blob(&self) -> bool {
        self.blob_id.is_some()
    }
}

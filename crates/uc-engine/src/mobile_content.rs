use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MobileSyncItemType {
    Text,
    Image,
    File,
    Group,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobileSyncDocument {
    pub item_type: MobileSyncItemType,
    pub text: String,
    pub data_name: Option<String>,
    pub has_data: bool,
    pub size: u64,
    pub hash: Option<String>,
    pub content_id: Option<String>,
}

impl fmt::Debug for MobileSyncDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MobileSyncDocument")
            .field("item_type", &self.item_type)
            .field("text_bytes", &self.text.len())
            .field("has_data_name", &self.data_name.is_some())
            .field("has_data", &self.has_data)
            .field("size", &self.size)
            .field("has_hash", &self.hash.is_some())
            .field("has_content_id", &self.content_id.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MobileContentAvailabilityInput {
    pub snapshot_hash: String,
}

impl fmt::Debug for MobileContentAvailabilityInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MobileContentAvailabilityInput([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ApplyMobileSyncDocumentInput {
    pub document: MobileSyncDocument,
    pub source_device_id: String,
}

impl fmt::Debug for ApplyMobileSyncDocumentInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ApplyMobileSyncDocumentInput")
            .field("item_type", &self.document.item_type)
            .field("has_data", &self.document.has_data)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ReadMobileSyncFileInput {
    pub data_name: String,
}

impl fmt::Debug for ReadMobileSyncFileInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ReadMobileSyncFileInput([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum MobileSyncDocumentApplyOutcome {
    Applied {
        entry_id: String,
        content_id: String,
    },
    Resurfaced {
        snapshot_hash: String,
        existing_entry_id: String,
        os_write_succeeded: bool,
    },
    DuplicateSkipped {
        snapshot_hash: String,
        existing_entry_id: String,
    },
    DecodeFailed {
        reason: String,
    },
    Buffered,
}

impl fmt::Debug for MobileSyncDocumentApplyOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, os_write_succeeded) = match self {
            Self::Applied { .. } => ("applied", None),
            Self::Resurfaced {
                os_write_succeeded, ..
            } => ("resurfaced", Some(*os_write_succeeded)),
            Self::DuplicateSkipped { .. } => ("duplicate_skipped", None),
            Self::DecodeFailed { .. } => ("decode_failed", None),
            Self::Buffered => ("buffered", None),
        };
        formatter
            .debug_struct("MobileSyncDocumentApplyOutcome")
            .field("kind", &kind)
            .field("os_write_succeeded", &os_write_succeeded)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MobileSyncFile {
    pub media_type: String,
    pub bytes: Vec<u8>,
}

impl fmt::Debug for MobileSyncFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MobileSyncFile")
            .field("has_media_type", &!self.media_type.is_empty())
            .field("byte_len", &self.bytes.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum MobileSyncFileReadOutcome {
    Found(Box<MobileSyncFile>),
    NotFound,
}

impl fmt::Debug for MobileSyncFileReadOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Found(file) => formatter
                .debug_tuple("MobileSyncFileReadOutcome::Found")
                .field(file)
                .finish(),
            Self::NotFound => formatter.write_str("MobileSyncFileReadOutcome::NotFound"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MobileFileUploadHandle(String);

impl MobileFileUploadHandle {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for MobileFileUploadHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("MobileFileUploadHandle([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct BeginMobileFileUploadInput {
    pub data_name: String,
    pub media_type: String,
}

impl fmt::Debug for BeginMobileFileUploadInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BeginMobileFileUploadInput")
            .field("has_data_name", &!self.data_name.is_empty())
            .field("has_media_type", &!self.media_type.is_empty())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AppendMobileFileUploadInput {
    pub handle: MobileFileUploadHandle,
    pub bytes: Vec<u8>,
}

impl fmt::Debug for AppendMobileFileUploadInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppendMobileFileUploadInput")
            .field("handle", &self.handle)
            .field("byte_len", &self.bytes.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct FinishMobileFileUploadInput {
    pub handle: MobileFileUploadHandle,
    pub media_type: String,
    pub source_device_id: String,
    pub transfer_id: String,
}

impl fmt::Debug for FinishMobileFileUploadInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FinishMobileFileUploadInput")
            .field("handle", &self.handle)
            .field("has_media_type", &!self.media_type.is_empty())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AbortMobileFileUploadInput {
    pub handle: MobileFileUploadHandle,
}

impl fmt::Debug for AbortMobileFileUploadInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AbortMobileFileUploadInput")
            .field("handle", &self.handle)
            .finish()
    }
}

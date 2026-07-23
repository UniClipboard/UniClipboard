use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;

use tokio::sync::broadcast;
use uc_application::facade::space_setup::PairingOutcome;

#[derive(Clone, PartialEq, Eq)]
pub enum DevOperation {
    SeedText { text: String },
    CaptureFilePaths { paths: Vec<PathBuf> },
    ListPairingInvitationAddresses,
    IssueInvitationForAddress { address: IpAddr },
    PublishBlob { bytes: Vec<u8> },
    FetchBlob { ticket: Vec<u8>, entry_id: String },
}

impl fmt::Debug for DevOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::SeedText { .. } => "seed_text",
            Self::CaptureFilePaths { .. } => "capture_file_paths",
            Self::ListPairingInvitationAddresses => "list_pairing_invitation_addresses",
            Self::IssueInvitationForAddress { .. } => "issue_invitation_for_address",
            Self::PublishBlob { .. } => "publish_blob",
            Self::FetchBlob { .. } => "fetch_blob",
        };
        formatter
            .debug_struct("DevOperation")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct DevCapturedFileSetLine {
    pub line_index: i64,
    pub root_index: Option<i64>,
    pub root_name: Option<String>,
    pub relative_path: Option<String>,
    pub member_kind: Option<String>,
    pub line_kind: String,
    pub exclude_reason: Option<String>,
}

impl fmt::Debug for DevCapturedFileSetLine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DevCapturedFileSetLine")
            .field("line_index", &self.line_index)
            .field("root_index", &self.root_index)
            .field("has_root_name", &self.root_name.is_some())
            .field("has_relative_path", &self.relative_path.is_some())
            .field("has_member_kind", &self.member_kind.is_some())
            .field("line_kind", &self.line_kind)
            .field("has_exclude_reason", &self.exclude_reason.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct DevCapturedFileSet {
    pub entry_id: String,
    pub deduplicated: bool,
    pub snapshot_hash: String,
    pub directory_structure: bool,
    pub content_digest_count: usize,
    pub lines: Vec<DevCapturedFileSetLine>,
}

impl fmt::Debug for DevCapturedFileSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DevCapturedFileSet")
            .field("entry_id", &self.entry_id)
            .field("deduplicated", &self.deduplicated)
            .field("directory_structure", &self.directory_structure)
            .field("content_digest_count", &self.content_digest_count)
            .field("line_count", &self.lines.len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DevPairingInvitationAddress {
    pub ip: IpAddr,
    pub port: u16,
}

impl fmt::Debug for DevPairingInvitationAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DevPairingInvitationAddress")
            .field("address", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct DevInvitation {
    pub code: String,
    pub expires_at_ms: i64,
}

impl fmt::Debug for DevInvitation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DevInvitation")
            .field("code", &"[REDACTED]")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct DevBlobPublished {
    pub ticket: Vec<u8>,
    pub entry_id: String,
    pub plaintext_hash: Vec<u8>,
    pub digest: Vec<u8>,
    pub reused_existing: bool,
}

impl fmt::Debug for DevBlobPublished {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DevBlobPublished")
            .field("entry_id", &self.entry_id)
            .field("reused_existing", &self.reused_existing)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum DevOperationResult {
    TextSeeded {
        entry_id: String,
    },
    FilePathsCaptured(DevCapturedFileSet),
    PairingInvitationAddresses(Vec<DevPairingInvitationAddress>),
    InvitationIssued(DevInvitation),
    BlobPublished(DevBlobPublished),
    BlobFetched {
        bytes: Vec<u8>,
        entry_id: String,
        plaintext_hash: Vec<u8>,
        digest: Vec<u8>,
    },
}

impl fmt::Debug for DevOperationResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::TextSeeded { .. } => "text_seeded",
            Self::FilePathsCaptured(_) => "file_paths_captured",
            Self::PairingInvitationAddresses(_) => "pairing_invitation_addresses",
            Self::InvitationIssued(_) => "invitation_issued",
            Self::BlobPublished(_) => "blob_published",
            Self::BlobFetched { .. } => "blob_fetched",
        };
        formatter
            .debug_struct("DevOperationResult")
            .field("kind", &kind)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum DevPairingOutcome {
    Success {
        peer_device_id: String,
        peer_device_name: String,
        peer_fingerprint: String,
    },
    Failure {
        reason: String,
    },
}

impl fmt::Debug for DevPairingOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Success { .. } => formatter.write_str("DevPairingOutcome::Success([REDACTED])"),
            Self::Failure { .. } => formatter.write_str("DevPairingOutcome::Failure([REDACTED])"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevPairingStreamError {
    Lagged,
    Closed,
}

impl fmt::Display for DevPairingStreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lagged => formatter.write_str("pairing outcome stream lagged"),
            Self::Closed => formatter.write_str("pairing outcome stream closed"),
        }
    }
}

pub struct DevPairingOutcomeStream {
    receiver: broadcast::Receiver<PairingOutcome>,
}

impl DevPairingOutcomeStream {
    pub(crate) fn new(receiver: broadcast::Receiver<PairingOutcome>) -> Self {
        Self { receiver }
    }

    pub async fn recv(&mut self) -> Result<DevPairingOutcome, DevPairingStreamError> {
        match self.receiver.recv().await {
            Ok(PairingOutcome::Success {
                peer_device_id,
                peer_device_name,
                peer_fingerprint,
            }) => Ok(DevPairingOutcome::Success {
                peer_device_id: peer_device_id.to_string(),
                peer_device_name,
                peer_fingerprint: peer_fingerprint.to_string(),
            }),
            Ok(PairingOutcome::Failure { reason }) => Ok(DevPairingOutcome::Failure {
                reason: reason.to_string(),
            }),
            Err(broadcast::error::RecvError::Lagged(_)) => Err(DevPairingStreamError::Lagged),
            Err(broadcast::error::RecvError::Closed) => Err(DevPairingStreamError::Closed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_debug_output_redacts_content_paths_and_tokens() {
        let operation = DevOperation::SeedText {
            text: "private clipboard text".into(),
        };
        let captured = DevOperationResult::FilePathsCaptured(DevCapturedFileSet {
            entry_id: "entry-1".into(),
            deduplicated: false,
            snapshot_hash: "private-snapshot-hash".into(),
            directory_structure: true,
            content_digest_count: 0,
            lines: vec![DevCapturedFileSetLine {
                line_index: 1,
                root_index: Some(0),
                root_name: Some("private-root".into()),
                relative_path: Some("private/file.txt".into()),
                member_kind: Some("f".into()),
                line_kind: "file".into(),
                exclude_reason: None,
            }],
        });
        let blob = DevOperationResult::BlobPublished(DevBlobPublished {
            ticket: b"private-ticket".to_vec(),
            entry_id: "entry-2".into(),
            plaintext_hash: b"private-plaintext-hash".to_vec(),
            digest: b"private-digest".to_vec(),
            reused_existing: false,
        });
        let address = DevPairingInvitationAddress {
            ip: "203.0.113.42".parse().expect("test address should parse"),
            port: 4242,
        };

        let debug = format!("{operation:?} {captured:?} {blob:?} {address:?}");
        for secret in [
            "private clipboard text",
            "private-root",
            "private/file.txt",
            "private-snapshot-hash",
            "private-ticket",
            "private-plaintext-hash",
            "private-digest",
            "203.0.113.42",
        ] {
            assert!(!debug.contains(secret));
        }
    }
}

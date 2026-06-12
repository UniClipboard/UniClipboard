use serde::{Deserialize, Serialize};

/// Mapping between a file transfer and its original filename.
/// Carried in clipboard sync so the receiver can pre-compute local cache paths
/// before the file transfer completes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferMapping {
    pub transfer_id: String,
    pub filename: String,
}

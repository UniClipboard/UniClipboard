use serde::{Deserialize, Serialize};

/// Message requesting resume of an interrupted chunked clipboard transfer.
///
/// Sent by the receiver to the original sender when a V3 transfer is
/// interrupted mid-stream. The sender looks up the cached payload by
/// `transfer_id` and re-sends from `start_chunk` onward.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TransferResumeMessage {
    /// Hex-encoded 16-byte UUID from the V3 header `transfer_id` field.
    pub transfer_id: String,
    /// First chunk index the receiver still needs (0-based).
    pub start_chunk: u32,
}

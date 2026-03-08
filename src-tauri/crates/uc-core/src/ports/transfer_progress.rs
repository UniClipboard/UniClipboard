//! Transfer progress reporting port.
//!
//! Provides progress tracking for chunked clipboard transfers,
//! enabling the frontend to display transfer progress UI.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Direction of a clipboard transfer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransferDirection {
    Sending,
    Receiving,
}

/// Progress of an ongoing clipboard transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub transfer_id: String,
    pub peer_id: String,
    pub direction: TransferDirection,
    pub chunks_completed: u32,
    pub total_chunks: u32,
    pub bytes_transferred: u64,
    /// Total bytes for this transfer, or `None` if unknown (e.g. receiving side).
    pub total_bytes: Option<u64>,
}

/// Port for reporting transfer progress events.
#[async_trait]
pub trait TransferProgressPort: Send + Sync {
    /// Report progress of an active transfer.
    async fn report_progress(&self, progress: TransferProgress) -> Result<()>;
}

/// No-op implementation of `TransferProgressPort` for tests and default usage.
pub struct NoopTransferProgressPort;

#[async_trait]
impl TransferProgressPort for NoopTransferProgressPort {
    async fn report_progress(&self, _progress: TransferProgress) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_progress_serializes_to_json_with_expected_fields() {
        let progress = TransferProgress {
            transfer_id: "abc-123".to_string(),
            peer_id: "peer-1".to_string(),
            direction: TransferDirection::Sending,
            chunks_completed: 5,
            total_chunks: 10,
            bytes_transferred: 1_310_720,
            total_bytes: Some(2_621_440),
        };
        let json = serde_json::to_value(&progress).unwrap();
        assert_eq!(json["transfer_id"], "abc-123");
        assert_eq!(json["peer_id"], "peer-1");
        assert_eq!(json["direction"], "Sending");
        assert_eq!(json["chunks_completed"], 5);
        assert_eq!(json["total_chunks"], 10);
        assert_eq!(json["bytes_transferred"], 1_310_720);
        assert_eq!(json["total_bytes"], 2_621_440);
    }

    #[test]
    fn transfer_progress_with_unknown_total_bytes() {
        let progress = TransferProgress {
            transfer_id: "abc-123".to_string(),
            peer_id: "peer-1".to_string(),
            direction: TransferDirection::Receiving,
            chunks_completed: 3,
            total_chunks: 10,
            bytes_transferred: 768_000,
            total_bytes: None,
        };
        let json = serde_json::to_value(&progress).unwrap();
        assert!(json["total_bytes"].is_null());
    }

    #[test]
    fn transfer_direction_serializes_correctly() {
        let sending = serde_json::to_string(&TransferDirection::Sending).unwrap();
        assert_eq!(sending, "\"Sending\"");

        let receiving = serde_json::to_string(&TransferDirection::Receiving).unwrap();
        assert_eq!(receiving, "\"Receiving\"");
    }

    #[test]
    fn transfer_progress_round_trip() {
        let progress = TransferProgress {
            transfer_id: "t-1".to_string(),
            peer_id: "p-1".to_string(),
            direction: TransferDirection::Receiving,
            chunks_completed: 3,
            total_chunks: 5,
            bytes_transferred: 768_000,
            total_bytes: Some(1_280_000),
        };
        let json = serde_json::to_string(&progress).unwrap();
        let restored: TransferProgress = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.transfer_id, "t-1");
        assert_eq!(restored.direction, TransferDirection::Receiving);
        assert_eq!(restored.chunks_completed, 3);
        assert_eq!(restored.total_bytes, Some(1_280_000));
    }
}

//! Transfer progress event forwarding to frontend.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use uc_core::ports::transfer_progress::TransferProgress;

/// Transfer progress event DTO for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgressEvent {
    pub transfer_id: String,
    pub peer_id: String,
    pub direction: String,
    pub chunks_completed: u32,
    pub total_chunks: u32,
    pub bytes_transferred: u64,
    /// Total bytes for this transfer, or `null` if unknown (e.g. receiving side).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
}

impl From<TransferProgress> for TransferProgressEvent {
    fn from(p: TransferProgress) -> Self {
        Self {
            transfer_id: p.transfer_id,
            peer_id: p.peer_id,
            direction: format!("{:?}", p.direction),
            chunks_completed: p.chunks_completed,
            total_chunks: p.total_chunks,
            bytes_transferred: p.bytes_transferred,
            total_bytes: p.total_bytes,
        }
    }
}

/// Forward a transfer progress event to the frontend via Tauri event channel.
pub fn forward_transfer_progress_event<R: tauri::Runtime>(
    app: &AppHandle<R>,
    progress: TransferProgress,
) -> Result<(), Box<dyn std::error::Error>> {
    let event: TransferProgressEvent = progress.into();
    app.emit("transfer://progress", event)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Listener;
    use uc_core::ports::transfer_progress::TransferDirection;

    #[test]
    fn transfer_progress_event_serializes_with_camel_case() {
        let event = TransferProgressEvent {
            transfer_id: "t-1".to_string(),
            peer_id: "p-1".to_string(),
            direction: "Sending".to_string(),
            chunks_completed: 3,
            total_chunks: 10,
            bytes_transferred: 786432,
            total_bytes: Some(2621440),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["transferId"], "t-1");
        assert_eq!(json["peerId"], "p-1");
        assert_eq!(json["direction"], "Sending");
        assert_eq!(json["chunksCompleted"], 3);
        assert_eq!(json["totalChunks"], 10);
        assert_eq!(json["bytesTransferred"], 786432);
        assert_eq!(json["totalBytes"], 2621440);

        // Ensure snake_case keys are absent (guards #[serde(rename_all = "camelCase")])
        assert!(json.get("transfer_id").is_none());
        assert!(json.get("peer_id").is_none());
        assert!(json.get("chunks_completed").is_none());
        assert!(json.get("total_chunks").is_none());
        assert!(json.get("bytes_transferred").is_none());
        assert!(json.get("total_bytes").is_none());
    }

    #[test]
    fn transfer_progress_event_omits_total_bytes_when_none() {
        let event = TransferProgressEvent {
            transfer_id: "t-2".to_string(),
            peer_id: "p-2".to_string(),
            direction: "Receiving".to_string(),
            chunks_completed: 1,
            total_chunks: 5,
            bytes_transferred: 262144,
            total_bytes: None,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert!(json.get("totalBytes").is_none());
    }

    #[test]
    fn from_transfer_progress_converts_correctly() {
        let progress = TransferProgress {
            transfer_id: "xfer-99".to_string(),
            peer_id: "peer-42".to_string(),
            direction: TransferDirection::Receiving,
            chunks_completed: 5,
            total_chunks: 8,
            bytes_transferred: 1_310_720,
            total_bytes: Some(2_097_152),
        };
        let event: TransferProgressEvent = progress.into();
        assert_eq!(event.transfer_id, "xfer-99");
        assert_eq!(event.direction, "Receiving");
        assert_eq!(event.chunks_completed, 5);
        assert_eq!(event.total_bytes, Some(2_097_152));
    }

    #[tokio::test]
    async fn forward_transfer_progress_event_emits_on_correct_channel() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);

        let tx_clone = tx.clone();
        app_handle.listen("transfer://progress", move |event: tauri::Event| {
            let _ = tx_clone.try_send(event.payload().to_string());
        });

        let progress = TransferProgress {
            transfer_id: "test-xfer".to_string(),
            peer_id: "test-peer".to_string(),
            direction: TransferDirection::Sending,
            chunks_completed: 1,
            total_chunks: 2,
            bytes_transferred: 262144,
            total_bytes: Some(524288),
        };

        forward_transfer_progress_event(&app_handle, progress)
            .expect("emit transfer progress event");

        let payload = rx.recv().await.expect("event payload");
        assert!(payload.contains("test-xfer"));
        assert!(payload.contains("transferId"));
    }
}

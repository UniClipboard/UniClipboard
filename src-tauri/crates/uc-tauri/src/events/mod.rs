//! Event Forwarding - Forward backend events to frontend
//! 事件转发 - 将后端事件转发到前端

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

pub mod p2p_pairing;
pub mod p2p_peer;
pub use p2p_pairing::{P2PPairingVerificationEvent, P2PPairingVerificationKind};
pub use p2p_peer::{P2PPeerConnectionEvent, P2PPeerDiscoveryEvent, P2PPeerNameUpdatedEvent};

/// Clipboard events emitted to frontend
/// 发送到前端的剪贴板事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClipboardEvent {
    /// New clipboard content captured
    NewContent { entry_id: String, preview: String },
    /// Clipboard content deleted
    Deleted { entry_id: String },
}

/// Encryption events emitted to frontend
/// 发送到前端的加密事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EncryptionEvent {
    /// Encryption initialized
    Initialized,
    /// Encryption session ready (keyring unlock completed)
    SessionReady,
    /// Encryption failed
    Failed { reason: String },
}

/// Forward libp2p startup error to frontend
/// 将 libp2p 启动错误转发到前端
pub fn forward_libp2p_start_failed<R: tauri::Runtime>(
    app: &AppHandle<R>,
    message: String,
) -> Result<(), Box<dyn std::error::Error>> {
    app.emit("libp2p://start-failed", message)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Listener;

    #[test]
    fn encryption_event_serializes_with_type_tag() {
        let ready = serde_json::to_value(EncryptionEvent::SessionReady).unwrap();
        assert_eq!(ready, serde_json::json!({ "type": "SessionReady" }));

        let failed = serde_json::to_value(EncryptionEvent::Failed {
            reason: "oops".to_string(),
        })
        .unwrap();
        assert_eq!(
            failed,
            serde_json::json!({ "type": "Failed", "reason": "oops" })
        );
    }

    #[tokio::test]
    async fn forward_libp2p_start_failed_emits_event() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);

        let tx_clone = tx.clone();
        app_handle.listen("libp2p://start-failed", move |event: tauri::Event| {
            let _ = tx_clone.try_send(event.payload().to_string());
        });

        forward_libp2p_start_failed(&app_handle, "boom".to_string())
            .expect("emit libp2p start failed event");

        let payload = rx.recv().await.expect("event payload");
        assert!(payload.contains("boom"));
    }
}

/// Forward clipboard event to frontend
/// 将剪贴板事件转发到前端
pub fn forward_clipboard_event<R: tauri::Runtime>(
    app: &AppHandle<R>,
    event: ClipboardEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    app.emit("clipboard://event", event)?;
    Ok(())
}

/// Forward encryption event to frontend
/// 将加密事件转发到前端
pub fn forward_encryption_event<R: tauri::Runtime>(
    app: &AppHandle<R>,
    event: EncryptionEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    app.emit("encryption://event", event)?;
    Ok(())
}

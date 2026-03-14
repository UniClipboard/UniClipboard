//! Event Forwarding - Forward backend events to frontend
//! 事件转发 - 将后端事件转发到前端

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

pub mod p2p_pairing;
pub mod p2p_peer;
pub mod transfer_progress;
pub use p2p_pairing::{P2PPairingVerificationEvent, P2PPairingVerificationKind};
pub use p2p_peer::{P2PPeerConnectionEvent, P2PPeerDiscoveryEvent, P2PPeerNameUpdatedEvent};
pub use transfer_progress::{forward_transfer_progress_event, TransferProgressEvent};

/// Clipboard events emitted to frontend
/// 发送到前端的剪贴板事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClipboardEvent {
    /// New clipboard content captured
    NewContent {
        entry_id: String,
        preview: String,
        origin: String,
    },
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
    fn clipboard_event_new_content_serializes_with_origin() {
        let event = ClipboardEvent::NewContent {
            entry_id: "abc".to_string(),
            preview: "hello".to_string(),
            origin: "local".to_string(),
        };
        let json = serde_json::to_value(event).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "type": "NewContent",
                "entry_id": "abc",
                "preview": "hello",
                "origin": "local"
            })
        );
    }

    #[test]
    fn clipboard_event_new_content_serializes_with_remote_origin() {
        let event = ClipboardEvent::NewContent {
            entry_id: "xyz".to_string(),
            preview: "world".to_string(),
            origin: "remote".to_string(),
        };
        let json = serde_json::to_value(event).unwrap();
        assert_eq!(json["origin"], "remote");
    }

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

    #[test]
    fn test_setting_changed_event_camelcase_serialization() {
        let event = SettingChangedEvent {
            setting_json: r#"{"key":"value"}"#.to_string(),
            timestamp: 1234567890,
        };
        let json_str = serde_json::to_string(&event).unwrap();

        // Must use camelCase keys
        assert!(
            json_str.contains("settingJson"),
            "expected camelCase key 'settingJson', got: {}",
            json_str
        );
        assert!(
            json_str.contains("timestamp"),
            "expected key 'timestamp', got: {}",
            json_str
        );

        // Must NOT use snake_case keys
        assert!(
            !json_str.contains("setting_json"),
            "unexpected snake_case key 'setting_json' in: {}",
            json_str
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

/// Setting changed event emitted to frontend
/// 发送到前端的设置变更事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingChangedEvent {
    pub setting_json: String,
    pub timestamp: u64,
}

/// Forward setting changed event to all windows
/// 将设置变更事件转发到所有窗口
pub fn forward_setting_changed_event<R: tauri::Runtime>(
    app: &AppHandle<R>,
    event: SettingChangedEvent,
) -> Result<(), Box<dyn std::error::Error>> {
    app.emit("setting-changed", event)?;
    Ok(())
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

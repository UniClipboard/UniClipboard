//! Application runtime management
//! 应用运行时管理

use crate::events::{
    forward_clipboard_event, forward_encryption_event, ClipboardEvent, EncryptionEvent,
};
use std::sync::Arc;
use tauri::AppHandle;
use uc_app::App;

/// The completed application runtime.
///
/// This struct holds the fully assembled App instance
/// and is managed by Tauri's state system.
pub struct Runtime {
    pub app: Arc<App>,
    /// Tauri AppHandle for emitting events to frontend
    pub app_handle: AppHandle,
}

impl Runtime {
    pub fn new(app: Arc<App>, app_handle: AppHandle) -> Self {
        Self { app, app_handle }
    }

    /// Handle clipboard captured event
    pub async fn on_clipboard_captured(&self, entry_id: String, preview: String) {
        let event = ClipboardEvent::NewContent { entry_id, preview };
        if let Err(e) = forward_clipboard_event(&self.app_handle, event) {
            eprintln!("Failed to forward clipboard event: {}", e);
        }
    }

    /// Handle encryption initialized event
    pub async fn on_encryption_initialized(&self) {
        let event = EncryptionEvent::Initialized;
        if let Err(e) = forward_encryption_event(&self.app_handle, event) {
            eprintln!("Failed to forward encryption event: {}", e);
        }
    }
}

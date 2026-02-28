//! In-memory and logging adapters for the lifecycle coordinator ports.
//!
//! These adapters are used to wire the `AppLifecycleCoordinator` into the
//! Tauri runtime. Tauri-specific emitters (using `AppHandle`) will be added
//! in a later task when the frontend lifecycle UI is connected.

use anyhow::Result;
use async_trait::async_trait;
use tauri::{AppHandle, Runtime};
use uc_app::usecases::{
    DeviceAnnouncer, LifecycleEvent, LifecycleEventEmitter, LifecycleState, LifecycleStatusPort,
    SessionReadyEmitter,
};
use uc_core::ports::{PeerDirectoryPort, SettingsPort};

use std::sync::Arc;

use crate::bootstrap::wiring::resolve_pairing_device_name;

// ---------------------------------------------------------------------------
// InMemoryLifecycleStatus
// ---------------------------------------------------------------------------

/// Stores lifecycle state in a `tokio::sync::Mutex`.
///
/// This adapter is intended to live as an `Arc<InMemoryLifecycleStatus>` inside
/// `AppRuntime` so that repeated calls to `app_lifecycle_coordinator()` share
/// the same status instance.
pub struct InMemoryLifecycleStatus {
    state: tokio::sync::Mutex<LifecycleState>,
}

impl InMemoryLifecycleStatus {
    pub fn new() -> Self {
        Self {
            state: tokio::sync::Mutex::new(LifecycleState::Idle),
        }
    }
}

#[async_trait]
impl LifecycleStatusPort for InMemoryLifecycleStatus {
    async fn set_state(&self, state: LifecycleState) -> Result<()> {
        *self.state.lock().await = state;
        Ok(())
    }

    async fn get_state(&self) -> LifecycleState {
        self.state.lock().await.clone()
    }
}

// ---------------------------------------------------------------------------
// LoggingLifecycleEventEmitter
// ---------------------------------------------------------------------------

/// Logs lifecycle events using `tracing`. Does not emit to the frontend.
///
/// This is a placeholder adapter. A Tauri-specific emitter that uses
/// `AppHandle::emit()` will replace this once the frontend lifecycle UI
/// is implemented.
pub struct LoggingLifecycleEventEmitter;

#[async_trait]
impl LifecycleEventEmitter for LoggingLifecycleEventEmitter {
    async fn emit_lifecycle_event(&self, event: LifecycleEvent) -> Result<()> {
        tracing::info!(event = ?event, "Lifecycle event");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// LoggingSessionReadyEmitter
// ---------------------------------------------------------------------------

/// Logs the session-ready signal using `tracing`. Does not emit to the frontend.
///
/// This is a placeholder adapter. A Tauri-specific emitter that uses
/// `AppHandle::emit()` will replace this once the frontend is ready.
pub struct LoggingSessionReadyEmitter;

#[async_trait]
impl SessionReadyEmitter for LoggingSessionReadyEmitter {
    async fn emit_ready(&self) -> Result<()> {
        tracing::info!("Session ready");
        Ok(())
    }
}

pub struct TauriSessionReadyEmitter<R: Runtime> {
    app_handle: Arc<std::sync::RwLock<Option<AppHandle<R>>>>,
}

impl<R: Runtime> TauriSessionReadyEmitter<R> {
    pub fn new(app_handle: Arc<std::sync::RwLock<Option<AppHandle<R>>>>) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl<R: Runtime> SessionReadyEmitter for TauriSessionReadyEmitter<R> {
    async fn emit_ready(&self) -> Result<()> {
        let guard = self.app_handle.read().unwrap_or_else(|poisoned| {
            tracing::error!(
                "RwLock poisoned in session ready emission, recovering from poisoned state"
            );
            poisoned.into_inner()
        });

        if let Some(app) = guard.as_ref() {
            if let Err(err) = crate::events::forward_encryption_event(
                app,
                crate::events::EncryptionEvent::SessionReady,
            ) {
                tracing::warn!(error = %err, "Failed to emit encryption session ready event");
            }
        } else {
            tracing::warn!("AppHandle not available, skipping encryption session ready emission");
        }

        tracing::info!("Session ready");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DeviceNameAnnouncer
// ---------------------------------------------------------------------------

/// Resolves the device name from settings and announces it over the network.
///
/// Used by `AppLifecycleCoordinator` to broadcast the device name after
/// the network runtime has started.
pub struct DeviceNameAnnouncer {
    network: Arc<dyn PeerDirectoryPort>,
    settings: Arc<dyn SettingsPort>,
}

impl DeviceNameAnnouncer {
    pub fn new(network: Arc<dyn PeerDirectoryPort>, settings: Arc<dyn SettingsPort>) -> Self {
        Self { network, settings }
    }
}

#[async_trait]
impl DeviceAnnouncer for DeviceNameAnnouncer {
    async fn announce(&self) -> Result<()> {
        let device_name = resolve_pairing_device_name(self.settings.clone()).await;
        self.network.announce_device_name(device_name).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tauri::Listener;

    #[tokio::test]
    async fn in_memory_lifecycle_status_defaults_to_idle() {
        let status = InMemoryLifecycleStatus::new();
        assert_eq!(status.get_state().await, LifecycleState::Idle);
    }

    #[tokio::test]
    async fn in_memory_lifecycle_status_set_and_get() {
        let status = InMemoryLifecycleStatus::new();
        status.set_state(LifecycleState::Ready).await.unwrap();
        assert_eq!(status.get_state().await, LifecycleState::Ready);
    }

    #[tokio::test]
    async fn logging_lifecycle_event_emitter_does_not_error() {
        let emitter = LoggingLifecycleEventEmitter;
        let result = emitter.emit_lifecycle_event(LifecycleEvent::Ready).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn logging_session_ready_emitter_does_not_error() {
        let emitter = LoggingSessionReadyEmitter;
        let result = emitter.emit_ready().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn tauri_session_ready_emitter_emits_frontend_event() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);

        let tx_clone = tx.clone();
        app_handle.listen("encryption://event", move |event: tauri::Event| {
            let _ = tx_clone.try_send(event.payload().to_string());
        });

        let emitter = TauriSessionReadyEmitter::new(Arc::new(std::sync::RwLock::new(Some(
            app_handle.clone(),
        ))));
        emitter.emit_ready().await.unwrap();

        let payload = rx.recv().await.expect("event payload");
        let value: Value = serde_json::from_str(&payload).expect("json payload");
        assert_eq!(value, serde_json::json!({ "type": "SessionReady" }));
    }

    #[tokio::test]
    async fn tauri_session_ready_emitter_without_handle_is_ok() {
        let app = tauri::test::mock_app();
        let app_handle = app.handle();
        let emitter = TauriSessionReadyEmitter::new(Arc::new(std::sync::RwLock::new(Some(
            app_handle.clone(),
        ))));

        *emitter
            .app_handle
            .write()
            .expect("rwlock should not be poisoned") = None;

        let result = emitter.emit_ready().await;
        assert!(result.is_ok());
    }
}

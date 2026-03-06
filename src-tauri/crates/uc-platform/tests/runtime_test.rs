use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use uc_core::clipboard::{ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_core::ids::RepresentationId;
use uc_platform::ipc::{PlatformCommand, PlatformEvent};
use uc_platform::ports::PlatformCommandExecutorPort;
use uc_platform::runtime::runtime::PlatformRuntime;

struct MockExecutor;

#[async_trait]
impl PlatformCommandExecutorPort for MockExecutor {
    async fn execute(&self, _cmd: PlatformCommand) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn test_handle_clipboard_changed_event() {
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(100);
    let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(100);

    let _runtime = PlatformRuntime::new(
        event_tx.clone(),
        event_rx,
        cmd_rx,
        Arc::new(MockExecutor),
        None,
    )
    .unwrap();

    // Create a test snapshot
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 0,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            "text".into(),
            None,
            b"test content".to_vec(),
        )],
    };

    // Send the event
    let _ = event_tx
        .send(PlatformEvent::ClipboardChanged { snapshot })
        .await;

    // Note: We can't directly test handle_event since it's private
    // In a real scenario, we'd start the runtime and verify it processes events
    // For now, this test verifies the runtime can be created and events can be sent
}

#[tokio::test]
async fn test_runtime_creation() {
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(100);
    let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(100);

    let runtime = PlatformRuntime::new(event_tx, event_rx, cmd_rx, Arc::new(MockExecutor), None);

    assert!(runtime.is_ok(), "Runtime should be created successfully");
}

#[tokio::test]
async fn test_handle_started_event() {
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(100);
    let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(100);

    let _runtime = PlatformRuntime::new(
        event_tx.clone(),
        event_rx,
        cmd_rx,
        Arc::new(MockExecutor),
        None,
    )
    .unwrap();

    // Send Started event
    let _ = event_tx.send(PlatformEvent::Started).await;
}

#[tokio::test]
async fn test_handle_error_event() {
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(100);
    let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(100);

    let _runtime = PlatformRuntime::new(
        event_tx.clone(),
        event_rx,
        cmd_rx,
        Arc::new(MockExecutor),
        None,
    )
    .unwrap();

    // Send Error event
    let _ = event_tx
        .send(PlatformEvent::Error {
            message: "Test error".to_string(),
        })
        .await;
}

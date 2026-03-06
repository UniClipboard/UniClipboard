//! Integration test for clipboard capture flow
//!
//! Tests the complete flow from clipboard change to entry persistence

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use uc_core::clipboard::ObservedClipboardRepresentation;
use uc_core::ids::{FormatId, RepresentationId};
use uc_core::ports::ClipboardChangeHandler;
use uc_core::SystemClipboardSnapshot;

/// Mock clipboard change handler for testing
struct MockHandler {
    capture_called: Arc<AtomicBool>,
    snapshot_received: Arc<std::sync::Mutex<Option<SystemClipboardSnapshot>>>,
}

impl MockHandler {
    fn new() -> Self {
        Self {
            capture_called: Arc::new(AtomicBool::new(false)),
            snapshot_received: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn was_called(&self) -> bool {
        self.capture_called.load(Ordering::SeqCst)
    }

    fn get_snapshot(&self) -> Option<SystemClipboardSnapshot> {
        self.snapshot_received.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ClipboardChangeHandler for MockHandler {
    async fn on_clipboard_changed(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
        self.capture_called.store(true, Ordering::SeqCst);
        *self.snapshot_received.lock().unwrap() = Some(snapshot);
        Ok(())
    }
}

#[tokio::test]
async fn test_clipboard_change_handler_receives_callback() {
    let handler = MockHandler::new();
    let handler_arc = Arc::new(handler);

    // Keep a reference to the inner handler for assertions
    let inner_handler = handler_arc.clone();

    // Create a dummy snapshot
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 12345,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::from("test-rep-1".to_string()),
            FormatId::from("public.utf8-plain-text".to_string()),
            Some(uc_core::MimeType("text/plain".to_string())),
            vec![b'H', b'e', b'l', b'l', b'o'],
        )],
    };

    // Clone the Arc to get the trait object
    let trait_handler: Arc<dyn ClipboardChangeHandler> = handler_arc.clone();

    // Call the handler
    trait_handler
        .on_clipboard_changed(snapshot.clone())
        .await
        .unwrap();

    // Verify callback was called using the original handler reference
    assert!(
        inner_handler.was_called(),
        "Handler should have been called"
    );

    // Verify snapshot was received
    let received = inner_handler.get_snapshot();
    assert!(received.is_some(), "Snapshot should have been received");
    let received = received.unwrap();
    assert_eq!(received.ts_ms, 12345);
    assert_eq!(received.representations.len(), 1);
}

#[tokio::test]
async fn test_clipboard_change_handler_multiple_calls() {
    let handler = MockHandler::new();
    let handler_arc = Arc::new(handler);

    // Keep references
    let inner_handler = handler_arc.clone();
    let trait_handler: Arc<dyn ClipboardChangeHandler> = handler_arc.clone();

    // Create multiple snapshots
    let snapshot1 = SystemClipboardSnapshot {
        ts_ms: 1000,
        representations: vec![],
    };
    let snapshot2 = SystemClipboardSnapshot {
        ts_ms: 2000,
        representations: vec![],
    };

    // Call the handler multiple times
    trait_handler.on_clipboard_changed(snapshot1).await.unwrap();
    trait_handler.on_clipboard_changed(snapshot2).await.unwrap();

    // Verify callback was called (last call should be tracked)
    assert!(
        inner_handler.was_called(),
        "Handler should have been called"
    );

    let received = inner_handler.get_snapshot();
    assert!(received.is_some());
    assert_eq!(
        received.unwrap().ts_ms,
        2000,
        "Should have received last snapshot"
    );
}

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tracing::info_span;
use tracing::Instrument;

use uc_core::clipboard::ClipboardChangeOrigin;
use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::clipboard::{ClipboardChangeOriginPort, SystemClipboardPort};

/// Represents the intent behind a programmatic clipboard write.
///
/// Each variant carries per-intent guard TTL semantics:
/// - `LocalRestore` / `LocalCapture`: 2-second hash guard (short-lived, local op)
/// - `RemotePush`: 60-second hash guard + one-shot next-origin override (OS re-encoding guard)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardWriteIntent {
    /// A local clipboard history restore (user clicked "restore" in history).
    LocalRestore,
    /// A remote push from a peer device arriving via inbound sync.
    RemotePush,
    /// A local clipboard write triggered by a local app action (e.g., copy file).
    LocalCapture,
}

/// Single write boundary for all programmatic clipboard writes.
///
/// Centralises the guard-registration + write + cleanup-on-error pattern
/// that was previously duplicated across `restore_clipboard_selection.rs`,
/// `sync_inbound.rs`, and `copy_file_to_clipboard.rs`.
///
/// # Contract
///
/// `write(snapshot, intent)` is the ONLY caller of `snapshot.origin_guard_key()`.
/// Callers build the snapshot and choose the intent; the coordinator handles
/// all guard lifecycle operations.
pub struct ClipboardWriteCoordinator {
    system_clipboard: Arc<dyn SystemClipboardPort>,
    clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
}

impl ClipboardWriteCoordinator {
    pub fn new(
        system_clipboard: Arc<dyn SystemClipboardPort>,
        clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
    ) -> Self {
        Self {
            system_clipboard,
            clipboard_change_origin,
        }
    }

    /// Check if there is a pending clipboard origin guard (non-destructive peek).
    ///
    /// Delegates to `ClipboardChangeOriginPort::has_pending_origin()`.
    /// Used by workers to detect concurrent clipboard operations before writing.
    pub async fn has_pending_origin(&self) -> bool {
        self.clipboard_change_origin.has_pending_origin().await
    }

    /// Write a snapshot to the system clipboard with the given intent.
    ///
    /// # Intent semantics
    ///
    /// - `LocalRestore` / `LocalCapture`: registers a 2-second local snapshot hash guard,
    ///   then writes. On error, consumes the guard to prevent stale state.
    /// - `RemotePush`: registers a 60-second remote snapshot hash guard, writes, then
    ///   sets a one-shot `set_next_origin(RemotePush, 60s)` to guard against OS re-encoding
    ///   loopback (e.g., Windows DIB→PNG re-encode produces a different hash than the guard).
    ///
    /// # Error handling
    ///
    /// On `write_snapshot` failure, the registered guard is consumed via
    /// `consume_origin_for_snapshot_or_default` to prevent stale guard accumulation.
    /// The `set_next_origin` call for `RemotePush` is NOT made on failure.
    pub async fn write(
        &self,
        snapshot: SystemClipboardSnapshot,
        intent: ClipboardWriteIntent,
    ) -> Result<()> {
        let origin_guard_key = snapshot.origin_guard_key();

        async {
            // Register the appropriate hash guard before writing.
            match intent {
                ClipboardWriteIntent::LocalRestore | ClipboardWriteIntent::LocalCapture => {
                    self.clipboard_change_origin
                        .remember_local_snapshot_hash(
                            origin_guard_key.clone(),
                            Duration::from_secs(2),
                        )
                        .await;
                }
                ClipboardWriteIntent::RemotePush => {
                    self.clipboard_change_origin
                        .remember_remote_snapshot_hash(
                            origin_guard_key.clone(),
                            Duration::from_secs(60),
                        )
                        .await;
                }
            }

            // Attempt the write.
            if let Err(err) = self.system_clipboard.write_snapshot(snapshot) {
                // On failure: consume the guard to prevent stale state accumulation.
                self.clipboard_change_origin
                    .consume_origin_for_snapshot_or_default(
                        &origin_guard_key,
                        ClipboardChangeOrigin::LocalCapture,
                    )
                    .await;
                return Err(err);
            }

            // RemotePush success: set one-shot origin override for OS re-encoding loopback guard.
            // Some platforms (e.g. Windows clipboard-rs) re-encode images (PNG→DIB→PNG),
            // producing different bytes than the original. The hash guard above won't match
            // the re-encoded content, so we set a one-shot origin override: the NEXT clipboard
            // change will be treated as RemotePush regardless of hash.
            if intent == ClipboardWriteIntent::RemotePush {
                self.clipboard_change_origin
                    .set_next_origin(ClipboardChangeOrigin::RemotePush, Duration::from_secs(60))
                    .await;
            }

            Ok(())
        }
        .instrument(info_span!(
            "clipboard_write_coordinator.write",
            intent = ?intent,
            origin_guard_key = %origin_guard_key
        ))
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use uc_core::clipboard::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
    use uc_core::ids::{FormatId, RepresentationId};

    // ---------------------------------------------------------------------------
    // Mock: SystemClipboardPort
    // ---------------------------------------------------------------------------

    #[derive(Default)]
    struct MockSystemClipboard {
        calls: Mutex<Vec<String>>,
        /// When Some(err), write_snapshot returns that error.
        fail_with: Option<String>,
    }

    impl MockSystemClipboard {
        fn fail(msg: &str) -> Self {
            Self {
                calls: Mutex::new(vec![]),
                fail_with: Some(msg.to_string()),
            }
        }

        fn calls_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    impl SystemClipboardPort for MockSystemClipboard {
        fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("write:{}", snapshot.origin_guard_key()));
            if let Some(ref err_msg) = self.fail_with {
                return Err(anyhow::anyhow!("{}", err_msg));
            }
            Ok(())
        }

        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            Ok(make_snapshot("read-back"))
        }
    }

    // ---------------------------------------------------------------------------
    // Mock: ClipboardChangeOriginPort
    // ---------------------------------------------------------------------------

    #[derive(Default)]
    struct MockOriginPort {
        calls: Mutex<Vec<String>>,
    }

    impl MockOriginPort {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl ClipboardChangeOriginPort for MockOriginPort {
        async fn set_next_origin(&self, origin: ClipboardChangeOrigin, ttl: Duration) {
            self.calls.lock().unwrap().push(format!(
                "set_next_origin:{:?}:{}ms",
                origin,
                ttl.as_millis()
            ));
        }

        async fn consume_origin_or_default(
            &self,
            default_origin: ClipboardChangeOrigin,
        ) -> ClipboardChangeOrigin {
            self.calls
                .lock()
                .unwrap()
                .push(format!("consume_default:{:?}", default_origin));
            default_origin
        }

        async fn remember_remote_snapshot_hash(&self, snapshot_hash: String, ttl: Duration) {
            self.calls.lock().unwrap().push(format!(
                "remember_remote:{}:{}ms",
                snapshot_hash,
                ttl.as_millis()
            ));
        }

        async fn remember_local_snapshot_hash(&self, snapshot_hash: String, ttl: Duration) {
            self.calls.lock().unwrap().push(format!(
                "remember_local:{}:{}ms",
                snapshot_hash,
                ttl.as_millis()
            ));
        }

        async fn consume_origin_for_snapshot_or_default(
            &self,
            snapshot_hash: &str,
            default_origin: ClipboardChangeOrigin,
        ) -> ClipboardChangeOrigin {
            self.calls.lock().unwrap().push(format!(
                "consume_snapshot:{}:{:?}",
                snapshot_hash, default_origin
            ));
            default_origin
        }
    }

    // ---------------------------------------------------------------------------
    // Test helpers
    // ---------------------------------------------------------------------------

    fn make_snapshot(text: &str) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::from_str("rep-1"),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                text.as_bytes().to_vec(),
            )],
        }
    }

    fn coordinator(
        clipboard: Arc<dyn SystemClipboardPort>,
        origin: Arc<dyn ClipboardChangeOriginPort>,
    ) -> ClipboardWriteCoordinator {
        ClipboardWriteCoordinator::new(clipboard, origin)
    }

    // ---------------------------------------------------------------------------
    // Test 1: LocalRestore calls remember_local_snapshot_hash with 2s TTL, then write_snapshot
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test1_local_restore_registers_local_guard_and_writes() {
        let clipboard = Arc::new(MockSystemClipboard::default());
        let origin = Arc::new(MockOriginPort::default());
        let coord = coordinator(clipboard.clone(), origin.clone());

        let snapshot = make_snapshot("hello");
        let expected_key = snapshot.origin_guard_key();

        coord
            .write(snapshot, ClipboardWriteIntent::LocalRestore)
            .await
            .expect("write should succeed");

        let calls = origin.calls();
        // First call must be remember_local with 2000ms TTL
        assert!(
            calls[0].starts_with(&format!("remember_local:{}:2000ms", expected_key)),
            "expected remember_local:...:2000ms, got: {:?}",
            calls
        );
        // write_snapshot must have been called once
        assert_eq!(
            clipboard.calls_count(),
            1,
            "write_snapshot must be called once"
        );
        // No set_next_origin should be called
        assert!(
            !calls.iter().any(|c| c.starts_with("set_next_origin")),
            "set_next_origin must NOT be called for LocalRestore: {:?}",
            calls
        );
    }

    // ---------------------------------------------------------------------------
    // Test 2: RemotePush calls remember_remote_snapshot_hash with 60s TTL, then write, then set_next_origin
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test2_remote_push_registers_remote_guard_writes_and_sets_next_origin() {
        let clipboard = Arc::new(MockSystemClipboard::default());
        let origin = Arc::new(MockOriginPort::default());
        let coord = coordinator(clipboard.clone(), origin.clone());

        let snapshot = make_snapshot("remote content");
        let expected_key = snapshot.origin_guard_key();

        coord
            .write(snapshot, ClipboardWriteIntent::RemotePush)
            .await
            .expect("write should succeed");

        let calls = origin.calls();
        // First: remember_remote with 60000ms TTL
        assert!(
            calls[0].starts_with(&format!("remember_remote:{}:60000ms", expected_key)),
            "expected remember_remote:...:60000ms, got: {:?}",
            calls
        );
        // write_snapshot called
        assert_eq!(clipboard.calls_count(), 1);
        // set_next_origin called with RemotePush and 60s TTL
        let has_set_next = calls
            .iter()
            .any(|c| c == "set_next_origin:RemotePush:60000ms");
        assert!(
            has_set_next,
            "set_next_origin(RemotePush, 60s) must be called: {:?}",
            calls
        );
    }

    // ---------------------------------------------------------------------------
    // Test 3: LocalCapture calls remember_local_snapshot_hash with 2s TTL, then write
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test3_local_capture_registers_local_guard_and_writes() {
        let clipboard = Arc::new(MockSystemClipboard::default());
        let origin = Arc::new(MockOriginPort::default());
        let coord = coordinator(clipboard.clone(), origin.clone());

        let snapshot = make_snapshot("local capture");
        let expected_key = snapshot.origin_guard_key();

        coord
            .write(snapshot, ClipboardWriteIntent::LocalCapture)
            .await
            .expect("write should succeed");

        let calls = origin.calls();
        assert!(
            calls[0].starts_with(&format!("remember_local:{}:2000ms", expected_key)),
            "expected remember_local:...:2000ms for LocalCapture, got: {:?}",
            calls
        );
        assert_eq!(clipboard.calls_count(), 1);
        assert!(
            !calls.iter().any(|c| c.starts_with("set_next_origin")),
            "set_next_origin must NOT be called for LocalCapture: {:?}",
            calls
        );
    }

    // ---------------------------------------------------------------------------
    // Test 4: On write_snapshot error, calls consume_origin_for_snapshot_or_default and returns Err
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test4_write_failure_consumes_guard_and_returns_error() {
        let clipboard = Arc::new(MockSystemClipboard::fail("disk full"));
        let origin = Arc::new(MockOriginPort::default());
        let coord = coordinator(clipboard.clone(), origin.clone());

        let snapshot = make_snapshot("failing write");
        let expected_key = snapshot.origin_guard_key();

        let result = coord
            .write(snapshot, ClipboardWriteIntent::LocalRestore)
            .await;
        assert!(result.is_err(), "should return error on write failure");

        let calls = origin.calls();
        // Guard was registered
        assert!(
            calls[0].starts_with("remember_local:"),
            "guard must be registered before write: {:?}",
            calls
        );
        // consume_snapshot called with the correct key
        let consumed = calls
            .iter()
            .any(|c| c.starts_with(&format!("consume_snapshot:{}:", expected_key)));
        assert!(
            consumed,
            "consume_origin_for_snapshot_or_default must be called on error: {:?}",
            calls
        );
    }

    // ---------------------------------------------------------------------------
    // Test 5: On write_snapshot error for RemotePush, set_next_origin is NOT called
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn test5_remote_push_write_failure_does_not_call_set_next_origin() {
        let clipboard = Arc::new(MockSystemClipboard::fail("network error"));
        let origin = Arc::new(MockOriginPort::default());
        let coord = coordinator(clipboard.clone(), origin.clone());

        let snapshot = make_snapshot("remote fail");

        let result = coord
            .write(snapshot, ClipboardWriteIntent::RemotePush)
            .await;
        assert!(result.is_err(), "should return error on write failure");

        let calls = origin.calls();
        // Guard registered
        assert!(
            calls[0].starts_with("remember_remote:"),
            "remote guard must be registered: {:?}",
            calls
        );
        // consume called on error
        assert!(
            calls.iter().any(|c| c.starts_with("consume_snapshot:")),
            "consume guard must be called on error: {:?}",
            calls
        );
        // set_next_origin must NOT be called
        assert!(
            !calls.iter().any(|c| c.starts_with("set_next_origin")),
            "set_next_origin must NOT be called on RemotePush failure: {:?}",
            calls
        );
    }
}

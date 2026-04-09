use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, warn};

use clipboard_rs::ClipboardHandler;

use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::SystemClipboardPort;

/// Minimal platform event type retained for clipboard watcher channel.
/// Full PlatformEvent (ipc module) was removed in Phase 65; only the
/// ClipboardChanged variant is needed by the watcher.
#[derive(Debug, Clone)]
pub enum PlatformEvent {
    /// Local clipboard content changed.
    ClipboardChanged { snapshot: SystemClipboardSnapshot },
}

/// Channel sender for platform events emitted by the clipboard watcher.
pub type PlatformEventSender = tokio::sync::mpsc::Sender<PlatformEvent>;

/// Time window to suppress rapid consecutive file clipboard events.
/// macOS fires multiple events when copying files (e.g. APFS→resolved path transition)
/// where content bytes may differ slightly.
const FILE_DEDUP_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

pub struct ClipboardWatcher {
    local_clipboard: Arc<dyn SystemClipboardPort>,
    sender: PlatformEventSender,
    last_meaningful_dedupe_key: Option<String>,
    last_file_emit_time: Option<Instant>,
}

impl ClipboardWatcher {
    pub fn new(local_clipboard: Arc<dyn SystemClipboardPort>, sender: PlatformEventSender) -> Self {
        Self {
            local_clipboard,
            sender,
            last_meaningful_dedupe_key: None,
            last_file_emit_time: None,
        }
    }
}

fn is_file_representation(rep: &uc_core::ObservedClipboardRepresentation) -> bool {
    uc_core::clipboard::is_file_mime_or_format(rep.mime.as_ref(), &rep.format_id)
}

fn dedupe_key(snapshot: &SystemClipboardSnapshot) -> Option<String> {
    snapshot.meaningful_origin_key()
}

/// Returns true if any representation in the snapshot is a file representation.
fn snapshot_has_files(snapshot: &SystemClipboardSnapshot) -> bool {
    snapshot.representations.iter().any(is_file_representation)
}

impl ClipboardHandler for ClipboardWatcher {
    fn on_clipboard_change(&mut self) {
        match self.local_clipboard.read_snapshot() {
            Ok(snapshot) => {
                let current_dedupe_key = dedupe_key(&snapshot);
                if let Some(key) = current_dedupe_key.as_ref() {
                    if self.last_meaningful_dedupe_key.as_deref() == Some(key.as_str()) {
                        debug!(
                            dedupe_key = %key,
                            "Skipping duplicated meaningful clipboard snapshot"
                        );
                        return;
                    }
                }

                // Time-window suppression for file snapshots: macOS fires
                // multiple clipboard events when copying files (APFS→resolved
                // path transition) where content bytes may differ slightly.
                if snapshot_has_files(&snapshot) {
                    let now = Instant::now();
                    if let Some(last) = self.last_file_emit_time {
                        if now.duration_since(last) < FILE_DEDUP_WINDOW {
                            debug!(
                                elapsed_ms = now.duration_since(last).as_millis(),
                                "Suppressing rapid consecutive file clipboard event"
                            );
                            return;
                        }
                    }
                }

                if let Err(err) = self
                    .sender
                    .try_send(PlatformEvent::ClipboardChanged { snapshot })
                {
                    warn!(
                        error_kind = "notify_channel_send_failed",
                        retryable = true,
                        error = %err,
                        "Failed to notify clipboard change"
                    );
                } else {
                    if current_dedupe_key
                        .as_ref()
                        .is_some_and(|k| k.starts_with("files:"))
                    {
                        self.last_file_emit_time = Some(Instant::now());
                    }
                    if let Some(key) = current_dedupe_key {
                        self.last_meaningful_dedupe_key = Some(key);
                    }
                }
            }

            Err(e) => {
                warn!(
                    error_kind = "platform_clipboard_read_failed",
                    retryable = true,
                    error = %e,
                    "Failed to read clipboard snapshot"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};

    struct SequenceClipboard {
        snapshots: Mutex<VecDeque<SystemClipboardSnapshot>>,
    }

    impl SystemClipboardPort for SequenceClipboard {
        fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
            let mut guard = self.snapshots.lock().expect("sequence clipboard lock");
            if let Some(next) = guard.pop_front() {
                Ok(next)
            } else {
                Err(anyhow::anyhow!("no snapshot left in test sequence"))
            }
        }

        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> Result<()> {
            Ok(())
        }
    }

    fn text_snapshot(content: &str) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::from("rep-1"),
                FormatId::from("text"),
                Some(MimeType::text_plain()),
                content.as_bytes().to_vec(),
            )],
        }
    }

    fn raw_snapshot(content: &[u8]) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::from("rep-raw"),
                FormatId::from("UnknownRaw"),
                None,
                content.to_vec(),
            )],
        }
    }

    fn browser_like_text_snapshot(plain_text: &str, html_payload: &str) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![
                ObservedClipboardRepresentation::new(
                    RepresentationId::from("rep-plain"),
                    FormatId::from("text"),
                    Some(MimeType::text_plain()),
                    plain_text.as_bytes().to_vec(),
                ),
                ObservedClipboardRepresentation::new(
                    RepresentationId::from("rep-html"),
                    FormatId::from("html"),
                    Some(MimeType::text_html()),
                    html_payload.as_bytes().to_vec(),
                ),
            ],
        }
    }

    #[test]
    fn suppresses_duplicate_meaningful_snapshot_even_if_raw_events_interleave() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let clipboard = Arc::new(SequenceClipboard {
            snapshots: Mutex::new(VecDeque::from(vec![
                text_snapshot("hello"),
                raw_snapshot(&[1]),
                text_snapshot("hello"),
            ])),
        });
        let mut watcher = ClipboardWatcher::new(clipboard, tx);

        watcher.on_clipboard_change();
        watcher.on_clipboard_change();
        watcher.on_clipboard_change();

        // text snapshot is emitted only once; raw snapshot still passes through
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn allows_same_meaningful_snapshot_after_meaningful_change() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let clipboard = Arc::new(SequenceClipboard {
            snapshots: Mutex::new(VecDeque::from(vec![
                text_snapshot("hello"),
                text_snapshot("world"),
                text_snapshot("hello"),
            ])),
        });
        let mut watcher = ClipboardWatcher::new(clipboard, tx);

        watcher.on_clipboard_change();
        watcher.on_clipboard_change();
        watcher.on_clipboard_change();

        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn suppresses_duplicate_when_plain_text_same_but_html_representation_changes() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let clipboard = Arc::new(SequenceClipboard {
            snapshots: Mutex::new(VecDeque::from(vec![
                browser_like_text_snapshot("https://example.com", "<a>v1</a>"),
                browser_like_text_snapshot("https://example.com", "<a>v2</a>"),
            ])),
        });
        let mut watcher = ClipboardWatcher::new(clipboard, tx);

        watcher.on_clipboard_change();
        watcher.on_clipboard_change();

        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }

    fn file_snapshot(uri_content: &str) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::from("rep-files"),
                FormatId::from("files"),
                Some(MimeType("text/uri-list".to_string())),
                uri_content.as_bytes().to_vec(),
            )],
        }
    }

    #[test]
    fn file_dedupe_key_is_generated() {
        let snap = file_snapshot("file:///tmp/test.docx");
        let key = dedupe_key(&snap);
        assert!(key.is_some());
        assert!(key.as_ref().is_some_and(|k| k.starts_with("files:")));
    }

    #[test]
    fn suppresses_duplicate_file_snapshot_by_hash() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let clipboard = Arc::new(SequenceClipboard {
            snapshots: Mutex::new(VecDeque::from(vec![
                file_snapshot("file:///tmp/test.docx"),
                file_snapshot("file:///tmp/test.docx"),
            ])),
        });
        let mut watcher = ClipboardWatcher::new(clipboard, tx);

        watcher.on_clipboard_change();
        watcher.on_clipboard_change();

        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn file_time_window_suppresses_rapid_different_content() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        // Two file snapshots with different content (different hashes),
        // but the time window should suppress the second one.
        let clipboard = Arc::new(SequenceClipboard {
            snapshots: Mutex::new(VecDeque::from(vec![
                file_snapshot("file:///tmp/test.docx"),
                file_snapshot("file:///tmp/test-resolved.docx"),
            ])),
        });
        let mut watcher = ClipboardWatcher::new(clipboard, tx);

        watcher.on_clipboard_change();
        // Immediately call again — within 500ms window
        watcher.on_clipboard_change();

        assert!(rx.try_recv().is_ok());
        assert!(
            rx.try_recv().is_err(),
            "Second file event within time window should be suppressed"
        );
    }

    #[test]
    fn is_file_representation_matches_files_format() {
        let rep = ObservedClipboardRepresentation::new(
            RepresentationId::from("r1"),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///tmp/x".to_vec(),
        );
        assert!(is_file_representation(&rep));
    }

    #[test]
    fn is_file_representation_matches_public_file_url() {
        let rep = ObservedClipboardRepresentation::new(
            RepresentationId::from("r1"),
            FormatId::from("public.file-url"),
            None,
            b"file:///tmp/x".to_vec(),
        );
        assert!(is_file_representation(&rep));
    }
}

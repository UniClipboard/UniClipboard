use std::sync::Arc;
use tracing::{debug, warn};

use clipboard_rs::ClipboardHandler;

use crate::ipc::PlatformEvent;
use crate::runtime::event_bus::PlatformEventSender;
use uc_core::ports::SystemClipboardPort;

pub struct ClipboardWatcher {
    local_clipboard: Arc<dyn SystemClipboardPort>,
    sender: PlatformEventSender,
    last_meaningful_snapshot_hash: Option<String>,
}

impl ClipboardWatcher {
    pub fn new(local_clipboard: Arc<dyn SystemClipboardPort>, sender: PlatformEventSender) -> Self {
        Self {
            local_clipboard,
            sender,
            last_meaningful_snapshot_hash: None,
        }
    }
}

impl ClipboardHandler for ClipboardWatcher {
    fn on_clipboard_change(&mut self) {
        match self.local_clipboard.read_snapshot() {
            Ok(snapshot) => {
                let snapshot_hash = snapshot.snapshot_hash().to_string();
                let is_meaningful = snapshot
                    .representations
                    .iter()
                    .any(|rep| rep.mime.is_some());
                if is_meaningful
                    && self.last_meaningful_snapshot_hash.as_deref() == Some(snapshot_hash.as_str())
                {
                    debug!(
                        snapshot_hash = %snapshot_hash,
                        "Skipping duplicated meaningful clipboard snapshot"
                    );
                    return;
                }
                if let Err(err) = self
                    .sender
                    .try_send(PlatformEvent::ClipboardChanged { snapshot })
                {
                    warn!(error = %err, "Failed to notify clipboard change");
                } else if is_meaningful {
                    self.last_meaningful_snapshot_hash = Some(snapshot_hash);
                }
            }

            Err(e) => {
                warn!(error = %e, "Failed to read clipboard snapshot");
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
            representations: vec![ObservedClipboardRepresentation {
                id: RepresentationId::from("rep-1"),
                format_id: FormatId::from("text"),
                mime: Some(MimeType::text_plain()),
                bytes: content.as_bytes().to_vec(),
            }],
        }
    }

    fn raw_snapshot(content: &[u8]) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![ObservedClipboardRepresentation {
                id: RepresentationId::from("rep-raw"),
                format_id: FormatId::from("UnknownRaw"),
                mime: None,
                bytes: content.to_vec(),
            }],
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
}

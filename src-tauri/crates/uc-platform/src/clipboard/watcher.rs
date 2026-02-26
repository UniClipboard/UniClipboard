use std::sync::Arc;
use tracing::{debug, warn};

use clipboard_rs::ClipboardHandler;

use crate::ipc::PlatformEvent;
use crate::runtime::event_bus::PlatformEventSender;
use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::SystemClipboardPort;

pub struct ClipboardWatcher {
    local_clipboard: Arc<dyn SystemClipboardPort>,
    sender: PlatformEventSender,
    last_meaningful_dedupe_key: Option<String>,
}

impl ClipboardWatcher {
    pub fn new(local_clipboard: Arc<dyn SystemClipboardPort>, sender: PlatformEventSender) -> Self {
        Self {
            local_clipboard,
            sender,
            last_meaningful_dedupe_key: None,
        }
    }
}

fn is_plain_text_representation(rep: &uc_core::ObservedClipboardRepresentation) -> bool {
    if let Some(mime) = rep.mime.as_ref() {
        let mime_str = mime.as_str();
        if mime_str.eq_ignore_ascii_case("text/plain")
            || mime_str.to_ascii_lowercase().starts_with("text/plain;")
            || mime_str.eq_ignore_ascii_case("public.utf8-plain-text")
        {
            return true;
        }
    }
    rep.format_id.eq_ignore_ascii_case("text")
}

fn is_text_representation(rep: &uc_core::ObservedClipboardRepresentation) -> bool {
    if let Some(mime) = rep.mime.as_ref() {
        let mime_str = mime.as_str();
        if mime_str.starts_with("text/") || mime_str.eq_ignore_ascii_case("public.utf8-plain-text")
        {
            return true;
        }
    }
    rep.format_id.eq_ignore_ascii_case("text")
        || rep.format_id.eq_ignore_ascii_case("html")
        || rep.format_id.eq_ignore_ascii_case("rtf")
}

fn is_image_representation(rep: &uc_core::ObservedClipboardRepresentation) -> bool {
    rep.mime
        .as_ref()
        .is_some_and(|mime| mime.as_str().starts_with("image/"))
        || rep.format_id.eq_ignore_ascii_case("image")
}

fn dedupe_key(snapshot: &SystemClipboardSnapshot) -> Option<String> {
    if let Some(rep) = snapshot
        .representations
        .iter()
        .find(|rep| is_plain_text_representation(rep))
    {
        let hash = rep.content_hash();
        return Some(format!("text:{}", hash.0));
    }

    if let Some(rep) = snapshot
        .representations
        .iter()
        .find(|rep| is_text_representation(rep))
    {
        let hash = rep.content_hash();
        return Some(format!("rich-text:{}", hash.0));
    }

    if let Some(rep) = snapshot
        .representations
        .iter()
        .find(|rep| is_image_representation(rep))
    {
        let hash = rep.content_hash();
        return Some(format!("image:{}", hash.0));
    }

    None
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
                if let Err(err) = self
                    .sender
                    .try_send(PlatformEvent::ClipboardChanged { snapshot })
                {
                    warn!(error = %err, "Failed to notify clipboard change");
                } else if let Some(key) = current_dedupe_key {
                    self.last_meaningful_dedupe_key = Some(key);
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

    fn browser_like_text_snapshot(plain_text: &str, html_payload: &str) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![
                ObservedClipboardRepresentation {
                    id: RepresentationId::from("rep-plain"),
                    format_id: FormatId::from("text"),
                    mime: Some(MimeType::text_plain()),
                    bytes: plain_text.as_bytes().to_vec(),
                },
                ObservedClipboardRepresentation {
                    id: RepresentationId::from("rep-html"),
                    format_id: FormatId::from("html"),
                    mime: Some(MimeType::text_html()),
                    bytes: html_payload.as_bytes().to_vec(),
                },
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
}

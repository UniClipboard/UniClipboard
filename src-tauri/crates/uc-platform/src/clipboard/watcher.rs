use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, warn};

// `clipboard_rs::ClipboardHandler` is only required by the macOS / Windows
// adapter that wraps `ClipboardWatcherContext`. The native Wayland and X11
// (x11rb) adapters drive `notify_change` directly, so as of Phase 4 the
// trait impl is gated to the platforms that still need `clipboard_rs`.
#[cfg(any(target_os = "macos", target_os = "windows"))]
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

impl ClipboardWatcher {
    /// Read a snapshot from the OS clipboard, run dedup, and forward to the
    /// channel. Called by every platform event loop on each detected change
    /// (XFIXES selection-notify on X11, `changeCount` tick on macOS,
    /// `WM_CLIPBOARDUPDATE` on Windows).
    ///
    /// Errors are logged at warn level and never propagated — a transient OS
    /// read failure must not bring down the watcher loop.
    pub fn notify_change(&mut self) {
        match self.local_clipboard.read_snapshot() {
            Ok(snapshot) => self.emit_with_dedup(snapshot),
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

    /// Forward an already-captured snapshot through the dedup pipeline.
    ///
    /// Used by event loops that obtain the snapshot bytes directly from the
    /// OS notification (Wayland `wlr-data-control` Selection event hands the
    /// caller a `DataControlOffer` plus its mime list — pulling bytes via
    /// `pipe + receive` from the same loop is much cheaper than going back
    /// through `SystemClipboardPort::read_snapshot`, which would open a
    /// fresh wayland connection round-trip).
    pub fn notify_with_snapshot(&mut self, snapshot: SystemClipboardSnapshot) {
        self.emit_with_dedup(snapshot);
    }

    fn emit_with_dedup(&mut self, snapshot: SystemClipboardSnapshot) {
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
}

// `ClipboardHandler` adapter for platforms whose event loop is built on top of
// `clipboard_rs::ClipboardWatcherContext` (macOS/Windows). Linux's native
// Wayland and X11 (x11rb) implementations call
// [`ClipboardWatcher::notify_change`] directly and do not go through this
// trait.
#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ClipboardHandler for ClipboardWatcher {
    fn on_clipboard_change(&mut self) {
        self.notify_change();
    }
}

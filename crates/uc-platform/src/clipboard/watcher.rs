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

/// Time window to suppress a rapid burst of same-size image clipboard events.
///
/// On X11 a clipboard owner (or a desktop clipboard manager such as Klipper /
/// CopyQ / GPaste) can re-assert selection ownership several times per second,
/// and some sources serialize the *same* image with non-deterministic padding /
/// alpha bytes on every read. The content hash — and therefore
/// `meaningful_origin_key` (`image:<hash>`) — then differs on every read, so
/// the key-based dedup above never fires and each read becomes a brand-new
/// entry + blob + outbound sync. Observed in the wild as a ~400 ms self-feeding
/// storm (see issue #957) that filled disk with hundreds of identical-looking
/// images.
///
/// Byte size is a cheap, stable proxy for image identity here: the storm keeps
/// identical dimensions while only the hash churns. A genuinely different image
/// (different byte size) passes through immediately; only a same-size image
/// arriving within this window of the previous image is suppressed. The window
/// is refreshed on every suppressed event so a sustained burst stays muted as
/// long as it keeps firing faster than the window.
const IMAGE_DEDUP_WINDOW: std::time::Duration = std::time::Duration::from_millis(1500);

pub struct ClipboardWatcher {
    local_clipboard: Arc<dyn SystemClipboardPort>,
    sender: PlatformEventSender,
    last_meaningful_dedupe_key: Option<String>,
    last_file_emit_time: Option<Instant>,
    /// `(observed_at, size_bytes)` of the most recent image-dominant snapshot,
    /// updated on every image observation (emitted or suppressed). Drives the
    /// [`IMAGE_DEDUP_WINDOW`] storm guard.
    last_image_seen: Option<(Instant, i64)>,
}

impl ClipboardWatcher {
    pub fn new(local_clipboard: Arc<dyn SystemClipboardPort>, sender: PlatformEventSender) -> Self {
        Self {
            local_clipboard,
            sender,
            last_meaningful_dedupe_key: None,
            last_file_emit_time: None,
            last_image_seen: None,
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

        // Image storm guard: an image-dominant snapshot whose image byte size
        // matches the previous image within `IMAGE_DEDUP_WINDOW` is treated as a
        // re-read of the same image (the hash churns under us; see
        // `IMAGE_DEDUP_WINDOW`). Keyed off the image representation's own size
        // (not the whole-snapshot total) so co-resident metadata reps can't make
        // two genuinely different images look identically sized, nor mask the
        // storm's stable image size. Computed before the value moves into
        // `try_send`.
        let image_size = current_dedupe_key
            .as_ref()
            .filter(|k| k.starts_with("image:"))
            .and_then(|_| snapshot.primary_image_size_bytes());
        if let Some(size) = image_size {
            let now = Instant::now();
            if let Some((last, last_size)) = self.last_image_seen {
                if last_size == size && now.duration_since(last) < IMAGE_DEDUP_WINDOW {
                    // Refresh the window so a sustained burst stays suppressed.
                    self.last_image_seen = Some((now, size));
                    debug!(
                        size_bytes = size,
                        elapsed_ms = now.duration_since(last).as_millis(),
                        "Suppressing rapid same-size image clipboard event (storm guard)"
                    );
                    return;
                }
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
            if let Some(size) = image_size {
                self.last_image_seen = Some((Instant::now(), size));
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

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::clipboard::{MimeType, ObservedClipboardRepresentation};
    use uc_core::ids::{FormatId, RepresentationId};

    /// `local_clipboard` is unused on the `notify_with_snapshot` path that
    /// these tests exercise, so a do-nothing stub suffices.
    struct StubClipboard;
    impl SystemClipboardPort for StubClipboard {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            Ok(SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            })
        }
        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn watcher() -> (ClipboardWatcher, tokio::sync::mpsc::Receiver<PlatformEvent>) {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        (ClipboardWatcher::new(Arc::new(StubClipboard), tx), rx)
    }

    /// Image snapshot of exactly `size` bytes filled with `fill`. Varying
    /// `fill` keeps the byte size constant while changing the blake3 hash —
    /// exactly the #957 storm shape where the same image serializes to
    /// different bytes on every read.
    fn image(size: usize, fill: u8) -> SystemClipboardSnapshot {
        let rep = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("image"),
            Some(MimeType("image/bmp".to_string())),
            vec![fill; size],
        );
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![rep],
        }
    }

    /// Image-dominant snapshot carrying a primary image rep plus a second
    /// (image) representation, so the whole-snapshot total differs from the
    /// primary image's own size. `meaningful_origin_key` keys on the first
    /// image rep, so the key stays `image:`.
    fn image_with_secondary(
        primary_size: usize,
        primary_fill: u8,
        secondary_size: usize,
        secondary_fill: u8,
    ) -> SystemClipboardSnapshot {
        let primary = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("image"),
            Some(MimeType("image/bmp".to_string())),
            vec![primary_fill; primary_size],
        );
        let secondary = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("image"),
            Some(MimeType("image/png".to_string())),
            vec![secondary_fill; secondary_size],
        );
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![primary, secondary],
        }
    }

    fn drain(rx: &mut tokio::sync::mpsc::Receiver<PlatformEvent>) -> usize {
        let mut n = 0;
        while rx.try_recv().is_ok() {
            n += 1;
        }
        n
    }

    #[test]
    fn same_size_image_with_churning_hash_is_storm_guarded() {
        // Reproduces issue #957: an X11 source re-emits the *same* image with
        // a different byte pattern (different hash) every few hundred ms, so
        // the meaningful-key dedup never fires. The storm guard must collapse
        // the burst to a single emit.
        let (mut w, mut rx) = watcher();
        for fill in 0u8..16 {
            w.notify_with_snapshot(image(4096, fill));
        }
        assert_eq!(
            drain(&mut rx),
            1,
            "same-size image burst with churning hash must collapse to one emit"
        );
    }

    #[test]
    fn differently_sized_images_all_pass() {
        // Genuinely different images (different byte size) must not be dropped.
        let (mut w, mut rx) = watcher();
        w.notify_with_snapshot(image(1000, 1));
        w.notify_with_snapshot(image(2000, 2));
        w.notify_with_snapshot(image(3000, 3));
        assert_eq!(drain(&mut rx), 3, "distinct-size images must all emit");
    }

    #[test]
    fn identical_image_collapses_via_key_dedup() {
        // Byte-identical images share a hash and collapse via the pre-existing
        // meaningful-key dedup; the storm guard must not regress this.
        let (mut w, mut rx) = watcher();
        w.notify_with_snapshot(image(1000, 7));
        w.notify_with_snapshot(image(1000, 7));
        assert_eq!(drain(&mut rx), 1);
    }

    #[test]
    fn different_images_with_equal_total_size_both_pass() {
        // Two genuinely different images whose *primary image* sizes differ
        // (100 vs 120) but whose whole-snapshot totals coincide (both 150).
        // Keying the guard off the snapshot total would wrongly suppress the
        // second; keying off the image's own size lets both through.
        let (mut w, mut rx) = watcher();
        w.notify_with_snapshot(image_with_secondary(100, 1, 50, 9)); // total 150, image 100
        w.notify_with_snapshot(image_with_secondary(120, 2, 30, 8)); // total 150, image 120
        assert_eq!(
            drain(&mut rx),
            2,
            "different images must emit even when snapshot totals match"
        );
    }

    #[test]
    fn same_image_size_with_churning_secondary_is_storm_guarded() {
        // The image's own size is stable across the burst (the #957 shape)
        // while a co-resident rep's size + content churn. Keying off the stable
        // image size still collapses the burst; keying off the (churning) total
        // would let every event through.
        let (mut w, mut rx) = watcher();
        for fill in 0u8..8 {
            w.notify_with_snapshot(image_with_secondary(4096, fill, 100 + fill as usize, fill));
        }
        assert_eq!(
            drain(&mut rx),
            1,
            "stable image size must storm-guard despite a churning secondary rep"
        );
    }
}

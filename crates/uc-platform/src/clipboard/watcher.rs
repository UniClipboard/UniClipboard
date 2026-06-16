use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

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
const FILE_DEDUP_WINDOW: Duration = Duration::from_millis(500);

/// Longest gap between two consecutive reads that still counts as part of the
/// *same* ongoing image burst (issue #957 storm guard).
///
/// On X11 a clipboard owner — the source app, or a desktop clipboard manager
/// such as Klipper / CopyQ / GPaste — can re-assert selection ownership on a
/// cadence; the freedesktop ClipboardManager spec even mandates that owners
/// reacquire the selection whenever their content or metadata changes, so an
/// `XfixesSelectionNotify` is *not* proof of new content. Some sources also
/// re-serialize the *same* image with non-deterministic padding / alpha bytes
/// on every read, so the content hash — and therefore `meaningful_origin_key`
/// (`image:<hash>`) — churns and the key-based dedup above never fires. Each
/// read would otherwise become a brand-new entry + blob + outbound sync: the
/// self-feeding storm of issue #957 that filled disk with thousands of
/// identical-looking images.
///
/// Byte size is a cheap, stable proxy for image identity here: the storm keeps
/// identical dimensions while only the hash churns. The guard latches on the
/// previous image's byte size and suppresses any same-size image arriving
/// within this gap; the latch is refreshed on every suppressed read, so a
/// sustained burst of *any* cadence below this gap stays muted indefinitely.
/// The original #957 fix used a fixed 1500 ms window, which a slower ~1.83 s
/// real-world storm slipped straight past — the gap between re-reads exceeded
/// the window, so the suppression branch was never entered. This gap is sized
/// well above plausible re-assert cadences; once a burst genuinely stops the
/// latch lapses after this much idle time, so a deliberate later re-copy of a
/// same-size image still emits, and any intervening non-image copy breaks it
/// immediately.
///
/// Note this is a heuristic guard, not a content-equality check: two *different*
/// images that serialize to the exact same byte length, copied back-to-back
/// within this gap with nothing in between, would be wrongly collapsed. A
/// content-stable image fingerprint (hashing decoded pixels rather than raw
/// serialized bytes) is the deeper fix and would let the key-based dedup above
/// handle that case without the size proxy.
const IMAGE_STORM_MAX_GAP: Duration = Duration::from_secs(5);

pub struct ClipboardWatcher {
    local_clipboard: Arc<dyn SystemClipboardPort>,
    sender: PlatformEventSender,
    last_meaningful_dedupe_key: Option<String>,
    last_file_emit_time: Option<Instant>,
    /// `(observed_at, size_bytes)` of the most recent image-dominant snapshot.
    /// Updated on every image observation (emitted or suppressed) and cleared
    /// when a non-image snapshot is emitted. Drives the [`IMAGE_STORM_MAX_GAP`]
    /// storm guard.
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

/// Largest serialized image we will decode to fingerprint. Above this the
/// decode + pixel hash is too costly to run on the watcher path, so we skip it
/// and fall back to the size-based storm latch.
const MAX_FINGERPRINT_DECODE_BYTES: usize = 64 * 1024 * 1024;

/// A decode-stable identity for a clipboard image: the blake3 hash of its
/// canonical RGBA8 pixel buffer plus dimensions.
///
/// Two reads of the *same* image produce the same fingerprint even when the
/// source re-serializes it with churning padding / alpha / metadata bytes (the
/// issue #957 storm shape) — decoding normalizes the container away, so unlike
/// the raw-byte content hash this does not churn. Returns `None` for images we
/// can't decode (unknown/unsupported format such as `image/xpm`, or corrupt
/// bytes) or that exceed [`MAX_FINGERPRINT_DECODE_BYTES`]; the caller then
/// falls back to the raw-byte key plus the size-based storm latch.
fn stable_image_fingerprint(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > MAX_FINGERPRINT_DECODE_BYTES {
        return None;
    }
    let image = image::load_from_memory(bytes).ok()?;
    let rgba = image.to_rgba8();
    let mut hasher = blake3::Hasher::new();
    hasher.update(&rgba.width().to_le_bytes());
    hasher.update(&rgba.height().to_le_bytes());
    hasher.update(rgba.as_raw());
    Some(hasher.finalize().to_hex().to_string())
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
        self.emit_with_dedup_at(snapshot, Instant::now());
    }

    /// Dedup core with an injected `now`, so the time-based guards are testable
    /// without sleeping. Production callers go through [`Self::emit_with_dedup`].
    fn emit_with_dedup_at(&mut self, snapshot: SystemClipboardSnapshot, now: Instant) {
        let raw_key = dedupe_key(&snapshot);
        let is_image = raw_key.as_deref().is_some_and(|k| k.starts_with("image:"));

        // For an image-dominant snapshot, prefer a decode-stable fingerprint
        // over the raw-byte content hash. The same image re-serialized with
        // churning padding / alpha (issue #957) otherwise yields a fresh
        // `image:<hash>` on every read and slips the key dedup below; the
        // fingerprint stays constant, so re-reads collapse via that dedup
        // regardless of cadence and without the size-proxy false-negative.
        // `None` (undecodable format like image/xpm, or oversized) falls back to
        // the raw key + the size-based storm latch.
        let image_fingerprint = if is_image {
            snapshot
                .primary_image_inline_bytes()
                .and_then(stable_image_fingerprint)
        } else {
            None
        };
        let current_dedupe_key = match (&raw_key, &image_fingerprint) {
            (Some(k), Some(fp)) if k.starts_with("image:") => Some(format!("image:{fp}")),
            _ => raw_key,
        };

        if let Some(key) = current_dedupe_key.as_ref() {
            if self.last_meaningful_dedupe_key.as_deref() == Some(key.as_str()) {
                // Info-level on purpose: this is the last silent spot where a
                // user-visible "copy did nothing" can hide (key is kind:hash,
                // never payload content).
                info!(
                    dedupe_key = %key,
                    "Skipping duplicated meaningful clipboard snapshot"
                );
                return;
            }
        }

        // Image storm guard (fallback for images we could NOT fingerprint): an
        // image-dominant snapshot whose image byte size matches the previous
        // image within `IMAGE_STORM_MAX_GAP` is treated as a re-read of the same
        // image (the raw hash churns under us; see `IMAGE_STORM_MAX_GAP`). Keyed
        // off the image representation's own size (not the whole-snapshot total)
        // so co-resident metadata reps can't make two genuinely different images
        // look identically sized. Decodable images are already handled by the
        // stable fingerprint key above, so the size latch is skipped for them —
        // it can't tell two same-size images apart and would otherwise drop a
        // genuinely different same-size image. Computed before the value moves
        // into `try_send`.
        let image_size = if is_image && image_fingerprint.is_none() {
            snapshot.primary_image_size_bytes()
        } else {
            None
        };
        if let Some(size) = image_size {
            if let Some((last, last_size)) = self.last_image_seen {
                if last_size == size && now.duration_since(last) < IMAGE_STORM_MAX_GAP {
                    // Refresh the latch so a sustained burst — at any cadence
                    // below IMAGE_STORM_MAX_GAP — stays suppressed.
                    self.last_image_seen = Some((now, size));
                    debug!(
                        size_bytes = size,
                        elapsed_ms = now.duration_since(last).as_millis(),
                        "Suppressing same-size image clipboard event (storm guard)"
                    );
                    return;
                }
            }
        }

        // Time-window suppression for file snapshots: macOS fires
        // multiple clipboard events when copying files (APFS→resolved
        // path transition) where content bytes may differ slightly.
        if snapshot_has_files(&snapshot) {
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
                self.last_file_emit_time = Some(now);
            }
            if let Some(size) = image_size {
                self.last_image_seen = Some((now, size));
            } else {
                // A genuinely-new non-image copy ends any ongoing image burst,
                // so drop the latch: a later same-size image must not be muted
                // as if the storm were still running.
                self.last_image_seen = None;
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

    /// Plain-text snapshot, so `meaningful_origin_key` keys on `text:` (not
    /// `image:`) — used to exercise the latch-reset-on-non-image path.
    fn text(s: &str) -> SystemClipboardSnapshot {
        let rep = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            s.as_bytes().to_vec(),
        );
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![rep],
        }
    }

    /// Encode a solid-colour `w`×`h` RGB image in `format` — real, decodable
    /// bytes so `stable_image_fingerprint` runs (unlike the `image()` helper's
    /// raw fill bytes, which don't decode and exercise the size-latch fallback).
    fn encode(format: image::ImageFormat, w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        let buf = image::RgbImage::from_pixel(w, h, image::Rgb(rgb));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(buf)
            .write_to(&mut out, format)
            .expect("encode test image");
        out.into_inner()
    }

    /// Image snapshot carrying real (decodable) `bytes`.
    fn decodable_image(bytes: Vec<u8>) -> SystemClipboardSnapshot {
        let rep = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("image"),
            Some(MimeType("image/png".to_string())),
            bytes,
        );
        SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![rep],
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

    #[test]
    fn slow_same_size_image_storm_collapses_regardless_of_cadence() {
        // Regression for the #957 0.15 recurrence: the real storm re-read the
        // same image every ~1.83 s, which exceeded the old fixed 1500 ms window
        // so the guard never fired and every read became a new entry + blob.
        // With a sustained-burst latch, a same-size churning-hash burst at a
        // cadence ABOVE the old window still collapses to a single emit.
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        for i in 0u8..16 {
            let at = base + Duration::from_millis(1830 * i as u64);
            w.emit_with_dedup_at(image(4096, i), at);
        }
        assert_eq!(
            drain(&mut rx),
            1,
            "a slow (~1.83s) same-size image storm must still collapse to one emit"
        );
    }

    #[test]
    fn same_size_image_after_long_idle_emits_again() {
        // Once a burst genuinely stops, the latch must lapse: a deliberate later
        // re-copy of a same-size image (gap beyond IMAGE_STORM_MAX_GAP) emits,
        // so the guard can't swallow real copies forever.
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        w.emit_with_dedup_at(image(4096, 1), base);
        w.emit_with_dedup_at(
            image(4096, 2),
            base + IMAGE_STORM_MAX_GAP + Duration::from_millis(1),
        );
        assert_eq!(
            drain(&mut rx),
            2,
            "a same-size image after the latch lapses must emit"
        );
    }

    #[test]
    fn intervening_non_image_breaks_image_latch() {
        // An intervening non-image copy (text) marks a genuinely-new sequence
        // and must break the image latch, so a following same-size image within
        // the gap is not wrongly muted.
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        w.emit_with_dedup_at(image(4096, 1), base); // emit, latch = 4096
        w.emit_with_dedup_at(text("hello"), base + Duration::from_millis(100)); // emit, clears latch
        w.emit_with_dedup_at(image(4096, 2), base + Duration::from_millis(200)); // emit, latch was cleared
        assert_eq!(
            drain(&mut rx),
            3,
            "a non-image copy between same-size images must break the latch"
        );
    }

    #[test]
    fn churning_serialization_of_same_image_collapses_via_fingerprint() {
        // The same image serialized two different ways (PNG vs BMP) yields
        // different raw bytes, different byte sizes, and a different raw-byte
        // hash — neither the raw key dedup nor the size latch could collapse
        // them. The decode-stable fingerprint must, even at a cadence beyond
        // IMAGE_STORM_MAX_GAP (so the size latch is irrelevant here).
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        let png = encode(image::ImageFormat::Png, 8, 8, [10, 20, 30]);
        let bmp = encode(image::ImageFormat::Bmp, 8, 8, [10, 20, 30]);
        assert_ne!(
            png.len(),
            bmp.len(),
            "the two serializations must differ in byte size"
        );
        w.emit_with_dedup_at(decodable_image(png), base);
        w.emit_with_dedup_at(
            decodable_image(bmp),
            base + IMAGE_STORM_MAX_GAP + Duration::from_secs(1),
        );
        assert_eq!(
            drain(&mut rx),
            1,
            "the same image in two serializations must collapse via the fingerprint"
        );
    }

    #[test]
    fn different_images_of_equal_byte_size_both_emit_via_fingerprint() {
        // Two genuinely different images encoded as same-dimension BMPs have the
        // SAME byte size (BMP size depends only on dimensions), so the size
        // latch alone would wrongly drop the second. The fingerprint tells them
        // apart by pixels, so both must emit — this is the size-proxy
        // false-negative the fingerprint exists to remove.
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        let a = encode(image::ImageFormat::Bmp, 8, 8, [10, 20, 30]);
        let b = encode(image::ImageFormat::Bmp, 8, 8, [200, 100, 50]);
        assert_eq!(
            a.len(),
            b.len(),
            "same-dimension BMPs must have equal byte size for this test to bite"
        );
        w.emit_with_dedup_at(decodable_image(a), base);
        w.emit_with_dedup_at(decodable_image(b), base + Duration::from_millis(100));
        assert_eq!(
            drain(&mut rx),
            2,
            "different images of equal byte size must both emit (no size false-negative)"
        );
    }
}

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

/// Window for the content-independent image *burst breaker*, the second line of
/// defence behind the decode-stable fingerprint (issue #957).
///
/// VirtualBox's shared clipboard (`VBoxShCl`) re-asserts CLIPBOARD ownership
/// every ~150 ms and re-serializes the *same* image with non-deterministic
/// bytes on every read. Excluding alpha from the fingerprint (the earlier #957
/// fix) was not enough: the decoded pixels themselves still drift, so the
/// fingerprint churns, the permanent `image:<fingerprint>` dedup fires at most
/// once, and the storm revives — observed on 0.16.0 as 87 captures of one image
/// in ~12 s. The size-based [`IMAGE_STORM_MAX_GAP`] latch can't backstop it
/// because that latch is deliberately skipped while a fingerprint exists (it
/// can't tell two same-size images apart).
///
/// This breaker catches the storm by *shape* rather than content: once two
/// consecutive same-size decodable images arrive within this window, every
/// further same-size image is suppressed until the size changes or the stream
/// goes quiet. It only engages for images we *could* fingerprint, so the
/// fp=None path keeps using the `IMAGE_STORM_MAX_GAP` latch unchanged.
///
/// The first two frames of any burst still pass — the breaker must observe the
/// burst before it can latch — so a deliberate pair of different same-size
/// images (the `different_images_of_equal_byte_size_both_emit_via_fingerprint`
/// case) is preserved. The window is sub-second-scale and far below any human
/// re-copy of two *different* same-size images, so collapsing only kicks in on
/// the third-and-later frame of a genuine machine-driven storm.
const IMAGE_BURST_BREAKER_WINDOW: Duration = Duration::from_millis(1500);

/// Relative tolerance for treating two image byte sizes as the *same* image
/// inside the burst breaker, as a percentage of the larger size.
///
/// VBoxShCl occasionally varies even the serialized byte *length* by a handful
/// of bytes between re-reads of one image (issue #957: 2_753_070 vs 2_753_058,
/// a 0.0004 % wobble). An exact-equality latch reads that wobble as a brand-new
/// image, resets, and lets the storm leak a few extra frames before it
/// re-latches. A small relative window absorbs the wobble while staying far
/// below the gap to a genuinely different image.
const IMAGE_BURST_SIZE_TOLERANCE_PERCENT: i64 = 1;

/// How long a freshly-emitted text / rich-text key keeps suppressing a
/// byte-identical re-read.
///
/// Images dedup *permanently*: their `image:<fingerprint>` key is a
/// decode-stable identity, so an identical key is provably the same image (the
/// `churning_*` fingerprint tests rely on this holding regardless of elapsed
/// time). Text is different — users deliberately re-copy the same string,
/// especially URLs, and such a re-copy should resurface the existing entry
/// downstream rather than be silently dropped. A permanent text key-dedup
/// turned every re-copy after the first into a no-op for as long as the
/// clipboard kept that content, which surfaced as "copying a URL did nothing".
///
/// The window only needs to absorb re-reads of the *same* physical copy: macOS
/// polls `changeCount` on the order of a second and can fire a second time when
/// an app appends metadata (e.g. LinkPresentation) a beat after the initial
/// write. That second read carries identical text but a *different*
/// `snapshot_hash`, so the downstream snapshot-hash resurface can't collapse it
/// — only this key window can. Sized above the poll cadence yet well below a
/// human re-copy interval, so a deliberate re-copy seconds later passes through
/// and resurfaces.
const MEANINGFUL_REDEDUP_WINDOW: Duration = Duration::from_secs(2);

/// State for the sub-second image [`IMAGE_BURST_BREAKER_WINDOW`] breaker.
/// `latched` flips to true once a second consecutive same-size decodable image
/// lands inside the window, after which further same-size frames are suppressed.
#[derive(Clone, Copy)]
struct ImageBurst {
    at: Instant,
    size: i64,
    latched: bool,
}

pub struct ClipboardWatcher {
    local_clipboard: Arc<dyn SystemClipboardPort>,
    sender: PlatformEventSender,
    /// The most recently emitted meaningful key and when it was emitted.
    /// Drives the re-dedup guard: an identical key re-collapses permanently for
    /// images, or only within [`MEANINGFUL_REDEDUP_WINDOW`] for text.
    last_meaningful: Option<(String, Instant)>,
    last_file_emit_time: Option<Instant>,
    /// `(observed_at, size_bytes)` of the most recent image-dominant snapshot.
    /// Updated on every image observation (emitted or suppressed) and cleared
    /// when a non-image snapshot is emitted. Drives the [`IMAGE_STORM_MAX_GAP`]
    /// storm guard.
    last_image_seen: Option<(Instant, i64)>,
    /// Sub-second burst-breaker state for *decodable* images whose fingerprint
    /// churns (issue #957 / VBoxShCl). Reset when the image size changes or a
    /// non-image snapshot is emitted. Drives the [`IMAGE_BURST_BREAKER_WINDOW`]
    /// breaker, which engages only for images we could fingerprint (the fp=None
    /// path stays on the [`IMAGE_STORM_MAX_GAP`] size latch).
    image_burst: Option<ImageBurst>,
}

impl ClipboardWatcher {
    pub fn new(local_clipboard: Arc<dyn SystemClipboardPort>, sender: PlatformEventSender) -> Self {
        Self {
            local_clipboard,
            sender,
            last_meaningful: None,
            last_file_emit_time: None,
            last_image_seen: None,
            image_burst: None,
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

/// Whether two image byte sizes are close enough to be the same image for the
/// [`IMAGE_BURST_BREAKER_WINDOW`] breaker — within
/// [`IMAGE_BURST_SIZE_TOLERANCE_PERCENT`] of the larger. Integer math: sizes are
/// capped at [`MAX_FINGERPRINT_DECODE_BYTES`] on the only path that calls this,
/// so `a.max(b) * 100` cannot overflow `i64`.
fn approx_same_image_size(a: i64, b: i64) -> bool {
    (a - b).abs() * 100 <= a.max(b) * IMAGE_BURST_SIZE_TOLERANCE_PERCENT
}

/// Largest serialized image we will decode to fingerprint. Above this the
/// decode + pixel hash is too costly to run on the watcher path, so we skip it
/// and fall back to the size-based storm latch.
const MAX_FINGERPRINT_DECODE_BYTES: usize = 64 * 1024 * 1024;

/// A decode-stable identity for a clipboard image: the blake3 hash of its
/// canonical RGB pixels (alpha deliberately excluded) plus dimensions.
///
/// Two reads of the *same* image produce the same fingerprint even when the
/// source re-serializes it with churning padding / alpha / metadata bytes (the
/// issue #957 storm shape) — decoding normalizes the container away, so unlike
/// the raw-byte content hash this does not churn. Returns `None` for images we
/// can't decode (unknown/unsupported format such as `image/xpm`, or corrupt
/// bytes) or that exceed [`MAX_FINGERPRINT_DECODE_BYTES`]; the caller then
/// falls back to the raw-byte key plus the size-based storm latch.
///
/// Alpha is excluded on purpose. VirtualBox's shared clipboard (`VBoxShCl`)
/// hands us a 32-bit BMP whose alpha byte is uninitialized memory that churns
/// on every re-serialization (issue #957). `to_rgba8()` faithfully carries that
/// garbage alpha, so hashing the full RGBA buffer makes the "stable"
/// fingerprint churn anyway, defeating the dedup and reviving the storm. The
/// visible image lives in RGB; for clipboard identity that is the stable,
/// sufficient signal.
fn stable_image_fingerprint(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > MAX_FINGERPRINT_DECODE_BYTES {
        return None;
    }
    let image = image::load_from_memory(bytes).ok()?;
    let rgba = image.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    let mut hasher = blake3::Hasher::new();
    hasher.update(&width.to_le_bytes());
    hasher.update(&height.to_le_bytes());
    // Neutralize the alpha byte of every RGBA pixel before hashing so churning
    // garbage alpha (see above) cannot perturb the fingerprint; a single
    // update over the contiguous buffer keeps this cheap on large images.
    let mut raw = rgba.into_raw();
    for px in raw.chunks_exact_mut(4) {
        px[3] = 0xFF;
    }
    hasher.update(&raw);
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
            if let Some((last_key, last_at)) = self.last_meaningful.as_ref() {
                if last_key == key {
                    // Image keys are a decode-stable fingerprint, so an
                    // identical key is the same image — dedup permanently.
                    // Text / rich-text keys are content-stable too, but a
                    // deliberate user re-copy should resurface downstream, so
                    // only collapse re-reads of the same physical copy, i.e.
                    // within MEANINGFUL_REDEDUP_WINDOW.
                    let permanent = key.starts_with("image:");
                    if permanent || now.duration_since(*last_at) < MEANINGFUL_REDEDUP_WINDOW {
                        // Info-level on purpose: this is the last silent spot
                        // where a user-visible "copy did nothing" can hide (key
                        // is kind:hash, never payload content).
                        info!(
                            dedupe_key = %key,
                            "Skipping duplicated meaningful clipboard snapshot"
                        );
                        return;
                    }
                }
            }
        }

        // Sub-second burst breaker for decodable images whose fingerprint
        // churns (issue #957 / VBoxShCl). The source re-serializes the same
        // image with non-deterministic bytes every ~150 ms, so even the
        // alpha-excluded fingerprint drifts and slips the permanent `image:`
        // dedup above, while the size latch below never runs for it (that latch
        // is fp=None-only). Collapse the storm by shape: once two consecutive
        // same-size decodable images land within IMAGE_BURST_BREAKER_WINDOW,
        // suppress every further same-size frame until the size changes or the
        // stream goes quiet. The first two frames still pass, so a deliberate
        // pair of different same-size images is preserved.
        // Burst-breaker state to commit *only* on a successful send, mirroring
        // how `last_image_seen` / `last_meaningful` are committed below. A frame
        // that never reaches downstream (e.g. `try_send` backpressure during the
        // very storm this guards) must not advance the latch and then mute a
        // later frame that did emit. The suppress path below still refreshes the
        // latch inline before returning — exactly as the storm guard does — so a
        // sustained burst stays muted regardless of send outcome.
        let mut pending_image_burst: Option<ImageBurst> = None;
        if is_image && image_fingerprint.is_some() {
            if let Some(size) = snapshot.primary_image_size_bytes() {
                match self.image_burst {
                    Some(prev)
                        if approx_same_image_size(prev.size, size)
                            && now.duration_since(prev.at) < IMAGE_BURST_BREAKER_WINDOW =>
                    {
                        if prev.latched {
                            self.image_burst = Some(ImageBurst {
                                at: now,
                                size,
                                latched: true,
                            });
                            debug!(
                                size_bytes = size,
                                elapsed_ms = now.duration_since(prev.at).as_millis(),
                                "Suppressing churning-fingerprint image burst (sub-second breaker)"
                            );
                            return;
                        }
                        // Second consecutive same-size read: latch now but still
                        // emit this one, so a genuine pair of same-size images
                        // both pass before suppression begins. Committed on send.
                        pending_image_burst = Some(ImageBurst {
                            at: now,
                            size,
                            latched: true,
                        });
                    }
                    _ => {
                        // First image, or size changed / gap too long: (re)start
                        // the burst tracking unlatched. Committed on send.
                        pending_image_burst = Some(ImageBurst {
                            at: now,
                            size,
                            latched: false,
                        });
                    }
                }
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
                // A genuinely-new non-image copy ends any ongoing image storm,
                // so drop the size latch: a later same-size image must not be
                // muted as if the storm were still running.
                self.last_image_seen = None;
            }
            // Commit the burst-breaker transition computed above now that the
            // frame actually reached downstream. A successfully-emitted
            // non-image copy also ends any ongoing decodable-image burst, so
            // clear the latch — stale burst state must not mute a later
            // same-size image within the old window.
            if let Some(burst) = pending_image_burst {
                self.image_burst = Some(burst);
            } else if !is_image {
                self.image_burst = None;
            }
            if let Some(key) = current_dedupe_key {
                self.last_meaningful = Some((key, now));
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
                file_content_digests: Vec::new(),
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
            file_content_digests: Vec::new(),
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
            file_content_digests: Vec::new(),
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
            file_content_digests: Vec::new(),
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

    /// Encode a solid-colour `w`×`h` RGBA image with a uniform `alpha` byte.
    /// Lets a test vary only the alpha channel — the #957 VBoxShCl churn shape
    /// where the visible RGB image is identical but the (uninitialized) alpha
    /// byte differs on every re-read.
    fn encode_rgba(w: u32, h: u32, rgb: [u8; 3], alpha: u8) -> Vec<u8> {
        let buf = image::RgbaImage::from_pixel(w, h, image::Rgba([rgb[0], rgb[1], rgb[2], alpha]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(buf)
            .write_to(&mut out, image::ImageFormat::Png)
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
            file_content_digests: Vec::new(),
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
    fn churning_alpha_of_same_image_collapses_via_fingerprint() {
        // Reproduces the #957 VirtualBox (VBoxShCl) recurrence: a 32-bit BMP
        // whose alpha byte is uninitialized memory churns on every re-read, so
        // the decoded RGBA buffer — and a full-RGBA fingerprint — differed every
        // time even though the visible RGB image was identical. That defeated
        // the dedup and revived the storm. Excluding alpha from the fingerprint
        // makes the two reads collapse to a single emit, even at a cadence
        // beyond IMAGE_STORM_MAX_GAP (so the size latch is irrelevant here).
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        let a = encode_rgba(8, 8, [10, 20, 30], 0x11);
        let b = encode_rgba(8, 8, [10, 20, 30], 0x22);
        w.emit_with_dedup_at(decodable_image(a), base);
        w.emit_with_dedup_at(
            decodable_image(b),
            base + IMAGE_STORM_MAX_GAP + Duration::from_secs(1),
        );
        assert_eq!(
            drain(&mut rx),
            1,
            "same RGB image with churning alpha must collapse via the fingerprint"
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

    #[test]
    fn text_recopy_after_window_reemits() {
        // Core regression for the macOS "copy did nothing" report: re-copying
        // the SAME text/URL after a human-scale pause (with no different
        // content in between) must emit again, so the downstream capture use
        // case can resurface the existing entry. The old permanent key-dedup
        // swallowed every such re-copy forever, even minutes apart.
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        w.emit_with_dedup_at(text("https://example.com/a"), base);
        w.emit_with_dedup_at(text("https://example.com/a"), base + Duration::from_secs(3));
        assert_eq!(
            drain(&mut rx),
            2,
            "a deliberate re-copy of the same text past the window must re-emit"
        );
    }

    #[test]
    fn text_recopy_within_window_collapses() {
        // A re-read of the SAME physical copy (e.g. macOS appending
        // LinkPresentation metadata a beat later — this changes snapshot_hash
        // but not the plain text) must still collapse. Otherwise it would
        // create a duplicate entry the snapshot-hash resurface can't catch.
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        w.emit_with_dedup_at(text("https://example.com/a"), base);
        w.emit_with_dedup_at(
            text("https://example.com/a"),
            base + Duration::from_millis(300),
        );
        assert_eq!(
            drain(&mut rx),
            1,
            "a same-copy re-read within the window must collapse"
        );
    }

    #[test]
    fn identical_image_recopy_after_window_still_collapses() {
        // Image keys carry a decode-stable fingerprint, so an identical key
        // always means the same image regardless of elapsed time — they must
        // keep deduping permanently (the churning_* fingerprint tests rely on
        // this). The text re-dedup window must not regress image behavior.
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        let png = encode(image::ImageFormat::Png, 8, 8, [10, 20, 30]);
        w.emit_with_dedup_at(decodable_image(png.clone()), base);
        w.emit_with_dedup_at(decodable_image(png), base + Duration::from_secs(60));
        assert_eq!(
            drain(&mut rx),
            1,
            "an identical image must dedup permanently, not just within the text window"
        );
    }

    #[test]
    fn churning_fingerprint_image_burst_is_broken_sub_second() {
        // The #957 0.16.0 recurrence: VBoxShCl hands us a *decodable* BMP whose
        // decoded pixels churn on every ~150 ms re-read, so the (alpha-excluded)
        // fingerprint differs each frame and slips the permanent fingerprint
        // dedup, while the byte size stays constant. The sub-second burst
        // breaker must collapse the storm even though every frame has a distinct
        // fingerprint — only the first two frames pass before it latches.
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        for i in 0u8..16 {
            // Same dimensions => identical byte size; different RGB => a
            // distinct fingerprint every frame (the churn the breaker survives).
            let bmp = encode(image::ImageFormat::Bmp, 8, 8, [i, 20, 30]);
            w.emit_with_dedup_at(
                decodable_image(bmp),
                base + Duration::from_millis(150 * i as u64),
            );
        }
        assert_eq!(
            drain(&mut rx),
            2,
            "a churning-fingerprint same-size image storm must collapse to the first two emits"
        );
    }

    #[test]
    fn churning_fingerprint_images_beyond_burst_window_each_emit() {
        // Same-size but genuinely different images spaced beyond the burst
        // window are deliberate copies, not a storm: each must emit. Guards the
        // breaker against swallowing real same-size copies made at human speed.
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        for i in 0u8..3 {
            let bmp = encode(image::ImageFormat::Bmp, 8, 8, [i * 50, 20, 30]);
            w.emit_with_dedup_at(
                decodable_image(bmp),
                base + (IMAGE_BURST_BREAKER_WINDOW + Duration::from_millis(1)) * i as u32,
            );
        }
        assert_eq!(
            drain(&mut rx),
            3,
            "same-size different images spaced beyond the burst window must all emit"
        );
    }

    #[test]
    fn intervening_non_image_breaks_burst_breaker_latch() {
        // A decodable same-size image burst latches the sub-second breaker; an
        // intervening non-image copy (text) must clear that latch so a later,
        // genuinely different same-size image within the old window still emits
        // instead of being muted by stale burst state. The storm-guard latch is
        // already cleared on a non-image emit; the fingerprint breaker must
        // behave the same. (Uses decodable BMPs so the fp=Some breaker path
        // runs, unlike `intervening_non_image_breaks_image_latch` which drives
        // the fp=None storm guard.)
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        // Same dimensions => identical byte size; different RGB => distinct
        // fingerprints, so these latch the breaker instead of key-deduping.
        let a = encode(image::ImageFormat::Bmp, 8, 8, [10, 20, 30]);
        let b = encode(image::ImageFormat::Bmp, 8, 8, [200, 100, 50]);
        let c = encode(image::ImageFormat::Bmp, 8, 8, [40, 90, 140]);
        w.emit_with_dedup_at(decodable_image(a), base); // emit, latch unlatched
        w.emit_with_dedup_at(decodable_image(b), base + Duration::from_millis(100)); // emit, latches
        w.emit_with_dedup_at(text("hello"), base + Duration::from_millis(200)); // emit, clears latch
        w.emit_with_dedup_at(decodable_image(c), base + Duration::from_millis(300)); // emit, latch cleared
        assert_eq!(
            drain(&mut rx),
            4,
            "a non-image copy between same-size images must clear the burst-breaker latch"
        );
    }

    #[test]
    fn burst_breaker_latch_not_advanced_when_send_fails() {
        // Burst state must only advance on a successful send, like every other
        // dedup latch. A capacity-1 channel lets the first frame queue, then the
        // next send fails (backpressure — exactly what a storm provokes). The
        // failed frame must not advance the latch; once the channel drains, a
        // following same-size frame must still emit rather than be muted by a
        // latch set on a frame that never reached downstream.
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let mut w = ClipboardWatcher::new(Arc::new(StubClipboard), tx);
        let base = Instant::now();
        let a = encode(image::ImageFormat::Bmp, 8, 8, [10, 20, 30]);
        let b = encode(image::ImageFormat::Bmp, 8, 8, [200, 100, 50]);
        w.emit_with_dedup_at(decodable_image(a), base); // queues into the single slot
        w.emit_with_dedup_at(decodable_image(b.clone()), base + Duration::from_millis(50)); // send fails
        assert_eq!(drain(&mut rx), 1, "only the first frame was queued");
        // `b` again (distinct fp from `a`, same size). Had the failed send
        // latched the breaker, this would be suppressed; it must emit.
        w.emit_with_dedup_at(decodable_image(b), base + Duration::from_millis(100));
        assert_eq!(
            drain(&mut rx),
            1,
            "a same-size frame after a failed send must still emit"
        );
    }

    #[test]
    fn approx_same_image_size_absorbs_small_wobble() {
        // The #957 VBoxShCl wobble (2_753_070 vs 2_753_058) must read as the
        // same image; a >1% difference must read as a different one.
        assert!(approx_same_image_size(2_753_070, 2_753_058));
        assert!(approx_same_image_size(2_753_058, 2_753_070));
        assert!(approx_same_image_size(1_000, 1_010)); // exactly 1%
        assert!(!approx_same_image_size(1_000, 1_011)); // just over 1%
        assert!(!approx_same_image_size(4_096, 8_192)); // clearly different
    }

    #[test]
    fn churning_fingerprint_burst_tolerates_small_size_wobble() {
        // VBoxShCl wobbles even the serialized byte length by a few bytes
        // between re-reads (issue #957). The breaker's size tolerance must keep
        // such a near-same-size churning burst collapsed instead of re-latching
        // on every wobble. A 64x64 BMP is ~12 KB, so a few trailing bytes is
        // well under the 1% tolerance.
        let (mut w, mut rx) = watcher();
        let base = Instant::now();
        for i in 0u8..16 {
            let mut bmp = encode(image::ImageFormat::Bmp, 64, 64, [i, 20, 30]);
            // Trailing zero bytes the BMP decoder ignores: wobble the byte size
            // a little (and churn the fingerprint via the RGB change) while
            // keeping it decodable, so the fp=Some breaker path runs.
            bmp.extend(std::iter::repeat(0u8).take((i % 3) as usize));
            assert!(
                stable_image_fingerprint(&bmp).is_some(),
                "padded BMP must still decode so the fp=Some breaker path runs"
            );
            w.emit_with_dedup_at(
                decodable_image(bmp),
                base + Duration::from_millis(150 * i as u64),
            );
        }
        assert_eq!(
            drain(&mut rx),
            2,
            "a near-same-size churning burst must collapse to exactly the first \
             two emits despite byte-size wobble (third-and-later suppressed)"
        );
    }
}

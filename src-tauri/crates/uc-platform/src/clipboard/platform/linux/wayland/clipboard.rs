//! `SystemClipboardPort` impl backed by `wlr-data-control-v1`.
//!
//! ## Why a dedicated worker thread
//!
//! `wayland-client::EventQueue` is `!Send` — every wayland operation has to
//! happen on the same thread that constructed the queue. The trait
//! [`uc_core::ports::SystemClipboardPort`] however exposes synchronous
//! `read_snapshot` / `write_snapshot` methods that any thread in the app may
//! call. So `WaylandClipboard` is a *facade* over a long-running worker
//! thread that owns the wayland connection; calls into the trait become
//! request messages over an mpsc channel and block on the reply.
//!
//! The worker also has to outlive any one request because writes register a
//! `DataControlSource` whose lifetime extends until the compositor cancels
//! it (a future `Send` event may need its bytes minutes later). Owning the
//! source proxy from a transient request handler isn't possible, so all
//! source state lives in the worker.
//!
//! ## Wakeup design
//!
//! The worker blocks in a `poll(2)` over two fds — the wayland socket and a
//! private eventfd. Sending a request writes `1` to the eventfd; the worker
//! wakes, drains the request queue, processes wayland events, and goes
//! back to sleep. This avoids the cost of a tokio runtime in the worker
//! and the cache-line ping-pong of a busy loop.

use std::collections::HashMap;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::sync::mpsc::{self, sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result};
use rustix::event::{poll, PollFd, PollFlags};
use tracing::{debug, info, warn};
use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::SystemClipboardPort;
use wayland_client::backend::ObjectId;
use wayland_client::{
    event_created_child,
    protocol::{wl_registry, wl_registry::WlRegistry, wl_seat, wl_seat::WlSeat},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1, EVT_DATA_OFFER_OPCODE},
    zwlr_data_control_manager_v1::{self, ZwlrDataControlManagerV1},
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

use super::snapshot::build_from_offer;

/// Cap on how long a caller will wait for the worker to process a request.
/// Reads usually return in microseconds (cached snapshot); writes in
/// milliseconds (single roundtrip to the compositor). 5 s is a generous
/// upper bound for "the worker is alive but stuck".
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

const WL_SEAT_VERSION: u32 = 7;
const ZWLR_DATA_CONTROL_MANAGER_VERSION: u32 = 2;

/// Public facade. Cheap to clone (`Arc` internally).
pub struct WaylandClipboard {
    inner: Arc<Inner>,
}

struct Inner {
    request_tx: mpsc::Sender<Request>,
    wakeup_fd: OwnedFd,
    /// `Mutex` so `Drop` can take + join. `Option` so we can `take()` it.
    worker: std::sync::Mutex<Option<JoinHandle<()>>>,
}

enum Request {
    Read(SyncSender<Result<SystemClipboardSnapshot>>),
    Write(SystemClipboardSnapshot, SyncSender<Result<()>>),
    Stop,
}

impl WaylandClipboard {
    /// Probe the running wayland session, bring up the worker thread on
    /// success.
    ///
    /// - `Ok(Some(_))` — wayland connect + manager bind succeeded; ready to
    ///   serve `read_snapshot` / `write_snapshot`.
    /// - `Ok(None)` — connect succeeded but `zwlr_data_control_manager_v1`
    ///   isn't advertised; caller falls back.
    /// - `Err(_)` — hard failure (eventfd creation, thread spawn, etc.).
    pub(crate) fn try_new() -> Result<Option<Self>> {
        // Probe before spawning the worker so a missing manager (e.g.
        // GNOME mutter < 47) doesn't burn a thread.
        let probe_conn = match Connection::connect_to_env() {
            Ok(c) => c,
            Err(e) => {
                debug!(error = %e, "wayland: cannot connect for clipboard backend");
                return Ok(None);
            }
        };
        if !probe_for_manager(&probe_conn)? {
            return Ok(None);
        }

        let wakeup_fd = rustix::event::eventfd(
            0,
            rustix::event::EventfdFlags::CLOEXEC | rustix::event::EventfdFlags::NONBLOCK,
        )
        .context("creating wayland clipboard wakeup eventfd")?;

        let worker_wakeup_fd = wakeup_fd
            .try_clone()
            .context("dup wayland clipboard wakeup eventfd for worker")?;

        let (request_tx, request_rx) = mpsc::channel::<Request>();

        let worker = std::thread::Builder::new()
            .name("wayland-clipboard-worker".into())
            .spawn(move || {
                if let Err(e) = worker_main(probe_conn, request_rx, worker_wakeup_fd) {
                    warn!(error = ?e, "wayland clipboard worker exited with error");
                }
            })
            .context("spawning wayland clipboard worker thread")?;

        Ok(Some(Self {
            inner: Arc::new(Inner {
                request_tx,
                wakeup_fd,
                worker: std::sync::Mutex::new(Some(worker)),
            }),
        }))
    }

    fn send_request(&self, req: Request) -> Result<()> {
        self.inner
            .request_tx
            .send(req)
            .map_err(|e| anyhow::anyhow!("wayland clipboard worker channel closed: {e}"))?;
        // Wake worker.
        let buf = 1u64.to_ne_bytes();
        if let Err(e) = rustix::io::write(&self.inner.wakeup_fd, &buf) {
            warn!(error = %e, "wayland clipboard wakeup write failed");
        }
        Ok(())
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Best-effort shutdown. If the worker has already crashed the send
        // returns Err; either way we proceed to join.
        let _ = self.request_tx.send(Request::Stop);
        let buf = 1u64.to_ne_bytes();
        let _ = rustix::io::write(&self.wakeup_fd, &buf);
        if let Some(handle) = self.worker.lock().ok().and_then(|mut g| g.take()) {
            if let Err(e) = handle.join() {
                warn!(?e, "wayland clipboard worker thread panicked on join");
            }
        }
    }
}

#[async_trait::async_trait]
impl SystemClipboardPort for WaylandClipboard {
    fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
        let (tx, rx) = sync_channel::<Result<SystemClipboardSnapshot>>(1);
        self.send_request(Request::Read(tx))?;
        match rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(res) => res,
            Err(_) => Err(anyhow::anyhow!(
                "wayland clipboard read timed out after {:?}",
                REQUEST_TIMEOUT
            )),
        }
    }

    fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        let (tx, rx) = sync_channel::<Result<()>>(1);
        self.send_request(Request::Write(snapshot, tx))?;
        match rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(res) => res,
            Err(_) => Err(anyhow::anyhow!(
                "wayland clipboard write timed out after {:?}",
                REQUEST_TIMEOUT
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

fn probe_for_manager(conn: &Connection) -> Result<bool> {
    let mut probe_queue = conn.new_event_queue::<ProbeState>();
    let qh = probe_queue.handle();
    let _registry = conn.display().get_registry(&qh, ());

    let mut state = ProbeState::default();
    probe_queue
        .roundtrip(&mut state)
        .context("wayland probe roundtrip 1 failed")?;
    probe_queue
        .roundtrip(&mut state)
        .context("wayland probe roundtrip 2 failed")?;

    if !state.has_manager {
        debug!("wayland: zwlr_data_control_manager_v1 not advertised (clipboard backend)");
        return Ok(false);
    }
    if !state.has_seat {
        warn!("wayland: data-control manager present but no wl_seat (clipboard backend)");
        return Ok(false);
    }
    Ok(true)
}

#[derive(Default)]
struct ProbeState {
    has_seat: bool,
    has_manager: bool,
}

impl Dispatch<WlRegistry, ()> for ProbeState {
    fn event(
        state: &mut Self,
        _registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { interface, .. } = event {
            match interface.as_str() {
                "wl_seat" => state.has_seat = true,
                "zwlr_data_control_manager_v1" => state.has_manager = true,
                _ => {}
            }
        }
    }
}

struct ActiveSource {
    source: ZwlrDataControlSourceV1,
    /// MIME → bytes; multiple MIMEs may share the same `Arc<Vec<u8>>` if a
    /// rep advertises aliases.
    payloads: HashMap<String, Arc<Vec<u8>>>,
}

struct WorkerState {
    seat: Option<WlSeat>,
    manager: Option<ZwlrDataControlManagerV1>,
    device: Option<ZwlrDataControlDeviceV1>,
    /// Per-offer MIME accumulation between `data_offer` and `selection`.
    offers_in_flight: HashMap<ObjectId, Vec<String>>,
    /// Latest snapshot built from a Selection event. Returned by
    /// `read_snapshot` requests.
    cached_snapshot: Option<SystemClipboardSnapshot>,
    /// Source we've published; lives until the compositor cancels it (a
    /// new owner takes the selection) or we replace it on the next write.
    active_source: Option<ActiveSource>,
    /// Number of `set_selection` calls we've issued whose echo we haven't
    /// yet observed. The compositor reflects every selection back to all
    /// data-control devices, including the one that issued it; if we tried
    /// to build a snapshot from our *own* offer we'd deadlock on
    /// `pipe_receive` — the matching `Send` event is queued behind us.
    /// Decremented (and the snapshot build skipped) on each Selection
    /// event while > 0.
    self_echo_pending: u32,
}

impl WorkerState {
    fn new() -> Self {
        Self {
            seat: None,
            manager: None,
            device: None,
            offers_in_flight: HashMap::new(),
            cached_snapshot: None,
            active_source: None,
            self_echo_pending: 0,
        }
    }
}

fn worker_main(conn: Connection, request_rx: Receiver<Request>, wakeup_fd: OwnedFd) -> Result<()> {
    info!("wayland clipboard worker: starting");

    let mut event_queue: EventQueue<WorkerState> = conn.new_event_queue();
    let qh = event_queue.handle();
    let _registry = conn.display().get_registry(&qh, ());

    let mut state = WorkerState::new();

    // Bind globals.
    event_queue
        .roundtrip(&mut state)
        .context("wayland clipboard startup roundtrip failed")?;

    let manager = state
        .manager
        .clone()
        .context("wlr-data-control manager disappeared after probe")?;
    let seat = state
        .seat
        .clone()
        .context("wl_seat disappeared after probe")?;

    state.device = Some(manager.get_data_device(&seat, &qh, ()));

    // Bind-time roundtrip so the compositor delivers the *current* selection
    // (if any). After this, `state.cached_snapshot` reflects the running
    // clipboard contents from the moment the worker started.
    event_queue
        .roundtrip(&mut state)
        .context("wayland clipboard device-bind roundtrip failed")?;

    loop {
        // 1. Drain pending wayland events (Selection/Send/Cancelled fire here).
        event_queue
            .dispatch_pending(&mut state)
            .context("wayland clipboard dispatch_pending failed")?;

        // 2. Drain any pending requests.
        loop {
            match request_rx.try_recv() {
                Ok(Request::Read(reply)) => {
                    let snap = state.cached_snapshot.clone().unwrap_or_else(empty_snapshot);
                    let _ = reply.send(Ok(snap));
                }
                Ok(Request::Write(snap, reply)) => {
                    let res = handle_write(&mut state, &qh, &manager, snap);
                    let _ = reply.send(res);
                }
                Ok(Request::Stop) => {
                    info!("wayland clipboard worker: stop request received");
                    return Ok(());
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    info!("wayland clipboard worker: request channel disconnected");
                    return Ok(());
                }
            }
        }

        // 3. Flush outgoing.
        event_queue
            .flush()
            .context("wayland clipboard event_queue flush failed")?;

        // 4. Block until wayland or wakeup fd is readable.
        let read_guard = match conn.prepare_read() {
            Some(g) => g,
            None => continue,
        };
        let wl_raw_fd = read_guard.connection_fd().as_raw_fd();
        let wakeup_raw_fd = wakeup_fd.as_raw_fd();

        // SAFETY: both fds outlive this poll call (`read_guard` keeps the
        // wayland fd alive, `wakeup_fd` is owned by the worker stack).
        let wl_borrow = unsafe { BorrowedFd::borrow_raw(wl_raw_fd) };
        let wakeup_borrow = unsafe { BorrowedFd::borrow_raw(wakeup_raw_fd) };
        let mut pfds = [
            PollFd::new(&wl_borrow, PollFlags::IN),
            PollFd::new(&wakeup_borrow, PollFlags::IN),
        ];

        let poll_res = poll(&mut pfds, -1);
        let wl_revents = pfds[0].revents();
        let wakeup_revents = pfds[1].revents();

        match poll_res {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => {
                drop(read_guard);
                continue;
            }
            Err(e) => return Err(e.into()),
        }

        // Drain wakeup eventfd so it doesn't immediately re-fire next time.
        if wakeup_revents.contains(PollFlags::IN) {
            let mut buf = [0u8; 8];
            let _ = rustix::io::read(&wakeup_fd, &mut buf);
        }

        if wl_revents.contains(PollFlags::IN) {
            if let Err(e) = read_guard.read() {
                return Err(anyhow::anyhow!(
                    "wayland clipboard read events failed: {e:?}"
                ));
            }
        } else {
            drop(read_guard);
        }
    }
}

fn empty_snapshot() -> SystemClipboardSnapshot {
    SystemClipboardSnapshot {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        representations: Vec::new(),
    }
}

fn handle_write(
    state: &mut WorkerState,
    qh: &QueueHandle<WorkerState>,
    manager: &ZwlrDataControlManagerV1,
    snapshot: SystemClipboardSnapshot,
) -> Result<()> {
    let device = state
        .device
        .as_ref()
        .context("wayland clipboard worker has no data device")?
        .clone();

    if snapshot.representations.is_empty() {
        // Spec: passing a null source to set_selection clears the clipboard.
        if let Some(prev) = state.active_source.take() {
            prev.source.destroy();
        }
        device.set_selection(None);
        state.cached_snapshot = None;
        state.self_echo_pending = state.self_echo_pending.saturating_add(1);
        return Ok(());
    }

    // Build the new source.
    let source = manager.create_data_source(qh, ());
    let mut payloads: HashMap<String, Arc<Vec<u8>>> = HashMap::new();

    for rep in &snapshot.representations {
        let mime_str = rep
            .mime
            .as_ref()
            .map(|m| m.0.clone())
            .or_else(|| default_mime_for_format(&rep.format_id).map(String::from));

        if let Some(mime) = mime_str {
            // Ignore duplicate offer of the same mime — wlroots accepts but
            // the second wins on read; cleaner to skip.
            if payloads.contains_key(&mime) {
                continue;
            }
            source.offer(mime.clone());
            payloads.insert(mime, Arc::new(rep.bytes.clone()));
        }
    }

    if payloads.is_empty() {
        // No mime could be derived; surface as error rather than offer
        // an empty source.
        source.destroy();
        anyhow::bail!("wayland clipboard write: no mime could be derived from snapshot");
    }

    // Replace previous source (compositor will eventually fire Cancelled
    // on it but we don't need to wait — releasing now is fine, the source
    // proxy itself stays valid in flight until the protocol ack).
    if let Some(prev) = state.active_source.take() {
        prev.source.destroy();
    }

    device.set_selection(Some(&source));
    // Update cache eagerly: the compositor will echo the selection back to
    // us, but we must not try to read from our own offer (would deadlock
    // — the matching Send event is queued behind us in the dispatch loop).
    state.cached_snapshot = Some(snapshot);
    state.self_echo_pending = state.self_echo_pending.saturating_add(1);
    state.active_source = Some(ActiveSource { source, payloads });
    Ok(())
}

/// Coarse format_id → mime mapping mirroring the writer side of
/// `CommonClipboardImpl::write_snapshot`. Falls back to `None` for unknown
/// format_ids; unknown reps are skipped (caller surfaces the error).
fn default_mime_for_format(format_id: &str) -> Option<&'static str> {
    match format_id {
        "text" => Some("text/plain;charset=utf-8"),
        "html" => Some("text/html"),
        "rtf" => Some("text/rtf"),
        "image" => Some("image/png"),
        "files" => Some("text/uri-list"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Dispatch impls (worker thread state)
// ---------------------------------------------------------------------------

impl Dispatch<WlRegistry, ()> for WorkerState {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => {
                    let v = version.min(WL_SEAT_VERSION);
                    state.seat = Some(registry.bind::<WlSeat, (), Self>(name, v, qh, ()));
                }
                "zwlr_data_control_manager_v1" if state.manager.is_none() => {
                    let v = version.min(ZWLR_DATA_CONTROL_MANAGER_VERSION);
                    state.manager =
                        Some(registry.bind::<ZwlrDataControlManagerV1, (), Self>(name, v, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<WlSeat, ()> for WorkerState {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for WorkerState {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: zwlr_data_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for WorkerState {
    fn event(
        state: &mut Self,
        _device: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_device_v1::Event::DataOffer { id } => {
                let oid = id.id();
                state.offers_in_flight.insert(oid, Vec::new());
            }
            zwlr_data_control_device_v1::Event::Selection { id } => {
                let Some(offer) = id else {
                    debug!("wayland clipboard: selection cleared");
                    if state.self_echo_pending > 0 {
                        state.self_echo_pending -= 1;
                    } else {
                        state.cached_snapshot = None;
                    }
                    return;
                };
                let oid = offer.id();
                let mimes = state.offers_in_flight.remove(&oid).unwrap_or_default();

                if state.self_echo_pending > 0 {
                    // Echo of our own set_selection. cached_snapshot was set
                    // eagerly in handle_write; trying to read this offer back
                    // would deadlock (Send sits behind us in the queue).
                    state.self_echo_pending -= 1;
                    debug!(
                        ?oid,
                        mime_count = mimes.len(),
                        "wayland clipboard: skipping self-echo selection"
                    );
                    offer.destroy();
                    return;
                }

                if mimes.is_empty() {
                    offer.destroy();
                    return;
                }
                match build_from_offer(conn, &offer, &mimes) {
                    Ok(snap) => state.cached_snapshot = Some(snap),
                    Err(e) => warn!(error = %e, "wayland clipboard: snapshot capture failed"),
                }
                offer.destroy();
            }
            zwlr_data_control_device_v1::Event::PrimarySelection { id } => {
                if let Some(offer) = id {
                    state.offers_in_flight.remove(&offer.id());
                    offer.destroy();
                }
            }
            zwlr_data_control_device_v1::Event::Finished => {
                debug!("wayland clipboard: data_control_device finished");
                state.device = None;
            }
            _ => {}
        }
    }

    event_created_child!(WorkerState, ZwlrDataControlDeviceV1, [
        EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for WorkerState {
    fn event(
        state: &mut Self,
        offer: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            let oid = offer.id();
            if let Some(mimes) = state.offers_in_flight.get_mut(&oid) {
                mimes.push(mime_type);
            }
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for WorkerState {
    fn event(
        state: &mut Self,
        source: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_source_v1::Event::Send { mime_type, fd } => {
                let active = match &state.active_source {
                    Some(a) if a.source.id() == source.id() => a,
                    _ => {
                        debug!(mime = %mime_type, "wayland clipboard: Send for stale source — ignoring");
                        return;
                    }
                };
                match active.payloads.get(&mime_type) {
                    Some(bytes) => {
                        write_payload(fd, bytes, &mime_type);
                    }
                    None => {
                        debug!(
                            mime = %mime_type,
                            "wayland clipboard: paster requested mime we don't carry — closing fd"
                        );
                        // Closing without writing == empty payload.
                        drop(fd);
                    }
                }
            }
            zwlr_data_control_source_v1::Event::Cancelled => {
                if let Some(active) = &state.active_source {
                    if active.source.id() == source.id() {
                        debug!("wayland clipboard: source cancelled by compositor");
                        if let Some(prev) = state.active_source.take() {
                            prev.source.destroy();
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn write_payload(fd: OwnedFd, bytes: &[u8], mime: &str) {
    // Set non-blocking? Compositor's read end might be slow. Use blocking
    // write but cap with a poll-based timeout so a stuck paster doesn't
    // wedge the worker.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut written = 0;

    while written < bytes.len() {
        let now = std::time::Instant::now();
        if now >= deadline {
            warn!(
                mime = %mime,
                wrote = written,
                total = bytes.len(),
                "wayland clipboard: write to paster timed out"
            );
            return;
        }
        let remaining_ms: i32 = (deadline - now)
            .as_millis()
            .min(i32::MAX as u128)
            .try_into()
            .unwrap_or(i32::MAX);

        // Wait for the fd to be writable.
        let mut pfd = [PollFd::new(&fd, PollFlags::OUT)];
        match poll(&mut pfd, remaining_ms) {
            Ok(0) => {
                warn!(mime = %mime, "wayland clipboard: write poll timed out");
                return;
            }
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => {
                warn!(mime = %mime, error = %e, "wayland clipboard: write poll failed");
                return;
            }
        }

        match rustix::io::write(&fd, &bytes[written..]) {
            Ok(0) => {
                warn!(mime = %mime, "wayland clipboard: write returned 0");
                return;
            }
            Ok(n) => written += n,
            Err(rustix::io::Errno::AGAIN) | Err(rustix::io::Errno::INTR) => continue,
            Err(e) => {
                warn!(mime = %mime, error = %e, "wayland clipboard: write failed");
                return;
            }
        }
    }
    // Closing fd signals EOF to the paster.
    drop(fd);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mime_covers_known_format_ids() {
        assert_eq!(
            default_mime_for_format("text"),
            Some("text/plain;charset=utf-8")
        );
        assert_eq!(default_mime_for_format("html"), Some("text/html"));
        assert_eq!(default_mime_for_format("rtf"), Some("text/rtf"));
        assert_eq!(default_mime_for_format("image"), Some("image/png"));
        assert_eq!(default_mime_for_format("files"), Some("text/uri-list"));
        assert_eq!(default_mime_for_format("unknown"), None);
    }

    /// `BorrowedFd` and friends are touchy with `unsafe` helpers; this is a
    /// belt-and-braces check that the empty-snapshot constructor at least
    /// compiles and produces a structure with no reps.
    #[test]
    fn empty_snapshot_has_no_reps() {
        let snap = empty_snapshot();
        assert!(snap.representations.is_empty());
        assert!(snap.ts_ms > 0);
    }
}
